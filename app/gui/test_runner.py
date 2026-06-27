"""Always-on-top manual test runner.

Executes a session of test cases: shows each case's preconditions + steps,
captures a local note, a comment, screenshots, and an overall outcome, then
pushes the outcomes to Azure DevOps as a Test Run (one run per session).

Only GET/POST/PATCH are used to record results — no DELETE. Submission is
behind an explicit confirm.
"""

import base64
import time
import xml.etree.ElementTree as ET

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton, QTableWidget,
    QTableWidgetItem, QHeaderView, QPlainTextEdit, QFileDialog, QFrame,
    QMessageBox, QRubberBand, QApplication, QScrollArea, QSizePolicy, QDialog,
    QLineEdit, QComboBox, QCheckBox,
)
from PyQt5.QtCore import Qt, pyqtSignal, QRect, QSize, QByteArray, QBuffer, QThreadPool
from PyQt5.QtGui import QImage, QPixmap, QCursor

from app.utils.xml_builder import parse_steps_xml, html_to_text
from app.utils.worker import Worker
from app.utils import settings as settings_mod
from app.utils import theme

# UI label -> Azure DevOps outcome value
_OUTCOMES = [("Pass", "Passed"), ("Fail", "Failed"),
             ("Blocked", "Blocked"), ("N/A", "NotApplicable")]

# Max height for a single step row, so a very long Expected Result doesn't
# dominate the runner (full text stays available via the cell tooltip).
_STEP_ROW_MAX_H = 96


def _image_to_b64(img: QImage) -> str:
    """PNG-encode a QImage and base64 it for the attachments API."""
    ba = QByteArray()
    buf = QBuffer(ba)
    buf.open(QBuffer.WriteOnly)
    img.save(buf, "PNG")
    buf.close()
    return base64.b64encode(bytes(ba)).decode("ascii")


def _parse_step_ids(xml_str: str) -> list:
    """Step `id` attributes (as strings), parallel to parse_steps_xml's output, so
    per-step results can target each step. Returns [] on empty/bad input."""
    if not xml_str or not xml_str.strip():
        return []
    try:
        root = ET.fromstring(xml_str)
    except ET.ParseError:
        return []
    return [s.get("id", "") for s in root.findall("step")]


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
                "bug_ids": c.get("bug_ids", []),
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

    # Per-step (iteration) results — additive and best-effort, so a problem here
    # never undoes the overall outcomes already recorded above.
    for c in runnable:
        rid = result_by_tc.get(c["tc_id"])
        if rid is None or not c.get("step_results"):
            continue
        try:
            client.update_result_steps(run_id, rid, c["step_results"])
        except Exception:
            pass

    client.complete_test_run(run_id)
    return {"run_id": run_id, "web_url": run.get("web_url", ""),
            "submitted": len(runnable), "skipped": skipped}


def create_bug(client, type_info, title, repro, severity, tc_id, pbi_id,
               area, iteration, screenshots_b64):
    """Create a bug/issue work item from a failed test, linked (Related) to the
    test case and PBI, with the given screenshots attached. Returns {id, url}.
    Worker thread. POST only — no DELETE."""
    import html as _html
    repro_html = "<div>" + _html.escape(repro).replace("\n", "<br>") + "</div>"
    fields = {"System.Title": title, type_info["repro_field"]: repro_html}
    if area:
        fields["System.AreaPath"] = area
    if iteration:
        fields["System.IterationPath"] = iteration
    if severity and type_info.get("has_severity"):
        fields["Microsoft.VSTS.Common.Severity"] = severity
    relations = []
    if tc_id:
        relations.append({"rel": "System.LinkTypes.Related",
                          "url": client.work_item_url(tc_id),
                          "attributes": {"comment": "Failing test case"}})
    if pbi_id:
        relations.append({"rel": "System.LinkTypes.Related",
                          "url": client.work_item_url(pbi_id),
                          "attributes": {"comment": "Backlog item under test"}})
    for i, b64 in enumerate(screenshots_b64, 1):
        try:
            url = client.add_workitem_attachment(base64.b64decode(b64),
                                                 f"tc{tc_id}_shot{i}.png")
            if url:
                relations.append({"rel": "AttachedFile", "url": url,
                                  "attributes": {"name": f"tc{tc_id}_shot{i}.png"}})
        except Exception:
            pass
    return client.create_work_item(type_info["type"], fields, relations)


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


