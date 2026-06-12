from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QLabel, QPushButton, QMessageBox, QFrame
)
from PyQt5.QtCore import Qt, pyqtSignal, QThreadPool
from PyQt5.QtGui import QFont, QCursor

from app.utils.worker import Worker


class AuthScreen(QWidget):
    connected = pyqtSignal()  # emitted once the user has signed in

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._msal_auth = None
        self._build_ui()

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

        self._subtitle = QLabel("Sign in with your Microsoft account to get started.")
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

        from app.utils import theme
        self.signin_btn = QPushButton("Sign in with Microsoft")
        self.signin_btn.setFixedHeight(38)
        self.signin_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 14px;")
        )
        self.signin_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.signin_btn.clicked.connect(self._on_msal_sign_in)
        card_layout.addWidget(self.signin_btn)

        self._signin_hint = QLabel(
            "Opens your browser to sign in — your organisation and projects "
            "are discovered automatically, and the access token refreshes itself."
        )
        self._signin_hint.setWordWrap(True)
        self._signin_hint.setStyleSheet("color: #888; font-size: 11px;")
        card_layout.addWidget(self._signin_hint)

        self.status_label = QLabel("")
        self.status_label.setStyleSheet("color: #888; font-size: 11px;")
        card_layout.addWidget(self.status_label)

        outer.addWidget(self._card)
        outer.addStretch()

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

    def _set_busy(self, busy: bool, text="Sign in with Microsoft"):
        self.signin_btn.setEnabled(not busy)
        self.signin_btn.setText(text)

    def _on_msal_sign_in(self):
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
        worker = Worker(self._msal_auth.sign_in_interactive)
        worker.signals.result.connect(self._on_msal_token)
        worker.signals.error.connect(self._on_msal_error)
        QThreadPool.globalInstance().start(worker)

    def _on_msal_token(self, token: str):
        tm = self.app_state.token_manager
        tm.update_token(token)
        tm.attach_msal(self._msal_auth)
        self._set_busy(False)
        self._refresh_signin_display()
        self.connected.emit()

    def _on_msal_error(self, exc: Exception):
        self._set_busy(False)
        QMessageBox.critical(
            self, "Sign-In Failed",
            f"Microsoft sign-in did not complete:\n\n{exc}\n\n"
            "Check your network connection and try again."
        )

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._card.setStyleSheet(
            f"#authCard {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 8px; }}"
        )
        self._subtitle.setStyleSheet(f"color: {t['text_dim']};")
        self._signin_hint.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self.signin_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 14px;")
        )
        self._refresh_signin_display()
