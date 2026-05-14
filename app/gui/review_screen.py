from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton,
    QTreeWidget, QTreeWidgetItem, QFrame, QMessageBox, QSizePolicy
)
from PyQt5.QtCore import Qt, pyqtSignal
from PyQt5.QtGui import QFont, QColor, QBrush, QCursor


class ReviewScreen(QWidget):
    confirmed = pyqtSignal()
    back_requested = pyqtSignal()

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._build_ui()

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(40, 30, 40, 30)
        layout.setSpacing(14)

        title = QLabel("Review & Confirm")
        font = QFont()
        font.setPointSize(16)
        font.setBold(True)
        title.setFont(font)
        layout.addWidget(title)

        self.summary_label = QLabel("")
        self.summary_label.setWordWrap(True)
        layout.addWidget(self.summary_label)

        # Warning banner
        self._warn_frame = QFrame()
        self._warn_frame.setStyleSheet(
            "QFrame { background: #fff3cd; border: 1px solid #ffc107; border-radius: 6px; }"
        )
        warn_layout = QHBoxLayout(self._warn_frame)
        warn_layout.setContentsMargins(14, 10, 14, 10)
        warn_icon = QLabel("⚠️")
        warn_icon.setStyleSheet("font-size: 20px; border: none;")
        warn_layout.addWidget(warn_icon)
        self.warn_text = QLabel("")
        self.warn_text.setWordWrap(True)
        self.warn_text.setStyleSheet(
            "color: #856404; font-size: 13px; font-weight: bold; border: none;"
        )
        warn_layout.addWidget(self.warn_text, 1)
        layout.addWidget(self._warn_frame)

        # Tree view of all queued test cases
        self.tree = QTreeWidget()
        self.tree.setHeaderLabels(["Test Case / Step", "Details"])
        self.tree.setColumnWidth(0, 340)
        self.tree.setAlternatingRowColors(True)
        self.tree.setEditTriggers(QTreeWidget.NoEditTriggers)
        layout.addWidget(self.tree)

        # Module/config info
        self.config_label = QLabel("")
        self.config_label.setStyleSheet("color: #555; font-size: 11px;")
        layout.addWidget(self.config_label)

        # Buttons
        btn_row = QHBoxLayout()
        self.back_btn = QPushButton("← Back")
        self.back_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 7px 20px; font-size: 13px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self.back_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.back_btn.clicked.connect(self.back_requested)
        btn_row.addWidget(self.back_btn)
        btn_row.addStretch()

        self.create_btn = QPushButton("Create All Test Cases")
        self.create_btn.setFixedHeight(40)
        self.create_btn.setStyleSheet(
            "QPushButton { background: #c42b2b; color: white; border-radius: 4px; "
            "font-size: 14px; font-weight: bold; padding: 0 28px; }"
            "QPushButton:hover { background: #a82020; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self.create_btn.clicked.connect(self._on_create)
        btn_row.addWidget(self.create_btn)
        layout.addLayout(btn_row)

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._warn_frame.setStyleSheet(
            f"QFrame {{ background: {t['review_warn_bg']}; "
            f"border: 1px solid {t['review_warn_border']}; border-radius: 6px; }}"
        )
        self.warn_text.setStyleSheet(
            f"color: {t['review_warn_text']}; font-size: 13px; font-weight: bold; border: none;"
        )
        self.config_label.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        self.back_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 7px 20px; font-size: 13px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )

    def on_enter(self):
        """Refresh display when this screen becomes active."""
        queue = self.app_state.queue
        n = len(queue)

        self.summary_label.setText(
            f"You are about to create <b>{n} Test Case{'s' if n != 1 else ''}</b> "
            f"linked to PBI <b>#{self.app_state.pbi_id}</b>: "
            f"{self.app_state.pbi_title}"
        )
        self.warn_text.setText(
            f"This will create {n} Test Case work item{'s' if n != 1 else ''} in Azure DevOps. "
            "This action cannot be undone. Review carefully before clicking Create."
        )

        module_info = (
            f"Module field: {self.app_state.module_ref}"
            if self.app_state.module_ref
            else "Module field: not configured (will be skipped)"
        )
        self.config_label.setText(
            f"Organisation: {self.app_state.token_manager.org_url}  |  "
            f"Project: {self.app_state.token_manager.project}  |  {module_info}"
        )

        from app.utils import theme as _theme
        _t = _theme.tokens()
        title_color = QColor(_t["tree_title"])
        meta_color = QColor(_t["tree_meta"])

        self.tree.clear()
        for tc in queue:
            tc_item = QTreeWidgetItem(self.tree)
            step_summary = f"{len(tc.steps)} step{'s' if len(tc.steps) != 1 else ''}"
            tc_item.setText(0, tc.title)
            tc_item.setText(1, step_summary)
            tc_item.setForeground(0, QBrush(title_color))

            # Tags / status / module sub-row
            meta_item = QTreeWidgetItem(tc_item)
            meta_item.setText(0, "   Metadata")
            parts = [f"Status: {tc.automation_status}"]
            if tc.tags:
                parts.append(f"Tags: {tc.tags}")
            if tc.module_value:
                parts.append(f"Module: {tc.module_value}")
            meta_item.setText(1, "  |  ".join(parts))
            meta_item.setForeground(0, QBrush(meta_color))
            meta_item.setForeground(1, QBrush(meta_color))

            for i, step in enumerate(tc.steps):
                step_item = QTreeWidgetItem(tc_item)
                step_item.setText(0, f"   Step {i + 1}: {step.action[:60]}{'…' if len(step.action) > 60 else ''}")
                step_item.setText(1, step.expected[:80] if step.expected else "(no expected result)")
                step_item.setForeground(1, QBrush(meta_color))

            tc_item.setExpanded(True)

        self.create_btn.setEnabled(n > 0)

    def _on_create(self):
        n = len(self.app_state.queue)
        reply = QMessageBox.question(
            self,
            "Confirm Creation",
            f"Create {n} Test Case{'s' if n != 1 else ''} linked to PBI "
            f"#{self.app_state.pbi_id}?\n\nThis cannot be undone.",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No,
        )
        if reply == QMessageBox.Yes:
            self.confirmed.emit()
