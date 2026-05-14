from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QComboBox, QTableWidget, QTableWidgetItem,
    QHeaderView, QMessageBox, QFrame, QSizePolicy
)
from PyQt5.QtCore import Qt, pyqtSignal
from PyQt5.QtGui import QFont, QCursor

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

        # Title
        title_row = QHBoxLayout()
        title_row.addWidget(QLabel("Title *"))
        self.title_edit = QLineEdit()
        self.title_edit.setPlaceholderText("e.g. Login with valid credentials")
        title_row.addWidget(self.title_edit)
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
        self.module_edit = QLineEdit()
        self.module_edit.setPlaceholderText("e.g. Authentication")
        meta_row.addWidget(self.module_edit)
        layout.addLayout(meta_row)

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

    def _add_step(self):
        row = self.steps_table.rowCount()
        self.steps_table.insertRow(row)
        self.steps_table.setItem(row, 0, QTableWidgetItem(""))
        self.steps_table.setItem(row, 1, QTableWidgetItem(""))
        # Update row header to show step number
        self.steps_table.setVerticalHeaderItem(row, QTableWidgetItem(str(row + 1)))

    def _remove_last_step(self):
        if self.steps_table.rowCount() > 1:
            self.steps_table.removeRow(self.steps_table.rowCount() - 1)

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
            module_value=self.module_edit.text().strip(),
        )
        self.test_case_queued.emit(tc)
        self._clear_form()

    def _clear_form(self):
        self.title_edit.clear()
        self.tags_edit.clear()
        self.module_edit.clear()
        self.status_combo.setCurrentIndex(0)
        self.steps_table.setRowCount(0)
        self._add_step()
