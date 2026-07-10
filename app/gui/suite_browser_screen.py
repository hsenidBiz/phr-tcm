"""Test Suites tab — browse test plans and their suite hierarchy (read-only).

Left: a lazy tree of the project's test plans with each plan's suites nested
beneath it like folders (mirroring Azure DevOps' Test Plans → Test Suites UI).
Right: the selected suite's test points — case title, outcome, id, state,
configuration and tester — with rows tinted by last outcome exactly like the
Run Tests lists.

Everything loads in the background and only on demand: plans on first show, a
plan's suites when its node is expanded, a suite's points when it is selected
(cached per session). This view is project-wide — it deliberately ignores the
currently selected PBI — and is pure GET: it never creates or modifies
anything in Azure DevOps.
"""

import webbrowser
from pathlib import Path
from urllib.parse import quote

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton, QLineEdit,
    QTreeWidget, QTreeWidgetItem, QHeaderView, QAbstractItemView, QProgressBar,
    QMessageBox,
)
from PyQt5.QtCore import Qt, QThreadPool, pyqtSignal
from PyQt5.QtGui import QBrush, QColor, QCursor

from app.utils.worker import Worker
from app.utils import theme, icons
from app.utils.logger import get_logger
from app.gui import outcome_style
from app.gui.delegates import StatusTintDelegate, apply_hover
from app.gui.grip_splitter import GripSplitter

log = get_logger(__name__)

_ROLE = Qt.UserRole

# Suite-type → icon glyph, matching ADO's visual language.
_SUITE_ICONS = {
    "requirementTestSuite": "bookmark",
    "staticTestSuite": "folder",
    "dynamicTestSuite": "filter",
}


def _tree_qss() -> str:
    t = theme.tokens()
    return (
        f"QTreeWidget {{ background: {t['surface']}; border: 1px solid {t['border']}; "
        f"border-radius: 6px; color: {t['text']}; outline: none; font-size: 13px; }}"
        "QTreeWidget::item { min-height: 26px; padding: 1px 4px; }"
        f"QTreeWidget::item:selected {{ background: {t['accent']}; color: #ffffff; }}"
        f"QHeaderView::section {{ background: {t['surface2']}; color: {t['text_dim']}; "
        f"border: none; border-bottom: 1px solid {t['border']}; border-right: 1px solid {t['border']}; "
        "padding: 5px 8px; font-size: 12px; }"
    )


