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
        import os
        from PyQt5.QtCore import QSize
        from PyQt5.QtGui import QIcon
        from app.utils import icons

        from app.utils import theme

        outer = QVBoxLayout(self)
        outer.setContentsMargins(60, 36, 60, 36)
        outer.setSpacing(0)

        # ONE card for the whole page — logo, title, subtitle and sign-in all
        # live inside it.
        self._card = QFrame()
        self._card.setObjectName("authCard")
        self._card.setFrameShape(QFrame.NoFrame)
        self._card.setStyleSheet(
            "#authCard { background: #f9f9f9; border: 1px solid #ddd; border-radius: 10px; }"
        )
        self._card.setMinimumWidth(440)
        self._card.setMaximumWidth(520)   # a centred card, not full page width
        card_layout = QVBoxLayout(self._card)
        card_layout.setContentsMargins(48, 44, 48, 36)
        card_layout.setSpacing(0)

        # App logo
        logo = QLabel()
        logo.setAlignment(Qt.AlignCenter)
        _logo_path = icons.resource_path(os.path.join("resources", "icon.ico"))
        if os.path.exists(_logo_path):
            logo.setPixmap(QIcon(_logo_path).pixmap(QSize(84, 84)))
        card_layout.addWidget(logo)
        card_layout.addSpacing(16)

        # Title
        title = QLabel("Azure DevOps\nTest Case Manager")
        title.setAlignment(Qt.AlignCenter)
        title_font = QFont()
        title_font.setPointSize(20)
        title_font.setBold(True)
        title.setFont(title_font)
        card_layout.addWidget(title)
        card_layout.addSpacing(8)

        self._subtitle = QLabel("Sign in with your Microsoft account to get started.")
        self._subtitle.setAlignment(Qt.AlignCenter)
        self._subtitle.setStyleSheet("color: #666;")
        card_layout.addWidget(self._subtitle)
        card_layout.addSpacing(28)

        self.signin_btn = QPushButton("Sign in with Microsoft")
        self.signin_btn.setFixedHeight(38)
        self.signin_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 14px;")
        )
        self.signin_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.signin_btn.clicked.connect(self._on_primary_clicked)
        card_layout.addWidget(self.signin_btn)
        card_layout.addSpacing(12)

        # Shown only when already signed in: lets the user re-authenticate as
        # someone else instead of just continuing.
        self._switch_btn = QPushButton("Sign in with a different account")
        self._switch_btn.setFlat(True)
        self._switch_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._switch_btn.setStyleSheet(
            "QPushButton { border: none; background: transparent; color: #0078d4; "
            "font-size: 12px; } QPushButton:hover { text-decoration: underline; }"
        )
        self._switch_btn.clicked.connect(self._on_msal_sign_in)
        self._switch_btn.setVisible(False)
        card_layout.addWidget(self._switch_btn)
        card_layout.addSpacing(8)

        self._signin_hint = QLabel(
            "Opens your browser to sign in. Your organisation and projects are "
            "detected automatically."
        )
        self._signin_hint.setWordWrap(True)
        self._signin_hint.setAlignment(Qt.AlignCenter)
        self._signin_hint.setStyleSheet("color: #888; font-size: 11px;")
        card_layout.addWidget(self._signin_hint)

        self.status_label = QLabel("")
        self.status_label.setAlignment(Qt.AlignCenter)
        self.status_label.setStyleSheet("color: #888; font-size: 11px;")
        card_layout.addWidget(self.status_label)

        # Centre the single card on the page.
        outer.addStretch()
        outer.addWidget(self._card, alignment=Qt.AlignHCenter)
        outer.addStretch()

    def showEvent(self, event):
        super().showEvent(event)
        self._update_for_auth_state()

    def _on_primary_clicked(self):
        # Already signed in this session → continue without re-authenticating;
        # otherwise start the interactive Microsoft sign-in.
        if self.app_state.token_manager.auto_refresh_active():
            self.connected.emit()
        else:
            self._on_msal_sign_in()

    def _update_for_auth_state(self):
        """Reflect whether the user is already signed in: offer a one-click
        Continue (no browser re-auth) instead of forcing another sign-in."""
        tm = self.app_state.token_manager
        if tm.auto_refresh_active():
            upn = tm.get_current_upn()
            who = f" as {upn}" if upn else ""
            self._subtitle.setText(f"You're already signed in{who}.")
            self.signin_btn.setText("Continue")
            self._signin_hint.setVisible(False)
            self._switch_btn.setVisible(True)
        else:
            self._subtitle.setText(
                "Sign in with your Microsoft account to get started.")
            self.signin_btn.setText("Sign in with Microsoft")
            self._signin_hint.setVisible(True)
            self._switch_btn.setVisible(False)
        self.signin_btn.setEnabled(True)
        self._refresh_signin_display()

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
        self._switch_btn.setEnabled(not busy)

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

        self._set_busy(True, "Signing in…")
        # Pass the window handle so MSAL can use the Windows broker (one-click,
        # shared Microsoft session); it falls back to the system browser on its
        # own if the broker is unavailable.
        try:
            hwnd = int(self.window().winId())
        except Exception:
            hwnd = None
        worker = Worker(self._msal_auth.sign_in_interactive,
                        parent_window_handle=hwnd)
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
        self._update_for_auth_state()
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
        self._switch_btn.setStyleSheet(
            f"QPushButton {{ border: none; background: transparent; "
            f"color: {t['accent']}; font-size: 12px; }} "
            f"QPushButton:hover {{ text-decoration: underline; }}"
        )
        self._refresh_signin_display()
