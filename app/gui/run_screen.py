"""Run Tests tab — build a session of test cases, then launch the runner.

Loads the PBI's test cases (with steps + preconditions), lets you search and
multi-select them, accumulate a session, and open the always-on-top runner.
"""

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton, QListWidget,
    QListWidgetItem, QLineEdit, QSplitter, QFrame, QMessageBox,
)
from PyQt5.QtCore import Qt, QThreadPool
from PyQt5.QtGui import QCursor

from app.utils.worker import Worker
from app.utils import theme
from app.gui.test_runner import TestRunner


class RunScreen(QWidget):
    """Tab for assembling and launching a manual test execution session."""

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._cases = []           # all loaded case dicts (rows map 1:1)
        self._session = []         # case dicts queued for the run
        self._loaded_pbi = None
        self._open_runners = []    # keep references so windows aren't GC'd
        self._build_ui()

    # ------------------------------------------------------------------ #
    #  Lifecycle / loading                                                #
    # ------------------------------------------------------------------ #

    def showEvent(self, event):
        super().showEvent(event)
        self.ensure_loaded()

    def ensure_loaded(self):
        pbi = self.app_state.pbi_id
        if pbi and pbi != self._loaded_pbi:
            # PBI changed (or first load) — a session built for the previous PBI
            # is no longer valid (those test cases aren't linked to this PBI), so
            # clear it before loading this PBI's cases.
            if self._session:
                self._clear_session()
            self._load_cases()

    def _load_cases(self):
        pbi_id = self.app_state.pbi_id
        if not pbi_id or self.app_state.token_manager.is_expired():
            return
        self._header_lbl.setText("Loading test cases…")
        self._refresh_btn.setEnabled(False)
        self._available.clear()
        self._cases = []
        # Preconditions + module come down as extra fields (needed by the runner).
        extra = [r for r in (self.app_state.module_ref, self.app_state.preconditions_ref) if r]
        worker = Worker(self.app_state.client.get_test_cases_for_pbi, pbi_id, extra)
        worker.signals.result.connect(lambda r: self._on_cases_loaded(pbi_id, r))
        worker.signals.error.connect(lambda exc: self._on_cases_error(exc))
        QThreadPool.globalInstance().start(worker)

    def _on_cases_loaded(self, pbi_id, result):
        cases, _total = result
        self._cases = cases
        self._loaded_pbi = pbi_id
        self.app_state.existing_cases = cases
        self.app_state.existing_cases_pbi = pbi_id
        self._rebuild_available()
        self._header_lbl.setText("")
        self._refresh_btn.setEnabled(True)

    def _on_cases_error(self, exc):
        self._header_lbl.setText(f"Could not load test cases: {exc}")
        self._refresh_btn.setEnabled(True)

    # ------------------------------------------------------------------ #
    #  UI                                                                 #
    # ------------------------------------------------------------------ #

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(8)

        hdr = QHBoxLayout()
        intro = QLabel(
            "Search and select test cases, add them to a session, then start the "
            "always-on-top runner to execute them and record outcomes."
        )
        intro.setWordWrap(True)
        intro.setStyleSheet("color: #555;")
        hdr.addWidget(intro, 1)
        self._refresh_btn = QPushButton("↺  Refresh")
        self._refresh_btn.setStyleSheet(theme.btn_neutral_qss())
        self._refresh_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._refresh_btn.clicked.connect(self._load_cases)
        hdr.addWidget(self._refresh_btn)
        layout.addLayout(hdr)

        self._header_lbl = QLabel("")
        self._header_lbl.setStyleSheet("color: #888; font-size: 11px;")
        layout.addWidget(self._header_lbl)

        splitter = QSplitter(Qt.Horizontal)

        # -- Left: available cases ----------------------------------------
        left = QWidget()
        lv = QVBoxLayout(left)
        lv.setContentsMargins(0, 0, 0, 0)
        lv.setSpacing(4)
        lv.addWidget(QLabel("<b>Test cases on this PBI</b>"))
        self._search = QLineEdit()
        self._search.setPlaceholderText("Search by ID or title…")
        self._search.setClearButtonEnabled(True)
        self._search.textChanged.connect(lambda _: self._apply_search())
        lv.addWidget(self._search)
        self._available = QListWidget()
        self._available.setAlternatingRowColors(True)
        self._available.setSelectionMode(QListWidget.ExtendedSelection)
        self._available.itemDoubleClicked.connect(lambda _it: self._add_to_session())
        lv.addWidget(self._available, 1)
        splitter.addWidget(left)

        # -- Middle: move buttons (PBI list  ⇄  session) ------------------
        mid = QWidget()
        mid.setFixedWidth(116)
        mv = QVBoxLayout(mid)
        mv.setContentsMargins(6, 0, 6, 0)
        mv.setSpacing(10)
        mv.addStretch()
        self._add_btn = QPushButton("Add  →")
        self._add_btn.setFixedWidth(96)
        self._add_btn.setToolTip("Add the selected test case(s) to the session")
        self._add_btn.setStyleSheet(theme.btn_primary_qss("padding: 6px 10px;"))
        self._add_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._add_btn.clicked.connect(self._add_to_session)
        mv.addWidget(self._add_btn)
        self._remove_btn = QPushButton("←  Remove")
        self._remove_btn.setFixedWidth(96)
        self._remove_btn.setToolTip(
            "Remove the selected test case(s) from the session "
            "(back to the PBI list)")
        self._remove_btn.setStyleSheet(theme.btn_neutral_qss())
        self._remove_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._remove_btn.clicked.connect(self._remove_from_session)
        mv.addWidget(self._remove_btn)
        mv.addStretch()
        splitter.addWidget(mid)

        # -- Right: session -----------------------------------------------
        right = QWidget()
        rv = QVBoxLayout(right)
        rv.setContentsMargins(0, 0, 0, 0)
        rv.setSpacing(4)
        self._session_lbl = QLabel("<b>Session (0)</b>")
        rv.addWidget(self._session_lbl)
        self._session_list = QListWidget()
        self._session_list.setAlternatingRowColors(True)
        self._session_list.setSelectionMode(QListWidget.ExtendedSelection)
        self._session_list.itemDoubleClicked.connect(lambda _it: self._remove_from_session())
        rv.addWidget(self._session_list, 1)
        self._start_btn = QPushButton("▶  Start Run")
        self._start_btn.setFixedHeight(36)
        self._start_btn.setEnabled(False)
        self._start_btn.setStyleSheet(theme.btn_primary_qss("font-size: 13px; padding: 0 20px;"))
        self._start_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._start_btn.clicked.connect(self._start_run)
        rv.addWidget(self._start_btn)
        splitter.addWidget(right)

        splitter.setStretchFactor(0, 1)
        splitter.setStretchFactor(1, 0)
        splitter.setStretchFactor(2, 1)
        splitter.setCollapsible(1, False)
        splitter.setSizes([460, 116, 380])
        layout.addWidget(splitter, 1)

    # ------------------------------------------------------------------ #
    #  Search / session management                                        #
    # ------------------------------------------------------------------ #

    @staticmethod
    def _label_for(tc):
        return f"#{tc.get('_id', '?')}  —  {tc.get('System.Title', '(no title)')}"

    def _rebuild_available(self):
        """Show every PBI case that isn't already in the session (a case lives in
        exactly one list at a time, so adding/removing moves it between them)."""
        self._available.clear()
        session_ids = {c.get("_id") for c in self._session}
        for tc in self._cases:
            if tc.get("_id") in session_ids:
                continue
            item = QListWidgetItem(self._label_for(tc))
            item.setData(Qt.UserRole, tc)
            self._available.addItem(item)
        self._apply_search()

    def _apply_search(self):
        query = self._search.text().strip().lower()
        for row in range(self._available.count()):
            item = self._available.item(row)
            item.setHidden(bool(query) and query not in item.text().lower())

    def _selected_available(self):
        return [it.data(Qt.UserRole) for it in self._available.selectedItems()]

    def _selected_session(self):
        return [it.data(Qt.UserRole) for it in self._session_list.selectedItems()]

    def _add_to_session(self):
        existing = {c.get("_id") for c in self._session}
        added = False
        for tc in self._selected_available():
            if tc.get("_id") not in existing:
                self._session.append(tc)
                existing.add(tc.get("_id"))
                added = True
        if added:
            self._rebuild_available()
            self._rebuild_session_list()

    def _rebuild_session_list(self):
        self._session_list.clear()
        for tc in self._session:
            item = QListWidgetItem(self._label_for(tc))
            item.setData(Qt.UserRole, tc)
            self._session_list.addItem(item)
        n = len(self._session)
        self._session_lbl.setText(f"<b>Session ({n})</b>")
        self._start_btn.setEnabled(n > 0)

    def _remove_from_session(self):
        remove_ids = {c.get("_id") for c in self._selected_session()}
        if not remove_ids:
            return
        self._session = [c for c in self._session if c.get("_id") not in remove_ids]
        self._rebuild_session_list()
        self._rebuild_available()

    def _clear_session(self):
        self._session = []
        self._rebuild_session_list()
        self._rebuild_available()

    def _start_run(self):
        from app.gui.helpers import warn_if_token_expired
        if warn_if_token_expired(self, self.app_state.token_manager):
            return
        if not self._session:
            return
        runner = TestRunner(self.app_state, list(self._session))
        self._open_runners.append(runner)
        runner.show()
        runner.raise_()
        runner.activateWindow()

    # ------------------------------------------------------------------ #
    #  Theme                                                              #
    # ------------------------------------------------------------------ #

    def refresh_theme(self):
        t = theme.tokens()
        self._header_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self._refresh_btn.setStyleSheet(theme.btn_neutral_qss())
        self._add_btn.setStyleSheet(theme.btn_primary_qss("padding: 6px 10px;"))
        self._remove_btn.setStyleSheet(theme.btn_neutral_qss())
        self._start_btn.setStyleSheet(theme.btn_primary_qss("font-size: 13px; padding: 0 20px;"))
