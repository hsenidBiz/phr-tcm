from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QComboBox, QTableWidget, QTableWidgetItem, QHeaderView,
    QListWidget, QListWidgetItem, QSplitter, QScrollArea, QFrame, QMessageBox,
    QCheckBox
)
from PyQt5.QtCore import Qt
from PyQt5.QtGui import QCursor

from app.utils.xml_builder import parse_steps_xml, build_steps_xml
from app.models.test_case import Step


class EditScreen(QWidget):
    """Tab for loading and editing existing test cases linked to the current PBI."""

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._cases = []
        self._current_idx = None
        self._loaded_pbi = None
        self._build_ui()

    # ------------------------------------------------------------------ #
    #  Lifecycle                                                           #
    # ------------------------------------------------------------------ #

    def showEvent(self, event):
        super().showEvent(event)
        if self.app_state.pbi_id and self.app_state.pbi_id != self._loaded_pbi:
            self._load_cases()

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
        self._refresh_btn = QPushButton("↺  Refresh")
        self._refresh_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 4px 12px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self._refresh_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._refresh_btn.clicked.connect(self._load_cases)
        hdr.addWidget(self._refresh_btn)
        self._rename_btn = QPushButton("✎  Rename…")
        self._rename_btn.setEnabled(False)
        self._rename_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 4px 12px; }"
            "QPushButton:hover { background: #e0e0e0; }"
            "QPushButton:disabled { color: #aaa; }"
        )
        self._rename_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._rename_btn.clicked.connect(self._open_rename_dialog)
        hdr.addWidget(self._rename_btn)
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
        from app.utils.settings import load_settings, save_settings
        self._mine_chk = QCheckBox("My cases only")
        self._mine_chk.setChecked(bool(load_settings().get("mine_only_filter", False)))
        self._mine_chk.setToolTip(
            "When checked, only test cases you created are shown"
        )
        self._mine_chk.toggled.connect(self._on_mine_filter_toggled)
        list_hdr.addWidget(self._mine_chk)
        left_v.addLayout(list_hdr)

        self._search_edit = QLineEdit()
        self._search_edit.setPlaceholderText("Search by ID or title…")
        self._search_edit.setClearButtonEnabled(True)
        self._search_edit.textChanged.connect(lambda _: self._apply_filters())
        left_v.addWidget(self._search_edit)

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

        self._form = QWidget()
        self._form.setVisible(False)
        fv = QVBoxLayout(self._form)
        fv.setContentsMargins(0, 0, 0, 0)
        fv.setSpacing(10)

        self._tc_id_lbl = QLabel("")
        self._tc_id_lbl.setStyleSheet("color: #555; font-size: 11px;")
        fv.addWidget(self._tc_id_lbl)

        # Title
        fv.addWidget(QLabel("Title"))
        self._title_edit = QLineEdit()
        fv.addWidget(self._title_edit)

        # Automation Status + Tags
        row2 = QHBoxLayout()

        ac = QVBoxLayout()
        ac.setSpacing(4)
        ac.addWidget(QLabel("Automation Status"))
        self._auto_combo = QComboBox()
        self._auto_combo.addItems(["Not Automated", "Planned"])
        self._auto_combo.setMinimumWidth(160)
        ac.addWidget(self._auto_combo)
        row2.addLayout(ac)

        tc = QVBoxLayout()
        tc.setSpacing(4)
        tc.addWidget(QLabel("Tags  (semicolon-separated)"))
        self._tags_edit = QLineEdit()
        self._tags_edit.setPlaceholderText("e.g. smoke; regression")
        tc.addWidget(self._tags_edit)
        row2.addLayout(tc)
        row2.addStretch()
        fv.addLayout(row2)

        # Module
        fv.addWidget(QLabel("Module"))
        self._module_edit = QLineEdit()
        self._module_edit.setPlaceholderText("e.g. Authentication")
        fv.addWidget(self._module_edit)

        # Steps
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
        fv.addWidget(self._steps_tbl, 1)

        # Save
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

        form_v.addWidget(self._form, 1)
        right_scroll.setWidget(form_root)
        splitter.addWidget(right_scroll)

        splitter.setSizes([280, 600])
        layout.addWidget(splitter, 1)

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
        self._sel_count_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self._mine_chk.setStyleSheet(f"color: {t['text_dim']}; font-size: 12px;")
        self._add_step_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 3px 10px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )

    # ------------------------------------------------------------------ #
    #  Loading                                                             #
    # ------------------------------------------------------------------ #

    def _load_cases(self):
        pbi_id = self.app_state.pbi_id
        if not pbi_id:
            return

        self._header_lbl.setText(
            f"PBI #{pbi_id}: {self.app_state.pbi_title}  —  Loading…"
        )
        self._refresh_btn.setEnabled(False)
        self._rename_btn.setEnabled(False)
        self._search_edit.clear()
        self._list.clear()
        self._cases = []
        self._current_idx = None
        self._form.setVisible(False)
        self._no_sel_lbl.setText("← Select a test case from the list to edit it.")
        self._no_sel_lbl.setVisible(True)
        self._sel_count_lbl.setText("")

        try:
            extra = [r for r in (self.app_state.module_ref,) if r]
            self._cases = self.app_state.client.get_test_cases_for_pbi(pbi_id, extra)
            self._loaded_pbi = pbi_id

            for tc in self._cases:
                tc_id = tc.get("_id", "?")
                title = tc.get("System.Title", "(no title)")
                self._list.addItem(QListWidgetItem(f"#{tc_id}  —  {title}"))

            n = len(self._cases)
            self._header_lbl.setText(
                f"PBI #{pbi_id}: {self.app_state.pbi_title}  —  "
                f"{n} test case{'s' if n != 1 else ''} found"
            )
            self._apply_filters()
        except Exception as exc:
            self._header_lbl.setText(
                f"PBI #{pbi_id}: {self.app_state.pbi_title}  —  Error: {exc}"
            )
        finally:
            self._refresh_btn.setEnabled(True)

    def _on_mine_filter_toggled(self, checked: bool):
        from app.utils.settings import save_settings
        save_settings({"mine_only_filter": checked})
        self._apply_filters()

    def _apply_filters(self):
        query = self._search_edit.text().strip().lower()
        mine_only = self._mine_chk.isChecked()
        current_upn = self.app_state.token_manager.get_current_upn()
        if current_upn:
            current_upn = current_upn.lower()

        for row in range(self._list.count()):
            item = self._list.item(row)
            tc = self._cases[row] if row < len(self._cases) else None

            by_search = bool(query) and query not in item.text().lower()
            by_owner = mine_only and tc is not None and not self._is_mine(tc, current_upn)
            item.setHidden(by_search or by_owner)

    @staticmethod
    def _is_mine(tc: dict, current_upn: str | None) -> bool:
        """Return True when the test case was created by the given UPN."""
        if not current_upn:
            return True  # Can't determine ownership — show everything
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
        # Preserve the order they appear in the list
        selected_cases = [
            self._cases[self._list.row(item)] for item in selected_items
        ]
        from app.gui.rename_dialog import PowerRenameDialog
        dlg = PowerRenameDialog(selected_cases, self.app_state.client, parent=self)
        dlg.exec_()
        self._refresh_list_from_cache()
        # Keep the edit form title in sync if a single case is currently in view
        if self._current_idx is not None and self._form.isVisible():
            tc = self._cases[self._current_idx]
            self._title_edit.setText(tc.get("System.Title", ""))

    def _refresh_list_from_cache(self):
        """Re-populate the list widget text from the in-memory cases cache."""
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
            self._no_sel_lbl.setText("← Select a test case from the list to edit it.")
            self._no_sel_lbl.setVisible(True)
            self._rename_btn.setEnabled(False)
            self._sel_count_lbl.setText("")
        elif n_sel == 1:
            row = self._list.row(selected[0])
            self._current_idx = row
            self._populate_form(row)
            self._form.setVisible(True)
            self._no_sel_lbl.setVisible(False)
            self._rename_btn.setEnabled(True)
            self._sel_count_lbl.setText(f"1 of {n_total} selected")
        else:
            self._current_idx = None
            self._form.setVisible(False)
            self._no_sel_lbl.setText(
                f"{n_sel} test cases selected — click  ✎ Rename…  to rename them all at once."
            )
            self._no_sel_lbl.setVisible(True)
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
        self._module_edit.setText(module_val)
        self._module_edit.setPlaceholderText(
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

    def _insert_step_row(self, num: int, action: str = "", expected: str = ""):
        row = self._steps_tbl.rowCount()
        self._steps_tbl.insertRow(row)

        num_item = QTableWidgetItem(str(num))
        num_item.setFlags(num_item.flags() & ~Qt.ItemIsEditable)
        num_item.setTextAlignment(Qt.AlignCenter)
        self._steps_tbl.setItem(row, 0, num_item)
        self._steps_tbl.setItem(row, 1, QTableWidgetItem(action))
        self._steps_tbl.setItem(row, 2, QTableWidgetItem(expected))

        rm_btn = QPushButton("✕")
        rm_btn.setFixedSize(26, 22)
        rm_btn.setStyleSheet(
            "QPushButton { background: #c42b1c; color: white; border-radius: 3px; "
            "font-size: 11px; font-weight: bold; }"
            "QPushButton:hover { background: #a4261a; }"
        )
        rm_btn.setCursor(QCursor(Qt.PointingHandCursor))
        rm_btn.clicked.connect(self._remove_step)
        self._steps_tbl.setCellWidget(row, 3, rm_btn)

    def _add_step(self):
        num = self._steps_tbl.rowCount() + 1
        self._insert_step_row(num)
        self._steps_tbl.scrollToBottom()

    def _remove_step(self):
        btn = self.sender()
        for r in range(self._steps_tbl.rowCount()):
            if self._steps_tbl.cellWidget(r, 3) is btn:
                self._steps_tbl.removeRow(r)
                self._renumber_steps()
                break

    def _renumber_steps(self):
        for r in range(self._steps_tbl.rowCount()):
            item = self._steps_tbl.item(r, 0)
            if item:
                item.setText(str(r + 1))

    # ------------------------------------------------------------------ #
    #  Save                                                                #
    # ------------------------------------------------------------------ #

    def _save_changes(self):
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

        steps = []
        for r in range(self._steps_tbl.rowCount()):
            a_item = self._steps_tbl.item(r, 1)
            e_item = self._steps_tbl.item(r, 2)
            action = a_item.text().strip() if a_item else ""
            expected = e_item.text().strip() if e_item else ""
            if action:
                steps.append(Step(action=action, expected=expected))

        fields = {
            "System.Title": title,
            "System.Tags": self._tags_edit.text().strip(),
            "Microsoft.VSTS.TCM.AutomationStatus": self._auto_combo.currentText(),
            "Microsoft.VSTS.TCM.Steps": build_steps_xml(steps),
        }
        if self.app_state.module_ref:
            fields[self.app_state.module_ref] = self._module_edit.text().strip()

        self._save_btn.setEnabled(False)
        self._save_btn.setText("Saving…")
        try:
            self.app_state.client.update_test_case_fields(tc_id, fields)

            # Update local cache so list stays in sync
            tc["System.Title"] = title
            tc["System.Tags"] = fields["System.Tags"]
            tc["Microsoft.VSTS.TCM.AutomationStatus"] = fields[
                "Microsoft.VSTS.TCM.AutomationStatus"
            ]
            if self.app_state.module_ref:
                tc[self.app_state.module_ref] = self._module_edit.text().strip()

            self._list.item(self._current_idx).setText(f"#{tc_id}  —  {title}")
            QMessageBox.information(
                self, "Saved", f"Test case #{tc_id} updated successfully."
            )
        except Exception as exc:
            QMessageBox.critical(
                self, "Save Failed",
                f"Could not update test case #{tc_id}:\n\n{exc}"
            )
        finally:
            self._save_btn.setEnabled(True)
            self._save_btn.setText("Save Changes")