class _CreateBugDialog(QDialog):
    """Prefilled dialog for filing a bug/issue from a failed test."""

    _SEVERITIES = ["1 - Critical", "2 - High", "3 - Medium", "4 - Low"]

    def __init__(self, parent, title, repro, has_severity, n_shots):
        super().__init__(parent)
        self.setWindowTitle("Create Bug")
        self.setWindowFlags(self.windowFlags() | Qt.WindowStaysOnTopHint)
        self.resize(480, 460)
        lay = QVBoxLayout(self)
        lay.setSpacing(6)
        lay.addWidget(QLabel("<b>Title</b>"))
        self._title_edit = QLineEdit(title)
        lay.addWidget(self._title_edit)
        lay.addWidget(QLabel("<b>Repro steps</b>" if has_severity else "<b>Description</b>"))
        self._repro_edit = QPlainTextEdit(repro)
        self._repro_edit.setMinimumHeight(180)
        lay.addWidget(self._repro_edit, 1)
        self._sev_combo = None
        if has_severity:
            lay.addWidget(QLabel("<b>Severity</b>"))
            self._sev_combo = QComboBox()
            self._sev_combo.addItems(self._SEVERITIES)
            self._sev_combo.setCurrentText("3 - Medium")
            lay.addWidget(self._sev_combo)
        self._attach_cb = QCheckBox(f"Attach this test's screenshots ({n_shots})")
        self._attach_cb.setChecked(n_shots > 0)
        self._attach_cb.setEnabled(n_shots > 0)
        lay.addWidget(self._attach_cb)
        self._link_cb = QCheckBox("Link the bug to this test result")
        self._link_cb.setChecked(True)
        lay.addWidget(self._link_cb)
        btn_row = QHBoxLayout()
        btn_row.addStretch()
        cancel = QPushButton("Cancel")
        cancel.setStyleSheet(theme.btn_neutral_qss())
        cancel.setCursor(QCursor(Qt.PointingHandCursor))
        cancel.clicked.connect(self.reject)
        btn_row.addWidget(cancel)
        create = QPushButton("Create Bug")
        create.setStyleSheet(theme.btn_primary_qss("padding: 6px 14px;"))
        create.setCursor(QCursor(Qt.PointingHandCursor))
        create.clicked.connect(self.accept)
        btn_row.addWidget(create)
        lay.addLayout(btn_row)

    def values(self) -> dict:
        return {
            "title": self._title_edit.text().strip(),
            "repro": self._repro_edit.toPlainText(),
            "severity": self._sev_combo.currentText() if self._sev_combo else None,
            "attach": self._attach_cb.isChecked(),
            "link": self._link_cb.isChecked(),
        }


