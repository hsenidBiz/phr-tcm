from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QComboBox, QMenu, QWidgetAction, QDialog, QDialogButtonBox,
    QMessageBox, QFrame, QSizePolicy
)
from PyQt5.QtCore import Qt, pyqtSignal, QThreadPool
from PyQt5.QtGui import QFont, QCursor

from app.utils.settings import load_settings, save_recent_pbi, remove_recent_pbi
from app.utils.worker import Worker


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
        layout.addSpacing(12)

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
            "Enter the work item ID of the PBI you want to link test cases to. "
            "You can find this number in the top-left corner of the PBI card in Azure DevOps."
        )
        self._pbi_note.setWordWrap(True)
        self._pbi_note.setStyleSheet("color: #555;")
        pbi_layout.addWidget(self._pbi_note)

        self.recent_pbi_btn = QPushButton("No recent PBIs")
        self.recent_pbi_btn.setEnabled(False)
        self.recent_pbi_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.recent_pbi_btn.setStyleSheet(
            "QPushButton { text-align: left; padding: 5px 10px; border: 1px solid #ccc; "
            "border-radius: 4px; background: white; min-height: 28px; }"
            "QPushButton:hover { background: #f0f0f0; }"
            "QPushButton:disabled { color: #888; background: #f5f5f5; border-color: #ddd; }"
        )
        self.recent_pbi_btn.clicked.connect(self._show_recent_pbi_menu)
        pbi_layout.addWidget(self.recent_pbi_btn)

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

        layout.addWidget(self._pbi_frame)
        layout.addSpacing(10)

        # Custom Fields — built once into a dialog, opened on demand
        self._build_custom_fields_dialog()

        cf_row = QHBoxLayout()
        self._edit_fields_btn = QPushButton("Edit Custom Fields…")
        self._edit_fields_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; border-radius: 4px; padding: 5px 14px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self._edit_fields_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._edit_fields_btn.clicked.connect(self._open_custom_fields)
        cf_row.addWidget(self._edit_fields_btn)
        cf_row.addStretch()
        layout.addLayout(cf_row)
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

    def _build_custom_fields_dialog(self):
        self._custom_fields_dlg = QDialog(self)
        self._custom_fields_dlg.setWindowTitle("Custom Fields")
        self._custom_fields_dlg.setMinimumWidth(540)

        dlg_layout = QVBoxLayout(self._custom_fields_dlg)
        dlg_layout.setContentsMargins(24, 20, 24, 20)
        dlg_layout.setSpacing(12)

        dlg_layout.addWidget(QLabel("<b>Custom Fields</b>"))

        self._dlg_note = QLabel(
            "Select which fields map to 'Module' and 'Preconditions' in your Test Case "
            "work item. Select 'None — skip this field' for any field your organisation does not use."
        )
        self._dlg_note.setWordWrap(True)
        self._dlg_note.setStyleSheet("color: #555;")
        dlg_layout.addWidget(self._dlg_note)

        fields_grid = QHBoxLayout()

        module_col = QVBoxLayout()
        module_col.addWidget(QLabel("Module Field"))
        self.field_combo = QComboBox()
        self.field_combo.setMinimumWidth(200)
        self.field_combo.addItem("Loading fields…", None)
        self.field_combo.setEnabled(False)
        module_col.addWidget(self.field_combo)
        fields_grid.addLayout(module_col)

        pre_col = QVBoxLayout()
        pre_col.addWidget(QLabel("Preconditions Field"))
        self.preconditions_combo = QComboBox()
        self.preconditions_combo.setMinimumWidth(200)
        self.preconditions_combo.addItem("Loading fields…", None)
        self.preconditions_combo.setEnabled(False)
        pre_col.addWidget(self.preconditions_combo)
        fields_grid.addLayout(pre_col)

        fields_grid.addStretch()
        dlg_layout.addLayout(fields_grid)

        self.load_fields_btn = QPushButton("Load Test Case Fields")
        self.load_fields_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; border-radius: 4px; padding: 5px 14px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self.load_fields_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.load_fields_btn.clicked.connect(self._load_fields)
        dlg_layout.addWidget(self.load_fields_btn)

        btn_box = QDialogButtonBox(QDialogButtonBox.Close)
        btn_box.rejected.connect(self._custom_fields_dlg.close)
        dlg_layout.addWidget(btn_box)

    def _open_custom_fields(self):
        self._custom_fields_dlg.exec_()

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._pbi_frame.setStyleSheet(
            f"#pbiFrame {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 8px; }}"
        )
        self.connected_label.setStyleSheet(f"color: {t['accent']};")
        self.recent_pbi_btn.setStyleSheet(
            f"QPushButton {{ text-align: left; padding: 5px 10px; "
            f"border: 1px solid {t['btn_border']}; border-radius: 4px; "
            f"background: {t['surface']}; min-height: 28px; color: {t['text']}; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
            f"QPushButton:disabled {{ color: {t['text_dim2']}; background: {t['surface']}; "
            f"border-color: {t['border']}; }}"
        )
        self._pbi_note.setStyleSheet(f"color: {t['text_dim']};")
        self.paths_note.setStyleSheet(f"color: {t['text_dim2']}; font-size: 11px;")
        _ro_style = (
            f"QLineEdit {{ background: {t['surface2']}; color: {t['text_dim']}; "
            f"border: 1px solid {t['border']}; border-radius: 4px; padding: 4px 8px; }}"
        )
        self.area_edit.setStyleSheet(_ro_style)
        self.iteration_edit.setStyleSheet(_ro_style)
        self._dlg_note.setStyleSheet(f"color: {t['text_dim']};")
        btn_neutral = (
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 5px 14px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
        self.load_fields_btn.setStyleSheet(btn_neutral)
        self._edit_fields_btn.setStyleSheet(btn_neutral)
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
        from app.utils import theme
        t = theme.tokens()
        tm = self.app_state.token_manager
        if tm.auto_refresh_active():
            self.connected_label.setStyleSheet(f"color: {t['accent']};")
            self.connected_label.setText(
                f"Connected to: {tm.org_url}/{tm.project}  |  Signed in — token refreshes automatically"
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

    def _populate_recent_pbis(self):
        recent = load_settings().get("recent_pbis", [])
        if recent:
            self.recent_pbi_btn.setText("— Select a recent PBI —  ▾")
            self.recent_pbi_btn.setEnabled(True)
        else:
            self.recent_pbi_btn.setText("No recent PBIs")
            self.recent_pbi_btn.setEnabled(False)

    def _show_recent_pbi_menu(self):
        recent = load_settings().get("recent_pbis", [])
        if not recent:
            return

        self._recent_menu = QMenu(self)
        self._recent_menu.setMinimumWidth(self.recent_pbi_btn.width())

        for r in recent:
            pbi_id = r["id"]
            text = f"#{r['id']}  —  {r['title']}"

            container = QWidget()
            row = QHBoxLayout(container)
            row.setContentsMargins(6, 3, 6, 3)
            row.setSpacing(6)

            select_btn = QPushButton(text)
            select_btn.setFlat(True)
            select_btn.setStyleSheet(
                "QPushButton { text-align: left; border: none; background: transparent; "
                "padding: 4px 6px; }"
                "QPushButton:hover { background: #e8f0fb; border-radius: 3px; }"
            )
            select_btn.setCursor(QCursor(Qt.PointingHandCursor))
            select_btn.clicked.connect(
                lambda checked=False, pid=pbi_id: self._select_recent_pbi(pid)
            )
            row.addWidget(select_btn, 1)

            remove_btn = QPushButton("✕")
            remove_btn.setFixedSize(22, 22)
            remove_btn.setCursor(QCursor(Qt.PointingHandCursor))
            remove_btn.setToolTip("Remove from recent")
            remove_btn.setStyleSheet(
                "QPushButton { background: transparent; border: none; color: #aaa; "
                "font-size: 11px; font-weight: bold; border-radius: 3px; }"
                "QPushButton:hover { color: #cc0000; background: #fee0e0; }"
            )
            remove_btn.clicked.connect(
                lambda checked=False, pid=pbi_id: self._remove_recent_pbi(pid)
            )
            row.addWidget(remove_btn)

            action = QWidgetAction(self._recent_menu)
            action.setDefaultWidget(container)
            self._recent_menu.addAction(action)

        pos = self.recent_pbi_btn.mapToGlobal(
            self.recent_pbi_btn.rect().bottomLeft()
        )
        self._recent_menu.exec_(pos)

    def _select_recent_pbi(self, pbi_id: int):
        if hasattr(self, "_recent_menu") and self._recent_menu:
            self._recent_menu.close()
        self.pbi_edit.setText(str(pbi_id))
        self._validate_pbi()

    def _remove_recent_pbi(self, pbi_id: int):
        if hasattr(self, "_recent_menu") and self._recent_menu:
            self._recent_menu.close()
        remove_recent_pbi(pbi_id)
        self._populate_recent_pbis()

    def _validate_pbi(self):
        from app.utils import theme
        text = self.pbi_edit.text().strip()
        if not text.isdigit():
            self.pbi_result_label.setStyleSheet(f"color: {theme.tokens()['error']};")
            self.pbi_result_label.setText("Please enter a numeric work item ID.")
            return

        self.validate_btn.setEnabled(False)
        self.validate_btn.setText("Checking…")
        pbi_id = int(text)

        worker = Worker(self.app_state.client.get_work_item, pbi_id)
        worker.signals.result.connect(lambda fields: self._on_pbi_result(pbi_id, fields))
        worker.signals.error.connect(lambda exc: self._on_pbi_error(text, exc))
        QThreadPool.globalInstance().start(worker)

    def _on_pbi_result(self, pbi_id: int, fields: dict):
        title = fields.get("System.Title", "Unknown")
        wtype = fields.get("System.WorkItemType", "")
        area = fields.get("System.AreaPath", "")
        iteration = fields.get("System.IterationPath", "")

        self.app_state.pbi_id = pbi_id
        self.app_state.pbi_title = f"{title} ({wtype})"
        self.app_state.area_path = area
        self.app_state.iteration_path = iteration

        from app.utils import theme
        self.pbi_result_label.setStyleSheet(f"color: {theme.tokens()['ok']};")
        self.pbi_result_label.setText(f"Found: {title} ({wtype})")

        save_recent_pbi(pbi_id, title)
        self._populate_recent_pbis()

        self.area_edit.setText(area)
        self.iteration_edit.setText(iteration)
        self.validate_btn.setEnabled(True)
        self.validate_btn.setText("Validate PBI")
        self._check_ready()

    def _on_pbi_error(self, text: str, exc: Exception):
        from app.utils import theme
        self.pbi_result_label.setStyleSheet(f"color: {theme.tokens()['error']};")
        if isinstance(exc, LookupError):
            self.pbi_result_label.setText(
                f"Work item #{text} not found in project '{self.app_state.token_manager.project}'. "
                "Double-check the ID."
            )
        else:
            self.pbi_result_label.setText(f"Error: {exc}")
        self.area_edit.clear()
        self.iteration_edit.clear()
        self.validate_btn.setEnabled(True)
        self.validate_btn.setText("Validate PBI")

    def _load_fields(self):
        self.load_fields_btn.setEnabled(False)
        self.load_fields_btn.setText("Loading…")

        worker = Worker(self.app_state.client.get_test_case_fields)
        worker.signals.result.connect(self._on_fields_result)
        worker.signals.error.connect(self._on_fields_error)
        QThreadPool.globalInstance().start(worker)

    def _on_fields_result(self, fields: list):
        self._fields = fields

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

        self.load_fields_btn.setEnabled(True)
        self.load_fields_btn.setText("Reload Fields")
        self._check_ready()

    def _on_fields_error(self, exc: Exception):
        QMessageBox.warning(self, "Field Load Error", f"Could not load Test Case fields:\n\n{exc}")
        for combo in (self.field_combo, self.preconditions_combo):
            combo.clear()
            combo.addItem("None — skip this field", None)
            combo.setEnabled(True)
        self.load_fields_btn.setEnabled(True)
        self.load_fields_btn.setText("Reload Fields")


    def _check_ready(self):
        pbi_ok = self.app_state.pbi_id is not None
        # field_combo is only enabled after _load_fields succeeds
        token_ok = not self.app_state.token_manager.is_expired()
        enabled = pbi_ok and self.field_combo.isEnabled() and token_ok
        self.continue_btn.setEnabled(enabled)
        if not token_ok:
            self.continue_btn.setToolTip("Token has expired — re-enter your token to continue")
        else:
            self.continue_btn.setToolTip("")

    def _on_continue(self):
        self.app_state.module_ref = self.field_combo.currentData()
        self.app_state.preconditions_ref = self.preconditions_combo.currentData()
        self.app_state.area_path = self.area_edit.text().strip()
        self.app_state.iteration_path = self.iteration_edit.text().strip()
        self.configured.emit()
