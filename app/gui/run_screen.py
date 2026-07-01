"""Run Tests tab — build a session of test cases, then launch the runner.

Loads the PBI's test cases (with steps + preconditions), lets you search and
multi-select them, accumulate a session, and open the always-on-top runner.
"""

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton, QListWidget,
    QListWidgetItem, QLineEdit, QSplitter, QFrame, QMessageBox, QProgressBar,
    QStyledItemDelegate, QStyle, QComboBox,
)
from PyQt5.QtCore import Qt, QThreadPool, QTimer
from PyQt5.QtGui import QCursor, QColor, QBrush

from app.utils.worker import Worker
from app.utils import theme
from app.gui.test_runner import TestRunner
from app.gui.delegates import HoverTrackerMixin
from app.gui.checkable_combo import CheckableComboBox


class _StatusColorDelegate(HoverTrackerMixin, QStyledItemDelegate):
    """Paints each row's status tint (a translucent BackgroundRole colour) over
    the list background, keeping the theme's normal text colour. Done in a
    delegate because the list lives under a styled QTabWidget, and
    QStyleSheetStyle otherwise ignores item background brushes set via
    setBackground(). Also overlays a subtle hover tint on the row under the
    mouse (see app.gui.delegates)."""

    def __init__(self, view):
        super().__init__(view)
        self._init_hover(view)

    def initStyleOption(self, option, index):
        # Plain (non-status) rows are painted by super().paint() below; tint
        # their background here so the hover never recolours the text.
        super().initStyleOption(option, index)
        if self._is_hovered(option, index):
            self._apply_hover_bg(option)

    def paint(self, painter, option, index):
        brush = index.data(Qt.BackgroundRole)
        color = brush.color() if isinstance(brush, QBrush) else None
        if (color is not None and color.alpha() > 0
                and not (option.state & QStyle.State_Selected)):
            painter.save()
            # Opaque base first, so every row of a given status is the exact same
            # shade (no alternating-row tint, no alpha stacking on repaint).
            painter.fillRect(option.rect, option.palette.base().color())
            painter.fillRect(option.rect, color)   # translucent status tint
            if self._is_hovered(option, index):
                # Tint the background *before* the text, so text colour is kept.
                painter.fillRect(option.rect, self._hover_overlay_color())
            painter.setPen(option.palette.text().color())
            rect = option.rect.adjusted(6, 0, -6, 0)
            text = str(index.data(Qt.DisplayRole) or "")
            painter.drawText(
                rect, Qt.AlignVCenter | Qt.AlignLeft,
                option.fontMetrics.elidedText(text, Qt.ElideRight, rect.width()))
            painter.restore()
        else:
            super().paint(painter, option, index)


