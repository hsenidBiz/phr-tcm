from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QLabel, QLineEdit,
    QPushButton, QMessageBox, QFrame
)
from PyQt5.QtCore import Qt, pyqtSignal, QThreadPool
from PyQt5.QtGui import QFont, QCursor

from app.utils.settings import load_settings, save_settings
from app.utils.worker import Worker


class AuthScreen(QWidget):
    connected = pyqtSignal()  # emitted when connection is validated

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._msal_auth = None
        self._build_ui()
        self._restore_settings()

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

        # Organisation URL
        card_layout.addWidget(QLabel("Organisation URL *"))
        self.org_edit = QLineEdit()
        self.org_edit.setPlaceholderText("https://dev.azure.com/yourorganisation")
        self.org_edit.returnPressed.connect(self._on_msal_sign_in)
        card_layout.addWidget(self.org_edit)

        # Project name
        card_layout.addWidget(QLabel("Project Name *"))
        self.project_edit = QLineEdit()
        self.project_edit.setPlaceholderText("My Project")
        self.project_edit.returnPressed.connect(self._on_msal_sign_in)
        card_layout.addWidget(self.project_edit)

        # Microsoft sign-in
        self.signin_btn = QPushButton("Sign in with Microsoft")
        self.signin_btn.setFixedHeight(38)
        self.signin_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; font-size: 14px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self.signin_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.signin_btn.clicked.connect(self._on_msal_sign_in)
        card_layout.addSpacing(4)
        card_layout.addWidget(self.signin_btn)

        self._signin_hint = QLabel(
            "Opens your browser to sign in with your Microsoft account — the "
            "access token is fetched and refreshed automatically."
        )
        self._signin_hint.setWordWrap(True)
        self._signin_hint.setStyleSheet("color: #888; font-size: 11px;")
        card_layout.addWidget(self._signin_hint)

        # Signed-in status
        self.status_label = QLabel("")
        self.status_label.setStyleSheet("color: #888; font-size: 11px;")
        card_layout.addWidget(self.status_label)

        outer.addWidget(self._card)
        outer.addStretch()

    def _restore_settings(self):
        s = load_settings()
        if s.get("org_url"):
            self.org_edit.setText(s["org_url"])
        if s.get("project"):
            self.project_edit.setText(s["project"])

    def _refresh_signin_display(self):
        from app.utils import theme
        t = theme.tokens()
        tm = self.app_state.token_manager
        if tm.auto_refresh_active():
            upn = tm.get_current_upn()
            who = f" as {upn}" if upn else ""
            self.status_label.setStyleSheet(f"color: {t['ok']}; font-size: 11px;")
            self.status_label.setText(f"Signed in{who} — token refreshes automatically")
        else:
            self.status_label.setText("")

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

    def _set_busy(self, busy: bool, text="Sign in with Microsoft"):
        self.signin_btn.setEnabled(not busy)
        self.signin_btn.setText(text)

    def _on_msal_sign_in(self):
        org_project = self._org_and_project()
        if org_project is None:
            return
        try:
            from app.auth.msal_auth import MsalAuthenticator
        except ImportError:
            QMessageBox.critical(
                self, "MSAL Not Available",
                "The 'msal' package is not installed.\n\nRun:  pip install msal"
            )
            return
        if self._msal_auth is None:
            self._msal_auth = MsalAuthenticator()

        self._set_busy(True, "Waiting for browser sign-in…")
        org, project = org_project
        worker = Worker(self._msal_auth.sign_in_interactive)
        worker.signals.result.connect(lambda token: self._on_msal_token(token, org, project))
        worker.signals.error.connect(self._on_msal_error)
        QThreadPool.globalInstance().start(worker)

    def _on_msal_token(self, token: str, org: str, project: str):
        self.app_state.token_manager.attach_msal(self._msal_auth)
        self._set_busy(True, "Validating project access…")
        self.app_state.token_manager.set_credentials(token, org, project)
        worker = Worker(self.app_state.client.validate_project)
        worker.signals.result.connect(lambda name: self._on_connected(token, org, name))
        worker.signals.error.connect(self._on_connect_error)
        QThreadPool.globalInstance().start(worker)

    def _on_msal_error(self, exc: Exception):
        self._set_busy(False)
        QMessageBox.critical(
            self, "Sign-In Failed",
            f"Microsoft sign-in did not complete:\n\n{exc}\n\n"
            "Check your network connection and try again."
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
        self._set_busy(False)
        self._refresh_signin_display()
        self.connected.emit()

    def _on_connect_error(self, exc: Exception):
        QMessageBox.critical(
            self, "Connection Failed",
            f"Could not connect to Azure DevOps:\n\n{exc}\n\n"
            "Check that the URL / project name are correct and that your "
            "account has access to the project."
        )
        self._set_busy(False)

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._card.setStyleSheet(
            f"#authCard {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 8px; }}"
        )
        self._subtitle.setStyleSheet(f"color: {t['text_dim']};")
        self._signin_hint.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self._refresh_signin_display()
