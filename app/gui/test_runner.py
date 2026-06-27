"""Always-on-top manual test runner.

Executes a session of test cases: shows each case's preconditions + steps,
captures a local note, a comment, screenshots, and an overall outcome, then
pushes the outcomes to Azure DevOps as a Test Run (one run per session).

Only GET/POST/PATCH are used to record results — no DELETE. Submission is
behind an explicit confirm.
"""

import base64
import time

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton, QTableWidget,
    QTableWidgetItem, QHeaderView, QPlainTextEdit, QFileDialog, QFrame,
    QMessageBox, QRubberBand, QApplication, QScrollArea, QSizePolicy, QDialog,
)
from PyQt5.QtCore import Qt, pyqtSignal, QRect, QSize, QByteArray, QBuffer, QThreadPool
from PyQt5.QtGui import QImage, QPixmap, QCursor

from app.utils.xml_builder import parse_steps_xml
from app.utils.worker import Worker
from app.utils import settings as settings_mod
from app.utils import theme

# UI label -> Azure DevOps outcome value
_OUTCOMES = [("Pass", "Passed"), ("Fail", "Failed"),
             ("Blocked", "Blocked"), ("N/A", "NotApplicable")]


def _image_to_b64(img: QImage) -> str:
    """PNG-encode a QImage and base64 it for the attachments API."""
    ba = QByteArray()
    buf = QBuffer(ba)
    buf.open(QBuffer.WriteOnly)
    img.save(buf, "PNG")
    buf.close()
    return base64.b64encode(bytes(ba)).decode("ascii")


def submit_session(client, pbi_id, pbi_title, plan_id, suite_id, area, iteration, per_case):
    """Push a session's outcomes to Azure DevOps as one Test Run. Runs on a
    worker thread. `per_case` items: {tc_id, outcome, comment, duration_ms,
    screenshots:[b64]}. Returns a summary dict. Raises on failure."""
    if not (plan_id and suite_id):
        plan_id, _name, suite_id = client.ensure_requirement_suite(pbi_id, area, iteration)

    points = client.get_test_points(plan_id, suite_id, [c["tc_id"] for c in per_case])
    point_by_tc = {}
    for p in points:
        tcid = p.get("test_case_id")
        if tcid and tcid not in point_by_tc:
            point_by_tc[tcid] = p["point_id"]

    runnable = [c for c in per_case if c["tc_id"] in point_by_tc]
    skipped = [c["tc_id"] for c in per_case if c["tc_id"] not in point_by_tc]
    if not runnable:
        raise RuntimeError(
            "None of the selected test cases have a test point in this PBI's "
            "test suite, so no outcomes could be recorded."
        )

    point_ids = [point_by_tc[c["tc_id"]] for c in runnable]
    run = client.create_test_run(plan_id, f"Manual run — PBI #{pbi_id}: {pbi_title}", point_ids)
    run_id = run["run_id"]

    result_by_tc = {
        r["test_case_id"]: r["result_id"]
        for r in client.get_run_results(run_id) if r["test_case_id"]
    }

    updates = []
    for c in runnable:
        rid = result_by_tc.get(c["tc_id"])
        if rid is not None:
            updates.append({
                "id": rid, "outcome": c["outcome"],
                "comment": c.get("comment", ""), "duration_ms": c.get("duration_ms", 0),
            })
    if updates:
        client.update_run_results(run_id, updates)

    for c in runnable:
        rid = result_by_tc.get(c["tc_id"])
        if rid is None:
            continue
        for i, b64 in enumerate(c.get("screenshots", []), 1):
            client.add_result_attachment(run_id, rid, b64, f"tc{c['tc_id']}_shot{i}.png")
            time.sleep(0.5)  # pacing, consistent with the create flow

    client.complete_test_run(run_id)
    return {"run_id": run_id, "web_url": run.get("web_url", ""),
            "submitted": len(runnable), "skipped": skipped}


