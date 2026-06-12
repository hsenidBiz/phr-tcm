from pathlib import Path

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QComboBox, QTableWidget, QTableWidgetItem, QHeaderView,
    QListWidget, QListWidgetItem, QSplitter, QScrollArea, QFrame, QMessageBox,
    QCheckBox, QFileDialog, QAbstractItemView, QShortcut
)
from PyQt5.QtCore import Qt, QThreadPool, pyqtSignal
from PyQt5.QtGui import QCursor, QKeySequence

from app.utils.xml_builder import parse_steps_xml, build_steps_xml
from app.utils.worker import Worker
from app.models.test_case import Step, TestCase


class EditScreen(QWidget):
    """Tab for loading and editing existing test cases linked to the current PBI."""

    test_case_queued = pyqtSignal(object)  # emits TestCase (Clone to Queue)

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._cases = []
        self._current_idx = None
        self._loaded_pbi = None
        self._bulk_queue: list = []
        self._bulk_total = 0
        self._bulk_done = 0
        self._bulk_errors = []
        self._build_ui()

    # ------------------------------------------------------------------ #
    #  Lifecycle                                                           #
    # ------------------------------------------------------------------ #

    def showEvent(self, event):
        super().showEvent(event)
        self._refresh_assigned_to_combo()
        self.ensure_loaded()

    def ensure_loaded(self):
        """Load cases for the configured PBI if not already loaded (also warms
        known_module_values and duplicate-title detection for other tabs)."""
        if self.app_state.pbi_id and self.app_state.pbi_id != self._loaded_pbi:
            self._load_cases()

    def _refresh_assigned_to_combo(self):
        from app.utils.members_cache import load_cached, attach_once, TeamMemberFetcher
        tm = self.app_state.client.tm

        if self.app_state.cached_team_members is None:
            on_disk = load_cached(tm.org_url, tm.project)
            if on_disk is not None:
                self.app_state.cached_team_members = on_disk

        self._populate_assigned_to_combo(self.app_state.cached_team_members or [])

        if self.app_state._team_members_fetcher is None:
            fetcher = TeamMemberFetcher(self.app_state.client)
            self.app_state._team_members_fetcher = fetcher
            fetcher.done.connect(self._on_members_fetched)
            fetcher.failed.connect(self._on_members_failed)
            fetcher.start()
        else:
            attach_once(self.app_state._team_members_fetcher, self._populate_assigned_to_combo)

    def _on_members_fetched(self, members: list):
        from app.utils.members_cache import save_to_disk
        tm = self.app_state.client.tm
        self.app_state.cached_team_members = members
        self.app_state._team_members_fetcher = None
        if members:
            save_to_disk(tm.org_url, tm.project, members)
        self._populate_assigned_to_combo(members)

    def _on_members_failed(self, _msg: str):
        # Keep any previously cached members; just allow a later retry.
        self.app_state._team_members_fetcher = None

    def _populate_assigned_to_combo(self, members: list):
        cur_data = self._bulk_assigned_combo.currentData()
        self._bulk_assigned_combo.blockSignals(True)
        self._bulk_assigned_combo.clear()
        self._bulk_assigned_combo.addItem("No change")
        for user in members:
            display = user.get("displayName", user.get("uniqueName", ""))
            unique = user.get("uniqueName", "")
            if display and unique:
                self._bulk_assigned_combo.addItem(display, unique)
        if cur_data:
            idx = self._bulk_assigned_combo.findData(cur_data)
            if idx >= 0:
                self._bulk_assigned_combo.setCurrentIndex(idx)
        self._bulk_assigned_combo.blockSignals(False)

    # ------------------------------------------------------------------ #
    #  UI construction                                                     #
    # ------------------------------------------------------------------ #

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(8)

        # Header row
        hdr = QHBoxLayout()
        self._header_lbl = QLabel("No PBI selected. Configure a PBI first.")
        self._header_lbl.setStyleSheet("color: #555; font-size: 12px;")
        hdr.addWidget(self._header_lbl, 1)

        _hdr_btn_style = (
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 4px 12px; }"
            "QPushButton:hover { background: #e0e0e0; }"
            "QPushButton:disabled { color: #aaa; }"
        )
        self._refresh_btn = QPushButton("↺  Refresh")
        self._refresh_btn.setStyleSheet(_hdr_btn_style)
        self._refresh_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._refresh_btn.clicked.connect(self._load_cases)
        hdr.addWidget(self._refresh_btn)

        self._rename_btn = QPushButton("✎  Rename…")
        self._rename_btn.setEnabled(False)
        self._rename_btn.setStyleSheet(_hdr_btn_style)
        self._rename_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._rename_btn.clicked.connect(self._open_rename_dialog)
        hdr.addWidget(self._rename_btn)

        self._export_btn = QPushButton("⬇ Export (.xlsx)")
        self._export_btn.setEnabled(False)
        self._export_btn.setStyleSheet(_hdr_btn_style)
        self._export_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._export_btn.clicked.connect(self._on_export_cases)
        hdr.addWidget(self._export_btn)

        layout.addLayout(hdr)

        # Splitter: left = list, right = form
        splitter = QSplitter(Qt.Horizontal)

        # -- Left: test case list -----------------------------------------
        left = QWidget()
        left_v = QVBoxLayout(left)
        left_v.setContentsMargins(0, 0, 0, 0)
        left_v.setSpacing(4)

        list_hdr = QHBoxLayout()
        list_hdr.setContentsMargins(0, 0, 0, 0)
        list_hdr.addWidget(QLabel("Test Cases:"))
        list_hdr.addStretch()
        from app.utils.settings import load_settings
        _s = load_settings()
        self._mine_chk = QCheckBox("My cases only")
        self._mine_chk.setChecked(bool(_s.get("mine_only_filter", False)))
        self._mine_chk.setToolTip("When checked, only test cases you created are shown")
        self._mine_chk.toggled.connect(self._on_mine_filter_toggled)
        list_hdr.addWidget(self._mine_chk)
        left_v.addLayout(list_hdr)

        self._search_edit = QLineEdit()
        self._search_edit.setPlaceholderText("Search by ID or title…")
        self._search_edit.setClearButtonEnabled(True)
        self._search_edit.textChanged.connect(lambda _: self._apply_filters())
        left_v.addWidget(self._search_edit)

        # Filter row: Status + Module
        filter_row = QHBoxLayout()
        filter_row.setContentsMargins(0, 0, 0, 0)
        filter_row.setSpacing(6)
        self._status_filter = QComboBox()
        self._status_filter.addItems(["All Statuses", "Not Automated", "Planned"])
        self._status_filter.setToolTip("Filter by automation status")
        saved_status = _s.get("status_filter", "All Statuses")
        idx = self._status_filter.findText(saved_status)
        if idx >= 0:
            self._status_filter.setCurrentIndex(idx)
        self._status_filter.currentIndexChanged.connect(self._on_filter_combo_changed)
        filter_row.addWidget(self._status_filter)

        self._module_filter = QComboBox()
        self._module_filter.addItem("All Modules")
        self._module_filter.setToolTip("Filter by module")
        self._module_filter.currentIndexChanged.connect(self._on_filter_combo_changed)
        filter_row.addWidget(self._module_filter)
        left_v.addLayout(filter_row)

        self._list = QListWidget()
        self._list.setAlternatingRowColors(True)
        self._list.setSelectionMode(QListWidget.ExtendedSelection)
        self._list.itemSelectionChanged.connect(self._on_selection_changed)
        left_v.addWidget(self._list)

        self._sel_count_lbl = QLabel("")
        self._sel_count_lbl.setStyleSheet("color: #888; font-size: 11px;")
        left_v.addWidget(self._sel_count_lbl)

        splitter.addWidget(left)

        # -- Right: edit form (scrollable) --------------------------------
        right_scroll = QScrollArea()
        right_scroll.setWidgetResizable(True)
        right_scroll.setFrameShape(QFrame.NoFrame)

        form_root = QWidget()
        form_v = QVBoxLayout(form_root)
        form_v.setContentsMargins(12, 0, 0, 0)
        form_v.setSpacing(10)

        self._no_sel_lbl = QLabel("← Select a test case from the list to edit it.")
        self._no_sel_lbl.setStyleSheet("color: #888;")
        form_v.addWidget(self._no_sel_lbl)

        # Single-case edit form
        self._form = QWidget()
        self._form.setVisible(False)
        fv = QVBoxLayout(self._form)
        fv.setContentsMargins(0, 0, 0, 0)
        fv.setSpacing(10)

        self._tc_id_lbl = QLabel("")
        self._tc_id_lbl.setStyleSheet("color: #555; font-size: 11px;")
        fv.addWidget(self._tc_id_lbl)

        fv.addWidget(QLabel("Title"))
        self._title_edit = QLineEdit()
        fv.addWidget(self._title_edit)

        row2 = QHBoxLayout()
        ac = QVBoxLayout()
        ac.setSpacing(4)
        ac.addWidget(QLabel("Automation Status"))
        self._auto_combo = QComboBox()
        self._auto_combo.addItems(["Not Automated", "Planned"])
        self._auto_combo.setMinimumWidth(160)
        ac.addWidget(self._auto_combo)
        row2.addLayout(ac)

        tc_col = QVBoxLayout()
        tc_col.setSpacing(4)
        tc_col.addWidget(QLabel("Tags  (semicolon-separated)"))
        self._tags_edit = QLineEdit()
        self._tags_edit.setPlaceholderText("e.g. smoke; regression")
        tc_col.addWidget(self._tags_edit)
        row2.addLayout(tc_col)
        row2.addStretch()
        fv.addLayout(row2)

        fv.addWidget(QLabel("Module"))
        self._module_edit = QComboBox()
        self._module_edit.setEditable(True)
        self._module_edit.setInsertPolicy(QComboBox.NoInsert)
        self._module_edit.lineEdit().setPlaceholderText("e.g. Authentication")
        fv.addWidget(self._module_edit)

        steps_hdr = QHBoxLayout()
        steps_hdr.addWidget(QLabel("Steps"))
        steps_hdr.addStretch()
        self._add_step_btn = QPushButton("+ Add Step")
        self._add_step_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 3px 10px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self._add_step_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._add_step_btn.clicked.connect(self._add_step)
        steps_hdr.addWidget(self._add_step_btn)
        fv.addLayout(steps_hdr)

        self._steps_tbl = QTableWidget(0, 4)
        self._steps_tbl.setHorizontalHeaderLabels(["#", "Action", "Expected Result", ""])
        hh = self._steps_tbl.horizontalHeader()
        hh.setSectionResizeMode(0, QHeaderView.Fixed)
        hh.setSectionResizeMode(1, QHeaderView.Stretch)
        hh.setSectionResizeMode(2, QHeaderView.Stretch)
        hh.setSectionResizeMode(3, QHeaderView.Fixed)
        self._steps_tbl.setColumnWidth(0, 30)
        self._steps_tbl.setColumnWidth(3, 36)
        self._steps_tbl.setMinimumHeight(160)
        self._steps_tbl.verticalHeader().setVisible(False)
        # Drag-and-drop row reordering
        self._steps_tbl.setDragEnabled(True)
        self._steps_tbl.setAcceptDrops(True)
        self._steps_tbl.setDragDropMode(QAbstractItemView.InternalMove)
        self._steps_tbl.setDefaultDropAction(Qt.MoveAction)
        self._steps_tbl.model().rowsMoved.connect(self._on_steps_rows_moved)
        fv.addWidget(self._steps_tbl, 1)

        self._save_btn = QPushButton("Save Changes")
        self._save_btn.setFixedHeight(36)
        self._save_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; "
            "font-size: 13px; padding: 0 20px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self._save_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._save_btn.clicked.connect(self._save_changes)
        fv.addWidget(self._save_btn)

        self._clone_btn = QPushButton("📋 Clone to Queue")
        self._clone_btn.setFixedHeight(36)
        self._clone_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; font-size: 13px; padding: 0 20px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self._clone_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._clone_btn.clicked.connect(self._on_clone_to_queue)
        fv.addWidget(self._clone_btn)

        form_v.addWidget(self._form, 1)

        # Multi-select bulk editor frame
        self._bulk_frame = QWidget()
        self._bulk_frame.setVisible(False)
        bfv = QVBoxLayout(self._bulk_frame)
        bfv.setContentsMargins(0, 0, 0, 0)
        bfv.setSpacing(10)

        self._bulk_count_lbl = QLabel("")
        self._bulk_count_lbl.setStyleSheet("font-weight: bold; font-size: 13px;")
        bfv.addWidget(self._bulk_count_lbl)

        bfv.addWidget(QLabel("Tags"))
        tags_row = QHBoxLayout()
        self._bulk_tags_edit = QLineEdit()
        self._bulk_tags_edit.setPlaceholderText("e.g. smoke; regression")
        self._bulk_tags_edit.textChanged.connect(self._update_bulk_save_btn)
        tags_row.addWidget(self._bulk_tags_edit)
        self._bulk_tags_mode = QComboBox()
        self._bulk_tags_mode.addItems(["Append", "Replace"])
        self._bulk_tags_mode.setToolTip("Append adds to existing tags; Replace overwrites them")
        tags_row.addWidget(self._bulk_tags_mode)
        bfv.addLayout(tags_row)

        bfv.addWidget(QLabel("Automation Status"))
        self._bulk_status_combo = QComboBox()
        self._bulk_status_combo.addItems(["No change", "Not Automated", "Planned"])
        self._bulk_status_combo.currentIndexChanged.connect(self._update_bulk_save_btn)
        bfv.addWidget(self._bulk_status_combo)

        bfv.addWidget(QLabel("Assigned To"))
        self._bulk_assigned_combo = QComboBox()
        self._bulk_assigned_combo.addItem("No change")
        self._bulk_assigned_combo.setMinimumWidth(200)
        self._bulk_assigned_combo.currentIndexChanged.connect(self._update_bulk_save_btn)
        bfv.addWidget(self._bulk_assigned_combo)

        self._bulk_save_btn = QPushButton("Save to Cases")
        self._bulk_save_btn.setFixedHeight(36)
        self._bulk_save_btn.setEnabled(False)
        self._bulk_save_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; "
            "font-size: 13px; padding: 0 20px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self._bulk_save_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._bulk_save_btn.clicked.connect(self._on_bulk_save)
        bfv.addWidget(self._bulk_save_btn)

        self._bulk_progress_lbl = QLabel("")
        self._bulk_progress_lbl.setStyleSheet("color: #555; font-size: 11px;")
        bfv.addWidget(self._bulk_progress_lbl)

        bfv.addStretch()
        form_v.addWidget(self._bulk_frame)

        right_scroll.setWidget(form_root)
        splitter.addWidget(right_scroll)

        splitter.setSizes([280, 600])
        layout.addWidget(splitter, 1)

        # Keyboard shortcut: Ctrl+S = Save Changes (only while this tab is visible)
        save_sc = QShortcut(QKeySequence("Ctrl+S"), self)
        save_sc.setContext(Qt.WidgetWithChildrenShortcut)
        save_sc.activated.connect(self._save_changes)

    # ------------------------------------------------------------------ #
    #  Theme                                                               #
    # ------------------------------------------------------------------ #

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._header_lbl.setStyleSheet(f"color: {t['text_dim']}; font-size: 12px;")
        header_btn_style = (
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 4px 12px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
            f"QPushButton:disabled {{ color: {t['text_dim2']}; }}"
        )
        self._refresh_btn.setStyleSheet(header_btn_style)
        self._rename_btn.setStyleSheet(header_btn_style)
        self._export_btn.setStyleSheet(header_btn_style)
        self._sel_count_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self._mine_chk.setStyleSheet(f"color: {t['text_dim']}; font-size: 12px;")
        self._no_sel_lbl.setStyleSheet(f"color: {t['text_dim2']};")
        self._tc_id_lbl.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        self._bulk_progress_lbl.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        self._add_step_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 3px 10px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
        self._clone_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; font-size: 13px; padding: 0 20px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )

    # ------------------------------------------------------------------ #
    #  Loading                                                             #
    # ------------------------------------------------------------------ #

    def _load_cases(self):
        pbi_id = self.app_state.pbi_id
        if not pbi_id:
            return
        if self.app_state.token_manager.is_expired():
            self._header_lbl.setText(
                "Token has expired — re-enter your token on the authentication screen to load test cases."
            )
            return

        self._header_lbl.setText(
            f"PBI #{pbi_id}: {self.app_state.pbi_title}  —  Loading…"
        )
        self._refresh_btn.setEnabled(False)
        self._rename_btn.setEnabled(False)
        self._export_btn.setEnabled(False)
        self._search_edit.clear()
        self._list.clear()
        self._cases = []
        self._current_idx = None
        self._form.setVisible(False)
        self._bulk_frame.setVisible(False)
        self._no_sel_lbl.setText("← Select a test case from the list to edit it.")
        self._no_sel_lbl.setVisible(True)
        self._sel_count_lbl.setText("")

        extra = [r for r in (self.app_state.module_ref,) if r]
        worker = Worker(self.app_state.client.get_test_cases_for_pbi, pbi_id, extra)
        worker.signals.result.connect(lambda r: self._on_cases_loaded(pbi_id, r))
        worker.signals.error.connect(lambda exc: self._on_cases_error(pbi_id, exc))
        QThreadPool.globalInstance().start(worker)

    def _on_cases_loaded(self, pbi_id: int, result: tuple):
        from app.utils.settings import load_settings
        cases, total = result
        self._cases = cases
        self._loaded_pbi = pbi_id
        # Share with the Import tab so it can detect duplicates / offer updates
        self.app_state.existing_cases = cases
        self.app_state.existing_cases_pbi = pbi_id

        for tc in cases:
            tc_id = tc.get("_id", "?")
            title = tc.get("System.Title", "(no title)")
            self._list.addItem(QListWidgetItem(f"#{tc_id}  —  {title}"))

        n = len(cases)
        if total > 200:
            summary = f"Showing 200 of {total} (API limit)"
        else:
            summary = f"{n} test case{'s' if n != 1 else ''} found"
        self._header_lbl.setText(f"PBI #{pbi_id}: {self.app_state.pbi_title}  —  {summary}")

        # Populate module filter from loaded cases
        self._module_filter.blockSignals(True)
        saved_module = load_settings().get("module_filter", "All Modules")
        self._module_filter.clear()
        self._module_filter.addItem("All Modules")
        if self.app_state.module_ref:
            module_vals = sorted({
                tc.get(self.app_state.module_ref, "") or ""
                for tc in cases
                if tc.get(self.app_state.module_ref)
            })
            for mv in module_vals:
                self._module_filter.addItem(mv)
            idx = self._module_filter.findText(saved_module)
            if idx >= 0:
                self._module_filter.setCurrentIndex(idx)
            # Share module values with other screens and refresh the edit combo
            self.app_state.known_module_values = module_vals
            from app.gui.helpers import refresh_module_combo
            refresh_module_combo(self._module_edit, module_vals)
        self._module_filter.blockSignals(False)

        self._apply_filters()
        self._refresh_btn.setEnabled(True)
        self._export_btn.setEnabled(bool(cases))

    def _on_cases_error(self, pbi_id: int, exc: Exception):
        self._header_lbl.setText(
            f"PBI #{pbi_id}: {self.app_state.pbi_title}  —  Error: {exc}"
        )
        self._refresh_btn.setEnabled(True)

    def _on_mine_filter_toggled(self, checked: bool):
        from app.utils.settings import save_settings
        save_settings({"mine_only_filter": checked})
        self._apply_filters()

    def _on_filter_combo_changed(self):
        from app.utils.settings import save_settings
        save_settings({
            "status_filter": self._status_filter.currentText(),
            "module_filter": self._module_filter.currentText(),
        })
        self._apply_filters()

    def _apply_filters(self):
        query = self._search_edit.text().strip().lower()
        mine_only = self._mine_chk.isChecked()
        status_filter = self._status_filter.currentText()
        module_filter = self._module_filter.currentText()
        current_upn = self.app_state.token_manager.get_current_upn()
        if current_upn:
            current_upn = current_upn.lower()

        for row in range(self._list.count()):
            item = self._list.item(row)
            tc = self._cases[row] if row < len(self._cases) else None

            by_search = bool(query) and query not in item.text().lower()
            by_owner = mine_only and tc is not None and not self._is_mine(tc, current_upn)
            by_status = (
                status_filter != "All Statuses" and tc is not None
                and tc.get("Microsoft.VSTS.TCM.AutomationStatus", "Not Automated") != status_filter
            )
            module_val = (
                tc.get(self.app_state.module_ref, "") or ""
                if tc is not None and self.app_state.module_ref else ""
            )
            by_module = (
                module_filter != "All Modules" and tc is not None
                and module_val != module_filter
            )
            item.setHidden(by_search or by_owner or by_status or by_module)

        if not self._list.selectedItems():
            self._update_filter_count()

    def _update_filter_count(self):
        """Show 'Showing X of Y' feedback while nothing is selected."""
        total = self._list.count()
        visible = sum(
            not self._list.item(r).isHidden() for r in range(total)
        )
        if total == 0:
            self._sel_count_lbl.setText("")
        elif visible == 0:
            self._sel_count_lbl.setText(f"No matches — adjust search or filters ({total} hidden)")
        elif visible < total:
            self._sel_count_lbl.setText(f"Showing {visible} of {total}")
        else:
            self._sel_count_lbl.setText(f"{total} test case{'s' if total != 1 else ''}")

    @staticmethod
    def _is_mine(tc: dict, current_upn: str | None) -> bool:
        if not current_upn:
            return True
        created_by = tc.get("System.CreatedBy", "")
        if isinstance(created_by, dict):
            creator = created_by.get("uniqueName", "").lower()
        else:
            creator = str(created_by).lower()
        return creator == current_upn

    def _open_rename_dialog(self):
        selected_items = self._list.selectedItems()
        if not selected_items:
            return
        selected_cases = [
            self._cases[self._list.row(item)] for item in selected_items
        ]
        from app.gui.rename_dialog import PowerRenameDialog
        dlg = PowerRenameDialog(selected_cases, self.app_state.client, parent=self)
        dlg.exec_()
        self._refresh_list_from_cache()
        if self._current_idx is not None and self._form.isVisible():
            tc = self._cases[self._current_idx]
            self._title_edit.setText(tc.get("System.Title", ""))

    def _refresh_list_from_cache(self):
        for row in range(self._list.count()):
            if row < len(self._cases):
                tc = self._cases[row]
                tc_id = tc.get("_id", "?")
                title = tc.get("System.Title", "(no title)")
                self._list.item(row).setText(f"#{tc_id}  —  {title}")

    # ------------------------------------------------------------------ #
    #  TC selection → populate form                                        #
    # ------------------------------------------------------------------ #

    def _on_selection_changed(self):
        selected = self._list.selectedItems()
        n_sel = len(selected)
        n_total = len(self._cases)

        if n_sel == 0:
            self._current_idx = None
            self._form.setVisible(False)
            self._bulk_frame.setVisible(False)
            self._no_sel_lbl.setText("← Select a test case from the list to edit it.")
            self._no_sel_lbl.setVisible(True)
            self._rename_btn.setEnabled(False)
            self._update_filter_count()
        elif n_sel == 1:
            row = self._list.row(selected[0])
            self._current_idx = row
            self._populate_form(row)
            self._form.setVisible(True)
            self._bulk_frame.setVisible(False)
            self._no_sel_lbl.setVisible(False)
            self._rename_btn.setEnabled(True)
            self._sel_count_lbl.setText(f"1 of {n_total} selected")
        else:
            self._current_idx = None
            self._form.setVisible(False)
            self._bulk_frame.setVisible(True)
            self._no_sel_lbl.setVisible(False)
            self._bulk_count_lbl.setText(f"Editing {n_sel} test cases")
            self._bulk_save_btn.setText(f"Save to {n_sel} Cases")
            self._bulk_tags_edit.clear()
            self._bulk_status_combo.setCurrentIndex(0)
            self._bulk_assigned_combo.setCurrentIndex(0)
            self._bulk_progress_lbl.setText("")
            self._update_bulk_save_btn()
            self._rename_btn.setEnabled(True)
            self._sel_count_lbl.setText(f"{n_sel} of {n_total} selected")

    def _populate_form(self, row: int):
        if row < 0 or row >= len(self._cases):
            return
        tc = self._cases[row]

        self._tc_id_lbl.setText(f"Work Item  #{tc.get('_id', '?')}")
        self._title_edit.setText(tc.get("System.Title", ""))

        status = tc.get("Microsoft.VSTS.TCM.AutomationStatus", "Not Automated")
        idx = self._auto_combo.findText(status)
        self._auto_combo.setCurrentIndex(idx if idx >= 0 else 0)

        self._tags_edit.setText(tc.get("System.Tags", "") or "")

        module_val = ""
        if self.app_state.module_ref:
            module_val = tc.get(self.app_state.module_ref, "") or ""
        self._module_edit.setCurrentText(module_val)
        self._module_edit.lineEdit().setPlaceholderText(
            "e.g. Authentication" if self.app_state.module_ref
            else "Module field not configured"
        )

        self._steps_tbl.setRowCount(0)
        steps = parse_steps_xml(tc.get("Microsoft.VSTS.TCM.Steps", "") or "")
        for i, step in enumerate(steps):
            self._insert_step_row(i + 1, step.action, step.expected)

    # ------------------------------------------------------------------ #
    #  Steps table helpers                                                 #
    # ------------------------------------------------------------------ #

    def _make_rm_wrap(self) -> QWidget:
        rm_btn = QPushButton("✕")
        rm_btn.setFixedSize(26, 22)
        rm_btn.setStyleSheet(
            "QPushButton { background: #c42b1c; color: white; border-radius: 3px; "
            "font-size: 11px; font-weight: bold; }"
            "QPushButton:hover { background: #a4261a; }"
        )
        rm_btn.setCursor(QCursor(Qt.PointingHandCursor))
        rm_btn.clicked.connect(self._remove_step)
        wrap = QWidget()
        lay = QHBoxLayout(wrap)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setAlignment(Qt.AlignCenter)
        lay.addWidget(rm_btn)
        return wrap

    def _insert_step_row(self, num: int, action: str = "", expected: str = ""):
        row = self._steps_tbl.rowCount()
        self._steps_tbl.insertRow(row)

        num_item = QTableWidgetItem(str(num))
        num_item.setFlags(num_item.flags() & ~Qt.ItemIsEditable)
        num_item.setTextAlignment(Qt.AlignCenter)
        self._steps_tbl.setItem(row, 0, num_item)
        self._steps_tbl.setItem(row, 1, QTableWidgetItem(action))
        self._steps_tbl.setItem(row, 2, QTableWidgetItem(expected))
        self._steps_tbl.setCellWidget(row, 3, self._make_rm_wrap())

    def _add_step(self):
        num = self._steps_tbl.rowCount() + 1
        self._insert_step_row(num)
        self._steps_tbl.scrollToBottom()

    def _remove_step(self):
        btn = self.sender()
        wrapper = btn.parent()
        for r in range(self._steps_tbl.rowCount()):
            if self._steps_tbl.cellWidget(r, 3) is wrapper:
                self._steps_tbl.removeRow(r)
                self._renumber_steps()
                break

    def _renumber_steps(self):
        for r in range(self._steps_tbl.rowCount()):
            item = self._steps_tbl.item(r, 0)
            if item:
                item.setText(str(r + 1))

    def _on_steps_rows_moved(self):
        self._renumber_steps()
        for r in range(self._steps_tbl.rowCount()):
            self._steps_tbl.setCellWidget(r, 3, self._make_rm_wrap())

    # ------------------------------------------------------------------ #
    #  Save single case                                                    #
    # ------------------------------------------------------------------ #

    def _collect_steps(self) -> list:
        """Read non-empty steps out of the steps table."""
        steps = []
        for r in range(self._steps_tbl.rowCount()):
            a_item = self._steps_tbl.item(r, 1)
            e_item = self._steps_tbl.item(r, 2)
            action = a_item.text().strip() if a_item else ""
            expected = e_item.text().strip() if e_item else ""
            if action:
                steps.append(Step(action=action, expected=expected))
        return steps

    def _save_changes(self):
        from app.gui.helpers import warn_if_token_expired
        if warn_if_token_expired(self, self.app_state.token_manager):
            return
        if self._current_idx is None:
            return

        tc = self._cases[self._current_idx]
        tc_id = tc.get("_id")
        if not tc_id:
            return

        title = self._title_edit.text().strip()
        if not title:
            QMessageBox.warning(self, "Validation", "Title cannot be empty.")
            return

        steps = self._collect_steps()

        fields = {
            "System.Title": title,
            "System.Tags": self._tags_edit.text().strip(),
            "Microsoft.VSTS.TCM.AutomationStatus": self._auto_combo.currentText(),
            "Microsoft.VSTS.TCM.Steps": build_steps_xml(steps),
        }
        if self.app_state.module_ref:
            fields[self.app_state.module_ref] = self._module_edit.currentText().strip()

        self._save_btn.setEnabled(False)
        self._save_btn.setText("Saving…")

        worker = Worker(self.app_state.client.update_test_case_fields, tc_id, fields)
        worker.signals.result.connect(
            lambda _, _tc=tc, _id=tc_id, _t=title, _f=dict(fields): self._on_save_done(_tc, _id, _t, _f)
        )
        worker.signals.error.connect(lambda exc, _id=tc_id: self._on_save_error(_id, exc))
        QThreadPool.globalInstance().start(worker)

    def _on_save_done(self, tc: dict, tc_id, title: str, fields: dict):
        tc["System.Title"] = title
        tc["System.Tags"] = fields["System.Tags"]
        tc["Microsoft.VSTS.TCM.AutomationStatus"] = fields["Microsoft.VSTS.TCM.AutomationStatus"]
        if self.app_state.module_ref and self.app_state.module_ref in fields:
            tc[self.app_state.module_ref] = fields[self.app_state.module_ref]
        if self._current_idx is not None:
            item = self._list.item(self._current_idx)
            if item:
                item.setText(f"#{tc_id}  —  {title}")
        self._save_btn.setEnabled(True)
        self._save_btn.setText("Save Changes")
        from app.gui.helpers import status_message
        status_message(self, f"Test case #{tc_id} updated successfully.")

    def _on_save_error(self, tc_id, exc: Exception):
        self._save_btn.setEnabled(True)
        self._save_btn.setText("Save Changes")
        QMessageBox.critical(
            self, "Save Failed",
            f"Could not update test case #{tc_id}:\n\n{exc}"
        )

    # ------------------------------------------------------------------ #
    #  Clone to queue                                                      #
    # ------------------------------------------------------------------ #

    def _on_clone_to_queue(self):
        if self._current_idx is None:
            return
        title = self._title_edit.text().strip() + " (Copy)"
        tc = TestCase(
            title=title,
            steps=self._collect_steps(),
            tags=self._tags_edit.text().strip(),
            automation_status=self._auto_combo.currentText(),
            module_value=self._module_edit.currentText().strip(),
        )
        # MainWindow shows a status-bar confirmation when the case is queued.
        self.test_case_queued.emit(tc)

    # ------------------------------------------------------------------ #
    #  Export cases to Excel                                               #
    # ------------------------------------------------------------------ #

    def _on_export_cases(self):
        if not self._cases:
            return
        path, _ = QFileDialog.getSaveFileName(
            self, "Export Test Cases",
            str(Path.home() / "Downloads" / "test_cases_export.xlsx"),
            "Excel Files (*.xlsx)",
        )
        if not path:
            return
        self._export_btn.setEnabled(False)
        self._export_btn.setText("Exporting…")
        worker = Worker(
            self._do_export_cases, self._cases, path,
            self.app_state.module_ref, self.app_state.preconditions_ref,
        )
        worker.signals.result.connect(lambda _: self._on_export_done(path))
        worker.signals.error.connect(self._on_export_error)
        QThreadPool.globalInstance().start(worker)

    @staticmethod
    def _do_export_cases(cases, path, module_ref, preconditions_ref):
        from app.utils.import_parser import export_cases_to_excel
        export_cases_to_excel(cases, path, module_ref, preconditions_ref)

    def _on_export_done(self, path: str):
        self._export_btn.setEnabled(True)
        self._export_btn.setText("⬇ Export (.xlsx)")
        QMessageBox.information(self, "Exported", f"Test cases exported to:\n{path}")

    def _on_export_error(self, exc: Exception):
        self._export_btn.setEnabled(True)
        self._export_btn.setText("⬇ Export (.xlsx)")
        QMessageBox.critical(self, "Export Error", f"Could not export:\n{exc}")

    # ------------------------------------------------------------------ #
    #  Bulk tag / status editor                                            #
    # ------------------------------------------------------------------ #

    def _update_bulk_save_btn(self):
        has_value = (
            bool(self._bulk_tags_edit.text().strip())
            or self._bulk_status_combo.currentIndex() != 0
            or self._bulk_assigned_combo.currentIndex() != 0
        )
        self._bulk_save_btn.setEnabled(has_value)

    def _on_bulk_save(self):
        from app.gui.helpers import warn_if_token_expired
        if warn_if_token_expired(self, self.app_state.token_manager):
            return
        selected = self._list.selectedItems()
        if not selected:
            return

        tags_text = self._bulk_tags_edit.text().strip()
        tags_mode = self._bulk_tags_mode.currentText()
        new_status = self._bulk_status_combo.currentText()
        assigned_to = self._bulk_assigned_combo.currentData()  # uniqueName or None

        updates = []
        for item in selected:
            idx = self._list.row(item)
            if idx >= len(self._cases):
                continue
            tc = self._cases[idx]
            tc_id = tc.get("_id")
            if not tc_id:
                continue
            fields = {}
            if tags_text:
                if tags_mode == "Append":
                    existing = tc.get("System.Tags", "") or ""
                    fields["System.Tags"] = f"{existing}; {tags_text}".strip("; ") if existing else tags_text
                else:
                    fields["System.Tags"] = tags_text
            if new_status != "No change":
                fields["Microsoft.VSTS.TCM.AutomationStatus"] = new_status
            if assigned_to:
                fields["System.AssignedTo"] = assigned_to
            if fields:
                updates.append((tc_id, idx, fields))

        if not updates:
            return

        self._bulk_queue = list(updates)
        self._bulk_total = len(updates)
        self._bulk_done = 0
        self._bulk_errors = []
        self._bulk_save_btn.setEnabled(False)
        self._bulk_progress_lbl.setText(f"Saving 0 / {self._bulk_total}…")
        self._process_next_bulk()

    def _process_next_bulk(self):
        if not self._bulk_queue:
            self._on_bulk_complete()
            return
        tc_id, idx, fields = self._bulk_queue.pop(0)
        worker = Worker(self.app_state.client.update_test_case_fields, tc_id, fields)
        worker.signals.result.connect(
            lambda _, i=idx, f=dict(fields): self._on_bulk_item_done(i, f)
        )
        worker.signals.error.connect(
            lambda exc, i=idx: self._on_bulk_item_error(i, exc)
        )
        QThreadPool.globalInstance().start(worker)

    def _on_bulk_item_done(self, idx: int, fields: dict):
        self._bulk_done += 1
        if idx < len(self._cases):
            self._cases[idx].update(fields)
        self._bulk_progress_lbl.setText(f"Saved {self._bulk_done} / {self._bulk_total}…")
        self._process_next_bulk()

    def _on_bulk_item_error(self, idx: int, exc: Exception):
        self._bulk_done += 1
        self._bulk_errors.append(str(exc))
        self._bulk_progress_lbl.setText(f"Saved {self._bulk_done} / {self._bulk_total}…")
        self._process_next_bulk()

    def _on_bulk_complete(self):
        self._bulk_save_btn.setEnabled(True)
        if self._bulk_errors:
            self._bulk_progress_lbl.setText(
                f"Done with {len(self._bulk_errors)} error(s)."
            )
            QMessageBox.warning(
                self, "Bulk Save",
                f"Completed with {len(self._bulk_errors)} error(s):\n"
                + "\n".join(self._bulk_errors[:5])
            )
        else:
            self._bulk_progress_lbl.setText(f"All {self._bulk_done} cases updated.")