class SuiteBrowserScreen(QWidget):
    """Read-only Test Plan → Test Suite → Test Points browser. Its cases can be
    sent to the Run Tests / Edit tabs via the two request signals."""

    # payload: {"case_ids": [...], "context": {plan_id, suite_id, suite_name,
    #           pbi_id, pbi_title}}
    run_suite_requested = pyqtSignal(dict)
    edit_suite_requested = pyqtSignal(dict)

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._loaded_key = None          # (org_url, project) the tree was built for
        self._gen = 0                    # bumped on every reset; stale workers ignored
        self._points_req = 0             # bumped per points request; stale results ignored
        self._points_cache = {}          # (plan_id, suite_id) -> list[point dict]
        self._current_suite = None       # (plan_id, suite_id) shown on the right
        self._current_suite_data = None  # full node data of the shown suite
        self._preloading = False         # eager background load (post-login) in flight
        self._preload_pending = 0        # plans whose suites are still loading
        self._build_ui()

    # ------------------------------------------------------------------ #
    #  Loading — plans                                                     #
    # ------------------------------------------------------------------ #

    def showEvent(self, event):
        super().showEvent(event)
        self.ensure_loaded()

    def _key(self):
        client = self.app_state.client
        if client is None:
            return None
        tm = client.tm
        return (tm.org_url, tm.project)

    def ensure_loaded(self):
        key = self._key()
        if key is None:
            self._tree_status.setText("Sign in and choose a project to browse its test plans.")
            self._tree_status.show()
            return
        if key != self._loaded_key:
            self._reset()
            self._loaded_key = key
            self._load_plans(use_cache=True)

    def preload(self):
        """Warm-load the whole plan/suite tree in the background as soon as the
        user reaches the main screen — before the Test Suites tab is ever opened
        — so it's already populated on arrival. A progress bar tracks how many
        plans' suites have loaded. No-op once the current project is loaded."""
        key = self._key()
        if key is None or key == self._loaded_key:
            return
        self._reset()
        self._loaded_key = key
        self._preloading = True
        self._load_plans(use_cache=True)

    def _reset(self):
        self._gen += 1
        self._points_req += 1
        self._points_cache.clear()
        self._current_suite = None
        self._preloading = False
        self._preload_pending = 0
        self._end_progress()
        self._tree.clear()
        self._points.clear()
        self._show_points_hint("Select a test suite to see its test points.")

    def _end_progress(self):
        self._progress.hide()

    def _on_refresh(self):
        """Manual refresh: drop every cache and re-fetch the plan list."""
        if self._key() is None:
            return
        self._reset()
        self._loaded_key = self._key()
        self._load_plans(use_cache=False)

    def _load_plans(self, use_cache: bool):
        gen = self._gen
        self._tree_status.setText("Loading test plans…")
        self._tree_status.show()
        self._refresh_btn.setEnabled(False)
        # Busy (indeterminate) until the plan count is known; a preload then
        # switches it to a determinate "N / M plans" bar once suites start.
        self._progress.setTextVisible(False)
        self._progress.setRange(0, 0)
        self._progress.show()
        worker = Worker(self.app_state.client.get_test_plans, use_cache=use_cache)
        worker.signals.result.connect(lambda plans: self._on_plans(gen, plans))
        worker.signals.error.connect(lambda exc: self._on_plans_error(gen, exc))
        QThreadPool.globalInstance().start(worker)

    def _on_plans(self, gen: int, plans: list):
        if gen != self._gen:
            return
        self._refresh_btn.setEnabled(True)
        self._tree_status.hide()
        if not plans:
            self._end_progress()
            self._preloading = False
            self._tree_status.setText("No test plans exist in this project yet.")
            self._tree_status.show()
            return
        plan_icon = icons.icon("briefcase", size=15)
        plan_items = []
        for plan in sorted(plans, key=lambda p: (p.get("name") or "").lower()):
            item = QTreeWidgetItem([plan.get("name", "") or f"Plan {plan.get('id')}"])
            item.setIcon(0, plan_icon)
            item.setData(0, _ROLE, {"kind": "plan", "plan_id": plan.get("id"),
                                    "loaded": False})
            item.setToolTip(0, plan.get("name", ""))
            self._tree.addTopLevelItem(item)
            # Dummy child so the expander arrow shows before the suites load.
            item.addChild(self._info_item("Loading…", kind="loading"))
            plan_items.append((item, plan.get("id")))
        self._apply_tree_filter()
        if self._preloading:
            self._start_suite_preload(gen, plan_items)
        else:
            self._end_progress()

    def _start_suite_preload(self, gen: int, plan_items: list):
        """Eagerly fetch every plan's suites in the background, advancing the
        progress bar as each plan resolves. Marking each plan 'loaded' up front
        means a user expand during the preload won't trigger a duplicate fetch."""
        self._preload_pending = len(plan_items)
        self._progress.setRange(0, len(plan_items))
        self._progress.setValue(0)
        self._progress.setFormat("Loading suites… %v / %m plans")
        self._progress.setTextVisible(True)
        self._progress.show()
        for item, plan_id in plan_items:
            data = item.data(0, _ROLE) or {}
            data["loaded"] = True
            item.setData(0, _ROLE, data)
            worker = Worker(self.app_state.client.get_all_suites, plan_id)
            worker.signals.result.connect(
                lambda suites, it=item, pid=plan_id:
                self._on_preload_suites(gen, it, pid, suites))
            worker.signals.error.connect(
                lambda exc, it=item, pid=plan_id:
                self._on_preload_suites_error(gen, it, pid, exc))
            QThreadPool.globalInstance().start(worker)

    def _on_preload_suites(self, gen: int, item: QTreeWidgetItem, plan_id: int,
                           suites: list):
        if gen != self._gen:
            return
        self._on_suites(gen, item, plan_id, suites)
        self._preload_step(gen)

    def _on_preload_suites_error(self, gen: int, item: QTreeWidgetItem,
                                 plan_id: int, exc: Exception):
        if gen != self._gen:
            return
        # Leaves the plan retryable (via expand); the preload still advances.
        self._on_suites_error(gen, item, plan_id, exc)
        self._preload_step(gen)

    def _preload_step(self, gen: int):
        if gen != self._gen:
            return
        self._preload_pending -= 1
        self._progress.setValue(self._progress.maximum() - max(self._preload_pending, 0))
        if self._preload_pending <= 0:
            self._preloading = False
            self._end_progress()

    def _on_plans_error(self, gen: int, exc: Exception):
        if gen != self._gen:
            return
        self._refresh_btn.setEnabled(True)
        self._preloading = False
        self._end_progress()
        self._loaded_key = None   # retry on next show
        self._tree_status.setText(f"Could not load test plans: {exc}")
        self._tree_status.show()

    # ------------------------------------------------------------------ #
    #  Loading — a plan's suites (on first expand)                         #
    # ------------------------------------------------------------------ #

    @staticmethod
    def _info_item(text: str, kind: str = "info") -> QTreeWidgetItem:
        it = QTreeWidgetItem([text])
        it.setData(0, _ROLE, {"kind": kind})
        it.setFlags(Qt.ItemIsEnabled)   # not selectable
        return it

    def _on_item_expanded(self, item: QTreeWidgetItem):
        data = item.data(0, _ROLE) or {}
        if data.get("kind") != "plan" or data.get("loaded"):
            return
        data["loaded"] = True            # only ever fetch a plan's suites once
        item.setData(0, _ROLE, data)
        gen = self._gen
        plan_id = data["plan_id"]
        worker = Worker(self.app_state.client.get_all_suites, plan_id)
        worker.signals.result.connect(
            lambda suites: self._on_suites(gen, item, plan_id, suites))
        worker.signals.error.connect(
            lambda exc: self._on_suites_error(gen, item, plan_id, exc))
        QThreadPool.globalInstance().start(worker)

    def _on_suites(self, gen: int, plan_item: QTreeWidgetItem, plan_id: int,
                   suites: list):
        if gen != self._gen:
            return
        plan_item.takeChildren()
        by_parent = {}
        for s in suites:
            by_parent.setdefault(s["parent_id"], []).append(s)
        # The plan's root suite (parent_id None) mirrors the plan itself in ADO,
        # so its children attach directly under the plan node.
        roots = by_parent.get(None, [])
        top = []
        for r in roots:
            top.extend(by_parent.get(r["id"], []))
        if not top:
            # A plan with no suites has nothing to browse — drop it from the
            # tree entirely (the preload resolves every plan, so empties
            # disappear shortly after load; a lazy expand of one does the same).
            idx = self._tree.indexOfTopLevelItem(plan_item)
            if idx >= 0:
                self._tree.takeTopLevelItem(idx)
            if self._tree.topLevelItemCount() == 0:
                self._tree_status.setText(
                    "No test plans with test suites in this project yet.")
                self._tree_status.show()
            return

        def add_children(parent_item, children):
            for s in sorted(children, key=lambda x: (x.get("name") or "").lower()):
                node = QTreeWidgetItem([s["name"]])
                node.setIcon(0, icons.icon(
                    _SUITE_ICONS.get(s["suite_type"], "folder"), size=15))
                node.setData(0, _ROLE, {
                    "kind": "suite", "plan_id": plan_id, "suite_id": s["id"],
                    "suite_type": s["suite_type"], "base_name": s["name"],
                    "requirement_id": s.get("requirement_id"),
                })
                node.setToolTip(0, s["name"])
                parent_item.addChild(node)
                add_children(node, by_parent.get(s["id"], []))

        add_children(plan_item, top)
        self._apply_tree_filter()

    def _on_suites_error(self, gen: int, plan_item: QTreeWidgetItem, plan_id: int,
                         exc: Exception):
        if gen != self._gen:
            return
        log.info("Could not list suites for plan %s: %s", plan_id, exc)
        plan_item.takeChildren()
        plan_item.addChild(self._info_item("(can't read this plan's suites)"))
        # Allow retrying on the next expand.
        data = plan_item.data(0, _ROLE) or {}
        data["loaded"] = False
        plan_item.setData(0, _ROLE, data)

    # ------------------------------------------------------------------ #
    #  Loading — a suite's points (on selection)                           #
    # ------------------------------------------------------------------ #

    def _on_tree_selection(self):
        items = self._tree.selectedItems()
        data = (items[0].data(0, _ROLE) or {}) if items else {}
        if data.get("kind") != "suite":
            self._current_suite = None
            self._current_suite_data = None
            self._show_points_hint("Select a test suite to see its test points.")
            return
        key = (data["plan_id"], data["suite_id"])
        self._current_suite = key
        self._current_suite_data = data
        self._suite_lbl.setText(data.get("base_name", ""))
        cached = self._points_cache.get(key)
        if cached is not None:
            self._render_points(items[0], cached)
            return
        self._points_req += 1
        req = self._points_req
        self._show_points_hint("Loading test points…")
        worker = Worker(self.app_state.client.get_test_points, key[0], key[1])
        worker.signals.result.connect(
            lambda pts, it=items[0]: self._on_points(req, key, it, pts))
        worker.signals.error.connect(lambda exc: self._on_points_error(req, exc))
        QThreadPool.globalInstance().start(worker)

    def _on_points(self, req: int, key: tuple, suite_item: QTreeWidgetItem,
                   points: list):
        if req != self._points_req:
            return
        self._points_cache[key] = points
        if self._current_suite == key:
            self._render_points(suite_item, points)

    def _on_points_error(self, req: int, exc: Exception):
        if req != self._points_req:
            return
        self._show_points_hint(f"Could not load test points: {exc}")

    def _render_points(self, suite_item: QTreeWidgetItem, points: list):
        # Lazy count on the suite node, ADO-style "Name (N)".
        data = suite_item.data(0, _ROLE) or {}
        base = data.get("base_name")
        if base is not None:
            suite_item.setText(0, f"{base}  ({len(points)})")

        self._points.clear()
        if not points:
            self._show_points_hint("This suite has no test points.")
            return
        self._points_hint.hide()
        self._points.show()
        for p in points:
            oc = (p.get("last_outcome") or "").lower()
            row = QTreeWidgetItem([
                p.get("test_case_name", "") or "",
                outcome_style.outcome_label(oc),
                str(p.get("test_case_id") or ""),
                p.get("test_case_state", "") or "",
                p.get("config_name", "") or "",
                p.get("tester", "") or "",
            ])
            tint = QBrush(outcome_style.outcome_tint(oc))
            for col in range(row.columnCount()):
                row.setBackground(col, tint)
            row.setToolTip(0, p.get("test_case_name", "") or "")
            row.setData(0, _ROLE, p.get("test_case_id"))
            self._points.addTopLevelItem(row)
        n = len(points)
        self._count_lbl.setText(f"{n} test point{'s' if n != 1 else ''}")
        self._count_lbl.show()
        self._set_send_enabled(True)

    def apply_outcomes(self, plan_id: int, suite_id: int, outcomes: dict):
        """A run submitted from this suite recorded new outcomes — patch the
        cached points (and re-tint the visible rows if this suite is shown)
        instead of forcing a full re-fetch. `outcomes` maps test_case_id ->
        outcome string. If this suite's points were never loaded, there's nothing
        to patch: the next lazy load will already include the new outcomes."""
        if not outcomes:
            return
        by_tc = {str(k): v for k, v in outcomes.items()}
        key = (plan_id, suite_id)
        points = self._points_cache.get(key)
        if points:
            for p in points:
                oc = by_tc.get(str(p.get("test_case_id")))
                if oc:
                    p["last_outcome"] = oc
        if self._current_suite == key:
            self._retint_points(by_tc)

    def _retint_points(self, by_tc: dict):
        """Update the visible points table's Outcome column + row tint in place."""
        for i in range(self._points.topLevelItemCount()):
            row = self._points.topLevelItem(i)
            oc = by_tc.get(str(row.data(0, _ROLE)))
            if not oc:
                continue
            ocl = oc.lower()
            row.setText(1, outcome_style.outcome_label(ocl))
            tint = QBrush(outcome_style.outcome_tint(ocl))
            for col in range(row.columnCount()):
                row.setBackground(col, tint)

    def _set_send_enabled(self, on: bool):
        if hasattr(self, "_run_btn"):
            self._run_btn.setEnabled(on)
            self._edit_btn.setEnabled(on)
            self._view_btn.setEnabled(on)

    def set_send_targets(self, run_visible: bool, edit_visible: bool):
        """Show/hide the 'Run Tests' and 'Edit' send buttons to match which
        target tabs are enabled. When the Run Tests or Edit Test Cases tab is
        hidden via Settings, its send button has nowhere to go, so it's hidden
        too."""
        if hasattr(self, "_run_btn"):
            self._run_btn.setVisible(run_visible)
            self._edit_btn.setVisible(edit_visible)

    def _shown_case_ids(self) -> list:
        """The selected point rows' test-case ids, or all shown if none selected.
        Rows carry their test_case_id in the _ROLE data (column 0)."""
        selected = self._points.selectedItems()
        rows = selected or [self._points.topLevelItem(i)
                            for i in range(self._points.topLevelItemCount())]
        ids = []
        for it in rows:
            cid = it.data(0, _ROLE)
            if cid and cid not in ids:
                ids.append(int(cid))
        return ids

    def _send_cases(self, signal):
        """Emit the selected test-case ids (or all shown if none selected) plus
        the suite context for the Run Tests / Edit tab to load."""
        ids = self._shown_case_ids()
        if not ids or not self._current_suite_data:
            return
        data = self._current_suite_data
        name = data.get("base_name", "")
        signal.emit({
            "case_ids": ids,
            "context": {
                "plan_id": data.get("plan_id"),
                "suite_id": data.get("suite_id"),
                "suite_name": name,
                # Requirement suites map to a PBI/requirement work item — use it so
                # the run gets a meaningful name and bug links; None otherwise.
                "pbi_id": data.get("requirement_id"),
                "pbi_title": name,
            },
        })

    def _on_view_suite(self):
        """Fetch the full test cases behind the shown/selected points and open a
        formatted HTML report in the browser — no file for the user to manage.
        Unlike the points table (a summary), this includes every case's steps."""
        ids = self._shown_case_ids()
        client = self.app_state.client
        if not ids or client is None:
            return
        name = (self._current_suite_data or {}).get("base_name", "")
        self._view_btn.setEnabled(False)
        self._view_btn.setText("  Opening…")
        worker = Worker(self._build_suite_html, client, ids, name)
        worker.signals.result.connect(self._on_view_ready)
        worker.signals.error.connect(self._on_view_error)
        QThreadPool.globalInstance().start(worker)

    @staticmethod
    def _build_suite_html(client, ids, suite_name):
        from app.utils import export_formats
        cases = client.get_test_cases_by_ids(ids)
        records = export_formats.cases_to_records(cases, None, None)
        subtitle = (f"Suite “{suite_name}” — {len(records)} test case(s)"
                    if suite_name else f"{len(records)} test case(s)")
        return export_formats.write_temp_html(records, subtitle=subtitle)

    def _on_view_ready(self, path: str):
        webbrowser.open(Path(path).as_uri())
        self._reset_view_btn()

    def _on_view_error(self, exc: Exception):
        self._reset_view_btn()
        QMessageBox.critical(self, "View Error", f"Could not open report:\n{exc}")

    def _reset_view_btn(self):
        self._view_btn.setText("  View")
        # Re-enable only while a suite's points are still on screen.
        self._view_btn.setEnabled(self._points.topLevelItemCount() > 0)

    def _show_points_hint(self, text: str):
        self._points.hide()
        self._count_lbl.hide()
        self._set_send_enabled(False)
        self._points_hint.setText(text)
        self._points_hint.show()
        if self._current_suite is None:
            self._suite_lbl.setText("Test points")

    def _on_point_double_clicked(self, item: QTreeWidgetItem, _col: int):
        tc_id = item.data(0, _ROLE)
        client = self.app_state.client
        if tc_id and client is not None:
            tm = client.tm
            webbrowser.open(f"{tm.org_url}/{quote(tm.project)}/_workitems/edit/{tc_id}")

    # ------------------------------------------------------------------ #
    #  Suite-name filter                                                   #
    # ------------------------------------------------------------------ #

    def _apply_tree_filter(self):
        text = self._filter_edit.text().strip().lower()

        def visit(item, ancestor_match: bool) -> bool:
            """Reveal `item` if it matches, an ancestor matched, or a descendant
            matches. Returns whether it or a descendant genuinely matched."""
            data = item.data(0, _ROLE) or {}
            kind = data.get("kind")
            # A lazy "Loading…" placeholder has no searchable name; keep it
            # visible whenever its plan is, so the plan keeps its expander arrow
            # and can still be opened to load (and then filter) its suites.
            if kind == "loading":
                item.setHidden(False)
                return False
            name = (data.get("base_name") or item.text(0)).lower()
            self_match = (not text) or (text in name and kind in ("plan", "suite"))
            # A matched node reveals its whole subtree; otherwise a descendant
            # match still pulls the node (and the path to it) into view.
            child_hit = False
            for i in range(item.childCount()):
                child_hit = visit(item.child(i),
                                  ancestor_match or self_match) or child_hit
            item.setHidden(not (ancestor_match or self_match or child_hit))
            return self_match or child_hit

        for i in range(self._tree.topLevelItemCount()):
            visit(self._tree.topLevelItem(i), False)

    # ------------------------------------------------------------------ #
    #  UI                                                                  #
    # ------------------------------------------------------------------ #

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 14, 20, 14)
        layout.setSpacing(10)

        self._splitter = GripSplitter(Qt.Horizontal)

        # ---- Left: plans + suites tree -------------------------------- #
        left = QWidget()
        lv = QVBoxLayout(left)
        lv.setContentsMargins(0, 0, 8, 0)
        lv.setSpacing(8)

        hdr = QHBoxLayout()
        self._plans_lbl = QLabel("Test Suites")
        hdr.addWidget(self._plans_lbl)
        hdr.addStretch()
        self._refresh_btn = QPushButton("  Refresh")
        self._refresh_btn.setIcon(icons.icon("refresh", size=14))
        self._refresh_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._refresh_btn.setToolTip("Reload the plan list and drop cached suites/points")
        self._refresh_btn.clicked.connect(self._on_refresh)
        hdr.addWidget(self._refresh_btn)
        lv.addLayout(hdr)

        self._filter_edit = QLineEdit()
        self._filter_edit.setPlaceholderText("Filter plans and suites…")
        self._filter_edit.setClearButtonEnabled(True)
        self._filter_edit.textChanged.connect(self._apply_tree_filter)
        lv.addWidget(self._filter_edit)

        self._tree_status = QLabel("")
        self._tree_status.setWordWrap(True)
        lv.addWidget(self._tree_status)
        self._tree_status.hide()

        self._progress = QProgressBar()
        self._progress.setFixedHeight(16)
        self._progress.setTextVisible(False)
        lv.addWidget(self._progress)
        self._progress.hide()

        self._tree = QTreeWidget()
        self._tree.setHeaderHidden(True)
        self._tree.setColumnCount(1)
        self._tree.setUniformRowHeights(True)
        self._tree.itemExpanded.connect(self._on_item_expanded)
        self._tree.itemSelectionChanged.connect(self._on_tree_selection)
        apply_hover(self._tree)
        lv.addWidget(self._tree, 1)

        self._splitter.addWidget(left)

        # ---- Right: the selected suite's test points ------------------ #
        right = QWidget()
        rv = QVBoxLayout(right)
        rv.setContentsMargins(8, 0, 0, 0)
        rv.setSpacing(8)

        rhdr = QHBoxLayout()
        self._suite_lbl = QLabel("Test points")
        rhdr.addWidget(self._suite_lbl)
        self._count_lbl = QLabel("")
        rhdr.addWidget(self._count_lbl)
        self._count_lbl.hide()
        rhdr.addStretch()
        # Send the suite's cases (selected rows, or all if none) to another tab.
        self._run_btn = QPushButton("  Run Tests")
        self._run_btn.setIcon(icons.icon("play", size=14))
        self._run_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._run_btn.setEnabled(False)
        self._run_btn.setToolTip("Send these test cases to the Run Tests tab")
        self._run_btn.clicked.connect(lambda: self._send_cases(self.run_suite_requested))
        rhdr.addWidget(self._run_btn)
        self._edit_btn = QPushButton("  Edit")
        self._edit_btn.setIcon(icons.icon("edit", size=14))
        self._edit_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._edit_btn.setEnabled(False)
        self._edit_btn.setToolTip("Send these test cases to the Edit Test Cases tab")
        self._edit_btn.clicked.connect(lambda: self._send_cases(self.edit_suite_requested))
        rhdr.addWidget(self._edit_btn)
        self._view_btn = QPushButton("  View")
        self._view_btn.setIcon(icons.icon("external-link", size=14))
        self._view_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._view_btn.setEnabled(False)
        self._view_btn.setToolTip(
            "Open a formatted report of these test cases in your browser")
        self._view_btn.clicked.connect(self._on_view_suite)
        rhdr.addWidget(self._view_btn)
        self._legend = self._build_legend()
        rhdr.addWidget(self._legend)
        rv.addLayout(rhdr)

        self._points = QTreeWidget()
        self._points.setColumnCount(6)
        self._points.setHeaderLabels(
            ["Title", "Outcome", "Case ID", "State", "Configuration", "Tester"])
        self._points.setRootIsDecorated(False)
        self._points.setUniformRowHeights(True)
        self._points.setAllColumnsShowFocus(True)
        self._points.setSelectionBehavior(QAbstractItemView.SelectRows)
        self._points.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self._points.setItemDelegate(StatusTintDelegate(self._points))
        self._points.itemDoubleClicked.connect(self._on_point_double_clicked)
        header = self._points.header()
        header.setSectionResizeMode(0, QHeaderView.Stretch)
        for col in range(1, 6):
            header.setSectionResizeMode(col, QHeaderView.ResizeToContents)
        header.setStretchLastSection(False)
        rv.addWidget(self._points, 1)

        self._points_hint = QLabel("Select a test suite to see its test points.")
        self._points_hint.setAlignment(Qt.AlignCenter)
        self._points_hint.setWordWrap(True)
        rv.addWidget(self._points_hint, 1)
        self._points.hide()

        self._splitter.addWidget(right)
        self._splitter.setStretchFactor(0, 0)
        self._splitter.setStretchFactor(1, 1)

        # Restore last session's split (same pattern as the Edit tab).
        sizes = [300, 640]
        from app.utils.settings import load_settings
        raw = load_settings().get("suite_splitter_sizes")
        if isinstance(raw, list) and len(raw) == 2:
            try:
                saved = [int(x) for x in raw]
                if all(s > 0 for s in saved):
                    sizes = saved
            except (TypeError, ValueError):
                pass
        self._splitter.setSizes(sizes)

        layout.addWidget(self._splitter, 1)
        self.refresh_theme()

    def splitter_sizes(self) -> list:
        """Current left/right pane sizes — persisted by MainWindow on close."""
        return list(self._splitter.sizes())

    # ------------------------------------------------------------------ #
    #  Legend + theme                                                      #
    # ------------------------------------------------------------------ #

    def _build_legend(self) -> QWidget:
        box = QWidget()
        lay = QHBoxLayout(box)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(10)
        self._legend_swatches = []
        for label, oc in outcome_style.LEGEND_ITEMS:
            sw = QLabel()
            sw.setFixedSize(10, 10)
            lbl = QLabel(label)
            self._legend_swatches.append((sw, lbl, oc))
            lay.addWidget(sw)
            lay.addWidget(lbl)
        return box

    def _color_legend(self):
        t = theme.tokens()
        base = QColor(t["surface"])
        for sw, lbl, oc in self._legend_swatches:
            r, g, b = outcome_style.legend_swatch_rgb(oc, base)
            sw.setStyleSheet(
                f"background: rgb({r},{g},{b}); border: 1px solid {t['border']}; "
                "border-radius: 2px;")
            lbl.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")

    def _retint_icons(self):
        """Re-resolve the theme-tinted item icons after a light/dark toggle."""
        plan_icon = icons.icon("briefcase", size=15)

        def visit(item):
            data = item.data(0, _ROLE) or {}
            if data.get("kind") == "plan":
                item.setIcon(0, plan_icon)
            elif data.get("kind") == "suite":
                item.setIcon(0, icons.icon(
                    _SUITE_ICONS.get(data.get("suite_type"), "folder"), size=15))
            for i in range(item.childCount()):
                visit(item.child(i))

        for i in range(self._tree.topLevelItemCount()):
            visit(self._tree.topLevelItem(i))

    def refresh_theme(self):
        t = theme.tokens()
        self._plans_lbl.setStyleSheet(
            f"font-size: 16px; font-weight: bold; color: {t['text']};")
        self._suite_lbl.setStyleSheet(
            f"font-size: 14px; font-weight: bold; color: {t['text']};")
        self._count_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self._tree_status.setStyleSheet(f"color: {t['text_dim']}; font-size: 12px;")
        self._progress.setStyleSheet(
            f"QProgressBar {{ background: {t['surface']}; border: 1px solid {t['border']}; "
            f"border-radius: 4px; text-align: center; color: {t['text_dim']}; "
            f"font-size: 11px; }} "
            f"QProgressBar::chunk {{ background: {t['accent']}; border-radius: 3px; }}")
        self._points_hint.setStyleSheet(f"color: {t['text_dim2']}; font-size: 13px;")
        self._refresh_btn.setStyleSheet(theme.btn_neutral_qss("padding: 5px 12px; font-size: 12px;"))
        self._refresh_btn.setIcon(icons.icon("refresh", size=14))
        _send_qss = theme.btn_neutral_qss("padding: 5px 12px; font-size: 12px;")
        self._run_btn.setStyleSheet(_send_qss)
        self._run_btn.setIcon(icons.icon("play", size=14))
        self._edit_btn.setStyleSheet(_send_qss)
        self._edit_btn.setIcon(icons.icon("edit", size=14))
        self._view_btn.setStyleSheet(_send_qss)
        self._view_btn.setIcon(icons.icon("external-link", size=14))
        self._tree.setStyleSheet(_tree_qss())
        self._points.setStyleSheet(_tree_qss())
        theme.style_inputs(self)
        self._color_legend()
        self._retint_icons()
