from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QComboBox, QTableWidget, QTableWidgetItem, QHeaderView,
    QListWidget, QListWidgetItem, QSplitter, QScrollArea, QFrame, QMessageBox
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
        layout.addLayout(hdr)

        # Splitter: left = list, right = form
        splitter = QSplitter(Qt.Horizontal)

        # -- Left: test case list -----------------------------------------
        left = QWidget()
        left_v = QVBoxLayout(left)
        left_v.setContentsMargins(0, 0, 0, 0)
        left_v.setSpacing(4)
        left_v.addWidget(QLabel("Test Cases:"))

        self._search_edit = QLineEdit()
        self._search_edit.setPlaceholderText("Search by ID or title…")
        self._search_edit.setClearButtonEnabled(True)
        self._search_edit.textChanged.connect(self._filter_list)
        left_v.addWidget(self._search_edit)

        self._list = QListWidget()
        self._list.setAlternatingRowColors(True)
        self._list.currentRowChanged.connect(self._on_tc_selected)
        left_v.addWidget(self._list)
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
        self._refresh_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 4px 12px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
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
        self._search_edit.clear()
        self._list.clear()
        self._cases = []
        self._current_idx = None
        self._form.setVisible(False)
        self._no_sel_lbl.setVisible(True)

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
        except Exception as exc:
            self._header_lbl.setText(
                f"PBI #{pbi_id}: {self.app_state.pbi_title}  —  Error: {exc}"
            )
        finally:
            self._refresh_btn.setEnabled(True)

    def _filter_list(self, query: str):
        q = query.strip().lower()
        for row in range(self._list.count()):
            item = self._list.item(row)
            item.setHidden(bool(q) and q not in item.text().lower())

    # ------------------------------------------------------------------ #
    #  TC selection → populate form                                        #
    # ------------------------------------------------------------------ #

    def _on_tc_selected(self, row: int):
        if row < 0 or row >= len(self._cases):
            return
        self._current_idx = row
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

        # Populate steps
        self._steps_tbl.setRowCount(0)
        steps = parse_steps_xml(tc.get("Microsoft.VSTS.TCM.Steps", "") or "")
        for i, step in enumerate(steps):
            self._insert_step_row(i + 1, step.action, step.expected)

        self._form.setVisible(True)
        self._no_sel_lbl.setVisible(False)

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
