"""My Work — a fast board of the work items assigned to you.

POC Phase 1: a read-only board. One WIQL round-trip (assigned to me, most
recently changed first) + one batched field GET, grouped into To Do / Doing /
Done columns by each state's process *category*. Search / type filter / sort
mirror the Run Tests bar. Double-click opens the item in the browser.
Editing, comments and the focus timer arrive in later phases.

Toggled from anywhere with Ctrl+Shift+M (see MainWindow._toggle_mywork).
"""

import webbrowser
from urllib.parse import quote

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton, QListWidget,
    QListWidgetItem, QLineEdit, QComboBox,
)
from PyQt5.QtCore import Qt, QThreadPool
from PyQt5.QtGui import QCursor, QColor, QBrush

from app.utils.worker import Worker
from app.utils import theme
from app.utils.anim import Spinner
from app.models.work_item import WorkItem, WORK_ITEM_FIELDS, COLUMNS
from app.gui.checkable_combo import CheckableComboBox
from app.gui import delegates

# Test artifacts are managed in the app's normal mode — keep the board about
# actual work (stories, bugs, tasks, …).
_EXCLUDED_TYPES = ("Test Case", "Test Suite", "Test Plan",
                   "Shared Steps", "Shared Parameter")

_MAX_ITEMS = 500   # WIQL $top cap — personal boards stay far below this


def _fetch_my_work(client) -> dict:
    """Worker-thread fetch: ids via WIQL (@Me), fields via the batched GET, and
    each distinct type's states (cached per type on the client). Read only."""
    excluded = ", ".join(f"'{t}'" for t in _EXCLUDED_TYPES)
    wiql = (
        "SELECT [System.Id] FROM workitems "
        "WHERE [System.TeamProject] = @project "
        "AND [System.AssignedTo] = @Me "
        f"AND [System.WorkItemType] NOT IN ({excluded}) "
        "ORDER BY [System.ChangedDate] DESC"
    )
    ids = client.query_work_items(wiql, top=_MAX_ITEMS)
    fields = client.get_work_items(ids, WORK_ITEM_FIELDS) if ids else []
    states = {}
    for f in fields:
        wtype = f.get("System.WorkItemType", "")
        if wtype and wtype not in states:
            try:
                states[wtype] = {s["name"]: (s["category"], s["color"])
                                 for s in client.get_work_item_states(wtype)}
            except Exception:
                states[wtype] = {}   # unknown process — column falls back to heuristic
    return {"fields": fields, "states": states}


