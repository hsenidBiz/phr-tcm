"""Small helpers shared across GUI screens."""

from PyQt5.QtCore import Qt, QTimer, pyqtSignal
from PyQt5.QtGui import QCursor
from PyQt5.QtWidgets import (
    QComboBox, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
)


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


def make_module_combo(placeholder: str = "e.g. Authentication") -> QComboBox:
    """The standard editable Module combo used by every entry screen."""
    combo = QComboBox()
    combo.setEditable(True)
    combo.setInsertPolicy(QComboBox.NoInsert)
    combo.lineEdit().setPlaceholderText(placeholder)
    return combo


def refresh_module_combo(combo, values) -> None:
    """Repopulate an editable module combo box, preserving the typed text."""
    cur = combo.currentText()
    combo.blockSignals(True)
    combo.clear()
    for v in values:
        combo.addItem(v)
    combo.setCurrentText(cur)
    combo.blockSignals(False)


def refresh_team_members(app_state, populate, on_done, on_failed) -> bool:
    """Shared "populate now from cache, then fetch in the background" flow for
    the Created By / Assigned To combos.

    Calls ``populate(members)`` immediately when a cached list exists (memory,
    falling back to the 24 h disk cache) and returns True; returns False when
    there is no cache yet so the caller can show a loading placeholder. Then
    starts the single shared background fetch — or attaches ``populate`` to the
    one already in flight — so fresh members arrive later.

    ``populate`` / ``on_done`` / ``on_failed`` must be methods of a GUI-thread
    QObject (the screen): the fetcher emits from a worker thread and relies on
    the receiver's thread affinity for queued delivery. ``on_done`` should call
    ``store_fetched_members`` then repopulate.
    """
    from app.utils.members_cache import TeamMemberFetcher, attach_once, load_cached
    tm = app_state.client.tm

    if app_state.cached_team_members is None:
        on_disk = load_cached(tm.org_url, tm.project)
        if on_disk is not None:
            app_state.cached_team_members = on_disk

    had_cache = app_state.cached_team_members is not None
    if had_cache:
        populate(app_state.cached_team_members)

    if app_state._team_members_fetcher is None:
        fetcher = TeamMemberFetcher(app_state.client)
        app_state._team_members_fetcher = fetcher
        fetcher.done.connect(on_done)
        fetcher.failed.connect(on_failed)
        fetcher.start()
    else:
        # Attach to the already-running fetch so this screen gets the result too.
        attach_once(app_state._team_members_fetcher, populate)
    return had_cache


def store_fetched_members(app_state, members: list) -> None:
    """Record a finished team-members fetch in the shared in-memory + disk
    caches and release the in-flight fetcher. Call from the GUI-thread ``done``
    handler before repopulating the combo."""
    from app.utils.members_cache import save_to_disk
    tm = app_state.client.tm
    app_state.cached_team_members = members
    app_state._team_members_fetcher = None
    if members:
        save_to_disk(tm.org_url, tm.project, members)


def fetch_project_tags(app_state, on_ready) -> None:
    """Fetch the project's existing tag names in the background and hand them to
    ``on_ready(names)`` on the GUI thread (for a TagLineEdit's suggestions).

    Cached per (org, project) on app_state so a second screen or a revisit reuses
    them; a project switch refetches. Best-effort — on failure yields ``[]`` and
    leaves the cache empty so the next visit retries. ``on_ready`` must be a
    GUI-thread QObject method."""
    from app.utils.worker import Worker
    from PyQt5.QtCore import QThreadPool
    tm = app_state.client.tm
    key = (tm.org_url, tm.project)
    cache = getattr(app_state, "_tags_cache", None)
    if cache and cache[0] == key:
        on_ready(cache[1])
        return

    def _ok(tags):
        names = sorted({t.get("name", "") for t in (tags or []) if t.get("name")})
        app_state._tags_cache = (key, names)
        on_ready(names)

    worker = Worker(app_state.client.get_tags)
    worker.signals.result.connect(_ok)
    worker.signals.error.connect(lambda _e: on_ready([]))
    QThreadPool.globalInstance().start(worker)


class UndoToast(QFrame):
    """Transient bottom-centred toast with a message and an Undo action.

    Create one per screen and call ``show_message()`` after a destructive
    operation; it auto-hides after a few seconds. Connect ``undo_clicked``
    to the screen's restore handler."""

    undo_clicked = pyqtSignal()

    def __init__(self, parent):
        super().__init__(parent)
        self.setVisible(False)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(14, 8, 10, 8)
        lay.setSpacing(10)
        self._label = QLabel("")
        lay.addWidget(self._label)
        self._undo_btn = QPushButton("Undo")
        self._undo_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._undo_btn.clicked.connect(self.undo_clicked)
        lay.addWidget(self._undo_btn)
        self._timer = QTimer(self)
        self._timer.setSingleShot(True)
        self._timer.setInterval(6000)
        self._timer.timeout.connect(self.hide)
        self.refresh_theme()

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self.setStyleSheet(
            f"QFrame {{ background: {t['surface2']}; border: 1px solid {t['border']}; "
            f"border-radius: 8px; }}"
            f"QLabel {{ color: {t['text']}; border: none; background: transparent; }}"
        )
        self._undo_btn.setStyleSheet(
            f"QPushButton {{ border: none; background: transparent; color: {t['accent']}; "
            f"font-weight: bold; padding: 2px 8px; }}"
            f"QPushButton:hover {{ color: {t['accent_hover']}; }}"
        )

    def show_message(self, text: str):
        self._label.setText(text)
        self.adjustSize()
        p = self.parentWidget()
        if p is not None:
            self.move((p.width() - self.width()) // 2,
                      p.height() - self.height() - 24)
        self.show()
        self.raise_()
        self._timer.start()


def status_message(widget, msg: str, timeout_ms: int = 5000) -> None:
    """Show a transient message in the main window's status bar (if available)."""
    win = widget.window()
    bar = getattr(win, "statusBar", None)
    if callable(bar):
        win.statusBar().showMessage(msg, timeout_ms)
