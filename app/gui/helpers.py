"""Small helpers shared across GUI screens."""

from PyQt5.QtWidgets import QMessageBox


def warn_if_token_expired(parent, token_manager) -> bool:
    """
    If the session has expired, show the standard warning and return True.
    Returns False when the token is still valid.
    """
    if not token_manager.is_expired():
        return False
    QMessageBox.warning(
        parent, "Session Expired",
        "Your Azure DevOps session has expired.\n\n"
        "Please go back to the authentication screen and sign in again."
    )
    return True


def refresh_module_combo(combo, values) -> None:
    """Repopulate an editable module combo box, preserving the typed text."""
    cur = combo.currentText()
    combo.blockSignals(True)
    combo.clear()
    for v in values:
        combo.addItem(v)
    combo.setCurrentText(cur)
    combo.blockSignals(False)


def status_message(widget, msg: str, timeout_ms: int = 5000) -> None:
    """Show a transient message in the main window's status bar (if available)."""
    win = widget.window()
    bar = getattr(win, "statusBar", None)
    if callable(bar):
        win.statusBar().showMessage(msg, timeout_ms)
