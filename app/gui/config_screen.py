from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QComboBox, QMessageBox, QFrame, QSizePolicy
)
from PyQt5.QtCore import Qt, pyqtSignal
from PyQt5.QtGui import QFont, QCursor

from app.utils.settings import load_settings, save_recent_pbi


class ConfigScreen(QWidget):
    configured = pyqtSignal()      # emitted when PBI is validated and module field chosen
    back_requested = pyqtSignal()  # emitted when user wants to return to auth screen

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._fields = []  # list of {"name": str, "referenceName": str}
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
        layout.addSpacing(24)

        # PBI section
        self._pbi_frame = QFrame()
        self._pbi_frame.setFrameShape(QFrame.StyledPanel)
        self._pbi_frame.setStyleSheet(
            "QFrame { background: #f9f9f9; border: 1px solid #ddd; border-radius: 8px; }"
        )
        pbi_layout = QVBoxLayout(self._pbi_frame)
        pbi_layout.setContentsMargins(24, 18, 24, 18)
        pbi_layout.setSpacing(10)

        pbi_layout.addWidget(QLabel("<b>Product Backlog Item (PBI)</b>"))

        self._pbi_note = QLabel(
            "Enter the work item ID of the PBI you want to link test cases to. "
            "You can find this number in the top-left corner of the PBI card in Azure DevOps."
        )
        self._pbi_note.setWordWrap(True)
        self._pbi_note.setStyleSheet("color: #555;")
        pbi_layout.addWidget(self._pbi_note)

        self.recent_pbi_combo = QComboBox()
        self.recent_pbi_combo.addItem("No recent PBIs", None)
        self.recent_pbi_combo.setEnabled(False)
        self.recent_pbi_combo.activated.connect(self._on_recent_pbi_selected)
        pbi_layout.addWidget(self.recent_pbi_combo)

        id_row = QHBoxLayout()
        self.pbi_edit = QLineEdit()
        self.pbi_edit.setPlaceholderText("e.g. 12345")
        self.pbi_edit.setMaximumWidth(140)
        self.pbi_edit.returnPressed.connect(self._validate_pbi)
        id_row.addWidget(self.pbi_edit)

        self.validate_btn = QPushButton("Validate PBI")
        self.validate_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; padding: 5px 14px; }"
            "QPushButton:hover { background: #106ebe; }"
        )
        self.validate_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.validate_btn.clicked.connect(self._validate_pbi)
        id_row.addWidget(self.validate_btn)
        id_row.addStretch()
        pbi_layout.addLayout(id_row)

        self.pbi_result_label = QLabel("")
        self.pbi_result_label.setWordWrap(True)
        pbi_layout.addWidget(self.pbi_result_label)

        # Area / Iteration path fields — populated after PBI is validated
        paths_grid = QHBoxLayout()

        area_col = QVBoxLayout()
        area_col.addWidget(QLabel("Area Path"))
        self.area_edit = QLineEdit()
        self.area_edit.setPlaceholderText("Inherited from PBI…")
        area_col.addWidget(self.area_edit)
        paths_grid.addLayout(area_col)

        iter_col = QVBoxLayout()
        iter_col.addWidget(QLabel("Iteration Path"))
        self.iteration_edit = QLineEdit()
        self.iteration_edit.setPlaceholderText("Inherited from PBI…")
        iter_col.addWidget(self.iteration_edit)
        paths_grid.addLayout(iter_col)

        self.paths_container = QWidget()
        self.paths_container.setLayout(paths_grid)
        self.paths_container.setVisible(False)
        pbi_layout.addWidget(self.paths_container)

        paths_note = QLabel(
            "Area and Iteration are inherited from the PBI. Edit above to override for this batch."
        )
        paths_note.setStyleSheet("color: #888; font-size: 11px;")
        paths_note.setWordWrap(True)
        self.paths_note = paths_note
        self.paths_note.setVisible(False)
        pbi_layout.addWidget(self.paths_note)

        layout.addWidget(self._pbi_frame)
        layout.addSpacing(18)

        # Module field section
        self._module_frame = QFrame()
        self._module_frame.setFrameShape(QFrame.StyledPanel)
        self._module_frame.setStyleSheet(
            "QFrame { background: #f9f9f9; border: 1px solid #ddd; border-radius: 8px; }"
        )
        mod_layout = QVBoxLayout(self._module_frame)
        mod_layout.setContentsMargins(24, 18, 24, 18)
        mod_layout.setSpacing(10)

        mod_layout.addWidget(QLabel("<b>Custom Fields</b>"))

        self._mod_note = QLabel(
            "Select which fields map to 'Module' and 'Preconditions' in your Test Case work item. "
            "Select 'None — skip this field' for any field your organisation does not use."
        )
        self._mod_note.setWordWrap(True)
        self._mod_note.setStyleSheet("color: #555;")
        mod_layout.addWidget(self._mod_note)

        fields_grid = QHBoxLayout()

        module_col = QVBoxLayout()
        module_col.addWidget(QLabel("Module Field"))
        self.field_combo = QComboBox()
        self.field_combo.setMinimumWidth(280)
        self.field_combo.addItem("Loading fields…", None)
        self.field_combo.setEnabled(False)
        module_col.addWidget(self.field_combo)
        fields_grid.addLayout(module_col)

        pre_col = QVBoxLayout()
        pre_col.addWidget(QLabel("Preconditions Field"))
        self.preconditions_combo = QComboBox()
        self.preconditions_combo.setMinimumWidth(280)
        self.preconditions_combo.addItem("Loading fields…", None)
        self.preconditions_combo.setEnabled(False)
        pre_col.addWidget(self.preconditions_combo)
        fields_grid.addLayout(pre_col)

        fields_grid.addStretch()
        mod_layout.addLayout(fields_grid)

        self.load_fields_btn = QPushButton("Load Test Case Fields")
        self.load_fields_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; border-radius: 4px; padding: 5px 14px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self.load_fields_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.load_fields_btn.clicked.connect(self._load_fields)
        mod_layout.addWidget(self.load_fields_btn)

        layout.addWidget(self._module_frame)
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

        self.continue_btn = QPushButton("Continue →")
        self.continue_btn.setFixedHeight(38)
        self.continue_btn.setEnabled(False)
        self.continue_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; font-size: 14px; padding: 0 20px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self.continue_btn.clicked.connect(self._on_continue)
        btn_row.addWidget(self.continue_btn)

        layout.addLayout(btn_row)

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        frame_ss = (
            f"QFrame {{ background: {t['surface']}; border: 1px solid {t['border']}; "
            f"border-radius: 8px; }}"
        )
        self._pbi_frame.setStyleSheet(frame_ss)
        self._module_frame.setStyleSheet(frame_ss)
        self.connected_label.setStyleSheet(f"color: {t['accent']};")
        self._pbi_note.setStyleSheet(f"color: {t['text_dim']};")
        self.paths_note.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        self._mod_note.setStyleSheet(f"color: {t['text_dim']};")
        btn_neutral = (
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 5px 14px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
        self.load_fields_btn.setStyleSheet(btn_neutral)
        self._back_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; font-size: 14px; padding: 0 20px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )

    def on_enter(self):
        """Called when this screen becomes active."""
        self.refresh_expiry()
        self._populate_recent_pbis()
        if not self.field_combo.isEnabled():
            self._load_fields()

    def refresh_expiry(self):
        """Update the connected label with the current expiry countdown and colour."""
        tm = self.app_state.token_manager
        display = tm.get_expiry_display()
        secs = tm.get_seconds_remaining()

        if "EXPIRED" in display or secs == 0:
            color = "#c00"
        elif secs > 0 and secs < 60:
            color = "#c00"
        elif secs < 300:
            color = "#e67e00"
        else:
            from app.utils import theme
            color = theme.tokens()["accent"]

        self.connected_label.setStyleSheet(f"color: {color};")
        self.connected_label.setText(
            f"Connected to: {tm.org_url}/{tm.project}  |  {display}"
        )

    def _populate_recent_pbis(self):
        recent = load_settings().get("recent_pbis", [])
        self.recent_pbi_combo.blockSignals(True)
        self.recent_pbi_combo.clear()
        if recent:
            self.recent_pbi_combo.addItem("— Select a recent PBI —", None)
            for r in recent:
                self.recent_pbi_combo.addItem(f"#{r['id']}  —  {r['title']}", r["id"])
            self.recent_pbi_combo.setEnabled(True)
        else:
            self.recent_pbi_combo.addItem("No recent PBIs", None)
            self.recent_pbi_combo.setEnabled(False)
        self.recent_pbi_combo.blockSignals(False)

    def _on_recent_pbi_selected(self, index):
        pbi_id = self.recent_pbi_combo.currentData()
        if pbi_id is not None:
            self.pbi_edit.setText(str(pbi_id))
            self._validate_pbi()

    def _validate_pbi(self):
        text = self.pbi_edit.text().strip()
        if not text.isdigit():
            self.pbi_result_label.setStyleSheet("color: #c00;")
            self.pbi_result_label.setText("Please enter a numeric work item ID.")
            return

        self.validate_btn.setEnabled(False)
        self.validate_btn.setText("Checking…")
        try:
            pbi_id = int(text)
            fields = self.app_state.client.get_work_item(pbi_id)
            title = fields.get("System.Title", "Unknown")
            wtype = fields.get("System.WorkItemType", "")
            area = fields.get("System.AreaPath", "")
            iteration = fields.get("System.IterationPath", "")

            self.app_state.pbi_id = pbi_id
            self.app_state.pbi_title = f"{title} ({wtype})"
            self.app_state.area_path = area
            self.app_state.iteration_path = iteration

            self.pbi_result_label.setStyleSheet("color: #080;")
            self.pbi_result_label.setText(f"Found: {title} ({wtype})")

            save_recent_pbi(pbi_id, title)
            self._populate_recent_pbis()

            # Populate and reveal the Area / Iteration fields
            self.area_edit.setText(area)
            self.iteration_edit.setText(iteration)
            self.paths_container.setVisible(True)
            self.paths_note.setVisible(True)

            self._check_ready()
        except LookupError:
            self.pbi_result_label.setStyleSheet("color: #c00;")
            self.pbi_result_label.setText(
                f"Work item #{text} not found in project '{self.app_state.token_manager.project}'. "
                "Double-check the ID."
            )
            self.paths_container.setVisible(False)
            self.paths_note.setVisible(False)
        except Exception as exc:
            self.pbi_result_label.setStyleSheet("color: #c00;")
            self.pbi_result_label.setText(f"Error: {exc}")
            self.paths_container.setVisible(False)
            self.paths_note.setVisible(False)
        finally:
            self.validate_btn.setEnabled(True)
            self.validate_btn.setText("Validate PBI")

    def _load_fields(self):
        self.load_fields_btn.setEnabled(False)
        self.load_fields_btn.setText("Loading…")
        try:
            self._fields = self.app_state.client.get_test_case_fields()

            for combo in (self.field_combo, self.preconditions_combo):
                combo.clear()
                combo.addItem("None — skip this field", None)
                for f in self._fields:
                    combo.addItem(f"{f['name']}  ({f['referenceName']})", f["referenceName"])
                combo.setEnabled(True)

            # Auto-select Module field
            for i in range(self.field_combo.count()):
                if "module" in self.field_combo.itemText(i).lower():
                    self.field_combo.setCurrentIndex(i)
                    break

            # Auto-select Preconditions field
            for i in range(self.preconditions_combo.count()):
                text = self.preconditions_combo.itemText(i).lower()
                if "prerequisite" in text or "precondition" in text:
                    self.preconditions_combo.setCurrentIndex(i)
                    break

            self._check_ready()
        except Exception as exc:
            QMessageBox.warning(self, "Field Load Error", f"Could not load Test Case fields:\n\n{exc}")
            for combo in (self.field_combo, self.preconditions_combo):
                combo.clear()
                combo.addItem("None — skip this field", None)
                combo.setEnabled(True)
        finally:
            self.load_fields_btn.setEnabled(True)
            self.load_fields_btn.setText("Reload Fields")

    def _check_ready(self):
        pbi_ok = self.app_state.pbi_id is not None
        # field_combo is only enabled after _load_fields succeeds
        self.continue_btn.setEnabled(pbi_ok and self.field_combo.isEnabled())

    def _on_continue(self):
        self.app_state.module_ref = self.field_combo.currentData()
        self.app_state.preconditions_ref = self.preconditions_combo.currentData()
        self.app_state.area_path = self.area_edit.text().strip()
        self.app_state.iteration_path = self.iteration_edit.text().strip()
        self.configured.emit()
