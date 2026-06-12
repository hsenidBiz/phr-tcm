import re

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QMessageBox, QFrame, QApplication
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
        self._msal_auth = None
        self._setting_token_programmatically = False
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

        # Microsoft sign-in (primary path — no token copying)
        self.signin_btn = QPushButton("Sign in with Microsoft")
        self.signin_btn.setFixedHeight(38)
        self.signin_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; font-size: 14px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self.signin_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.signin_btn.clicked.connect(self._on_msal_sign_in)
        card_layout.addWidget(self.signin_btn)

        self._signin_hint = QLabel(
            "Opens your browser to sign in — the token is fetched and refreshed "
            "automatically. Fill in the Organisation URL and Project Name below first."
        )
        self._signin_hint.setWordWrap(True)
        self._signin_hint.setStyleSheet("color: #888; font-size: 11px;")
        card_layout.addWidget(self._signin_hint)

        self._divider_label = QLabel("— or paste a token manually —")
        self._divider_label.setAlignment(Qt.AlignCenter)
        self._divider_label.setStyleSheet("color: #888; font-size: 11px;")
        card_layout.addWidget(self._divider_label)

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
        self.token_edit.returnPressed.connect(self._on_connect)
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
        self.org_edit.returnPressed.connect(self._on_connect)
        card_layout.addWidget(self.org_edit)

        # Project name
        card_layout.addWidget(QLabel("Project Name *"))
        self.project_edit = QLineEdit()
        self.project_edit.setPlaceholderText("My Project")
        self.project_edit.returnPressed.connect(self._on_connect)
        card_layout.addWidget(self.project_edit)

        # Connect button
        self.connect_btn = QPushButton("Connect")
        self.connect_btn.setFixedHeight(38)
        self.connect_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; font-size: 14px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self.connect_btn.setCursor(QCursor(Qt.PointingHandCursor))
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
        self._check_clipboard()

    def _restore_settings(self):
        s = load_settings()
        if s.get("org_url"):
            self.org_edit.setText(s["org_url"])
        if s.get("project"):
            self.project_edit.setText(s["project"])

    def _on_token_changed(self, text):
        if not self._setting_token_programmatically:
            # The user is taking manual control — stop MSAL auto-refresh so the
            # pasted token is what actually gets sent.
            self.app_state.token_manager.detach_msal()
        self.app_state.token_manager.update_token(text)
        self._refresh_expiry_display()
        if text.strip():
            self._expiry_timer.start()
        else:
            self._expiry_timer.stop()

    def _refresh_expiry_display(self):
        from app.utils import theme
        t = theme.tokens()
        tm = self.app_state.token_manager
        if tm.auto_refresh_active():
            upn = tm.get_current_upn()
            who = f" as {upn}" if upn else ""
            self.expiry_label.setStyleSheet(f"color: {t['ok']}; font-size: 11px;")
            self.expiry_label.setText(f"Signed in{who} — token refreshes automatically")
            return
        display = tm.get_expiry_display()
        secs = tm.get_seconds_remaining()
        if "EXPIRED" in display:
            self.expiry_label.setStyleSheet(f"color: {t['error']}; font-size: 11px;")
            self._expiry_timer.stop()
        elif secs < 0:
            self.expiry_label.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        elif secs < 60:
            self.expiry_label.setStyleSheet(f"color: {t['error']}; font-size: 11px;")
        elif secs < 300:
            self.expiry_label.setStyleSheet(f"color: {t['warn_fg']}; font-size: 11px;")
        else:
            self.expiry_label.setStyleSheet(f"color: {t['ok']}; font-size: 11px;")
        self.expiry_label.setText(display)

    def _toggle_visibility(self, checked):
        self.token_edit.setEchoMode(QLineEdit.Normal if checked else QLineEdit.Password)
        self.show_btn.setText("Hide token" if checked else "Show token")

    def _org_and_project(self) -> tuple[str, str] | None:
        """Validate and return (org, project) from the form, or None if invalid."""
        org = self.org_edit.text().strip()
        project = self.project_edit.text().strip()
        if not org:
            QMessageBox.warning(self, "Missing Field", "Please enter the Organisation URL.")
            return None
        if not project:
            QMessageBox.warning(self, "Missing Field", "Please enter the Project Name.")
            return None
        if not org.startswith("https://dev.azure.com/"):
            reply = QMessageBox.question(
                self, "URL Format",
                f"The URL '{org}' doesn't start with 'https://dev.azure.com/'.\n"
                "Continue anyway?",
                QMessageBox.Yes | QMessageBox.No,
            )
            if reply == QMessageBox.No:
                return None
        return org, project

    def _set_buttons_busy(self, busy: bool, connect_text="Connect", signin_text="Sign in with Microsoft"):
        self.connect_btn.setEnabled(not busy)
        self.connect_btn.setText(connect_text)
        self.signin_btn.setEnabled(not busy)
        self.signin_btn.setText(signin_text)

    def _start_validation(self, token: str, org: str, project: str):
        """Verify project access with the given token, then emit connected."""
        self.app_state.token_manager.set_credentials(token, org, project)
        worker = Worker(self.app_state.client.validate_project)
        worker.signals.result.connect(lambda name: self._on_connected(token, org, name))
        worker.signals.error.connect(self._on_connect_error)
        QThreadPool.globalInstance().start(worker)

    def _on_connect(self):
        token = self.token_edit.text().strip()
        if not token:
            QMessageBox.warning(self, "Missing Field", "Please paste your Bearer token.")
            return
        org_project = self._org_and_project()
        if org_project is None:
            return
        self._set_buttons_busy(True, connect_text="Connecting…")
        self._start_validation(token, *org_project)

    # ------------------------------------------------------------------ #
    #  Microsoft (MSAL) sign-in                                            #
    # ------------------------------------------------------------------ #

    def _on_msal_sign_in(self):
        org_project = self._org_and_project()
        if org_project is None:
            return
        try:
            from app.auth.msal_auth import MsalAuthenticator
        except ImportError:
            QMessageBox.critical(
                self, "MSAL Not Available",
                "The 'msal' package is not installed.\n\n"
                "Run:  pip install msal\n\n"
                "Until then, use the manual token paste below."
            )
            return
        if self._msal_auth is None:
            self._msal_auth = MsalAuthenticator()

        self._set_buttons_busy(True, signin_text="Waiting for browser sign-in…")
        org, project = org_project
        worker = Worker(self._msal_auth.sign_in_interactive)
        worker.signals.result.connect(lambda token: self._on_msal_token(token, org, project))
        worker.signals.error.connect(self._on_msal_error)
        QThreadPool.globalInstance().start(worker)

    def _on_msal_token(self, token: str, org: str, project: str):
        self.app_state.token_manager.attach_msal(self._msal_auth)
        # Sync the field without detaching auto-refresh (it drives the
        # expiry timer and keeps update_token in the loop).
        self._setting_token_programmatically = True
        try:
            self.token_edit.setText(token)
        finally:
            self._setting_token_programmatically = False
        self.signin_btn.setText("Validating project access…")
        self._start_validation(token, org, project)

    def _on_msal_error(self, exc: Exception):
        self._set_buttons_busy(False)
        QMessageBox.critical(
            self, "Sign-In Failed",
            f"Microsoft sign-in did not complete:\n\n{exc}\n\n"
            "You can still connect by pasting a Bearer token manually below."
        )

    def _on_connected(self, token: str, org: str, project_name: str):
        self.app_state.token_manager.set_credentials(token, org, project_name)
        save_settings({"org_url": org, "project": project_name})
        # Clear stale member cache so the new project's users are fetched fresh.
        # Disconnect any in-flight fetcher first so it won't overwrite the cleared cache.
        fetcher = self.app_state._team_members_fetcher
        if fetcher is not None:
            try:
                fetcher.done.disconnect()
            except TypeError:
                pass
        self.app_state.cached_team_members = None
        self.app_state._team_members_fetcher = None
        self._set_buttons_busy(False)
        self._refresh_expiry_display()
        self.connected.emit()

    def _on_connect_error(self, exc: Exception):
        QMessageBox.critical(
            self, "Connection Failed",
            f"Could not connect to Azure DevOps:\n\n{exc}\n\n"
            "Check that your token is valid and the URL / project name are correct."
        )
        self._set_buttons_busy(False)

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
        self._clipboard_label.setStyleSheet(f"color: {t['ok']}; font-size: 11px;")
        self._signin_hint.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self._divider_label.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self._refresh_expiry_display()

    def prefill_token(self, token: str):
        """Keep this screen's token field in sync after a mid-run token refresh."""
        self.token_edit.setText(token)
