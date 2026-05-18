from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QComboBox, QTableWidget, QTableWidgetItem,
    QHeaderView, QMessageBox, QFrame, QSizePolicy, QAbstractItemView, QShortcut
)
from PyQt5.QtCore import Qt, pyqtSignal
from PyQt5.QtGui import QFont, QCursor, QKeySequence

from app.models.test_case import TestCase, Step


class ManualEntryWidget(QWidget):
    """Tab widget for manually building and queuing a single test case."""

    test_case_queued = pyqtSignal(object)  # emits TestCase

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._build_ui()

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(12)

        # Title row (with Templates button)
        title_row = QHBoxLayout()
        title_row.addWidget(QLabel("Title *"))
        self.title_edit = QLineEdit()
        self.title_edit.setPlaceholderText("e.g. Login with valid credentials")
        title_row.addWidget(self.title_edit)
        self._tmpl_btn = QPushButton("📄 Templates…")
        self._tmpl_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 5px 10px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
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
        meta_row.addSpacing(20)

        meta_row.addWidget(QLabel("Module"))
        self.module_edit = QComboBox()
        self.module_edit.setEditable(True)
        self.module_edit.setInsertPolicy(QComboBox.NoInsert)
        self.module_edit.lineEdit().setPlaceholderText("e.g. Authentication")
        meta_row.addWidget(self.module_edit)
        layout.addLayout(meta_row)

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

        self._add_step_btn = QPushButton("+ Add Step")
        self._add_step_btn.setStyleSheet(
            "QPushButton { background: #e8f4e8; border: 1px solid #8bc48b; "
            "border-radius: 3px; padding: 3px 10px; }"
            "QPushButton:hover { background: #d0ebd0; }"
        )
        self._add_step_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._add_step_btn.clicked.connect(self._add_step)
        steps_header.addWidget(self._add_step_btn)

        self._remove_step_btn = QPushButton("- Remove Last")
        self._remove_step_btn.setStyleSheet(
            "QPushButton { background: #fde8e8; border: 1px solid #e88b8b; "
            "border-radius: 3px; padding: 3px 10px; }"
            "QPushButton:hover { background: #f8d0d0; }"
        )
        self._remove_step_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._remove_step_btn.clicked.connect(self._remove_last_step)
        steps_header.addWidget(self._remove_step_btn)
        layout.addLayout(steps_header)

        self.steps_table = QTableWidget(0, 2)
        self.steps_table.setHorizontalHeaderLabels(["Action *", "Expected Result"])
        self.steps_table.horizontalHeader().setSectionResizeMode(0, QHeaderView.Stretch)
        self.steps_table.horizontalHeader().setSectionResizeMode(1, QHeaderView.Stretch)
        self.steps_table.verticalHeader().setDefaultSectionSize(32)
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

        self.clear_btn = QPushButton("Clear Form")
        self.clear_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 6px 16px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self.clear_btn.clicked.connect(self._clear_form)
        bottom_row.addWidget(self.clear_btn)
        bottom_row.addSpacing(10)

        self.queue_btn = QPushButton("Add to Queue")
        self.queue_btn.setFixedHeight(34)
        self.queue_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; "
            "font-size: 13px; padding: 0 20px; }"
            "QPushButton:hover { background: #106ebe; }"
        )
        self.queue_btn.clicked.connect(self._on_queue)
        bottom_row.addWidget(self.queue_btn)
        layout.addLayout(bottom_row)

        # Keyboard shortcut: Ctrl+Return = Add to Queue
        QShortcut(QKeySequence("Ctrl+Return"), self).activated.connect(self._on_queue)

    def showEvent(self, event):
        super().showEvent(event)
        self._refresh_module_combo()

    def _refresh_module_combo(self):
        vals = self.app_state.known_module_values
        cur = self.module_edit.currentText()
        self.module_edit.blockSignals(True)
        self.module_edit.clear()
        for v in vals:
            self.module_edit.addItem(v)
        self.module_edit.setCurrentText(cur)
        self.module_edit.blockSignals(False)

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._add_step_btn.setStyleSheet(
            f"QPushButton {{ background: {t['green_btn_bg']}; border: 1px solid {t['green_btn_border']}; "
            f"border-radius: 3px; padding: 3px 10px; }}"
            f"QPushButton:hover {{ background: {t['green_btn_hover']}; }}"
        )
        self._remove_step_btn.setStyleSheet(
            f"QPushButton {{ background: {t['red_btn_bg']}; border: 1px solid {t['red_btn_border']}; "
            f"border-radius: 3px; padding: 3px 10px; }}"
            f"QPushButton:hover {{ background: {t['red_btn_hover']}; }}"
        )
        self.clear_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 6px 16px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
        self._tmpl_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 5px 10px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )

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

        tc = TestCase(
            title=title,
            steps=steps,
            tags=self.tags_edit.text().strip(),
            automation_status=self.status_combo.currentText(),
            module_value=self.module_edit.currentText().strip(),
            preconditions=self.preconditions_edit.text().strip(),
        )
        self.test_case_queued.emit(tc)
        self._clear_form()

    def _clear_form(self):
        self.title_edit.clear()
        self.tags_edit.clear()
        self.module_edit.setCurrentText("")
        self.preconditions_edit.clear()
        self.status_combo.setCurrentIndex(0)
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

        self.steps_table.setRowCount(0)
        for step_data in tmpl.get("steps", []):
            row = self.steps_table.rowCount()
            self.steps_table.insertRow(row)
            self.steps_table.setItem(row, 0, QTableWidgetItem(step_data.get("action", "")))
            self.steps_table.setItem(row, 1, QTableWidgetItem(step_data.get("expected", "")))
            self.steps_table.setVerticalHeaderItem(row, QTableWidgetItem(str(row + 1)))
        if not self.steps_table.rowCount():
            self._add_step()