class RunScreen(QWidget):
    """Tab for assembling and launching a manual test execution session."""

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._cases = []           # all loaded case dicts (rows map 1:1)
        self._session = []         # case dicts queued for the run
        self._loaded_pbi = None
        self._open_runners = []    # keep references so windows aren't GC'd
        self._plan_poll = None     # QTimer that waits for test-plan detection
        self._plan_poll_ticks = 0
        self._build_ui()

    # ------------------------------------------------------------------ #
    #  Lifecycle / loading                                                #
    # ------------------------------------------------------------------ #

    def showEvent(self, event):
        super().showEvent(event)
        self.ensure_loaded()

    def ensure_loaded(self):
        pbi = self.app_state.pbi_id
        if not pbi:
            return
        # The case list + outcome colours depend on the test plan/suite. If that's
        # still being resolved, show a loading panel and wait for it to finish.
        if getattr(self.app_state, "test_plan_detecting", False) and pbi != self._loaded_pbi:
            self._show_plan_loading()
            return
        if pbi != self._loaded_pbi:
            # PBI changed (or first load) — a session built for the previous PBI
            # is no longer valid (those test cases aren't linked to this PBI), so
            # clear it before loading this PBI's cases.
            if self._session:
                self._clear_session()
            self._load_cases()
        else:
            # Same PBI revisited — refresh the status colours (the points cache
            # may have been invalidated by a submitted run).
            self._prefetch_points()

    def _show_plan_loading(self):
        self._splitter.hide()
        self._loading_panel.show()
        self._plan_poll_ticks = 0
        self._update_plan_loading()
        if self._plan_poll is None:
            self._plan_poll = QTimer(self)
            self._plan_poll.setInterval(150)
            self._plan_poll.timeout.connect(self._poll_plan)
        self._plan_poll.start()

    def _hide_plan_loading(self):
        if self._plan_poll is not None:
            self._plan_poll.stop()
        self._loading_panel.hide()
        self._splitter.show()

    def _poll_plan(self):
        self._plan_poll_ticks += 1
        # Resolved (or errored) → stop waiting and load. Also give up after ~30s
        # so a stuck detection never traps the page on the loading screen.
        if (not getattr(self.app_state, "test_plan_detecting", False)
                or self._plan_poll_ticks > 200):
            self._hide_plan_loading()
            pbi = self.app_state.pbi_id
            if pbi and pbi != self._loaded_pbi:
                if self._session:
                    self._clear_session()
                self._load_cases()
            elif pbi:
                self._prefetch_points()
            return
        self._update_plan_loading()

    def _update_plan_loading(self):
        prog = getattr(self.app_state, "test_plan_progress", None)
        if prog and prog[1]:
            cur, total = prog
            self._loading_bar.setRange(0, total)
            self._loading_bar.setValue(cur)
            self._loading_lbl.setText(
                f"Checking if a test plan exists for this PBI…  (plan {cur} of {total})")
        else:
            self._loading_bar.setRange(0, 0)   # indeterminate
            self._loading_lbl.setText("Checking if a test plan exists for this PBI…")

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
        self._prefetch_points()

    def adopt_shared_cache(self):
        """Rebuild the PBI case list from the shared existing-cases cache (updated
        in place right after a create/update on this PBI) — no network re-fetch of
        the case list. No-op unless the cache is authoritative for the current PBI.
        Session (right-hand) selections are preserved."""
        pbi = self.app_state.pbi_id
        cache = self.app_state.existing_cases
        if not pbi or cache is None or self.app_state.existing_cases_pbi != pbi:
            return
        self._on_cases_loaded(pbi, (cache, len(cache)))

    def _on_cases_error(self, exc):
        self._header_lbl.setText(f"Could not load test cases: {exc}")
        self._refresh_btn.setEnabled(True)

    def _prefetch_points(self):
        """Warm app_state's test-points cache in the background so the runner can
        show previous outcomes instantly when it opens. Read-only; silent on
        failure. Caches the whole suite (all loaded cases) so any session subset
        is covered."""
        plan, suite = self.app_state.test_plan_id, self.app_state.suite_id
        if not (plan and suite):
            return
        key = (plan, suite)
        if key in self.app_state.test_points_by_suite:
            self._color_lists()   # already cached -> colour now
            return
        if not any(c.get("_id") for c in self._cases):
            return
        # Fetch the WHOLE suite's points (no testCaseId filter). One read covers
        # every loaded case AND sidesteps a long comma-separated testCaseId list,
        # which silently fails for a big PBI (~50 cases) — that left every row
        # uncoloured even though the data existed (the runner, fetching only its
        # small session subset, still got outcomes).
        worker = Worker(self.app_state.client.get_test_points, plan, suite)
        worker.signals.result.connect(lambda pts, k=key: self._on_points_prefetched(k, pts))
        worker.signals.error.connect(lambda _exc: None)
        QThreadPool.globalInstance().start(worker)

    def _on_points_prefetched(self, key, pts):
        self.app_state.test_points_by_suite[key] = pts
        self._color_lists()
        # Outcomes just became known — re-run a result filter if one is active.
        if self._result_combo.checked_data():
            self._apply_filters()

    # Status colours, applied as a translucent tint over the list background so
    # they read as dim hints rather than bold blocks. _OUTCOME_ALPHA (0–255) is
    # the one knob: lower = dimmer/more transparent.
    _OUTCOME_BG = {
        "passed": "#1E5E1E",          # dark green
        "failed": "#7A2222",          # dark red
        "blocked": "#6E5A12",         # dark amber / yellow
        "paused": "#4A2A6E",          # dark purple
        "notapplicable": "#4A4A4A",   # dark neutral grey
    }
    _OUTCOME_ACTIVE_BG = "#1E3F6E"    # dark blue — not yet run (Active)
    _OUTCOME_ALPHA = 150             # darker, richer tint (still slightly translucent)

    @classmethod
    def _outcome_label(cls, oc):
        return {"passed": "Passed", "failed": "Failed", "blocked": "Blocked",
                "paused": "Paused",
                "notapplicable": "Not Applicable"}.get(oc, "Active (not run)")

    def _color_list(self, list_widget):
        """Tint each row of `list_widget` by its case's last recorded outcome so
        the status is readable at a glance. Uses the prefetched points cache;
        rows with no data yet are left at the default colour."""
        key = (self.app_state.test_plan_id, self.app_state.suite_id)
        points = self.app_state.test_points_by_suite.get(key)
        if not points:
            return
        by_tc = {}
        for p in points:
            tc = p.get("test_case_id")
            if tc and tc not in by_tc:
                by_tc[tc] = (p.get("last_outcome") or "").lower()
        for row in range(list_widget.count()):
            item = list_widget.item(row)
            case = item.data(Qt.UserRole) or {}
            oc = by_tc.get(case.get("_id"))
            if oc is None:
                continue
            col = QColor(self._OUTCOME_BG.get(oc, self._OUTCOME_ACTIVE_BG))
            col.setAlpha(self._OUTCOME_ALPHA)
            item.setBackground(QBrush(col))
            item.setToolTip(f"Last result: {self._outcome_label(oc)}")

    def _color_lists(self):
        """Re-tint both the PBI list and the session list from the points cache."""
        self._color_list(self._available)
        self._color_list(self._session_list)

    # Legend explaining the row tints. ("_active" = has a test point but no
    # recorded result yet → the dark-blue _OUTCOME_ACTIVE_BG tint.)
    _LEGEND_ITEMS = [
        ("Passed", "passed"), ("Failed", "failed"), ("Blocked", "blocked"),
        ("Paused", "paused"), ("N/A", "notapplicable"), ("Not run", "_active"),
    ]

    def _build_legend(self):
        """A small row of colour swatches + labels matching the list row tints."""
        row = QHBoxLayout()
        row.setContentsMargins(2, 0, 2, 0)
        row.setSpacing(6)
        self._legend_caption = QLabel("Last result:")
        row.addWidget(self._legend_caption)
        self._legend_swatches = []   # (swatch_label, outcome_key)
        self._legend_labels = [self._legend_caption]
        for text, oc in self._LEGEND_ITEMS:
            row.addSpacing(6)
            sw = QLabel()
            sw.setFixedSize(13, 13)
            self._legend_swatches.append((sw, oc))
            row.addWidget(sw)
            lbl = QLabel(text)
            self._legend_labels.append(lbl)
            row.addWidget(lbl)
        row.addStretch()
        self._color_legend()
        return row

    def _color_legend(self):
        """Fill each swatch with the SAME colour a row gets — the status tint
        composited over the list's base — and theme the labels."""
        t = theme.tokens()
        base = self._available.palette().base().color()
        a = self._OUTCOME_ALPHA / 255.0
        for sw, oc in self._legend_swatches:
            hexcol = self._OUTCOME_ACTIVE_BG if oc == "_active" else self._OUTCOME_BG[oc]
            tint = QColor(hexcol)
            r = round(base.red() * (1 - a) + tint.red() * a)
            g = round(base.green() * (1 - a) + tint.green() * a)
            b = round(base.blue() * (1 - a) + tint.blue() * a)
            sw.setStyleSheet(
                f"background: rgb({r},{g},{b}); border: 1px solid {t['border']}; border-radius: 3px;")
        for lbl in self._legend_labels:
            lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")

    # ------------------------------------------------------------------ #
    #  UI                                                                 #
    # ------------------------------------------------------------------ #

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(8)

        hdr = QHBoxLayout()
        intro = QLabel(
            "Add test cases to a session, then start the runner to record outcomes."
        )
        intro.setWordWrap(True)
        intro.setStyleSheet("color: #555;")
        hdr.addWidget(intro, 1)
        from app.utils import icons
        self._refresh_btn = QPushButton("Refresh")
        self._refresh_btn.setIcon(icons.icon("refresh", size=15))
        self._refresh_btn.setStyleSheet(theme.btn_neutral_qss())
        self._refresh_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._refresh_btn.clicked.connect(self._load_cases)
        hdr.addWidget(self._refresh_btn)
        layout.addLayout(hdr)

        self._header_lbl = QLabel("")
        self._header_lbl.setStyleSheet("color: #888; font-size: 11px;")
        layout.addWidget(self._header_lbl)

        from app.gui.grip_splitter import GripSplitter
        splitter = GripSplitter(Qt.Horizontal)

        # -- Left: available cases ----------------------------------------
        left = QWidget()
        lv = QVBoxLayout(left)
        lv.setContentsMargins(0, 0, 0, 0)
        lv.setSpacing(4)
        self._avail_lbl = QLabel("<b>Test cases on this PBI</b>")
        lv.addWidget(self._avail_lbl)
        self._search = QLineEdit()
        self._search.setPlaceholderText("Search by ID or title…")
        self._search.setClearButtonEnabled(True)
        self._search.textChanged.connect(lambda _: self._on_filter_changed())
        lv.addWidget(self._search)
        lv.addLayout(self._build_filter_row())
        self._available = QListWidget()
        # No alternating row colours here — they'd show through the translucent
        # status tint as two shades per outcome and look like a jumble.
        self._available.setAlternatingRowColors(False)
        self._available.setItemDelegate(_StatusColorDelegate(self._available))
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
        self._add_btn = QPushButton("Add")
        self._add_btn.setIcon(icons.icon("arrow-right", size=15))
        self._add_btn.setLayoutDirection(Qt.RightToLeft)
        self._add_btn.setFixedWidth(96)
        self._add_btn.setToolTip("Add the selected test case(s) to the session")
        self._add_btn.setStyleSheet(theme.btn_neutral_qss("padding: 6px 10px;"))
        self._add_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._add_btn.clicked.connect(self._add_to_session)
        mv.addWidget(self._add_btn)
        self._remove_btn = QPushButton("Remove")
        self._remove_btn.setIcon(icons.icon("arrow-left", size=15))
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
        self._session_list.setAlternatingRowColors(False)
        self._session_list.setItemDelegate(_StatusColorDelegate(self._session_list))
        self._session_list.setSelectionMode(QListWidget.ExtendedSelection)
        self._session_list.itemDoubleClicked.connect(lambda _it: self._remove_from_session())
        rv.addWidget(self._session_list, 1)
        self._start_btn = QPushButton("Start run")
        self._start_btn.setIcon(icons.icon("play", color="white", size=15))
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
        self._splitter = splitter
        layout.addWidget(splitter, 1)
        layout.addLayout(self._build_legend())

        # Loading panel shown while the test plan/suite is being resolved (the
        # case list + outcome colours can't load until that's known).
        self._loading_panel = QWidget()
        lp = QVBoxLayout(self._loading_panel)
        lp.addStretch()
        spin = QLabel()
        spin.setPixmap(icons.pixmap("flask", color=theme.tokens()["text_dim2"], size=40))
        spin.setAlignment(Qt.AlignCenter)
        lp.addWidget(spin)
        self._loading_lbl = QLabel("Checking if a test plan exists for this PBI…")
        self._loading_lbl.setAlignment(Qt.AlignCenter)
        self._loading_lbl.setStyleSheet("font-size: 14px; color: #888;")
        lp.addWidget(self._loading_lbl)
        bar_row = QHBoxLayout()
        bar_row.addStretch()
        self._loading_bar = QProgressBar()
        self._loading_bar.setRange(0, 0)   # indeterminate until the count is known
        self._loading_bar.setFixedWidth(360)
        bar_row.addWidget(self._loading_bar)
        bar_row.addStretch()
        lp.addLayout(bar_row)
        lp.addStretch()
        self._loading_panel.hide()
        layout.addWidget(self._loading_panel, 1)

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
        for tc in self._sorted_cases():
            if tc.get("_id") in session_ids:
                continue
            item = QListWidgetItem(self._label_for(tc))
            item.setData(Qt.UserRole, tc)
            self._available.addItem(item)
        self._apply_filters()
        self._color_list(self._available)

    # ------------------------------------------------------------------ #
    #  Filtering (left "Test cases on this PBI" list)                     #
    # ------------------------------------------------------------------ #

    def _build_filter_row(self):
        """A compact bar under the search box: filter by last result and created
        date, sort by date/title, plus a clear-all button shown only when a
        filter is active. Combos are themed by the window's style_combos() pass."""
        from app.utils import icons
        row = QHBoxLayout()
        row.setContentsMargins(0, 0, 0, 0)
        row.setSpacing(6)

        self._filter_icon = QLabel()
        self._filter_icon.setPixmap(
            icons.pixmap("filter", color=theme.tokens()["text_dim2"], size=15))
        self._filter_icon.setToolTip("Filter and sort the list of test cases")
        row.addWidget(self._filter_icon)

        self._result_combo = CheckableComboBox(all_text="All results")
        self._result_combo.setToolTip(
            "Filter by the last recorded result — tick one or more")
        for label, data in (
            ("Passed", "passed"), ("Failed", "failed"), ("Blocked", "blocked"),
            ("Paused", "paused"), ("Not applicable", "notapplicable"),
            ("Not run", "notrun"),
        ):
            self._result_combo.addCheckItem(label, data)

        self._sort_combo = QComboBox()
        self._sort_combo.setToolTip("Sort the list of test cases")
        for label, data in (
            ("Default order", None), ("Newest first", "newest"),
            ("Oldest first", "oldest"), ("Title A–Z", "az"), ("Title Z–A", "za"),
        ):
            self._sort_combo.addItem(label, data)

        self._result_combo.changed.connect(self._on_filter_changed)
        self._sort_combo.currentIndexChanged.connect(lambda _i: self._on_sort_changed())
        for cb in (self._result_combo, self._sort_combo):
            cb.setMinimumWidth(92)
            cb.setCursor(QCursor(Qt.PointingHandCursor))
            row.addWidget(cb, 1)

        self._clear_filters_btn = QPushButton()
        self._clear_filters_btn.setIcon(icons.icon("x", size=13))
        self._clear_filters_btn.setToolTip("Clear all filters")
        self._clear_filters_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._clear_filters_btn.setFixedSize(28, 28)
        self._clear_filters_btn.setStyleSheet(theme.btn_ghost_qss("padding: 4px;"))
        self._clear_filters_btn.clicked.connect(self._clear_filters)
        self._clear_filters_btn.setVisible(False)
        row.addWidget(self._clear_filters_btn)
        return row

    def _filters_active(self):
        # Sort order is not a filter, so it never affects the clear button/count.
        return bool(
            self._search.text().strip()
            or self._result_combo.checked_data())

    def _on_filter_changed(self):
        self._apply_filters()
        self._clear_filters_btn.setVisible(self._filters_active())

    def _on_sort_changed(self):
        # Re-order needs a rebuild (filters/colours re-apply inside).
        self._rebuild_available()

    def _clear_filters(self):
        """Reset the search + result filter (sort order is left as chosen).
        clear_checks emits nothing, so a single _on_filter_changed does the one
        refresh."""
        self._search.blockSignals(True)
        self._search.clear()
        self._result_combo.clear_checks()
        self._search.blockSignals(False)
        self._on_filter_changed()

    def _outcomes_by_case(self):
        """Map {test_case_id: outcome} (lower-case) from the prefetched points
        cache. Cases with a point but no recorded result — or no point at all —
        are absent, and treated as 'not run' by the filter."""
        key = (self.app_state.test_plan_id, self.app_state.suite_id)
        points = self.app_state.test_points_by_suite.get(key) or []
        by_tc = {}
        for p in points:
            tc = p.get("test_case_id")
            oc = (p.get("last_outcome") or "").lower()
            if tc and oc and tc not in by_tc:
                by_tc[tc] = oc
        return by_tc

    @staticmethod
    def _created_dt(case):
        """Parse a case's System.CreatedDate to a UTC datetime, or None. Reads
        only the date+time prefix so varied fractional-second formats from ADO
        never break parsing."""
        raw = case.get("System.CreatedDate")
        if not raw:
            return None
        try:
            from datetime import datetime, timezone
            dt = datetime.strptime(str(raw)[:19], "%Y-%m-%dT%H:%M:%S")
            return dt.replace(tzinfo=timezone.utc)
        except Exception:
            return None

    def _sorted_cases(self):
        """`self._cases` ordered for the current sort combo (default = as loaded).
        Cases missing a created date sort to the bottom of either date order."""
        mode = self._sort_combo.currentData()
        cases = list(self._cases)
        if mode in ("newest", "oldest"):
            from datetime import datetime, timezone
            lo = datetime.min.replace(tzinfo=timezone.utc)
            hi = datetime.max.replace(tzinfo=timezone.utc)
            if mode == "newest":   # newest→oldest, undated last
                cases.sort(key=lambda c: self._created_dt(c) or lo, reverse=True)
            else:                  # oldest→newest, undated last
                cases.sort(key=lambda c: self._created_dt(c) or hi)
        elif mode in ("az", "za"):
            cases.sort(key=lambda c: str(c.get("System.Title", "")).lower(),
                       reverse=(mode == "za"))
        return cases

    def _apply_filters(self):
        """Hide every available case that doesn't match the search text and the
        result filter, then update the count. The result filter is multi-select:
        a case matches if its outcome is any of the ticked results (no ticks =
        show all)."""
        query = self._search.text().strip().lower()
        results = set(self._result_combo.checked_data())
        outcomes = self._outcomes_by_case() if results else {}

        visible = 0
        for row in range(self._available.count()):
            item = self._available.item(row)
            case = item.data(Qt.UserRole) or {}
            hidden = bool(query) and query not in item.text().lower()
            if not hidden and results:
                oc = outcomes.get(case.get("_id")) or "notrun"
                hidden = oc not in results
            item.setHidden(hidden)
            if not hidden:
                visible += 1
        self._update_filter_count(visible)

    def _update_filter_count(self, visible):
        total = self._available.count()
        if total and self._filters_active():
            self._avail_lbl.setText(
                "<b>Test cases on this PBI</b>  "
                f"<span style='color:{theme.tokens()['text_dim2']}'>"
                f"{visible} of {total}</span>")
        else:
            self._avail_lbl.setText("<b>Test cases on this PBI</b>")

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
        self._color_list(self._session_list)

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
        self.register_runner(runner)
        runner.show()
        runner.raise_()
        runner.activateWindow()

    def register_runner(self, runner):
        """Track an open runner and recolour our lists live when it submits
        results. Also used by the main window's resume-run path."""
        self._open_runners.append(runner)
        try:
            runner.results_submitted.connect(self._on_results_submitted)
        except Exception:
            pass

    def _on_results_submitted(self):
        """A runner just recorded outcomes and updated the shared points cache —
        recolour both lists immediately (and re-apply an active result filter,
        since a row's outcome may have changed what it should match)."""
        self._color_lists()
        if self._result_combo.checked_data():
            self._apply_filters()

    # ------------------------------------------------------------------ #
    #  Theme                                                              #
    # ------------------------------------------------------------------ #

    def refresh_theme(self):
        from app.utils import icons
        t = theme.tokens()
        self._header_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self._refresh_btn.setStyleSheet(theme.btn_neutral_qss())
        self._refresh_btn.setIcon(icons.icon("refresh", size=15))
        self._add_btn.setStyleSheet(theme.btn_neutral_qss("padding: 6px 10px;"))
        self._add_btn.setIcon(icons.icon("arrow-right", size=15))
        self._remove_btn.setStyleSheet(theme.btn_neutral_qss())
        self._remove_btn.setIcon(icons.icon("arrow-left", size=15))
        self._start_btn.setStyleSheet(theme.btn_primary_qss("font-size: 13px; padding: 0 20px;"))
        self._start_btn.setIcon(icons.icon("play", color="white", size=15))
        self._filter_icon.setPixmap(icons.pixmap("filter", color=t["text_dim2"], size=15))
        self._clear_filters_btn.setIcon(icons.icon("x", size=13))
        self._clear_filters_btn.setStyleSheet(theme.btn_ghost_qss("padding: 4px;"))
        self._update_filter_count(  # refresh the count's dim colour for the theme
            sum(0 if self._available.item(r).isHidden() else 1
                for r in range(self._available.count())))
        self._color_legend()   # re-composite swatches over the new theme base