class TestRunner(QWidget):
    """Top-level always-on-top window that runs a session of test cases."""

    def __init__(self, app_state, cases: list, restore: dict = None):
        super().__init__()
        self.app_state = app_state
        self._ready = False           # gates autosave until construction completes
        self._dirty = bool(restore)   # whether there's session content worth saving
        self._suspend_dirty = False   # set while programmatically filling fields
        # Snapshot the PBI context at launch (or from the restored session) so that
        # changing the PBI elsewhere can never redirect this submission, and a
        # resumed run still targets the PBI it was started for.
        if restore:
            pbi = restore.get("pbi", {})
            self._pbi_id = pbi.get("id")
            self._pbi_title = pbi.get("title", "")
            self._plan_id = pbi.get("plan_id")
            self._suite_id = pbi.get("suite_id")
            self._area = pbi.get("area", "")
            self._iteration = pbi.get("iteration", "")
            self._preconditions_ref = pbi.get("preconditions_ref")
        else:
            self._pbi_id = app_state.pbi_id
            self._pbi_title = app_state.pbi_title
            self._plan_id = app_state.test_plan_id
            self._suite_id = app_state.suite_id
            self._area = app_state.area_path
            self._iteration = app_state.iteration_path
            self._preconditions_ref = app_state.preconditions_ref
        self.cases = cases
        # Per-case execution state, parallel to self.cases.
        if restore:
            self.state = self._rehydrate_state(restore, len(cases))
            self.idx = min(max(restore.get("idx", 0), 0), max(len(cases) - 1, 0))
        else:
            self.state = [self._blank_state() for _ in cases]
            self.idx = 0
        self._enter_monotonic = None
        # "Always on top" persists across sessions until the user changes it
        # (defaults ON for a brand-new install).
        from app.utils.settings import load_settings
        self._pinned = bool(load_settings().get("always_on_top", True))
        self._overlay = None

        self.setWindowTitle("Test Runner")
        self.setMinimumSize(480, 640)
        self.resize(540, 760)
        flags = self.windowFlags() | Qt.Window
        if self._pinned:
            flags |= Qt.WindowStaysOnTopHint
        self.setWindowFlags(flags)
        self._build_ui()
        self._load_case(self.idx)
        self._ready = True
        if not restore:
            self._start_preload()

    # ------------------------------------------------------------------ #
    #  Session persistence (resume an interrupted run)                    #
    # ------------------------------------------------------------------ #

    @staticmethod
    def _blank_state() -> dict:
        return {"outcome": "", "comment": "", "screenshots": [], "shot_files": [],
                "elapsed_ms": 0, "step_outcomes": {}, "last_outcome": "", "bug_ids": [],
                "_last_run_id": None, "_last_result_id": None, "_comment_fetched": False}

    def _rehydrate_state(self, restore: dict, n: int) -> list:
        from app.utils.settings import run_shots_dir
        shots_dir = run_shots_dir()
        saved = restore.get("state", [])
        out = []
        for i in range(n):
            s = saved[i] if i < len(saved) else {}
            st = self._blank_state()
            st["outcome"] = s.get("outcome", "")
            st["comment"] = s.get("comment", "")
            st["elapsed_ms"] = s.get("elapsed_ms", 0)
            st["last_outcome"] = s.get("last_outcome", "")
            st["_comment_fetched"] = True   # the saved comment is authoritative
            st["step_outcomes"] = {
                int(k): v for k, v in (s.get("step_outcomes") or {}).items()
            }
            st["bug_ids"] = list(s.get("bug_ids", []) or [])
            for fname in s.get("shot_files", []) or []:
                img = QImage(str(shots_dir / fname))
                if not img.isNull():
                    st["screenshots"].append(img)
                    st["shot_files"].append(fname)
            out.append(st)
        return out

    def _mark_dirty(self):
        self._dirty = True

    def _save_session(self):
        """Persist the session to disk so it survives an interrupted close."""
        if not self._ready:
            return
        # Capture the visible case's in-progress comment before serializing.
        self.state[self.idx]["comment"] = self._comment_edit.toPlainText()
        state_out = []
        for st in self.state:
            state_out.append({
                "outcome": st["outcome"],
                "comment": st["comment"],
                "elapsed_ms": st["elapsed_ms"],
                "last_outcome": st.get("last_outcome", ""),
                "step_outcomes": {str(k): v for k, v in st["step_outcomes"].items()},
                "shot_files": list(st.get("shot_files", [])),
                "bug_ids": list(st.get("bug_ids", [])),
            })
        data = {
            "version": 1,
            "saved_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "pbi": {
                "id": self._pbi_id, "title": self._pbi_title,
                "plan_id": self._plan_id, "suite_id": self._suite_id,
                "area": self._area, "iteration": self._iteration,
                "preconditions_ref": self._preconditions_ref,
            },
            "idx": self.idx,
            "cases": self.cases,
            "state": state_out,
        }
        try:
            from app.utils.settings import save_run_session
            save_run_session(data)
        except Exception:
            pass

    def _persist_shot(self, img: QImage) -> str:
        """Write a screenshot PNG to the session folder; return its filename."""
        import uuid
        from app.utils.settings import run_shots_dir
        fname = f"{uuid.uuid4().hex}.png"
        try:
            img.save(str(run_shots_dir() / fname), "PNG")
        except Exception:
            pass
        return fname

    @staticmethod
    def _delete_shot_file(fname: str):
        from app.utils.settings import run_shots_dir
        try:
            (run_shots_dir() / fname).unlink(missing_ok=True)
        except Exception:
            pass

    # ------------------------------------------------------------------ #
    #  UI                                                                 #
    # ------------------------------------------------------------------ #

    def _build_ui(self):
        t = theme.tokens()
        root = QVBoxLayout(self)
        root.setContentsMargins(12, 10, 12, 10)
        root.setSpacing(6)

        # Header: progress + pin
        hdr = QHBoxLayout()
        self._progress_lbl = QLabel("")
        self._progress_lbl.setStyleSheet("font-weight: bold; font-size: 13px;")
        hdr.addWidget(self._progress_lbl)
        hdr.addStretch()
        from app.utils import icons as _icons
        self._pin_btn = QPushButton()
        self._pin_btn.setIcon(_icons.icon("pin", size=14))
        self._pin_btn.setCheckable(True)
        self._pin_btn.setChecked(self._pinned)
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
        bl.setSpacing(6)

        bl.addWidget(self._section_label("Preconditions"))
        self._pre_lbl = QLabel("")
        self._pre_lbl.setWordWrap(True)
        self._pre_lbl.setTextFormat(Qt.PlainText)
        self._pre_lbl.setStyleSheet(
            f"background: {t['surface']}; border: 1px solid {t['border']}; "
            f"border-radius: 4px; padding: 8px; color: {t['text']};"
        )
        bl.addWidget(self._pre_lbl)

        steps_hdr = QHBoxLayout()
        steps_hdr.addWidget(self._section_label("Steps"))
        steps_hdr.addStretch()
        steps_hint = QLabel("mark ✓ / ✗ per step")
        steps_hint.setStyleSheet("color: #888; font-size: 11px;")
        steps_hdr.addWidget(steps_hint)
        bl.addLayout(steps_hdr)
        self._steps_tbl = QTableWidget(0, 4)
        self._steps_tbl.setHorizontalHeaderLabels(["#", "Action", "Expected Result", "Result"])
        hh = self._steps_tbl.horizontalHeader()
        hh.setSectionResizeMode(0, QHeaderView.Fixed)
        hh.setSectionResizeMode(1, QHeaderView.Stretch)
        hh.setSectionResizeMode(2, QHeaderView.Stretch)
        hh.setSectionResizeMode(3, QHeaderView.Fixed)
        self._steps_tbl.setColumnWidth(0, 30)
        self._steps_tbl.setColumnWidth(3, 92)
        self._steps_tbl.verticalHeader().setVisible(False)
        self._steps_tbl.setEditTriggers(QTableWidget.NoEditTriggers)
        self._steps_tbl.setWordWrap(True)
        self._steps_tbl.setMinimumHeight(120)
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
        self._comment_edit.textChanged.connect(self._on_comment_edited)
        bl.addWidget(self._comment_edit)

        # Screenshots
        sshot_hdr = QHBoxLayout()
        sshot_hdr.addWidget(self._section_label("Screenshots"))
        sshot_hdr.addStretch()
        for label, ic, slot in (("Paste", "clipboard", self._paste_shot),
                                ("File", "folder", self._file_shot),
                                ("Capture", "camera", self._capture_shot)):
            b = QPushButton(label)
            b.setIcon(_icons.icon(ic, size=15))
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

        # Create-bug row (enabled only when the case is marked Failed)
        bug_row = QHBoxLayout()
        bug_row.addStretch()
        from app.utils import icons
        self._bug_btn = QPushButton("Create bug")
        self._bug_btn.setIcon(icons.icon("bug", size=15))
        self._bug_btn.setEnabled(False)
        self._bug_btn.setToolTip("File a linked Azure DevOps bug from this failed test")
        self._bug_btn.setStyleSheet(theme.btn_neutral_qss())
        self._bug_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._bug_btn.clicked.connect(self._on_create_bug)
        bug_row.addWidget(self._bug_btn)
        root.addLayout(bug_row)

        # Nav row: Prev (left corner)  ·  Submit (centre)  ·  Next (right corner)
        nav = QHBoxLayout()
        self._prev_btn = QPushButton("Prev")
        self._prev_btn.setIcon(icons.icon("arrow-left", size=15))
        self._prev_btn.setStyleSheet(theme.btn_neutral_qss())
        self._prev_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._prev_btn.clicked.connect(lambda: self._go(self.idx - 1))
        self._next_btn = QPushButton("Next")
        self._next_btn.setIcon(icons.icon("arrow-right", size=15))
        self._next_btn.setLayoutDirection(Qt.RightToLeft)
        self._next_btn.setStyleSheet(theme.btn_neutral_qss())
        self._next_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._next_btn.clicked.connect(lambda: self._go(self.idx + 1))
        self._submit_btn = QPushButton("Submit results")
        self._submit_btn.setIcon(icons.icon("check", color="white", size=15))
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
        if self._dirty:
            self._save_session()

    def _load_case(self, idx: int):
        self.idx = idx
        case = self.cases[idx]
        tc_id = case.get("_id", "?")
        self._progress_lbl.setText(f"{idx + 1} of {len(self.cases)}")
        self._title_lbl.setText(f"#{tc_id}  —  {case.get('System.Title', '(no title)')}")

        pre_ref = self._preconditions_ref
        pre = html_to_text(case.get(pre_ref, "") if pre_ref else "")
        self._pre_lbl.setText(pre if pre else "(none)")

        self._steps_tbl.setRowCount(0)
        steps = parse_steps_xml(case.get("Microsoft.VSTS.TCM.Steps", "") or "")
        for i, step in enumerate(steps):
            r = self._steps_tbl.rowCount()
            self._steps_tbl.insertRow(r)
            num = QTableWidgetItem(str(i + 1))
            num.setTextAlignment(Qt.AlignCenter | Qt.AlignTop)
            self._steps_tbl.setItem(r, 0, num)
            for col, txt in ((1, step.action), (2, step.expected)):
                item = QTableWidgetItem(txt)
                item.setTextAlignment(Qt.AlignLeft | Qt.AlignTop)
                item.setToolTip(txt)  # full text on hover when a long row is capped
                self._steps_tbl.setItem(r, col, item)
            self._steps_tbl.setCellWidget(r, 3, self._make_step_result_cell(i))
        self._steps_tbl.resizeRowsToContents()
        # Stop one long Expected Result from ballooning the row — cap it; the
        # full text remains readable via the cell tooltip.
        for r in range(self._steps_tbl.rowCount()):
            if self._steps_tbl.rowHeight(r) > _STEP_ROW_MAX_H:
                self._steps_tbl.setRowHeight(r, _STEP_ROW_MAX_H)

        st = self.state[idx]
        self._suspend_dirty = True
        self._comment_edit.setPlainText(st["comment"])
        self._suspend_dirty = False
        self._notes_edit.setPlainText(settings_mod.get_execution_note(tc_id))
        self._refresh_outcome_buttons(st["outcome"])
        self._last_lbl.setText(self._format_last(st.get("last_outcome", "")))
        self._refresh_shots()
        self._prev_btn.setEnabled(idx > 0)
        self._next_btn.setEnabled(idx < len(self.cases) - 1)
        self._enter_monotonic = time.monotonic()
        self._update_outcome_summary()
        self._maybe_fetch_comment(idx)   # lazy: only the case you actually view

    # ------------------------------------------------------------------ #
    #  Outcome + pin                                                      #
    # ------------------------------------------------------------------ #

    def _set_outcome(self, ado_value: str):
        self.state[self.idx]["outcome"] = ado_value
        self._refresh_outcome_buttons(ado_value)
        self._update_outcome_summary()
        self._mark_dirty()
        self._save_session()

    def _on_comment_edited(self):
        if not self._suspend_dirty:
            self._mark_dirty()

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
        self._bug_btn.setEnabled(selected == "Failed")

    def _update_outcome_summary(self):
        done = sum(1 for s in self.state if s["outcome"])
        self._status_lbl.setText(f"{done} of {len(self.cases)} marked")

    # ------------------------------------------------------------------ #
    #  Per-step pass/fail                                                 #
    # ------------------------------------------------------------------ #

    def _make_step_result_cell(self, step_idx: int) -> QWidget:
        """A ✓ / ✗ toggle pair for one step row, reflecting its saved outcome."""
        current = self.state[self.idx]["step_outcomes"].get(step_idx)
        w = QWidget()
        lay = QHBoxLayout(w)
        lay.setContentsMargins(2, 2, 2, 2)
        lay.setSpacing(4)
        for label, oc, color in (("✓", "Passed", "#4caf50"), ("✗", "Failed", "#e53935")):
            b = QPushButton(label)
            b.setCheckable(True)
            b.setFixedSize(36, 26)
            b.setCursor(QCursor(Qt.PointingHandCursor))
            on = (oc == current)
            b.setChecked(on)
            if on:
                b.setStyleSheet(
                    f"QPushButton {{ background: {color}; color: white; border: none; "
                    f"border-radius: 3px; font-weight: bold; }}"
                )
            else:
                b.setStyleSheet(theme.btn_neutral_qss())
            b.clicked.connect(lambda _c, si=step_idx, o=oc: self._set_step_outcome(si, o))
            lay.addWidget(b)
        return w

    def _set_step_outcome(self, step_idx: int, outcome: str):
        step_oc = self.state[self.idx]["step_outcomes"]
        if step_oc.get(step_idx) == outcome:
            step_oc.pop(step_idx, None)      # click the active result again to clear it
        else:
            step_oc[step_idx] = outcome
        # Re-render just this row's result cell, then derive the overall outcome.
        self._steps_tbl.setCellWidget(step_idx, 3, self._make_step_result_cell(step_idx))
        self._derive_overall()
        self._mark_dirty()
        self._save_session()

    def _derive_overall(self):
        """Drive the overall outcome from the step results: any failed step ->
        Failed; every step passed -> Passed; otherwise leave the overall as-is
        (Blocked / N/A stay manual)."""
        step_oc = self.state[self.idx]["step_outcomes"]
        if not step_oc:
            return
        if any(o == "Failed" for o in step_oc.values()):
            derived = "Failed"
        elif (self._steps_tbl.rowCount()
              and len(step_oc) == self._steps_tbl.rowCount()
              and all(o == "Passed" for o in step_oc.values())):
            derived = "Passed"
        else:
            derived = None
        if derived:
            self.state[self.idx]["outcome"] = derived
            self._refresh_outcome_buttons(derived)
            self._update_outcome_summary()

    def _build_step_results(self, case, state):
        """Build the ADO iterationDetails payload from per-step outcomes, or None
        if no steps were individually marked."""
        step_oc = state.get("step_outcomes") or {}
        if not step_oc:
            return None
        step_ids = _parse_step_ids(case.get("Microsoft.VSTS.TCM.Steps", "") or "")
        action_results = []
        for idx, sid in enumerate(step_ids):
            oc = step_oc.get(idx)
            if not oc:
                continue
            action_results.append({
                "actionPath": format(int(sid), "08X") if str(sid).isdigit() else str(sid),
                "iterationId": 1,
                "stepIdentifier": str(sid),
                "outcome": oc,
            })
        if not action_results:
            return None
        return [{
            "id": 1,
            "outcome": state.get("outcome") or "Failed",
            "actionResults": action_results,
        }]

    # ------------------------------------------------------------------ #
    #  Pre-load existing results                                          #
    # ------------------------------------------------------------------ #

    _NORMALIZE = {"passed": "Passed", "failed": "Failed",
                  "blocked": "Blocked", "notapplicable": "NotApplicable"}

    def _start_preload(self):
        """Fetch the suite's test points once (one round-trip) to pre-fill every
        case's last *outcome* immediately. Comments are fetched lazily per case
        (see _maybe_fetch_comment) so the user never waits on N requests up front."""
        if not (self._plan_id and self._suite_id):
            return
        tc_ids = [c.get("_id") for c in self.cases if c.get("_id")]
        if not tc_ids:
            return
        # Instant path: the Run Tests tab pre-fetches the suite's points into a
        # session cache, so a warm cache means zero round-trips on runner open.
        cached = self.app_state.test_points_by_suite.get((self._plan_id, self._suite_id))
        if cached is not None:
            self._on_points(cached)
            return
        self._status_lbl.setText("Loading previous results…")
        worker = Worker(self.app_state.client.get_test_points,
                        self._plan_id, self._suite_id, tc_ids)
        worker.signals.result.connect(self._on_points)
        worker.signals.error.connect(lambda _exc: self._update_outcome_summary())
        QThreadPool.globalInstance().start(worker)

    def _on_points(self, points: list):
        """Apply last outcomes for all cases from the single points call, then kick
        off a lazy comment fetch for the case currently on screen."""
        point_by_tc = {}
        for p in points:
            tcid = p.get("test_case_id")
            if tcid and tcid not in point_by_tc:    # first (default config) wins
                point_by_tc[tcid] = p
        for i, case in enumerate(self.cases):
            p = point_by_tc.get(case.get("_id"))
            if not p:
                continue
            st = self.state[i]
            st["last_outcome"] = p.get("last_outcome", "")
            st["_last_run_id"] = p.get("last_run_id")
            st["_last_result_id"] = p.get("last_result_id")
            norm = self._NORMALIZE.get((p.get("last_outcome") or "").lower())
            if norm and not st["outcome"]:           # never clobber a user's mark
                st["outcome"] = norm
        # Outcomes are known now — re-render the visible case's outcome + badge.
        st = self.state[self.idx]
        self._refresh_outcome_buttons(st["outcome"])
        self._last_lbl.setText(self._format_last(st.get("last_outcome", "")))
        self._update_outcome_summary()
        self._maybe_fetch_comment(self.idx)          # visible case's comment first

    def _maybe_fetch_comment(self, idx: int):
        """Fetch one case's last comment in the background, at most once, on demand."""
        st = self.state[idx]
        if st.get("_comment_fetched") or st.get("comment"):
            return
        rid, run = st.get("_last_result_id"), st.get("_last_run_id")
        if not (rid and run):
            return
        st["_comment_fetched"] = True                # fetch at most once per case
        worker = Worker(self.app_state.client.get_result, run, rid)
        worker.signals.result.connect(lambda data, i=idx: self._on_comment(i, data))
        worker.signals.error.connect(lambda _exc: None)
        QThreadPool.globalInstance().start(worker)

    def _on_comment(self, idx: int, data: dict):
        comment = (data or {}).get("comment", "")
        if not comment:
            return
        st = self.state[idx]
        if st["comment"]:                            # already set (e.g. user typed)
            return
        if idx == self.idx:
            # Visible case: fill only if the user hasn't started typing.
            if self._comment_edit.toPlainText().strip():
                return
            st["comment"] = comment
            self._suspend_dirty = True
            self._comment_edit.setPlainText(comment)
            self._suspend_dirty = False
        else:
            st["comment"] = comment

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
        from app.utils.settings import save_settings
        save_settings({"always_on_top": self._pinned})

    def _update_pin_btn(self):
        self._pin_btn.setText("Always on top: ON" if self._pinned
                              else "Always on top: OFF")

    # ------------------------------------------------------------------ #
    #  Screenshots                                                        #
    # ------------------------------------------------------------------ #

    def _add_image(self, img: QImage):
        if img is None or img.isNull():
            return
        st = self.state[self.idx]
        st["screenshots"].append(img)
        st["shot_files"].append(self._persist_shot(img))
        self._refresh_shots()
        self._mark_dirty()
        self._save_session()

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
        st = self.state[self.idx]
        shots = st["screenshots"]
        if 0 <= i < len(shots):
            del shots[i]
            files = st.get("shot_files", [])
            if 0 <= i < len(files):
                self._delete_shot_file(files.pop(i))
            self._refresh_shots()
            self._mark_dirty()
            self._save_session()

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
    #  Create bug from a failed test                                      #
    # ------------------------------------------------------------------ #

    def _build_repro(self, case, st) -> str:
        lines = [f"Failed while executing Test Case #{case.get('_id', '?')}: "
                 f"{case.get('System.Title', '')}".rstrip(), ""]
        steps = parse_steps_xml(case.get("Microsoft.VSTS.TCM.Steps", "") or "")
        if steps:
            lines.append("Steps:")
            for i, s in enumerate(steps, 1):
                so = st.get("step_outcomes", {}).get(i - 1)
                mark = f"  [{so}]" if so else ""
                expected = f"  =>  {s.expected}" if s.expected else ""
                lines.append(f"  {i}. {s.action or ''}{expected}{mark}")
            lines.append("")
        if st.get("comment", "").strip():
            lines.append("Failure detail:")
            lines.append(st["comment"].strip())
        return "\n".join(lines).strip()

    def _on_create_bug(self):
        from app.gui.helpers import warn_if_token_expired
        if warn_if_token_expired(self, self.app_state.token_manager):
            return
        if self.state[self.idx]["outcome"] != "Failed":
            return
        # Resolve the bug work-item type off the UI thread, then show the dialog.
        self._bug_btn.setEnabled(False)
        self._bug_btn.setText("…")
        worker = Worker(self.app_state.client.detect_bug_type)
        worker.signals.result.connect(self._show_bug_dialog)
        worker.signals.error.connect(self._on_bug_error)
        QThreadPool.globalInstance().start(worker)

    def _show_bug_dialog(self, type_info: dict):
        self._bug_btn.setText("Create bug")
        idx = self.idx
        st = self.state[idx]
        case = self.cases[idx]
        st["comment"] = self._comment_edit.toPlainText()   # feed the repro
        title = f"[Bug] {case.get('System.Title', 'Test failed')}"[:255]
        dlg = _CreateBugDialog(self, title, self._build_repro(case, st),
                               type_info.get("has_severity", True),
                               len(st["screenshots"]))
        accepted = dlg.exec_() == QDialog.Accepted
        self._bug_btn.setEnabled(st["outcome"] == "Failed")
        if not accepted:
            return
        vals = dlg.values()
        if not vals["title"]:
            QMessageBox.information(self, "Title required", "Enter a title for the bug.")
            return
        shots = [_image_to_b64(i) for i in st["screenshots"]] if vals["attach"] else []
        self._bug_btn.setEnabled(False)
        self._bug_btn.setText("Creating bug…")
        worker = Worker(
            create_bug, self.app_state.client, type_info, vals["title"],
            vals["repro"], vals["severity"], case.get("_id"), self._pbi_id,
            self._area, self._iteration, shots,
        )
        worker.signals.result.connect(
            lambda res, i=idx, link=vals["link"]: self._on_bug_created(i, res, link))
        worker.signals.error.connect(self._on_bug_error)
        QThreadPool.globalInstance().start(worker)

    def _on_bug_created(self, idx: int, res: dict, link: bool):
        self._bug_btn.setText("Create bug")
        self._bug_btn.setEnabled(self.state[self.idx]["outcome"] == "Failed")
        bug_id = res.get("id")
        if bug_id and link:
            self.state[idx].setdefault("bug_ids", []).append(bug_id)
            self._mark_dirty()
            self._save_session()
        self._status_lbl.setText(f"Created bug #{bug_id}." if bug_id else "Bug created.")
        extra = f"\n\n{res.get('url')}" if res.get("url") else ""
        QMessageBox.information(
            self, "Bug Created",
            f"Created bug #{bug_id}, linked to the test case"
            + (" and this test result." if link else ".") + extra)

    def _on_bug_error(self, exc: Exception):
        self._bug_btn.setText("Create bug")
        self._bug_btn.setEnabled(self.state[self.idx]["outcome"] == "Failed")
        QMessageBox.critical(self, "Bug Not Created",
                             f"Could not create the bug:\n\n{exc}")

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
                "step_results": self._build_step_results(c, s),
                "bug_ids": list(s.get("bug_ids", [])),
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
        self._submit_btn.setText("Submit results")
        # The just-recorded outcomes make the cached points stale — drop them so
        # the next runner for this suite reflects the submitted results.
        self.app_state.test_points_by_suite.pop((self._plan_id, self._suite_id), None)
        # The run is recorded — the resumable session is complete; drop it.
        from app.utils.settings import clear_run_session
        clear_run_session()
        self._dirty = False
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
        self._submit_btn.setText("Submit results")
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
