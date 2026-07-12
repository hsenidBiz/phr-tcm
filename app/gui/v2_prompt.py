"""One-way door to V2: a dismissible prompt telling v1 users the Tauri
rewrite has replaced this app.

Shown shortly after the main window appears (unless opted out). "Get V2"
opens the V2 releases page in the browser; V2 installs side-by-side, so
users can switch at their own pace and v1 keeps working meanwhile.
"""

from PyQt5.QtCore import QUrl
from PyQt5.QtGui import QDesktopServices
from PyQt5.QtWidgets import QCheckBox, QHBoxLayout, QLabel, QPushButton

from app.gui.frameless import FramelessDialog
from app.utils import theme
from app.utils.settings import load_settings, save_settings

V2_RELEASES_URL = (
    "https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases/releases/latest"
)

_OPTOUT_KEY = "v2_prompt_optout"


def maybe_show_v2_prompt(parent):
    """Show the upgrade prompt unless the user opted out earlier."""
    if load_settings().get(_OPTOUT_KEY):
        return
    dlg = _V2PromptDialog(parent)
    dlg.exec_()


class _V2PromptDialog(FramelessDialog):
    def __init__(self, parent):
        super().__init__(parent, "Test Case Manager V2 is ready", resizable=False)
        t = theme.tokens()

        heading = QLabel("A faster Test Case Manager has replaced this app.")
        heading.setStyleSheet(f"color: {t['text']}; font-size: 14px; font-weight: 600;")
        heading.setWordWrap(True)
        self.content_layout.addWidget(heading)

        body = QLabel(
            "Version 2 is a ground-up rewrite: quicker start-up, selectable "
            "themes, a searchable Test Suites browser, a Work Manager board, "
            "and more - with the same Microsoft sign-in and the same "
            "safety guarantees (it never deletes anything in Azure DevOps).\n\n"
            "V2 installs alongside this version and updates itself, so you "
            "can switch whenever you're ready. This version still works but "
            "no longer receives new features."
        )
        body.setStyleSheet(f"color: {t['text_dim']}; font-size: 12px;")
        body.setWordWrap(True)
        body.setMinimumWidth(380)
        self.content_layout.addWidget(body)

        self._optout = QCheckBox("Don't show this again")
        self._optout.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        self.content_layout.addWidget(self._optout)

        row = QHBoxLayout()
        row.addStretch(1)
        later = QPushButton("Later")
        later.setStyleSheet(theme.btn_neutral_qss())
        later.clicked.connect(self.reject)
        row.addWidget(later)
        get_v2 = QPushButton("Get V2")
        get_v2.setStyleSheet(theme.btn_primary_qss())
        get_v2.setDefault(True)
        get_v2.clicked.connect(self._on_get_v2)
        row.addWidget(get_v2)
        self.content_layout.addLayout(row)

        self.finalize_frameless()

    def _on_get_v2(self):
        QDesktopServices.openUrl(QUrl(V2_RELEASES_URL))
        self.accept()

    def done(self, result):  # noqa: D102 - persist the opt-out on any close
        if self._optout.isChecked():
            save_settings({_OPTOUT_KEY: True})
        super().done(result)
