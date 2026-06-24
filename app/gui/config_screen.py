from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QComboBox, QListWidget, QListWidgetItem,
    QMessageBox, QFrame
)
from PyQt5.QtCore import Qt, pyqtSignal, QThreadPool, QTimer, QEvent
from PyQt5.QtGui import QFont, QCursor

from app.utils.settings import (
    load_settings, save_settings, save_recent_pbi, remove_recent_pbi,
    recent_pbis_for_project,
)
from app.utils.worker import Worker


class ConfigScreen(QWidget):
    configured = pyqtSignal()      # emitted when PBI is validated and module field chosen
    back_requested = pyqtSignal()  # emitted when user wants to return to auth screen

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._orgs_loaded = False
        self._orgs_loading = False
        self._projects_loaded = False
        self._projects_loading = False
        self._build_ui()

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(60, 40, 60, 40)
        layout.setSpacing(0)

        title = QLabel("Configuration")
        font = QFont()
        font.setPointSize(17)
        font.setBold(True)
        title.setFont(font)
        layout.addWidget(title)
        layout.addSpacing(4)

        self.connected_label = QLabel("")
        self.connected_label.setStyleSheet("color: #0078d4;")
        layout.addWidget(self.connected_label)
        layout.addSpacing(12)

        # Organisation & Project — discovered from the signed-in account
        self._proj_frame = QFrame()
        self._proj_frame.setObjectName("projFrame")
        self._proj_frame.setFrameShape(QFrame.NoFrame)
        self._proj_frame.setStyleSheet(
            "#projFrame { background: #f9f9f9; border: 1px solid #ddd; border-radius: 8px; }"
        )
        proj_layout = QVBoxLayout(self._proj_frame)
        proj_layout.setContentsMargins(24, 18, 24, 18)
        proj_layout.setSpacing(10)
        proj_layout.addWidget(QLabel("<b>Organisation &amp; Project</b>"))

        self._proj_note = QLabel(
            "Discovered from your signed-in account — no manual entry needed. "
            "The selected project is remembered until you change it."
        )
        self._proj_note.setWordWrap(True)
        self._proj_note.setStyleSheet("color: #555;")
        proj_layout.addWidget(self._proj_note)

        op_row = QHBoxLayout()
        org_col = QVBoxLayout()
        org_col.addWidget(QLabel("Organisation"))
        self.org_combo = QComboBox()
        self.org_combo.setMinimumWidth(220)
        self.org_combo.addItem("Loading…", None)
        self.org_combo.setEnabled(False)
        self.org_combo.currentIndexChanged.connect(self._on_org_changed)
        org_col.addWidget(self.org_combo)
        self._org_container = QWidget()
        self._org_container.setLayout(org_col)
        op_row.addWidget(self._org_container)

        proj_col = QVBoxLayout()
        proj_col.addWidget(QLabel("Project"))
        self.project_combo = QComboBox()
        self.project_combo.setMinimumWidth(220)
        self.project_combo.addItem("Loading…", None)
        self.project_combo.setEnabled(False)
        self.project_combo.currentIndexChanged.connect(self._on_project_changed)
        proj_col.addWidget(self.project_combo)
        proj_container = QWidget()
        proj_container.setLayout(proj_col)
        op_row.addWidget(proj_container)
        op_row.addStretch()
        proj_layout.addLayout(op_row)

        layout.addWidget(self._proj_frame)
        layout.addSpacing(10)

        # PBI section
        self._pbi_frame = QFrame()
        self._pbi_frame.setObjectName("pbiFrame")
        self._pbi_frame.setFrameShape(QFrame.NoFrame)
        self._pbi_frame.setStyleSheet(
            "#pbiFrame { background: #f9f9f9; border: 1px solid #ddd; border-radius: 8px; }"
        )
        pbi_layout = QVBoxLayout(self._pbi_frame)
        pbi_layout.setContentsMargins(24, 18, 24, 18)
        pbi_layout.setSpacing(10)

        pbi_layout.addWidget(QLabel("<b>Product Backlog Item (PBI)</b>"))

        self._pbi_note = QLabel(
            "Search for the PBI you want to link test cases to — by title or "
            "work item ID. Click the field to pick from your recent PBIs."
        )
        self._pbi_note.setWordWrap(True)
        self._pbi_note.setStyleSheet("color: #555;")
        pbi_layout.addWidget(self._pbi_note)

        # Searchable PBI picker: shows recent PBIs on focus and live-searches
        # Azure DevOps work items as you type (debounced).
        self.pbi_search = QLineEdit()
        self.pbi_search.setPlaceholderText(
            "Search work items by title or ID — click to see recent PBIs…"
        )
        self.pbi_search.textChanged.connect(self._on_pbi_search_text)
        pbi_layout.addWidget(self.pbi_search)

        self.pbi_dropdown = QListWidget()
        self.pbi_dropdown.setVisible(False)
        self.pbi_dropdown.setMinimumHeight(220)
        self.pbi_dropdown.setMaximumHeight(380)
        self.pbi_dropdown.setStyleSheet(
            "QListWidget { border: 1px solid #ccc; border-radius: 4px; "
            "background: white; outline: none; font-size: 13px; }"
            "QListWidget::item { padding: 8px 10px; }"
            "QListWidget::item:hover { background: #e8f0fe; }"
            "QListWidget::item:selected { background: #0078d4; color: white; }"
        )
        self.pbi_dropdown.setCursor(QCursor(Qt.PointingHandCursor))
        self.pbi_dropdown.itemClicked.connect(self._on_pbi_dropdown_clicked)
        pbi_layout.addWidget(self.pbi_dropdown)

        # Install filters only after BOTH widgets exist — eventFilter()
        # references each of them and Qt delivers events during construction.
        self.pbi_search.installEventFilter(self)
        self.pbi_dropdown.installEventFilter(self)

        self._pbi_search_timer = QTimer(self)
        self._pbi_search_timer.setSingleShot(True)
        self._pbi_search_timer.setInterval(450)
        self._pbi_search_timer.timeout.connect(self._run_pbi_search)
        self._pbi_search_seq = 0
        self._pbi_dropdown_mode = "recent"

        # Currently selected PBI — prominent, always visible
        self.selected_pbi_label = QLabel("No PBI selected — search above to choose one")
        self.selected_pbi_label.setWordWrap(True)
        self.selected_pbi_label.setStyleSheet("color: #888; font-size: 13px;")
        pbi_layout.addWidget(self.selected_pbi_label)

        # Transient status / error messages for the PBI fetch
        self.pbi_result_label = QLabel("")
        self.pbi_result_label.setWordWrap(True)
        pbi_layout.addWidget(self.pbi_result_label)

        # Area / Iteration path fields — populated after PBI is validated
        paths_grid = QHBoxLayout()

        area_col = QVBoxLayout()
        area_col.addWidget(QLabel("Area Path"))
        self.area_edit = QLineEdit()
        self.area_edit.setPlaceholderText("Inherited from PBI…")
        self.area_edit.setReadOnly(True)
        self.area_edit.setStyleSheet(
            "QLineEdit { background: #f0f0f0; color: #555; border: 1px solid #ddd; border-radius: 4px; padding: 4px 8px; }"
        )
        area_col.addWidget(self.area_edit)
        paths_grid.addLayout(area_col)

        iter_col = QVBoxLayout()
        iter_col.addWidget(QLabel("Iteration Path"))
        self.iteration_edit = QLineEdit()
        self.iteration_edit.setPlaceholderText("Inherited from PBI…")
        self.iteration_edit.setReadOnly(True)
        self.iteration_edit.setStyleSheet(
            "QLineEdit { background: #f0f0f0; color: #555; border: 1px solid #ddd; border-radius: 4px; padding: 4px 8px; }"
        )
        iter_col.addWidget(self.iteration_edit)
        paths_grid.addLayout(iter_col)

        self.paths_container = QWidget()
        self.paths_container.setLayout(paths_grid)
        pbi_layout.addWidget(self.paths_container)

        paths_note = QLabel(
            "Area and Iteration paths are inherited from the PBI."
        )
        paths_note.setStyleSheet("color: #888; font-size: 11px;")
        paths_note.setWordWrap(True)
        self.paths_note = paths_note
        pbi_layout.addWidget(self.paths_note)

        # Test plan / suite status for the selected PBI. Test cases are added to a
        # requirement-based suite under this plan so they show on the board.
        self.test_plan_label = QLabel("")
        self.test_plan_label.setWordWrap(True)
        self.test_plan_label.setStyleSheet("color: #888; font-size: 12px;")
        self.test_plan_label.setVisible(False)
        self._tp_seq = 0
        pbi_layout.addWidget(self.test_plan_label)

        layout.addWidget(self._pbi_frame)
        layout.addSpacing(10)

        # Custom field mapping — discovered and auto-selected in the background;
        # the org's Test Case fields are fixed, so there is no manual override UI.
        self._build_field_combos()
        layout.addStretch()

        # Button row — Back (left) | Continue (right)
        btn_row = QHBoxLayout()

        self._back_btn = QPushButton("← Back")
        self._back_btn.setFixedHeight(38)
        self._back_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; font-size: 14px; padding: 0 20px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self._back_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._back_btn.clicked.connect(self.back_requested)
        btn_row.addWidget(self._back_btn)
        btn_row.addStretch()

        from app.utils import theme
        self.continue_btn = QPushButton("Continue →")
        self.continue_btn.setFixedHeight(38)
        self.continue_btn.setEnabled(False)
        self.continue_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 14px; padding: 0 20px;")
        )
        self.continue_btn.clicked.connect(self._on_continue)
        btn_row.addWidget(self.continue_btn)

        layout.addLayout(btn_row)

    def _build_field_combos(self):
        """Hidden data holders for the discovered Module / Preconditions field
        mapping. Populated by _load_fields(); read by _on_continue()."""
        self.field_combo = QComboBox()
        self.field_combo.addItem("Loading fields…", None)
        self.field_combo.setEnabled(False)
        self.preconditions_combo = QComboBox()
        self.preconditions_combo.addItem("Loading fields…", None)
        self.preconditions_combo.setEnabled(False)
        self._fields_loaded = False
        self._fields_loading = False

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._pbi_frame.setStyleSheet(
            f"#pbiFrame {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 8px; }}"
        )
        self._proj_frame.setStyleSheet(
            f"#projFrame {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 8px; }}"
        )
        self._proj_note.setStyleSheet(f"color: {t['text_dim']};")
        self.connected_label.setStyleSheet(f"color: {t['accent']};")
        self.pbi_dropdown.setStyleSheet(
            f"QListWidget {{ border: 1px solid {t['border']}; border-radius: 4px; "
            f"background: {t['tag_inner_bg']}; outline: none; font-size: 13px; }}"
            f"QListWidget::item {{ padding: 8px 10px; color: {t['text']}; }}"
            f"QListWidget::item:hover {{ background: {t['tag_unsel_hover']}; }}"
            f"QListWidget::item:selected {{ background: {t['accent']}; color: white; }}"
        )
        self._pbi_note.setStyleSheet(f"color: {t['text_dim']};")
        self.paths_note.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        _ro_style = (
            f"QLineEdit {{ background: {t['surface2']}; color: {t['text_dim']}; "
            f"border: 1px solid {t['border']}; border-radius: 4px; padding: 4px 8px; }}"
        )
        self.area_edit.setStyleSheet(_ro_style)
        self.iteration_edit.setStyleSheet(_ro_style)
        self._back_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; font-size: 14px; padding: 0 20px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
        self.continue_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 14px; padding: 0 20px;")
        )
        self._refresh_selected_pbi_label()

    def on_enter(self):
        """Called when this screen becomes active."""
        self.refresh_expiry()
        # Failed loads retry on the next visit; flags guard re-entry.
        if not self._orgs_loaded and not self._orgs_loading:
            self._load_orgs()
        elif self._orgs_loaded and not self._projects_loaded and not self._projects_loading:
            self._load_projects()
        elif (self.app_state.token_manager.project
                and not self._fields_loaded and not self._fields_loading):
            self._load_fields()

    # ------------------------------------------------------------------ #
    #  Organisation / project discovery                                    #
    # ------------------------------------------------------------------ #

    def _load_orgs(self):
        self._orgs_loading = True
        worker = Worker(self.app_state.client.get_organizations)
        worker.signals.result.connect(self._on_orgs_result)
        worker.signals.error.connect(self._on_orgs_error)
        QThreadPool.globalInstance().start(worker)

    def _on_orgs_result(self, orgs: list):
        self._orgs_loading = False
        if not orgs:
            QMessageBox.warning(
                self, "No Organisations Found",
                "Your account does not belong to any Azure DevOps organisation."
            )
            return
        self._orgs_loaded = True
        saved_org = load_settings().get("org_url", "")
        self.org_combo.blockSignals(True)
        self.org_combo.clear()
        for o in orgs:
            self.org_combo.addItem(o["name"], o["url"])
        idx = self.org_combo.findData(saved_org)
        self.org_combo.setCurrentIndex(idx if idx >= 0 else 0)
        self.org_combo.setEnabled(True)
        self.org_combo.blockSignals(False)
        # With a single organisation there is nothing to choose
        self._org_container.setVisible(len(orgs) > 1)
        self._load_projects()

    def _on_orgs_error(self, exc: Exception):
        self._orgs_loading = False
        QMessageBox.warning(
            self, "Discovery Error",
            f"Could not discover your Azure DevOps organisations:\n\n{exc}\n\n"
            "It will be retried the next time this screen is shown."
        )

    def _on_org_changed(self, index):
        if self.org_combo.itemData(index):
            self._projects_loaded = False
            self._load_projects()

    def _load_projects(self):
        org_url = self.org_combo.currentData()
        if not org_url:
            return
        self._projects_loading = True
        self.project_combo.blockSignals(True)
        self.project_combo.clear()
        self.project_combo.addItem("Loading projects…", None)
        self.project_combo.setEnabled(False)
        self.project_combo.blockSignals(False)
        worker = Worker(self.app_state.client.get_projects, org_url)
        worker.signals.result.connect(self._on_projects_result)
        worker.signals.error.connect(self._on_projects_error)
        QThreadPool.globalInstance().start(worker)

    def _on_projects_result(self, names: list):
        self._projects_loading = False
        self._projects_loaded = True
        self.project_combo.blockSignals(True)
        self.project_combo.clear()
        self.project_combo.addItem("— Select a project —", None)
        for n in names:
            self.project_combo.addItem(n, n)
        saved = load_settings().get("project", "")
        idx = self.project_combo.findData(saved) if saved else -1
        if idx > 0:
            self.project_combo.setCurrentIndex(idx)
        self.project_combo.setEnabled(True)
        self.project_combo.blockSignals(False)
        if idx > 0:
            self._apply_project(saved)
        else:
            self.refresh_expiry()
            self._check_ready()

    def _on_projects_error(self, exc: Exception):
        self._projects_loading = False
        self.project_combo.blockSignals(True)
        self.project_combo.clear()
        self.project_combo.addItem("Could not load projects", None)
        self.project_combo.setEnabled(False)
        self.project_combo.blockSignals(False)
        QMessageBox.warning(
            self, "Discovery Error",
            f"Could not load the projects in this organisation:\n\n{exc}\n\n"
            "It will be retried the next time this screen is shown."
        )

    def _on_project_changed(self, index):
        name = self.project_combo.itemData(index)
        if name:
            self._apply_project(name)

    def _apply_project(self, project: str):
        """Point the app at the selected project and reset project-scoped state."""
        org_url = self.org_combo.currentData() or ""
        tm = self.app_state.token_manager
        unchanged = (tm.org_url == org_url and tm.project == project)
        tm.set_org_project(org_url, project)
        save_settings({"org_url": org_url, "project": project})

        if not unchanged:
            # Team member cache is project-scoped — disconnect any in-flight
            # fetcher first so it won't overwrite the cleared cache.
            fetcher = self.app_state._team_members_fetcher
            if fetcher is not None:
                try:
                    fetcher.done.disconnect()
                except TypeError:
                    pass
            self.app_state.cached_team_members = None
            self.app_state._team_members_fetcher = None
            # Validated PBI and its inherited paths no longer apply
            self.app_state.pbi_id = None
            self.app_state.pbi_title = ""
            self.app_state.area_path = ""
            self.app_state.iteration_path = ""
            self.pbi_result_label.setText("")
            self.area_edit.clear()
            self.iteration_edit.clear()
            self._reset_test_plan_state()
            self._refresh_selected_pbi_label()
            # PBI-scoped caches shared with the editing tabs
            self.app_state.existing_cases = []
            self.app_state.existing_cases_pbi = None
            self.app_state.known_module_values = []
            # Test Case fields are project-scoped — rediscover
            self._fields_loaded = False

        if not self._fields_loaded and not self._fields_loading:
            self._load_fields()
        self.refresh_expiry()
        self._check_ready()

        # Restore the most recently used PBI for this project (covers app
        # restart and project switches alike).
        if self.app_state.pbi_id is None:
            recent = recent_pbis_for_project(project)
            if recent:
                self._select_pbi(recent[0]["id"])

    def refresh_expiry(self):
        """Update the connected label with the current expiry countdown and colour."""
        from app.utils import theme
        t = theme.tokens()
        tm = self.app_state.token_manager
        if tm.auto_refresh_active():
            self.connected_label.setStyleSheet(f"color: {t['accent']};")
            if tm.project:
                self.connected_label.setText(
                    f"Connected to: {tm.org_url}/{tm.project}  |  Signed in — token refreshes automatically"
                )
            else:
                self.connected_label.setText(
                    "Signed in — select a project below to continue"
                )
            self._check_ready()
            return
        display = tm.get_expiry_display()
        secs = tm.get_seconds_remaining()

        if "EXPIRED" in display or secs == 0:
            color = t["error"]
        elif secs > 0 and secs < 60:
            color = t["error"]
        elif secs < 300:
            color = t["warn_fg"]
        else:
            color = t["accent"]

        self.connected_label.setStyleSheet(f"color: {color};")
        self.connected_label.setText(
            f"Connected to: {tm.org_url}/{tm.project}  |  {display}"
        )
        self._check_ready()

    # ------------------------------------------------------------------ #
    #  PBI search / recents dropdown                                       #
    # ------------------------------------------------------------------ #

    def eventFilter(self, obj, event):
        if obj is self.pbi_search and event.type() == QEvent.FocusIn:
            if not self.pbi_search.text().strip():
                self._show_recent_dropdown()
        elif obj is self.pbi_dropdown and event.type() == QEvent.KeyPress:
            if event.key() == Qt.Key_Delete and self._pbi_dropdown_mode == "recent":
                item = self.pbi_dropdown.currentItem()
                pid = item.data(Qt.UserRole) if item else None
                if pid:
                    remove_recent_pbi(pid)
                    self._show_recent_dropdown()
                return True
        return super().eventFilter(obj, event)

    def _show_recent_dropdown(self):
        self._pbi_dropdown_mode = "recent"
        recent = recent_pbis_for_project(self.app_state.token_manager.project)
        self.pbi_dropdown.clear()
        if not recent:
            self.pbi_dropdown.setVisible(False)
            return
        for r in recent:
            item = QListWidgetItem(f"#{r['id']}  —  {r['title']}")
            item.setData(Qt.UserRole, r["id"])
            item.setToolTip("Click to select — press Delete to remove from recents")
            self.pbi_dropdown.addItem(item)
        self.pbi_dropdown.setVisible(True)

    def _on_pbi_search_text(self, text: str):
        text = text.strip()
        self._pbi_search_seq += 1  # invalidates any in-flight search results
        self._pbi_search_timer.stop()
        if not text:
            self._show_recent_dropdown()
            return
        if len(text) < 3 and not text.isdigit():
            self.pbi_dropdown.setVisible(False)
            return
        self._pbi_search_timer.start()

    def _run_pbi_search(self):
        text = self.pbi_search.text().strip()
        if not text or not self.app_state.token_manager.project:
            return
        self._pbi_dropdown_mode = "search"
        self.pbi_dropdown.clear()
        searching = QListWidgetItem("Searching…")
        searching.setFlags(Qt.NoItemFlags)
        self.pbi_dropdown.addItem(searching)
        self.pbi_dropdown.setVisible(True)

        seq = self._pbi_search_seq
        worker = Worker(self.app_state.client.search_work_items, text)
        worker.signals.result.connect(lambda results: self._on_pbi_search_results(seq, results))
        worker.signals.error.connect(lambda exc: self._on_pbi_search_error(seq, exc))
        QThreadPool.globalInstance().start(worker)

    def _on_pbi_search_results(self, seq: int, results: list):
        if seq != self._pbi_search_seq:
            return  # stale — the text changed after this search started
        self.pbi_dropdown.clear()
        if not results:
            empty = QListWidgetItem("No matching work items")
            empty.setFlags(Qt.NoItemFlags)
            self.pbi_dropdown.addItem(empty)
            self.pbi_dropdown.setVisible(True)
            return
        for r in results:
            item = QListWidgetItem(f"#{r['id']}  —  {r['title']}    [{r['type']}]")
            item.setData(Qt.UserRole, r["id"])
            self.pbi_dropdown.addItem(item)
        self.pbi_dropdown.setVisible(True)

    def _on_pbi_search_error(self, seq: int, exc: Exception):
        if seq != self._pbi_search_seq:
            return
        self.pbi_dropdown.clear()
        err = QListWidgetItem(f"Search failed: {exc}")
        err.setFlags(Qt.NoItemFlags)
        self.pbi_dropdown.addItem(err)
        self.pbi_dropdown.setVisible(True)

    def _on_pbi_dropdown_clicked(self, item):
        pid = item.data(Qt.UserRole)
        if not pid:
            return
        self.pbi_dropdown.setVisible(False)
        self.pbi_search.blockSignals(True)
        self.pbi_search.clear()
        self.pbi_search.blockSignals(False)
        self._select_pbi(pid)

    def _select_pbi(self, pbi_id: int):
        """Fetch the picked work item's details and make it the active PBI."""
        from app.utils import theme
        if not self.app_state.token_manager.project:
            return
        self.pbi_result_label.setStyleSheet(f"color: {theme.tokens()['text_dim']};")
        self.pbi_result_label.setText(f"Loading #{pbi_id}…")
        worker = Worker(self.app_state.client.get_work_item, pbi_id)
        worker.signals.result.connect(lambda fields: self._on_pbi_result(pbi_id, fields))
        worker.signals.error.connect(lambda exc: self._on_pbi_error(pbi_id, exc))
        QThreadPool.globalInstance().start(worker)

    def _refresh_selected_pbi_label(self):
        from app.utils import theme
        t = theme.tokens()
        if self.app_state.pbi_id:
            self.selected_pbi_label.setText(
                f"Selected:  #{self.app_state.pbi_id} — {self.app_state.pbi_title}"
            )
            self.selected_pbi_label.setStyleSheet(
                f"color: {t['ok']}; font-size: 13px; font-weight: bold;"
            )
        else:
            self.selected_pbi_label.setText("No PBI selected — search above to choose one")
            self.selected_pbi_label.setStyleSheet(f"color: {t['text_dim2']}; font-size: 13px;")

    def _on_pbi_result(self, pbi_id: int, fields: dict):
        title = fields.get("System.Title", "Unknown")
        wtype = fields.get("System.WorkItemType", "")
        area = fields.get("System.AreaPath", "")
        iteration = fields.get("System.IterationPath", "")

        self.app_state.pbi_id = pbi_id
        self.app_state.pbi_title = f"{title} ({wtype})"
        self.app_state.area_path = area
        self.app_state.iteration_path = iteration

        self.pbi_result_label.setText("")
        self._refresh_selected_pbi_label()

        save_recent_pbi(pbi_id, title, self.app_state.token_manager.project)

        self.area_edit.setText(area)
        self.iteration_edit.setText(iteration)
        self._detect_test_plan(pbi_id, area)
        self._check_ready()

    def _on_pbi_error(self, pbi_id: int, exc: Exception):
        from app.utils import theme
        self.pbi_result_label.setStyleSheet(f"color: {theme.tokens()['error']};")
        if isinstance(exc, LookupError):
            self.pbi_result_label.setText(
                f"Work item #{pbi_id} was not found in project "
                f"'{self.app_state.token_manager.project}'."
            )
        else:
            self.pbi_result_label.setText(f"Error: {exc}")
        self.area_edit.clear()
        self.iteration_edit.clear()
        self._reset_test_plan_state()
        self._refresh_selected_pbi_label()

    # ------------------------------------------------------------------ #
    #  Test plan / suite detection (board visibility)                      #
    # ------------------------------------------------------------------ #

    def _reset_test_plan_state(self):
        self._tp_seq += 1  # invalidates any in-flight detection
        self.app_state.test_plan_id = None
        self.app_state.test_plan_name = ""
        self.app_state.suite_id = None
        self.app_state.test_plan_pbi = None
        self.test_plan_label.setVisible(False)
        self.test_plan_label.setText("")

    def _detect_test_plan(self, pbi_id: int, area_path: str):
        """Find (read-only) the test plan/suite for this PBI so the user can see
        whether one exists. Nothing is created here — that happens at creation
        time. Runs in the background; results are discarded if the PBI changes."""
        # Already resolved for this PBI this session — re-render, don't refetch.
        if self.app_state.test_plan_pbi == pbi_id:
            self.test_plan_label.setVisible(True)
            self._apply_test_plan_label()
            return
        self._reset_test_plan_state()
        self.test_plan_label.setVisible(True)
        self.test_plan_label.setStyleSheet("color: #888; font-size: 12px;")
        self.test_plan_label.setText("🧪 Checking for a test plan for this PBI…")
        self._tp_seq += 1
        seq = self._tp_seq
        worker = Worker(self._do_detect_test_plan, pbi_id, area_path)
        worker.signals.result.connect(
            lambda res, s=seq, p=pbi_id: self._on_test_plan_detected(s, p, res)
        )
        worker.signals.error.connect(lambda exc, s=seq: self._on_test_plan_error(s, exc))
        QThreadPool.globalInstance().start(worker)

    def _do_detect_test_plan(self, pbi_id: int, area_path: str) -> dict:
        client = self.app_state.client
        # Session-cached plan list; reused across PBI selections (and for both
        # lookups below) so we don't re-list every plan each time.
        plans = client.get_test_plans(use_cache=True)
        plan, suite = client.find_existing_suite_for_pbi(pbi_id, area_path, plans=plans)
        if suite:
            return {"plan": plan, "suite": suite}
        # No suite yet — report the plan it would land in (if one exists).
        return {"plan": client.find_plan_for_pbi_area(area_path, plans=plans), "suite": None}

    def _on_test_plan_detected(self, seq: int, pbi_id: int, res: dict):
        if seq != self._tp_seq or pbi_id != self.app_state.pbi_id:
            return  # stale — PBI changed after this detection started
        plan = res.get("plan")
        suite = res.get("suite")

        self.app_state.test_plan_id = plan["id"] if plan else None
        self.app_state.test_plan_name = plan["name"] if plan else ""
        self.app_state.suite_id = suite["id"] if suite else None
        self.app_state.test_plan_pbi = pbi_id

        self.test_plan_label.setVisible(True)
        self._apply_test_plan_label()

    def _apply_test_plan_label(self):
        """Render the test-plan status label from the resolved app_state fields.
        Shared by fresh detection and the same-PBI re-render path."""
        from app.utils import theme
        t = theme.tokens()
        name = self.app_state.test_plan_name
        if self.app_state.suite_id is not None:
            self.test_plan_label.setStyleSheet(f"color: {t['ok']}; font-size: 12px;")
            self.test_plan_label.setText(
                f"🧪 Test Plan: <b>{name}</b> — a test suite already exists for this "
                "PBI; new test cases are added to it."
            )
        elif name:
            self.test_plan_label.setStyleSheet(f"color: {t['warn_fg']}; font-size: 12px;")
            self.test_plan_label.setText(
                f"🧪 Test Plan: <b>{name}</b> — no test suite exists for this PBI yet. "
                "One is created automatically when you add test cases."
            )
        else:
            self.test_plan_label.setStyleSheet(f"color: {t['warn_fg']}; font-size: 12px;")
            self.test_plan_label.setText(
                "🧪 No test plan exists for this PBI's area yet. A test plan and suite are "
                "created automatically when you add test cases."
            )

    def _on_test_plan_error(self, seq: int, exc: Exception):
        if seq != self._tp_seq:
            return
        self.test_plan_label.setVisible(True)
        self.test_plan_label.setStyleSheet("color: #888; font-size: 12px;")
        msg = str(exc).strip()
        if len(msg) > 180:
            msg = msg[:180] + "…"
        self.test_plan_label.setText(
            f"🧪 Could not check the test plan status: {msg}  "
            "(A suite is still created automatically when you add test cases.)"
        )

    def _load_fields(self):
        self._fields_loading = True
        worker = Worker(self.app_state.client.get_test_case_fields)
        worker.signals.result.connect(self._on_fields_result)
        worker.signals.error.connect(self._on_fields_error)
        QThreadPool.globalInstance().start(worker)

    def _on_fields_result(self, fields: list):
        self._fields_loading = False
        self._fields_loaded = True

        for combo in (self.field_combo, self.preconditions_combo):
            combo.clear()
            combo.addItem("None — skip this field", None)
            for f in fields:
                combo.addItem(f"{f['name']}  ({f['referenceName']})", f["referenceName"])
            combo.setEnabled(True)

        for i in range(self.field_combo.count()):
            if "module" in self.field_combo.itemText(i).lower():
                self.field_combo.setCurrentIndex(i)
                break

        for i in range(self.preconditions_combo.count()):
            t = self.preconditions_combo.itemText(i).lower()
            if "prerequisite" in t or "precondition" in t:
                self.preconditions_combo.setCurrentIndex(i)
                break

        self._check_ready()

    def _on_fields_error(self, exc: Exception):
        # Leave _fields_loaded False so the next visit to this screen retries
        # automatically. The combos fall back to "skip" so the user can still
        # continue (test cases are then created without Module/Preconditions).
        self._fields_loading = False
        QMessageBox.warning(
            self, "Field Load Error",
            f"Could not load Test Case fields:\n\n{exc}\n\n"
            "It will be retried the next time this screen is shown. You can "
            "continue without the Module / Preconditions fields in the meantime."
        )
        for combo in (self.field_combo, self.preconditions_combo):
            combo.clear()
            combo.addItem("None — skip this field", None)
            combo.setEnabled(True)
        self._check_ready()


    def _check_ready(self):
        project_ok = bool(self.app_state.token_manager.project)
        pbi_ok = self.app_state.pbi_id is not None
        # field_combo is only enabled after _load_fields succeeds
        token_ok = not self.app_state.token_manager.is_expired()
        enabled = project_ok and pbi_ok and self.field_combo.isEnabled() and token_ok
        self.continue_btn.setEnabled(enabled)
        if not token_ok:
            self.continue_btn.setToolTip("Session has expired — sign in again to continue")
        else:
            self.continue_btn.setToolTip("")

    def _on_continue(self):
        self.app_state.module_ref = self.field_combo.currentData()
        self.app_state.preconditions_ref = self.preconditions_combo.currentData()
        self.app_state.area_path = self.area_edit.text().strip()
        self.app_state.iteration_path = self.iteration_edit.text().strip()
        self.configured.emit()