def fetch_existing(client, plan_id, suite_id, tc_ids):
    """Look up each test case's last recorded outcome + comment so the runner can
    pre-populate them. Returns {tc_id: {last_outcome, comment}}. Best-effort — a
    comment lookup that fails is skipped. Read-only."""
    out = {}
    if not (plan_id and suite_id) or not tc_ids:
        return out
    for p in client.get_test_points(plan_id, suite_id, tc_ids):
        tcid = p.get("test_case_id")
        if not tcid or tcid in out:
            continue  # first point (default configuration) per test case wins
        entry = {"last_outcome": p.get("last_outcome", ""), "comment": ""}
        rid, run = p.get("last_result_id"), p.get("last_run_id")
        if rid and run:
            try:
                entry["comment"] = client.get_result(run, rid).get("comment", "")
            except Exception:
                pass
        out[tcid] = entry
    return out


class _CaptureOverlay(QWidget):
    """Full-virtual-desktop dimming overlay for drag-to-select screen capture."""
    captured = pyqtSignal(object)  # QImage or None

    def __init__(self):
        super().__init__()
        self.setWindowFlags(Qt.Window | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint)
        self.setWindowOpacity(0.30)
        self.setStyleSheet("background: #101010;")
        self.setCursor(QCursor(Qt.CrossCursor))
        self.setGeometry(QApplication.primaryScreen().virtualGeometry())
        self._origin = None
        self._rubber = QRubberBand(QRubberBand.Rectangle, self)

    def mousePressEvent(self, e):
        self._origin = e.pos()
        self._rubber.setGeometry(QRect(self._origin, QSize()))
        self._rubber.show()

    def mouseMoveEvent(self, e):
        if self._origin is not None:
            self._rubber.setGeometry(QRect(self._origin, e.pos()).normalized())

    def mouseReleaseEvent(self, e):
        if self._origin is None:
            return
        rect = QRect(self._origin, e.pos()).normalized()
        self._rubber.hide()
        self.hide()
        QApplication.processEvents()  # ensure the overlay is gone before grabbing
        if rect.width() < 5 or rect.height() < 5:
            self.captured.emit(None)
        else:
            gtl = self.mapToGlobal(rect.topLeft())
            pix = QApplication.primaryScreen().grabWindow(
                0, gtl.x(), gtl.y(), rect.width(), rect.height()
            )
            self.captured.emit(pix.toImage() if not pix.isNull() else None)
        self.close()

    def keyPressEvent(self, e):
        if e.key() == Qt.Key_Escape:
            self.hide()
            self.captured.emit(None)
            self.close()


