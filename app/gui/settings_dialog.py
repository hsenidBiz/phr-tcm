"""Application settings dialog, opened from the gear icon in the title bar.

Exposes which main-page tabs are visible and a demo-mode toggle. Each tab can be
toggled on or off; at least one must stay enabled. The chosen values are returned
to the caller (MainWindow) which applies and persists them."""
from PyQt5.QtCore import Qt
from PyQt5.QtWidgets import (
    QLabel, QHBoxLayout, QFrame, QCheckBox, QPushButton, QMessageBox,
)
from PyQt5.QtGui import QCursor

from app.gui import frameless
from app.utils import theme


class SettingsDialog(frameless.FramelessDialog):
    """Toggle which main tabs are shown and whether demo mode is on. `tab_defs`
    is a list of (key, widget, label); `visible_keys` is the currently-enabled
    set; `demo_on` is the current demo-mode state. On accept, `result_keys` holds
    the new visible-tab set and `demo_enabled` the new demo state (both None if
    cancelled)."""

    def __init__(self, parent, tab_defs, visible_keys, demo_on):
        super().__init__(parent, "Settings", resizable=False)
        self.result_keys = None
        self.demo_enabled = None
        self._checks = {}

        lay = self.content_layout
        lay.setSpacing(10)
        lay.setContentsMargins(18, 14, 18, 16)

        t = theme.tokens()
        heading = QLabel("Visible tabs")
        heading.setStyleSheet(f"color: {t['text']}; font-size: 14px; font-weight: 600;")
        lay.addWidget(heading)
        sub = QLabel("Choose which tabs appear on the main screen.")
        sub.setStyleSheet(f"color: {t['text_dim']}; font-size: 12px;")
        lay.addWidget(sub)

        for key, _widget, label in tab_defs:
            cb = QCheckBox(label)
            cb.setChecked(key in visible_keys)
            cb.setCursor(QCursor(Qt.PointingHandCursor))
            cb.setStyleSheet(f"QCheckBox {{ color: {t['text']}; font-size: 13px; padding: 2px 0; }}")
            self._checks[key] = cb
            lay.addWidget(cb)

        # Divider + demo mode ------------------------------------------------
        line = QFrame()
        line.setFrameShape(QFrame.HLine)
        line.setStyleSheet(f"color: {t['border']};")
        lay.addWidget(line)

        demo_heading = QLabel("Demo mode")
        demo_heading.setStyleSheet(f"color: {t['text']}; font-size: 14px; font-weight: 600;")
        lay.addWidget(demo_heading)
        self._demo_check = QCheckBox("Enable demo mode (sample data, no sign-in)")
        self._demo_check.setChecked(bool(demo_on))
        self._demo_check.setCursor(QCursor(Qt.PointingHandCursor))
        self._demo_check.setStyleSheet(f"QCheckBox {{ color: {t['text']}; font-size: 13px; padding: 2px 0; }}")
        lay.addWidget(self._demo_check)
        demo_sub = QLabel(
            "Explore the app with fake test cases, suites and work items — nothing "
            "is sent to Azure DevOps. Turn off to sign in and use real data.")
        demo_sub.setWordWrap(True)
        demo_sub.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        lay.addWidget(demo_sub)

        btns = QHBoxLayout()
        btns.addStretch()
        cancel = QPushButton("Cancel")
        cancel.setStyleSheet(theme.btn_neutral_qss())
        cancel.setCursor(QCursor(Qt.PointingHandCursor))
        cancel.clicked.connect(self.reject)
        btns.addWidget(cancel)
        save = QPushButton("Save")
        save.setStyleSheet(theme.btn_primary_qss("padding: 6px 18px;"))
        save.setCursor(QCursor(Qt.PointingHandCursor))
        save.clicked.connect(self._on_save)
        btns.addWidget(save)
        lay.addLayout(btns)

        self.setMinimumWidth(360)
        self.finalize_frameless()

    def _on_save(self):
        keys = {k for k, cb in self._checks.items() if cb.isChecked()}
        if not keys:
            QMessageBox.information(
                self, "Settings", "At least one tab must stay enabled.")
            return
        self.result_keys = keys
        self.demo_enabled = self._demo_check.isChecked()
        self.accept()