class MyWorkScreen(QWidget):
    """Read-only Kanban-style view of the work items assigned to the signed-in
    user in the current project."""

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._items: list[WorkItem] = []
        self._states_by_type: dict = {}     # {type: {state: (category, color)}}
        self._loaded_key = None             # (org_url, project) the items belong to
        self._loading = False
        self._build_ui()

    # ------------------------------------------------------------------ #
    #  Lifecycle / loading                                                #
    # ------------------------------------------------------------------ #

    def on_enter(self):
        """Called when the mode is toggled on — (re)load if the org/project
        changed since the last load (first entry included)."""
        tm = self.app_state.token_manager
        key = (tm.org_url, tm.project)
        if key != self._loaded_key and not self._loading:
            self.refresh()

    def refresh(self):
        if self._loading:
            return
        tm = self.app_state.token_manager
        key = (tm.org_url, tm.project)
        self._loading = True
        self._refresh_btn.setEnabled(False)
        self._status_lbl.setText("Loading your work items…")
        self._spinner.start()
        worker = Worker(_fetch_my_work, self.app_state.client)
        worker.signals.result.connect(lambda res, k=key: self._on_loaded(k, res))
        worker.signals.error.connect(self._on_error)
        QThreadPool.globalInstance().start(worker)

    def _on_loaded(self, key, result):
        self._loading = False
        self._spinner.stop()
        self._refresh_btn.setEnabled(True)
        self._status_lbl.setText("")
        self._loaded_key = key
        self._items = [WorkItem(f) for f in result.get("fields", [])]
        self._states_by_type = result.get("states", {})
        self._repopulate_type_filter()
        self._rebuild()

    def _on_error(self, exc):
        self._loading = False
        self._spinner.stop()
        self._refresh_btn.setEnabled(True)
        self._status_lbl.setText(f"Could not load work items: {exc}")

    # ------------------------------------------------------------------ #
    #  UI                                                                 #
    # ------------------------------------------------------------------ #

    def _build_ui(self):
        from app.utils import icons
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 14, 16, 12)
        layout.setSpacing(8)

        # Header: title · count · spinner · refresh · return hint
        hdr = QHBoxLayout()
        self._title_lbl = QLabel("<b>My Work</b>")
        self._title_lbl.setStyleSheet("font-size: 16px;")
        hdr.addWidget(self._title_lbl)
        hdr.addSpacing(10)
        self._count_lbl = QLabel("")
        hdr.addWidget(self._count_lbl)
        self._spinner = Spinner(size=16, line_width=2)
        self._spinner.stop()   # hidden until a load starts
        hdr.addWidget(self._spinner)
        hdr.addStretch()
        self._hint_lbl = QLabel("Ctrl+Shift+M to return")
        hdr.addWidget(self._hint_lbl)
        hdr.addSpacing(8)
        self._refresh_btn = QPushButton("Refresh")
        self._refresh_btn.setIcon(icons.icon("refresh", size=15))
        self._refresh_btn.setStyleSheet(theme.btn_neutral_qss())
        self._refresh_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._refresh_btn.clicked.connect(self.refresh)
        hdr.addWidget(self._refresh_btn)
        layout.addLayout(hdr)

        self._status_lbl = QLabel("")
        layout.addWidget(self._status_lbl)

        # Filter bar: search · type multi-select · sort · clear
        bar = QHBoxLayout()
        bar.setSpacing(6)
        self._search = QLineEdit()
        self._search.setPlaceholderText("Search by ID or title…")
        self._search.setClearButtonEnabled(True)
        self._search.textChanged.connect(lambda _t: self._on_filter_changed())
        bar.addWidget(self._search, 2)
        self._type_combo = CheckableComboBox(all_text="All types")
        self._type_combo.setToolTip("Filter by work item type — tick one or more")
        self._type_combo.changed.connect(self._on_filter_changed)
        bar.addWidget(self._type_combo, 1)
        self._sort_combo = QComboBox()
        self._sort_combo.setToolTip("Sort the cards inside each column")
        for label, data in (
            ("Recently changed", None), ("Priority", "priority"),
            ("Title A–Z", "az"), ("Title Z–A", "za"),
        ):
            self._sort_combo.addItem(label, data)
        self._sort_combo.currentIndexChanged.connect(lambda _i: self._rebuild())
        bar.addWidget(self._sort_combo, 1)
        self._clear_btn = QPushButton()
        self._clear_btn.setIcon(icons.icon("x", size=13))
        self._clear_btn.setToolTip("Clear all filters")
        self._clear_btn.setFixedSize(28, 28)
        self._clear_btn.setStyleSheet(theme.btn_ghost_qss("padding: 4px;"))
        self._clear_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._clear_btn.clicked.connect(self._clear_filters)
        self._clear_btn.setVisible(False)
        bar.addWidget(self._clear_btn)
        for cb in (self._type_combo, self._sort_combo):
            cb.setMinimumWidth(120)
            cb.setCursor(QCursor(Qt.PointingHandCursor))
        layout.addLayout(bar)

        # Board: three columns, each a header label + card list
        board = QHBoxLayout()
        board.setSpacing(10)
        self._col_labels = {}
        self._col_lists = {}
        for col in COLUMNS:
            col_v = QVBoxLayout()
            col_v.setSpacing(4)
            lbl = QLabel(f"<b>{col}</b>")
            self._col_labels[col] = lbl
            col_v.addWidget(lbl)
            lst = QListWidget()
            lst.setAlternatingRowColors(False)
            lst.setWordWrap(True)
            lst.setSelectionMode(QListWidget.SingleSelection)
            lst.itemDoubleClicked.connect(self._open_in_browser)
            delegates.apply_hover(lst)
            self._col_lists[col] = lst
            col_v.addWidget(lst, 1)
            board.addLayout(col_v, 1)
        layout.addLayout(board, 1)

        self._empty_lbl = QLabel("")
        self._empty_lbl.setAlignment(Qt.AlignCenter)
        self._empty_lbl.setVisible(False)
        layout.addWidget(self._empty_lbl)

        self.refresh_theme()

    # ------------------------------------------------------------------ #
    #  Filters / board build                                              #
    # ------------------------------------------------------------------ #

    def _repopulate_type_filter(self):
        """Fill the type multi-select from the loaded items, keeping any ticks
        that still apply."""
        keep = set(self._type_combo.checked_data())
        self._type_combo._model.removeRows(0, self._type_combo._model.rowCount())
        types = sorted({wi.type for wi in self._items if wi.type})
        for t in types:
            self._type_combo.addCheckItem(t, t)
        self._type_combo.set_checked_data([t for t in keep if t in types])

    def _filters_active(self) -> bool:
        return bool(self._search.text().strip() or self._type_combo.checked_data())

    def _on_filter_changed(self):
        self._rebuild()
        self._clear_btn.setVisible(self._filters_active())

    def _clear_filters(self):
        self._search.blockSignals(True)
        self._search.clear()
        self._search.blockSignals(False)
        self._type_combo.clear_checks()
        self._on_filter_changed()

    def _filtered_sorted(self) -> list:
        query = self._search.text().strip().lower()
        types = set(self._type_combo.checked_data())
        items = [
            wi for wi in self._items
            if (not types or wi.type in types)
            and (not query or query in wi.title.lower() or query == str(wi.id))
        ]
        mode = self._sort_combo.currentData()
        if mode == "priority":   # lower number = more urgent; unset last
            items.sort(key=lambda w: (w.priority is None, w.priority or 0))
        elif mode in ("az", "za"):
            items.sort(key=lambda w: w.title.lower(), reverse=(mode == "za"))
        return items   # default: WIQL order (recently changed first)

    def _state_color(self, wi: WorkItem):
        entry = (self._states_by_type.get(wi.type) or {}).get(wi.state)
        if entry and entry[1]:
            col = QColor(f"#{entry[1].lstrip('#')}")
            if col.isValid():
                col.setAlpha(46)   # subtle translucent tint over the list base
                return col
        return None

    def _rebuild(self):
        for lst in self._col_lists.values():
            lst.clear()
        counts = {col: 0 for col in COLUMNS}
        # WorkItem.column wants {type: {state: category}} — strip the colors.
        cat_map = {t: {s: v[0] for s, v in m.items()}
                   for t, m in self._states_by_type.items()}
        shown = 0
        for wi in self._filtered_sorted():
            col = wi.column(cat_map)
            if col is None:   # Removed — hidden
                continue
            item = QListWidgetItem(f"#{wi.id}  ·  {wi.type}\n{wi.title}")
            item.setData(Qt.UserRole, wi)
            tip = f"State: {wi.state}"
            if wi.priority is not None:
                tip += f"\nPriority: {wi.priority}"
            if wi.iteration_path:
                tip += f"\nIteration: {wi.iteration_path}"
            tip += "\n\nDouble-click to open in Azure DevOps"
            item.setToolTip(tip)
            color = self._state_color(wi)
            if color is not None:
                item.setBackground(QBrush(color))
            self._col_lists[col].addItem(item)
            counts[col] += 1
            shown += 1
        for col, lbl in self._col_labels.items():
            lbl.setText(f"<b>{col}</b>  <span style='color:{theme.tokens()['text_dim2']}'>"
                        f"{counts[col]}</span>")
        total = len(self._items)
        if self._filters_active() and total:
            self._count_lbl.setText(f"{shown} of {total} items")
        else:
            self._count_lbl.setText(f"{total} item{'s' if total != 1 else ''}"
                                    if self._loaded_key else "")
        self._empty_lbl.setVisible(self._loaded_key is not None and total == 0)
        self._empty_lbl.setText(
            "No work items are assigned to you in this project." if total == 0 else "")

    # ------------------------------------------------------------------ #
    #  Actions                                                            #
    # ------------------------------------------------------------------ #

    def _open_in_browser(self, item):
        wi = item.data(Qt.UserRole)
        if wi is None or wi.id is None:
            return
        tm = self.app_state.token_manager
        webbrowser.open(f"{tm.org_url}/{quote(tm.project)}/_workitems/edit/{wi.id}")

    # ------------------------------------------------------------------ #
    #  Theme                                                              #
    # ------------------------------------------------------------------ #

    def refresh_theme(self):
        from app.utils import icons
        t = theme.tokens()
        self._count_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 12px;")
        self._hint_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self._status_lbl.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        self._empty_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 13px;")
        self._spinner.set_color(t["accent"])
        self._refresh_btn.setStyleSheet(theme.btn_neutral_qss())
        self._refresh_btn.setIcon(icons.icon("refresh", size=15))
        self._clear_btn.setStyleSheet(theme.btn_ghost_qss("padding: 4px;"))
        self._clear_btn.setIcon(icons.icon("x", size=13))
        if self._items:
            self._rebuild()   # column headers embed a theme colour
