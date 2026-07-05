from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QComboBox, QTableWidgetItem,
    QHeaderView, QMessageBox, QAbstractItemView, QShortcut
)
from PyQt5.QtCore import Qt, pyqtSignal
from PyQt5.QtGui import QCursor, QKeySequence

from app.models.test_case import TestCase, Step
from app.gui.steps_table import StepsTable


class ManualEntryWidget(QWidget):
    """Tab widget for manually building and queuing a single test case."""

    test_case_queued = pyqtSignal(object)  # emits TestCase

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._build_ui()

    def _build_ui(self):
        from app.utils import theme, icons
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(12)

        # Title row (with Templates button)
        title_row = QHBoxLayout()
        title_row.addWidget(QLabel("Title *"))
        self.title_edit = QLineEdit()
        self.title_edit.setPlaceholderText("e.g. Login with valid credentials")
        title_row.addWidget(self.title_edit)
        self._tmpl_btn = QPushButton("Templates")
        self._tmpl_btn.setIcon(icons.icon("file-text", size=16))
        self._tmpl_btn.setStyleSheet(theme.btn_neutral_qss("padding: 5px 12px;"))
        self._tmpl_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._tmpl_btn.clicked.connect(self._open_templates)
        title_row.addWidget(self._tmpl_btn)
        layout.addLayout(title_row)

        # Metadata row
        meta_row = QHBoxLayout()
        meta_row.addWidget(QLabel("Automation Status"))
        self.status_combo = QComboBox()
        self.status_combo.addItems(["Not Automated", "Planned"])
        meta_row.addWidget(self.status_combo)
        meta_row.addSpacing(20)

        meta_row.addWidget(QLabel("Tags"))
        self.tags_edit = QLineEdit()
        self.tags_edit.setPlaceholderText("smoke; regression (semicolon-separated)")
        meta_row.addWidget(self.tags_edit)
        layout.addLayout(meta_row)

        # Metadata row 2 (Module and Created By)
        meta_row2 = QHBoxLayout()
        meta_row2.addWidget(QLabel("Module"))
        from app.gui.helpers import make_module_combo
        self.module_edit = make_module_combo()
        meta_row2.addWidget(self.module_edit)
        meta_row2.addSpacing(20)

        meta_row2.addWidget(QLabel("Created By"))
        self.created_by_combo = QComboBox()
        self.created_by_combo.setPlaceholderText("Select a user")
        self.created_by_combo.addItem("(Current User)")
        meta_row2.addWidget(self.created_by_combo)
        layout.addLayout(meta_row2)

        # Preconditions row
        pre_row = QHBoxLayout()
        pre_row.addWidget(QLabel("Preconditions"))
        self.preconditions_edit = QLineEdit()
        self.preconditions_edit.setPlaceholderText(
            "e.g. User is logged in as HR Admin  (leave blank to skip)"
        )
        pre_row.addWidget(self.preconditions_edit)
        layout.addLayout(pre_row)

        # Steps table
        steps_header = QHBoxLayout()
        steps_label = QLabel("Steps")
        steps_label.setStyleSheet("font-weight: bold;")
        steps_header.addWidget(steps_label)
        steps_header.addStretch()

        self._add_step_btn = QPushButton("Add step")
        self._add_step_btn.setIcon(icons.icon("plus", size=15))
        self._add_step_btn.setStyleSheet(theme.btn_neutral_qss("padding: 4px 10px;"))
        self._add_step_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._add_step_btn.clicked.connect(self._add_step)
        steps_header.addWidget(self._add_step_btn)

        self._remove_step_btn = QPushButton("Remove last")
        self._remove_step_btn.setIcon(icons.icon("minus", size=15))
        self._remove_step_btn.setStyleSheet(theme.btn_ghost_qss("padding: 4px 10px;"))
        self._remove_step_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._remove_step_btn.clicked.connect(self._remove_last_step)
        steps_header.addWidget(self._remove_step_btn)
        layout.addLayout(steps_header)

        self.steps_table = StepsTable(0, 2, action_col=0, expected_col=1)
        self.steps_table.request_add_row.connect(self._add_step)
        self.steps_table.request_paste.connect(self._on_paste_steps)
        self.steps_table.request_duplicate.connect(self._on_duplicate_step)
        self.steps_table.setToolTip(
            "Enter: next step (adds a row at the end) · Ctrl+V: paste rows · "
            "Ctrl+D: duplicate step · drag to reorder")
        self.steps_table.setHorizontalHeaderLabels(["Action *", "Expected Result"])
        self.steps_table.horizontalHeader().setSectionResizeMode(0, QHeaderView.Stretch)
        self.steps_table.horizontalHeader().setSectionResizeMode(1, QHeaderView.Stretch)
        self.steps_table.verticalHeader().setDefaultSectionSize(32)
        self.steps_table.setAlternatingRowColors(True)
        theme.style_item_view(self.steps_table)   # modern flat header, no grid/frame
        self.steps_table.setMinimumHeight(160)
        # Drag-and-drop row reordering
        self.steps_table.setDragEnabled(True)
        self.steps_table.setAcceptDrops(True)
        self.steps_table.setDragDropMode(QAbstractItemView.InternalMove)
        self.steps_table.setDefaultDropAction(Qt.MoveAction)
        self.steps_table.model().rowsMoved.connect(self._renumber_steps_header)
        layout.addWidget(self.steps_table)

        # Add first empty row
        self._add_step()

        # Bottom buttons
        bottom_row = QHBoxLayout()
        bottom_row.addStretch()

        self.clear_btn = QPushButton("Clear form")
        self.clear_btn.setStyleSheet(theme.btn_neutral_qss("padding: 6px 16px;"))
        self.clear_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.clear_btn.clicked.connect(self._clear_form)
        bottom_row.addWidget(self.clear_btn)
        bottom_row.addSpacing(10)

        self.queue_btn = QPushButton("Add to queue")
        self.queue_btn.setIcon(icons.icon("plus", color="white", size=15))
        self.queue_btn.setFixedHeight(34)
        self.queue_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 13px; padding: 0 20px;")
        )
        self.queue_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.queue_btn.clicked.connect(self._on_queue)
        bottom_row.addWidget(self.queue_btn)
        layout.addLayout(bottom_row)

        # Keyboard shortcut: Ctrl+Return = Add to Queue (only while this tab is visible)
        queue_sc = QShortcut(QKeySequence("Ctrl+Return"), self)
        queue_sc.setContext(Qt.WidgetWithChildrenShortcut)
        queue_sc.activated.connect(self._on_queue)

    def showEvent(self, event):
        super().showEvent(event)
        self._refresh_module_combo()
        self._refresh_created_by_combo()

    def _refresh_module_combo(self):
        from app.gui.helpers import refresh_module_combo
        refresh_module_combo(self.module_edit, self.app_state.known_module_values)

    def _refresh_created_by_combo(self):
        from app.gui.helpers import refresh_team_members
        if not refresh_team_members(self.app_state, self._populate_created_by_combo,
                                    self._on_members_fetched, self._on_members_failed):
            # No cached members yet — show a placeholder until the fetch lands.
            self.created_by_combo.blockSignals(True)
            self.created_by_combo.clear()
            self.created_by_combo.addItem("Loading users…")
            self.created_by_combo.blockSignals(False)

    def _on_members_fetched(self, members: list):
        from app.gui.helpers import store_fetched_members
        store_fetched_members(self.app_state, members)
        self._populate_created_by_combo(members)

    def _on_members_failed(self, _msg: str):
        # Keep previously cached members; clear the fetcher so a later retry can happen.
        self.app_state._team_members_fetcher = None
        self._populate_created_by_combo(self.app_state.cached_team_members or [])

    def _populate_created_by_combo(self, members: list):
        cur = self.created_by_combo.currentText()
        self.created_by_combo.blockSignals(True)
        self.created_by_combo.clear()
        self.created_by_combo.addItem("(Current User)")
        for user in members:
            display = user.get("displayName", user.get("uniqueName", ""))
            unique = user.get("uniqueName", "")
            if display and unique:
                self.created_by_combo.addItem(display, unique)
        if cur and cur not in ("(Current User)", "Loading users…"):
            idx = self.created_by_combo.findText(cur)
            if idx >= 0:
                self.created_by_combo.setCurrentIndex(idx)
        self.created_by_combo.blockSignals(False)

    def refresh_theme(self):
        from app.utils import theme, icons
        theme.style_item_view(self.steps_table)   # re-tint the flat header
        self._add_step_btn.setStyleSheet(theme.btn_neutral_qss("padding: 4px 10px;"))
        self._add_step_btn.setIcon(icons.icon("plus", size=15))
        self._remove_step_btn.setStyleSheet(theme.btn_ghost_qss("padding: 4px 10px;"))
        self._remove_step_btn.setIcon(icons.icon("minus", size=15))
        self.clear_btn.setStyleSheet(theme.btn_neutral_qss("padding: 6px 16px;"))
        self._tmpl_btn.setStyleSheet(theme.btn_neutral_qss("padding: 5px 12px;"))
        self._tmpl_btn.setIcon(icons.icon("file-text", size=16))
        self.queue_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 13px; padding: 0 20px;")
        )
        self.queue_btn.setIcon(icons.icon("plus", color="white", size=15))

    def _add_step(self):
        row = self.steps_table.rowCount()
        self.steps_table.insertRow(row)
        self.steps_table.setItem(row, 0, QTableWidgetItem(""))
        self.steps_table.setItem(row, 1, QTableWidgetItem(""))
        self.steps_table.setVerticalHeaderItem(row, QTableWidgetItem(str(row + 1)))

    def _remove_last_step(self):
        if self.steps_table.rowCount() > 1:
            self.steps_table.removeRow(self.steps_table.rowCount() - 1)

    def _renumber_steps_header(self):
        """Renumber vertical header labels after drag-drop reorder."""
        for r in range(self.steps_table.rowCount()):
            self.steps_table.setVerticalHeaderItem(r, QTableWidgetItem(str(r + 1)))

    def _on_paste_steps(self, start_row: int, pairs: list):
        """Fill steps from `start_row` downward with pasted (action, expected)
        pairs — overwriting existing rows and appending new ones as needed, the
        way pasting a block into a spreadsheet behaves (Ctrl+V)."""
        tbl = self.steps_table
        r = max(start_row, 0)
        for action, expected in pairs:
            if r >= tbl.rowCount():
                row = tbl.rowCount()
                tbl.insertRow(row)
                tbl.setItem(row, 0, QTableWidgetItem(action))
                tbl.setItem(row, 1, QTableWidgetItem(expected))
            else:
                for c, val in ((0, action), (1, expected)):
                    it = tbl.item(r, c)
                    if it is None:
                        it = QTableWidgetItem()
                        tbl.setItem(r, c, it)
                    it.setText(val)
            r += 1
        self._renumber_steps_header()

    def _on_duplicate_step(self, row: int):
        """Insert a copy of `row` directly below it (Ctrl+D)."""
        tbl = self.steps_table
        if row < 0 or row >= tbl.rowCount():
            return
        a_item = tbl.item(row, 0)
        e_item = tbl.item(row, 1)
        action = a_item.text() if a_item else ""
        expected = e_item.text() if e_item else ""
        tbl.insertRow(row + 1)
        tbl.setItem(row + 1, 0, QTableWidgetItem(action))
        tbl.setItem(row + 1, 1, QTableWidgetItem(expected))
        self._renumber_steps_header()

    def _collect_steps(self) -> list:
        steps = []
        for row in range(self.steps_table.rowCount()):
            action_item = self.steps_table.item(row, 0)
            expected_item = self.steps_table.item(row, 1)
            action = action_item.text().strip() if action_item else ""
            expected = expected_item.text().strip() if expected_item else ""
            if action:
                steps.append(Step(action=action, expected=expected))
        return steps

    def _on_queue(self):
        title = self.title_edit.text().strip()
        if not title:
            QMessageBox.warning(self, "Missing Title", "Please enter a title for the test case.")
            return

        steps = self._collect_steps()
        if not steps:
            QMessageBox.warning(
                self, "No Steps",
                "Please add at least one step with a non-empty Action."
            )
            return

        created_by = ""
        cur_text = self.created_by_combo.currentText()
        if cur_text != "(Current User)":
            created_by = self.created_by_combo.currentData()

        tc = TestCase(
            title=title,
            steps=steps,
            tags=self.tags_edit.text().strip(),
            automation_status=self.status_combo.currentText(),
            module_value=self.module_edit.currentText().strip(),
            preconditions=self.preconditions_edit.text().strip(),
            created_by=created_by,
        )
        ok, err = tc.is_valid()
        if not ok:
            QMessageBox.warning(self, "Invalid Test Case", err)
            return
        # MainWindow clears the form via on_queue_accepted() only if the case
        # was actually added (the duplicate-title dialog may reject it).
        self.test_case_queued.emit(tc)

    def on_queue_accepted(self):
        """Called by MainWindow after the emitted case was added to the queue."""
        self._clear_form()

    def _clear_form(self):
        self.title_edit.clear()
        self.tags_edit.clear()
        self.module_edit.setCurrentText("")
        self.preconditions_edit.clear()
        self.status_combo.setCurrentIndex(0)
        self.created_by_combo.setCurrentIndex(0)
        self.steps_table.setRowCount(0)
        self._add_step()

    # ------------------------------------------------------------------ #
    #  Templates                                                           #
    # ------------------------------------------------------------------ #

    def _open_templates(self):
        from app.gui.template_dialog import TemplateDialog
        state = {
            "title": self.title_edit.text().strip(),
            "steps": [
                {
                    "action": (self.steps_table.item(r, 0).text().strip()
                               if self.steps_table.item(r, 0) else ""),
                    "expected": (self.steps_table.item(r, 1).text().strip()
                                 if self.steps_table.item(r, 1) else ""),
                }
                for r in range(self.steps_table.rowCount())
            ],
            "tags": self.tags_edit.text().strip(),
            "automation_status": self.status_combo.currentText(),
            "module_value": self.module_edit.currentText().strip(),
            "preconditions": self.preconditions_edit.text().strip(),
            "created_by": self.created_by_combo.currentData() if self.created_by_combo.currentText() != "(Current User)" else "",
        }
        dlg = TemplateDialog(current_form_state=state, parent=self)
        dlg.applied.connect(self._apply_template)
        dlg.exec_()

    def _apply_template(self, tmpl: dict):
        self.title_edit.setText(tmpl.get("title", ""))
        self.tags_edit.setText(tmpl.get("tags", ""))
        self.module_edit.setCurrentText(tmpl.get("module_value", ""))
        self.preconditions_edit.setText(tmpl.get("preconditions", ""))
        idx = self.status_combo.findText(tmpl.get("automation_status", "Not Automated"))
        self.status_combo.setCurrentIndex(max(idx, 0))

        created_by = tmpl.get("created_by", "")
        if created_by:
            idx = self.created_by_combo.findData(created_by)
            if idx >= 0:
                self.created_by_combo.setCurrentIndex(idx)
            else:
                self.created_by_combo.setCurrentIndex(0)
        else:
            self.created_by_combo.setCurrentIndex(0)

        self.steps_table.setRowCount(0)
        for step_data in tmpl.get("steps", []):
            row = self.steps_table.rowCount()
            self.steps_table.insertRow(row)
            self.steps_table.setItem(row, 0, QTableWidgetItem(step_data.get("action", "")))
            self.steps_table.setItem(row, 1, QTableWidgetItem(step_data.get("expected", "")))
            self.steps_table.setVerticalHeaderItem(row, QTableWidgetItem(str(row + 1)))
        if not self.steps_table.rowCount():
            self._add_step()
