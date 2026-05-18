import re

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QMessageBox, QFrame, QSizePolicy, QApplication
)
from PyQt5.QtCore import Qt, pyqtSignal, QTimer, QThreadPool
from PyQt5.QtGui import QFont, QCursor

from app.utils.settings import load_settings, save_settings
from app.utils.worker import Worker


class AuthScreen(QWidget):
    connected = pyqtSignal()  # emitted when connection is validated

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._build_ui()
        self._restore_settings()
        self._expiry_timer = QTimer(self)
        self._expiry_timer.setInterval(1000)
        self._expiry_timer.timeout.connect(self._refresh_expiry_display)
        QApplication.clipboard().dataChanged.connect(self._on_clipboard_changed)
        self._check_clipboard()

    def _build_ui(self):
        outer = QVBoxLayout(self)
        outer.setContentsMargins(60, 40, 60, 40)
        outer.setSpacing(0)

        # Title
        title = QLabel("Azure DevOps\nTest Case Creator")
        title.setAlignment(Qt.AlignCenter)
        title_font = QFont()
        title_font.setPointSize(20)
        title_font.setBold(True)
        title.setFont(title_font)
        outer.addWidget(title)
        outer.addSpacing(8)

        self._subtitle = QLabel("Connect to your Azure DevOps organisation to get started.")
        self._subtitle.setAlignment(Qt.AlignCenter)
        self._subtitle.setStyleSheet("color: #666;")
        outer.addWidget(self._subtitle)
        outer.addSpacing(28)

        # Card frame
        self._card = QFrame()
        self._card.setObjectName("authCard")
        self._card.setFrameShape(QFrame.NoFrame)
        self._card.setStyleSheet(
            "#authCard { background: #f9f9f9; border: 1px solid #ddd; border-radius: 8px; }"
        )
        card_layout = QVBoxLayout(self._card)
        card_layout.setContentsMargins(30, 24, 30, 24)
        card_layout.setSpacing(14)

        # How to get the token
        self._help_label = QLabel(
            "<b>How to get your token:</b> Open Azure DevOps in your browser &rarr; "
            "press <b>F12</b> &rarr; Network tab &rarr; click any request &rarr; "
            "copy the value after <code>Bearer </code> in the Authorization header."
        )
        self._help_label.setWordWrap(True)
        self._help_label.setStyleSheet(
            "background: #fffbe6; border: 1px solid #ffe58f; border-radius: 4px; "
            "padding: 8px; color: #333;"
        )
        card_layout.addWidget(self._help_label)

        # Bearer token field
        card_layout.addWidget(QLabel("Bearer Token *"))
        self.token_edit = QLineEdit()
        self.token_edit.setPlaceholderText("Paste your Bearer token here…")
        self.token_edit.setEchoMode(QLineEdit.Password)
        self.token_edit.textChanged.connect(self._on_token_changed)
        card_layout.addWidget(self.token_edit)

        self.expiry_label = QLabel("")
        self.expiry_label.setStyleSheet("color: #888; font-size: 11px;")
        card_layout.addWidget(self.expiry_label)

        self._clipboard_label = QLabel("")
        self._clipboard_label.setStyleSheet("color: #080; font-size: 11px;")
        card_layout.addWidget(self._clipboard_label)

        # Show/hide toggle
        toggle_row = QHBoxLayout()
        toggle_row.addStretch()
        self.show_btn = QPushButton("Show token")
        self.show_btn.setCheckable(True)
        self.show_btn.setStyleSheet("border: none; color: #0078d4; background: transparent;")
        self.show_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.show_btn.toggled.connect(self._toggle_visibility)
        toggle_row.addWidget(self.show_btn)
        card_layout.addLayout(toggle_row)

        # Organisation URL
        card_layout.addWidget(QLabel("Organisation URL *"))
        self.org_edit = QLineEdit()
        self.org_edit.setPlaceholderText("https://dev.azure.com/yourorganisation")
        card_layout.addWidget(self.org_edit)

        # Project name
        card_layout.addWidget(QLabel("Project Name *"))
        self.project_edit = QLineEdit()
        self.project_edit.setPlaceholderText("My Project")
        card_layout.addWidget(self.project_edit)

        # Connect button
        self.connect_btn = QPushButton("Connect")
        self.connect_btn.setFixedHeight(38)
        self.connect_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; font-size: 14px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self.connect_btn.clicked.connect(self._on_connect)
        card_layout.addSpacing(4)
        card_layout.addWidget(self.connect_btn)

        outer.addWidget(self._card)
        outer.addStretch()

    @staticmethod
    def _is_jwt(text: str) -> bool:
        if not text or len(text) < 100:
            return False
        parts = text.split('.')
        if len(parts) != 3:
            return False
        return all(re.match(r'^[A-Za-z0-9_\-]+$', p) for p in parts if p)

    def _check_clipboard(self):
        text = QApplication.clipboard().text().strip()
        if self._is_jwt(text) and not self.token_edit.text():
            self.token_edit.setText(text)
            self._clipboard_label.setText("Token auto-filled from clipboard")
            QTimer.singleShot(4000, lambda: self._clipboard_label.setText(""))

    def _on_clipboard_changed(self):
        text = QApplication.clipboard().text().strip()
        if self._is_jwt(text) and text != self.token_edit.text():
            self.token_edit.setText(text)
            self._clipboard_label.setText("Token auto-filled from clipboard")
            QTimer.singleShot(4000, lambda: self._clipboard_label.setText(""))

    def _restore_settings(self):
        s = load_settings()
        if s.get("org_url"):
            self.org_edit.setText(s["org_url"])
        if s.get("project"):
            self.project_edit.setText(s["project"])

    def _on_token_changed(self, text):
        self.app_state.token_manager.update_token(text)
        self._refresh_expiry_display()
        if text.strip():
            self._expiry_timer.start()
        else:
            self._expiry_timer.stop()

    def _refresh_expiry_display(self):
        display = self.app_state.token_manager.get_expiry_display()
        secs = self.app_state.token_manager.get_seconds_remaining()
        if "EXPIRED" in display:
            self.expiry_label.setStyleSheet("color: #c00; font-size: 11px;")
            self._expiry_timer.stop()
        elif secs < 0:
            self.expiry_label.setStyleSheet("color: #888; font-size: 11px;")
        elif secs < 60:
            self.expiry_label.setStyleSheet("color: #c00; font-size: 11px;")
        elif secs < 300:
            self.expiry_label.setStyleSheet("color: #e67e00; font-size: 11px;")
        else:
            self.expiry_label.setStyleSheet("color: #080; font-size: 11px;")
        self.expiry_label.setText(display)

    def _toggle_visibility(self, checked):
        self.token_edit.setEchoMode(QLineEdit.Normal if checked else QLineEdit.Password)
        self.show_btn.setText("Hide token" if checked else "Show token")

    def _on_connect(self):
        token = self.token_edit.text().strip()
        org = self.org_edit.text().strip()
        project = self.project_edit.text().strip()

        if not token:
            QMessageBox.warning(self, "Missing Field", "Please paste your Bearer token.")
            return
        if not org:
            QMessageBox.warning(self, "Missing Field", "Please enter the Organisation URL.")
            return
        if not project:
            QMessageBox.warning(self, "Missing Field", "Please enter the Project Name.")
            return

        if not org.startswith("https://dev.azure.com/"):
            reply = QMessageBox.question(
                self, "URL Format",
                f"The URL '{org}' doesn't start with 'https://dev.azure.com/'.\n"
                "Continue anyway?",
                QMessageBox.Yes | QMessageBox.No,
            )
            if reply == QMessageBox.No:
                return

        self.connect_btn.setEnabled(False)
        self.connect_btn.setText("Connecting…")
        self.app_state.token_manager.set_credentials(token, org, project)

        worker = Worker(self.app_state.client.validate_project)
        worker.signals.result.connect(lambda name: self._on_connected(token, org, name))
        worker.signals.error.connect(self._on_connect_error)
        QThreadPool.globalInstance().start(worker)

    def _on_connected(self, token: str, org: str, project_name: str):
        self.app_state.token_manager.set_credentials(token, org, project_name)
        save_settings({"org_url": org, "project": project_name})
        self.connect_btn.setEnabled(True)
        self.connect_btn.setText("Connect")
        self.connected.emit()

    def _on_connect_error(self, exc: Exception):
        QMessageBox.critical(
            self, "Connection Failed",
            f"Could not connect to Azure DevOps:\n\n{exc}\n\n"
            "Check that your token is valid and the URL / project name are correct."
        )
        self.connect_btn.setEnabled(True)
        self.connect_btn.setText("Connect")

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._card.setStyleSheet(
            f"#authCard {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 8px; }}"
        )
        self._subtitle.setStyleSheet(f"color: {t['text_dim']};")
        self._help_label.setStyleSheet(
            f"background: {t['warn_bg']}; border: 1px solid {t['warn_border']}; "
            f"border-radius: 4px; padding: 8px; color: {t['text']};"
        )
        self.show_btn.setStyleSheet(
            f"border: none; color: {t['accent']}; background: transparent;"
        )

    def prefill_token(self, token: str):
        """Called from progress screen when user supplies a refreshed token."""
        self.token_edit.setText(token)