class TestRunner(QWidget):
    """Top-level always-on-top window that runs a session of test cases."""

    def __init__(self, app_state, cases: list):
        super().__init__()
        self.app_state = app_state
        # Snapshot the PBI context at launch so that changing the PBI elsewhere in
        # the app can never redirect this submission to a different PBI's results.
        self._pbi_id = app_state.pbi_id
        self._pbi_title = app_state.pbi_title
        self._plan_id = app_state.test_plan_id
        self._suite_id = app_state.suite_id
        self._area = app_state.area_path
        self._iteration = app_state.iteration_path
        self.cases = cases
        self.idx = 0
        # Per-case execution state, parallel to self.cases.
        self.state = [
            {"outcome": "", "comment": "", "screenshots": [], "elapsed_ms": 0}
            for _ in cases
        ]
        self._enter_monotonic = None
        self._pinned = True
        self._overlay = None

        self.setWindowTitle("Test Runner")
        self.setMinimumSize(480, 640)
        self.resize(540, 760)
        self.setWindowFlags(self.windowFlags() | Qt.Window | Qt.WindowStaysOnTopHint)
        self._build_ui()
        self._load_case(0)
        self._start_preload()

    # ------------------------------------------------------------------ #
    #  UI                                                                 #
    # ------------------------------------------------------------------ #

    def _build_ui(self):
        t = theme.tokens()
        root = QVBoxLayout(self)
        root.setContentsMargins(14, 12, 14, 12)
        root.setSpacing(8)

        # Header: progress + pin
        hdr = QHBoxLayout()
        self._progress_lbl = QLabel("")
        self._progress_lbl.setStyleSheet("font-weight: bold; font-size: 13px;")
        hdr.addWidget(self._progress_lbl)
        hdr.addStretch()
        self._pin_btn = QPushButton()
        self._pin_btn.setCheckable(True)
        self._pin_btn.setChecked(True)
        self._pin_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._pin_btn.clicked.connect(self._toggle_pin)
        hdr.addWidget(self._pin_btn)
        root.addLayout(hdr)

        # Title
        self._title_lbl = QLabel("")
        self._title_lbl.setWordWrap(True)
        self._title_lbl.setStyleSheet("font-size: 14px; font-weight: bold;")
        root.addWidget(self._title_lbl)

        # Scrollable middle (preconditions + steps + notes/comment + screenshots)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.NoFrame)
        body = QWidget()
        bl = QVBoxLayout(body)
        bl.setContentsMargins(0, 0, 0, 0)
        bl.setSpacing(8)

        bl.addWidget(self._section_label("Preconditions"))
        self._pre_lbl = QLabel("")
        self._pre_lbl.setWordWrap(True)
        self._pre_lbl.setTextFormat(Qt.RichText)
        self._pre_lbl.setStyleSheet(
            f"background: {t['surface']}; border: 1px solid {t['border']}; "
            f"border-radius: 4px; padding: 8px; color: {t['text']};"
        )
        bl.addWidget(self._pre_lbl)

        bl.addWidget(self._section_label("Steps"))
        self._steps_tbl = QTableWidget(0, 3)
        self._steps_tbl.setHorizontalHeaderLabels(["#", "Action", "Expected Result"])
        hh = self._steps_tbl.horizontalHeader()
        hh.setSectionResizeMode(0, QHeaderView.Fixed)
        hh.setSectionResizeMode(1, QHeaderView.Stretch)
        hh.setSectionResizeMode(2, QHeaderView.Stretch)
        self._steps_tbl.setColumnWidth(0, 30)
        self._steps_tbl.verticalHeader().setVisible(False)
        self._steps_tbl.setEditTriggers(QTableWidget.NoEditTriggers)
        self._steps_tbl.setWordWrap(True)
        self._steps_tbl.setMinimumHeight(160)
        bl.addWidget(self._steps_tbl)

        bl.addWidget(self._section_label("Notes (local only — not sent to DevOps)"))
        self._notes_edit = QPlainTextEdit()
        self._notes_edit.setPlaceholderText("Private notes for this test case, kept on this machine…")
        self._notes_edit.setFixedHeight(64)
        bl.addWidget(self._notes_edit)

        bl.addWidget(self._section_label("Comment (saved to the DevOps result)"))
        self._comment_edit = QPlainTextEdit()
        self._comment_edit.setPlaceholderText("Optional comment recorded with the outcome…")
        self._comment_edit.setFixedHeight(56)
        bl.addWidget(self._comment_edit)

        # Screenshots
        sshot_hdr = QHBoxLayout()
        sshot_hdr.addWidget(self._section_label("Screenshots"))
        sshot_hdr.addStretch()
        for label, slot in (("📋 Paste", self._paste_shot),
                            ("📁 File", self._file_shot),
                            ("📷 Capture", self._capture_shot)):
            b = QPushButton(label)
            b.setCursor(QCursor(Qt.PointingHandCursor))
            b.setStyleSheet(theme.btn_neutral_qss())
            b.clicked.connect(slot)
            sshot_hdr.addWidget(b)
        bl.addLayout(sshot_hdr)
        self._shots_row = QHBoxLayout()
        self._shots_row.setAlignment(Qt.AlignLeft)
        _shots_wrap = QWidget()
        _shots_wrap.setLayout(self._shots_row)
        bl.addWidget(_shots_wrap)

        bl.addStretch()
        scroll.setWidget(body)
        root.addWidget(scroll, 1)

        # "Previous result" badge (filled in once the pre-load completes)
        self._last_lbl = QLabel("")
        self._last_lbl.setStyleSheet("color: #888; font-size: 11px;")
        root.addWidget(self._last_lbl)

        # Outcome buttons
        outcome_row = QHBoxLayout()
        self._outcome_btns = {}
        for label, ado in _OUTCOMES:
            b = QPushButton(label)
            b.setCheckable(True)
            b.setFixedHeight(34)
            b.setCursor(QCursor(Qt.PointingHandCursor))
            b.clicked.connect(lambda _c, a=ado: self._set_outcome(a))
            self._outcome_btns[ado] = b
            outcome_row.addWidget(b)
        root.addLayout(outcome_row)

        # Nav row: Prev (left corner)  ·  Submit (centre)  ·  Next (right corner)
        nav = QHBoxLayout()
        self._prev_btn = QPushButton("◀ Prev")
        self._prev_btn.setStyleSheet(theme.btn_neutral_qss())
        self._prev_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._prev_btn.clicked.connect(lambda: self._go(self.idx - 1))
        self._next_btn = QPushButton("Next ▶")
        self._next_btn.setStyleSheet(theme.btn_neutral_qss())
        self._next_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._next_btn.clicked.connect(lambda: self._go(self.idx + 1))
        self._submit_btn = QPushButton("✔ Submit Results")
        self._submit_btn.setFixedHeight(34)
        self._submit_btn.setStyleSheet(theme.btn_primary_qss("padding: 0 18px;"))
        self._submit_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._submit_btn.clicked.connect(self._on_submit)
        nav.addWidget(self._prev_btn)
        nav.addStretch()
        nav.addWidget(self._submit_btn)
        nav.addStretch()
        nav.addWidget(self._next_btn)
        root.addLayout(nav)

        self._status_lbl = QLabel("")
        self._status_lbl.setWordWrap(True)
        self._status_lbl.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        root.addWidget(self._status_lbl)

        self._update_pin_btn()
        self._update_outcome_summary()

    @staticmethod
    def _section_label(text: str) -> QLabel:
        lbl = QLabel(text)
        lbl.setStyleSheet("font-weight: bold; font-size: 11px;")
        return lbl

    # ------------------------------------------------------------------ #
    #  Case navigation                                                    #
    # ------------------------------------------------------------------ #

    def _go(self, new_idx: int):
        if new_idx < 0 or new_idx >= len(self.cases):
            return
        self._commit_current()
        self._load_case(new_idx)

    def _commit_current(self):
        """Save the editable fields of the current case into state + notes file."""
        st = self.state[self.idx]
        st["comment"] = self._comment_edit.toPlainText()
        if self._enter_monotonic is not None:
            st["elapsed_ms"] += int((time.monotonic() - self._enter_monotonic) * 1000)
            self._enter_monotonic = None
        tc_id = self.cases[self.idx].get("_id")
        if tc_id is not None:
            settings_mod.save_execution_note(tc_id, self._notes_edit.toPlainText())

    def _load_case(self, idx: int):
        self.idx = idx
        case = self.cases[idx]
        tc_id = case.get("_id", "?")
        self._progress_lbl.setText(f"{idx + 1} of {len(self.cases)}")
        self._title_lbl.setText(f"#{tc_id}  —  {case.get('System.Title', '(no title)')}")

        pre_ref = self.app_state.preconditions_ref
        pre = (case.get(pre_ref, "") if pre_ref else "") or ""
        self._pre_lbl.setText(pre if pre.strip() else "<i>(none)</i>")

        self._steps_tbl.setRowCount(0)
        steps = parse_steps_xml(case.get("Microsoft.VSTS.TCM.Steps", "") or "")
        for i, step in enumerate(steps):
            r = self._steps_tbl.rowCount()
            self._steps_tbl.insertRow(r)
            num = QTableWidgetItem(str(i + 1))
            num.setTextAlignment(Qt.AlignCenter)
            self._steps_tbl.setItem(r, 0, num)
            self._steps_tbl.setItem(r, 1, QTableWidgetItem(step.action))
            self._steps_tbl.setItem(r, 2, QTableWidgetItem(step.expected))
        self._steps_tbl.resizeRowsToContents()

        st = self.state[idx]
        self._comment_edit.setPlainText(st["comment"])
        self._notes_edit.setPlainText(settings_mod.get_execution_note(tc_id))
        self._refresh_outcome_buttons(st["outcome"])
        self._last_lbl.setText(self._format_last(st.get("last_outcome", "")))
        self._refresh_shots()
        self._prev_btn.setEnabled(idx > 0)
        self._next_btn.setEnabled(idx < len(self.cases) - 1)
        self._enter_monotonic = time.monotonic()
        self._update_outcome_summary()

    # ------------------------------------------------------------------ #
    #  Outcome + pin                                                      #
    # ------------------------------------------------------------------ #

    def _set_outcome(self, ado_value: str):
        self.state[self.idx]["outcome"] = ado_value
        self._refresh_outcome_buttons(ado_value)
        self._update_outcome_summary()

    def _refresh_outcome_buttons(self, selected: str):
        colors = {"Passed": "#4caf50", "Failed": "#e53935",
                  "Blocked": "#fb8c00", "NotApplicable": "#9e9e9e"}
        for ado, btn in self._outcome_btns.items():
            on = (ado == selected)
            btn.setChecked(on)
            if on:
                btn.setStyleSheet(
                    f"QPushButton {{ background: {colors[ado]}; color: white; "
                    f"border: none; border-radius: 4px; font-weight: bold; }}"
                )
            else:
                btn.setStyleSheet(theme.btn_neutral_qss())

    def _update_outcome_summary(self):
        done = sum(1 for s in self.state if s["outcome"])
        self._status_lbl.setText(f"{done} of {len(self.cases)} marked")

    # ------------------------------------------------------------------ #
    #  Pre-load existing results                                          #
    # ------------------------------------------------------------------ #

    _NORMALIZE = {"passed": "Passed", "failed": "Failed",
                  "blocked": "Blocked", "notapplicable": "NotApplicable"}

    def _start_preload(self):
        """Fetch each case's last recorded outcome + comment in the background and
        pre-fill them — only into fields the user hasn't already touched."""
        if not (self._plan_id and self._suite_id):
            return
        tc_ids = [c.get("_id") for c in self.cases if c.get("_id")]
        if not tc_ids:
            return
        self._status_lbl.setText("Loading previous results…")
        worker = Worker(fetch_existing, self.app_state.client,
                        self._plan_id, self._suite_id, tc_ids)
        worker.signals.result.connect(self._on_preload)
        worker.signals.error.connect(lambda _exc: self._update_outcome_summary())
        QThreadPool.globalInstance().start(worker)

    def _on_preload(self, existing: dict):
        # Preserve anything typed for the visible case before applying pre-load.
        self.state[self.idx]["comment"] = self._comment_edit.toPlainText()
        for i, case in enumerate(self.cases):
            data = existing.get(case.get("_id"))
            if not data:
                continue
            st = self.state[i]
            st["last_outcome"] = data.get("last_outcome", "")
            norm = self._NORMALIZE.get((data.get("last_outcome") or "").lower())
            if norm and not st["outcome"]:          # never clobber a user's mark
                st["outcome"] = norm
            if data.get("comment") and not st["comment"]:
                st["comment"] = data["comment"]
        # Re-render just the current case's outcome/comment/badge (NOT the notes,
        # which would reload from disk and lose any in-progress typing).
        st = self.state[self.idx]
        self._comment_edit.setPlainText(st["comment"])
        self._refresh_outcome_buttons(st["outcome"])
        self._last_lbl.setText(self._format_last(st.get("last_outcome", "")))
        self._update_outcome_summary()

    @staticmethod
    def _format_last(last: str) -> str:
        v = {"passed": "Passed", "failed": "Failed", "blocked": "Blocked",
             "notapplicable": "Not Applicable"}.get((last or "").lower())
        return f"Previous result: {v}" if v else "Previous result: —"

    def _toggle_pin(self):
        self._pinned = self._pin_btn.isChecked()
        flags = self.windowFlags()
        if self._pinned:
            flags |= Qt.WindowStaysOnTopHint
        else:
            flags &= ~Qt.WindowStaysOnTopHint
        self.setWindowFlags(flags)
        self.show()  # required after changing window flags
        self._update_pin_btn()

    def _update_pin_btn(self):
        self._pin_btn.setText("📌 Always on top: ON" if self._pinned
                              else "📌 Always on top: OFF")

    # ------------------------------------------------------------------ #
    #  Screenshots                                                        #
    # ------------------------------------------------------------------ #

    def _add_image(self, img: QImage):
        if img is None or img.isNull():
            return
        self.state[self.idx]["screenshots"].append(img)
        self._refresh_shots()

    def _paste_shot(self):
        img = QApplication.clipboard().image()
        if img is None or img.isNull():
            self._status_lbl.setText("No image on the clipboard to paste.")
            return
        self._add_image(img)

    def _file_shot(self):
        path, _ = QFileDialog.getOpenFileName(
            self, "Attach Screenshot", "", "Images (*.png *.jpg *.jpeg *.bmp)"
        )
        if path:
            self._add_image(QImage(path))

    def _capture_shot(self):
        self.hide()  # keep the runner out of the shot
        QApplication.processEvents()
        overlay = _CaptureOverlay()
        overlay.captured.connect(self._on_captured)
        self._overlay = overlay
        overlay.show()
        overlay.activateWindow()
        overlay.raise_()

    def _on_captured(self, img):
        self.show()
        self.raise_()
        self._overlay = None
        if img is not None:
            self._add_image(img)

    def _refresh_shots(self):
        while self._shots_row.count():
            item = self._shots_row.takeAt(0)
            w = item.widget()
            if w is not None:
                w.deleteLater()
        shots = self.state[self.idx]["screenshots"]
        for i, img in enumerate(shots):
            thumb = QLabel()
            thumb.setPixmap(QPixmap.fromImage(img).scaled(
                64, 64, Qt.KeepAspectRatio, Qt.SmoothTransformation))
            thumb.setToolTip("Left-click to view · right-click to remove")
            thumb.setCursor(QCursor(Qt.PointingHandCursor))
            thumb.setStyleSheet("border: 1px solid #888;")
            thumb.mousePressEvent = lambda e, idx=i: self._on_shot_click(e, idx)
            self._shots_row.addWidget(thumb)
        if not shots:
            empty = QLabel("(none)")
            empty.setStyleSheet("color: #888; font-size: 11px;")
            self._shots_row.addWidget(empty)

    def _remove_shot(self, i: int):
        shots = self.state[self.idx]["screenshots"]
        if 0 <= i < len(shots):
            del shots[i]
            self._refresh_shots()

    def _on_shot_click(self, event, i: int):
        if event.button() == Qt.RightButton:
            self._remove_shot(i)
        elif event.button() == Qt.LeftButton:
            self._view_shot(i)

    def _view_shot(self, i: int):
        """Open the screenshot at a viewable size (scaled to fit the screen)."""
        shots = self.state[self.idx]["screenshots"]
        if not (0 <= i < len(shots)):
            return
        pix = QPixmap.fromImage(shots[i])
        avail = QApplication.primaryScreen().availableGeometry()
        max_w, max_h = int(avail.width() * 0.85), int(avail.height() * 0.85)
        if pix.width() > max_w or pix.height() > max_h:
            pix = pix.scaled(max_w, max_h, Qt.KeepAspectRatio, Qt.SmoothTransformation)
        dlg = QDialog(self)
        dlg.setWindowTitle(f"Screenshot {i + 1}")
        # Stay above the runner even while it's pinned on top.
        dlg.setWindowFlags(dlg.windowFlags() | Qt.WindowStaysOnTopHint)
        lay = QVBoxLayout(dlg)
        lay.setContentsMargins(8, 8, 8, 8)
        lbl = QLabel()
        lbl.setPixmap(pix)
        lay.addWidget(lbl)
        dlg.exec_()

    # ------------------------------------------------------------------ #
    #  Submit                                                             #
    # ------------------------------------------------------------------ #

    def _on_submit(self):
        from app.gui.helpers import warn_if_token_expired
        if warn_if_token_expired(self, self.app_state.token_manager):
            return
        self._commit_current()
        marked = [(c, s) for c, s in zip(self.cases, self.state) if s["outcome"]]
        if not marked:
            QMessageBox.information(self, "Nothing to submit",
                                   "Mark at least one test case with an outcome first.")
            return

        n = len(marked)
        reply = QMessageBox.question(
            self, "Submit Results",
            f"Record outcomes for {n} test case{'s' if n != 1 else ''} as a Test Run "
            f"on PBI #{self._pbi_id} in Azure DevOps?\n\n"
            "This creates a test run and cannot be undone.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if reply != QMessageBox.Yes:
            return

        per_case = []
        for c, s in marked:
            per_case.append({
                "tc_id": c.get("_id"),
                "outcome": s["outcome"],
                "comment": s["comment"],
                "duration_ms": s["elapsed_ms"],
                "screenshots": [_image_to_b64(img) for img in s["screenshots"]],
            })

        self._submit_btn.setEnabled(False)
        self._submit_btn.setText("Submitting…")
        self._status_lbl.setText("Submitting results to Azure DevOps…")
        worker = Worker(
            submit_session, self.app_state.client, self._pbi_id,
            self._pbi_title, self._plan_id, self._suite_id, self._area,
            self._iteration, per_case,
        )
        worker.signals.result.connect(self._on_submit_done)
        worker.signals.error.connect(self._on_submit_error)
        QThreadPool.globalInstance().start(worker)

    def _on_submit_done(self, summary: dict):
        self._submit_btn.setEnabled(True)
        self._submit_btn.setText("✔ Submit Results")
        skipped = summary.get("skipped") or []
        msg = f"Recorded {summary['submitted']} outcome(s) in test run #{summary['run_id']}."
        if skipped:
            msg += (f"\n\n{len(skipped)} case(s) had no test point in the suite and were "
                    f"skipped: {', '.join('#' + str(s) for s in skipped[:8])}.")
        if summary.get("web_url"):
            msg += f"\n\nView in Azure DevOps:\n{summary['web_url']}"
        self._status_lbl.setText(f"Submitted to run #{summary['run_id']}.")
        QMessageBox.information(self, "Results Submitted", msg)

    def _on_submit_error(self, exc: Exception):
        self._submit_btn.setEnabled(True)
        self._submit_btn.setText("✔ Submit Results")
        self._status_lbl.setText("Submit failed.")
        QMessageBox.critical(self, "Submit Failed",
                             f"Could not record the results:\n\n{exc}")

    def closeEvent(self, event):
        # Persist notes for the case currently shown.
        try:
            self._commit_current()
        except Exception:
            pass
        super().closeEvent(event)
