import re

from PyQt5.QtCore import Qt, QTimer
from PyQt5.QtGui import QColor, QCursor, QFont
from PyQt5.QtWidgets import (
    QAbstractItemView,
    QCheckBox,
    QDialog,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QMessageBox,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
)

from app.gui import frameless


class PowerRenameDialog(frameless.FramelessMixin, QDialog):
    """PowerRename-style batch rename for test case titles.

    Shows a live Before/After preview table with per-row checkboxes.
    Supports full Python regex (or plain literal search) with optional
    case-insensitivity, and pushes approved renames to Azure DevOps.
    """

    def __init__(self, cases: list, client, parent=None):
        super().__init__(parent)
        self._cases = cases          # list of field-dicts, each with '_id' and 'System.Title'
        self._client = client
        self._updating_table = False  # guard against recursive itemChanged signals
        self._rename_queue = []
        self._rename_total = 0
        self._rename_done = 0
        self._rename_errors = []
        self._rename_running = False

        self._preview_timer = QTimer(self)
        self._preview_timer.setSingleShot(True)
        self._preview_timer.timeout.connect(self._update_preview)

        self.setWindowTitle("Rename Test Cases")
        self.setMinimumSize(860, 520)
        self.resize(960, 640)
        self._build_ui()
        self.init_frameless("Rename Test Cases", resizable=True,
                            show_min=False, show_max=False)
        from app.utils import theme
        theme.style_scrollbars(self)
        self._update_preview()

    # ------------------------------------------------------------------ #
    #  UI construction                                                     #
    # ------------------------------------------------------------------ #

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 20, 20, 16)
        layout.setSpacing(10)

        # Title
        title_lbl = QLabel("Rename Test Cases")
        f = QFont()
        f.setPointSize(14)
        f.setBold(True)
        title_lbl.setFont(f)
        layout.addWidget(title_lbl)

        self._hint_lbl = QLabel(
            "Enter a search pattern and a replacement. "
            "The preview table updates in real time. "
            "Check the rows you want to rename, then click Apply."
        )
        self._hint_lbl.setWordWrap(True)
        layout.addWidget(self._hint_lbl)

        # ── Pattern / replace inputs ──────────────────────────────────────
        inputs = QVBoxLayout()
        inputs.setSpacing(6)

        pr = QHBoxLayout()
        pr.setSpacing(8)
        search_lbl = QLabel("Search:")
        search_lbl.setFixedWidth(68)
        pr.addWidget(search_lbl)
        self._pattern_edit = QLineEdit()
        self._pattern_edit.setPlaceholderText("Regular expression pattern…")
        self._pattern_edit.textChanged.connect(self._schedule_preview)
        pr.addWidget(self._pattern_edit, 1)
        self._regex_chk = QCheckBox("Regex")
        self._regex_chk.setChecked(True)
        self._regex_chk.toggled.connect(self._schedule_preview)
        pr.addWidget(self._regex_chk)
        self._case_chk = QCheckBox("Case sensitive")
        self._case_chk.setChecked(False)
        self._case_chk.toggled.connect(self._schedule_preview)
        pr.addWidget(self._case_chk)
        inputs.addLayout(pr)

        rr = QHBoxLayout()
        rr.setSpacing(8)
        replace_lbl = QLabel("Replace:")
        replace_lbl.setFixedWidth(68)
        rr.addWidget(replace_lbl)
        self._replace_edit = QLineEdit()
        self._replace_edit.setPlaceholderText(
            "Replacement text  —  use \\1, \\2 … for captured groups in regex mode"
        )
        self._replace_edit.textChanged.connect(self._schedule_preview)
        rr.addWidget(self._replace_edit, 1)
        inputs.addLayout(rr)

        layout.addLayout(inputs)

        # Error / status row (styled in _refresh_footer_styles)
        self._error_lbl = QLabel("")
        layout.addWidget(self._error_lbl)

        # ── Preview table ─────────────────────────────────────────────────
        self._table = QTableWidget(len(self._cases), 4)
        self._table.setHorizontalHeaderLabels(["", "ID", "Current Name", "New Name"])
        hh = self._table.horizontalHeader()
        hh.setSectionResizeMode(0, QHeaderView.Fixed)
        hh.setSectionResizeMode(1, QHeaderView.Fixed)
        hh.setSectionResizeMode(2, QHeaderView.Stretch)
        hh.setSectionResizeMode(3, QHeaderView.Stretch)
        self._table.setColumnWidth(0, 36)
        self._table.setColumnWidth(1, 72)
        self._table.verticalHeader().setVisible(False)
        self._table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self._table.setSelectionMode(QAbstractItemView.NoSelection)
        self._table.setAlternatingRowColors(True)

        self._updating_table = True
        for row, tc in enumerate(self._cases):
            tc_id = tc.get("_id", "?")
            title = tc.get("System.Title", "(no title)")

            chk = QTableWidgetItem()
            chk.setFlags(Qt.ItemIsUserCheckable | Qt.ItemIsEnabled)
            chk.setCheckState(Qt.Checked)
            chk.setTextAlignment(Qt.AlignCenter)
            self._table.setItem(row, 0, chk)

            id_item = QTableWidgetItem(f"#{tc_id}")
            id_item.setTextAlignment(Qt.AlignCenter)
            self._table.setItem(row, 1, id_item)

            self._table.setItem(row, 2, QTableWidgetItem(title))
            self._table.setItem(row, 3, QTableWidgetItem(title))
        self._updating_table = False

        self._table.itemChanged.connect(self._on_item_changed)
        layout.addWidget(self._table, 1)

        # ── Footer ────────────────────────────────────────────────────────
        foot = QHBoxLayout()

        self._sel_all_btn = QPushButton("Select All")
        self._sel_all_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._sel_all_btn.clicked.connect(self._select_all)
        foot.addWidget(self._sel_all_btn)

        self._desel_all_btn = QPushButton("Deselect All")
        self._desel_all_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._desel_all_btn.clicked.connect(self._deselect_all)
        foot.addWidget(self._desel_all_btn)

        foot.addSpacing(12)

        self._match_lbl = QLabel("")
        self._match_lbl.setStyleSheet("color: #555; font-size: 11px;")
        foot.addWidget(self._match_lbl)

        foot.addStretch()

        self._cancel_btn = QPushButton("Cancel")
        self._cancel_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._cancel_btn.clicked.connect(self.reject)
        foot.addWidget(self._cancel_btn)

        self._apply_btn = QPushButton("Apply rename")
        self._apply_btn.setFixedHeight(36)
        self._apply_btn.setEnabled(False)
        self._apply_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._apply_btn.clicked.connect(self._apply_rename)
        foot.addWidget(self._apply_btn)

        layout.addLayout(foot)

        from app.utils import theme
        self._apply_btn_style_normal = theme.btn_primary_qss(
            "border-radius: 4px; font-size: 13px; padding: 0 20px;"
        )
        self._apply_btn.setStyleSheet(self._apply_btn_style_normal)
        self._refresh_footer_styles()

    # ------------------------------------------------------------------ #
    #  Preview logic                                                       #
    # ------------------------------------------------------------------ #

    def _schedule_preview(self):
        self._preview_timer.start(200)

    def _compute_new_name(self, title: str) -> tuple[str, bool]:
        """Return (new_title, valid).  valid=False when the regex is broken."""
        pattern = self._pattern_edit.text()
        replacement = self._replace_edit.text()
        use_regex = self._regex_chk.isChecked()
        flags = 0 if self._case_chk.isChecked() else re.IGNORECASE

        if not pattern:
            return title, True
        try:
            if use_regex:
                new_title = re.sub(pattern, replacement, title, flags=flags)
            else:
                # Literal mode: treat replacement as a plain string (no group refs)
                new_title = re.sub(
                    re.escape(pattern),
                    lambda _: replacement,
                    title,
                    flags=flags,
                )
            return new_title, True
        except re.error:
            return title, False

    def _update_preview(self):
        from app.utils import theme
        dark = theme.is_dark()
        changed_bg = QColor("#1a3a1a") if dark else QColor("#d4edda")
        changed_fg = QColor("#90ee90") if dark else QColor("#155724")
        default_bg = QColor()
        default_fg = QColor()

        # Validate regex before computing anything
        pattern = self._pattern_edit.text()
        if self._regex_chk.isChecked() and pattern:
            try:
                flags = 0 if self._case_chk.isChecked() else re.IGNORECASE
                re.compile(pattern, flags)
                self._error_lbl.setText("")
            except re.error as exc:
                self._error_lbl.setText(f"Invalid regex: {exc}")
                self._updating_table = True
                for row in range(self._table.rowCount()):
                    orig = self._table.item(row, 2).text()
                    item3 = self._table.item(row, 3)
                    item3.setText(orig)
                    item3.setBackground(default_bg)
                    item3.setForeground(default_fg)
                self._updating_table = False
                self._apply_btn.setEnabled(False)
                self._match_lbl.setText("")
                return
        else:
            self._error_lbl.setText("")

        match_count = 0
        self._updating_table = True
        try:
            for row in range(self._table.rowCount()):
                orig = self._table.item(row, 2).text()
                new_name, _ = self._compute_new_name(orig)
                item3 = self._table.item(row, 3)
                item3.setText(new_name)
                if new_name != orig:
                    item3.setBackground(changed_bg)
                    item3.setForeground(changed_fg)
                    match_count += 1
                else:
                    item3.setBackground(default_bg)
                    item3.setForeground(default_fg)
        finally:
            self._updating_table = False

        self._match_lbl.setText(
            f"{match_count} match{'es' if match_count != 1 else ''}"
        )
        self._update_apply_btn_state()

    def _update_apply_btn_state(self):
        enabled = any(
            self._table.item(r, 0).checkState() == Qt.Checked
            and self._table.item(r, 2).text() != self._table.item(r, 3).text()
            and self._table.item(r, 3).text().strip()
            for r in range(self._table.rowCount())
        )
        self._apply_btn.setEnabled(enabled)

    def _on_item_changed(self, item):
        if not self._updating_table and item.column() == 0:
            self._update_apply_btn_state()

    # ------------------------------------------------------------------ #
    #  Selection helpers                                                   #
    # ------------------------------------------------------------------ #

    def _select_all(self):
        self._updating_table = True
        for row in range(self._table.rowCount()):
            self._table.item(row, 0).setCheckState(Qt.Checked)
        self._updating_table = False
        self._update_apply_btn_state()

    def _deselect_all(self):
        self._updating_table = True
        for row in range(self._table.rowCount()):
            self._table.item(row, 0).setCheckState(Qt.Unchecked)
        self._updating_table = False
        self._update_apply_btn_state()

    # ------------------------------------------------------------------ #
    #  Apply rename                                                        #
    # ------------------------------------------------------------------ #

    def _apply_rename(self):
        to_rename = [
            (row, self._cases[row], self._table.item(row, 3).text())
            for row in range(self._table.rowCount())
            if self._table.item(row, 0).checkState() == Qt.Checked
            and self._table.item(row, 2).text() != self._table.item(row, 3).text()
            and self._table.item(row, 3).text().strip()
        ]

        if not to_rename:
            return

        n = len(to_rename)
        reply = QMessageBox.question(
            self,
            "Confirm Rename",
            f"Rename {n} test case{'s' if n != 1 else ''} in Azure DevOps?\n"
            "This cannot be undone.",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No,
        )
        if reply != QMessageBox.Yes:
            return

        self._apply_btn.setEnabled(False)
        self._apply_btn.setText("Renaming…")
        self._cancel_btn.setEnabled(False)

        # Renames run serially on the thread pool so the dialog stays responsive
        # and progress can be reported per item.
        self._rename_queue = list(to_rename)
        self._rename_total = n
        self._rename_done = 0
        self._rename_errors = []
        self._rename_running = True
        self._match_lbl.setText(f"Renaming 0 / {self._rename_total}…")
        self._process_next_rename()

    def _process_next_rename(self):
        if not self._rename_queue:
            self._on_rename_complete()
            return
        from PyQt5.QtCore import QThreadPool
        from app.utils.worker import Worker
        row, tc, new_name = self._rename_queue.pop(0)
        tc_id = tc.get("_id")
        worker = Worker(self._client.update_test_case_fields, tc_id, {"System.Title": new_name})
        worker.signals.result.connect(
            lambda _, r=row, t=tc, nm=new_name: self._on_rename_item_done(r, t, nm)
        )
        worker.signals.error.connect(
            lambda exc, _id=tc_id: self._on_rename_item_error(_id, exc)
        )
        QThreadPool.globalInstance().start(worker)

    def _on_rename_item_done(self, row: int, tc: dict, new_name: str):
        tc["System.Title"] = new_name
        # Advance the "Current Name" column so the row looks settled
        self._updating_table = True
        self._table.item(row, 2).setText(new_name)
        self._table.item(row, 3).setBackground(QColor())
        self._table.item(row, 3).setForeground(QColor())
        self._updating_table = False
        self._rename_done += 1
        self._on_rename_progress()

    def _on_rename_item_error(self, tc_id, exc: Exception):
        self._rename_errors.append(f"#{tc_id}: {exc}")
        self._on_rename_progress()

    def _on_rename_progress(self):
        processed = self._rename_done + len(self._rename_errors)
        self._match_lbl.setText(f"Renaming {processed} / {self._rename_total}…")
        self._process_next_rename()

    def _on_rename_complete(self):
        self._rename_running = False
        self._apply_btn.setText("Apply rename")
        self._cancel_btn.setEnabled(True)
        self._update_apply_btn_state()

        if self._rename_errors:
            QMessageBox.warning(
                self,
                "Partial Rename",
                f"Renamed {self._rename_done} of {self._rename_total} test case(s).\n\nErrors:\n"
                + "\n".join(self._rename_errors),
            )
            self._match_lbl.setText(
                f"Renamed {self._rename_done} of {self._rename_total} — see errors above."
            )
        else:
            QMessageBox.information(
                self,
                "Renamed",
                f"Successfully renamed {self._rename_done} test case{'s' if self._rename_done != 1 else ''}.",
            )
            self.accept()

    def reject(self):
        # Don't allow the dialog to close while renames are in flight —
        # worker callbacks would touch a destroyed table.
        if self._rename_running:
            return
        super().reject()

    # ------------------------------------------------------------------ #
    #  Theme                                                               #
    # ------------------------------------------------------------------ #

    def refresh_theme(self):
        self._refresh_footer_styles()
        self._update_preview()

    def _refresh_footer_styles(self):
        from app.utils import theme
        t = theme.tokens()
        btn_style = (
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 6px 14px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
        self._sel_all_btn.setStyleSheet(btn_style)
        self._desel_all_btn.setStyleSheet(btn_style)
        self._cancel_btn.setStyleSheet(btn_style)
        self._match_lbl.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        self._hint_lbl.setStyleSheet(f"color: {t['text_dim']}; font-size: 12px;")
        self._error_lbl.setStyleSheet(f"color: {t['error']}; font-size: 11px; padding: 0 2px;")
