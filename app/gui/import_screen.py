import os
from pathlib import Path

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton,
    QFileDialog, QTableWidget, QTableWidgetItem, QHeaderView,
    QMessageBox, QFrame, QSizePolicy, QComboBox, QLineEdit,
    QListWidget
)
from PyQt5.QtCore import Qt, pyqtSignal
from PyQt5.QtGui import QColor, QCursor

from app.utils.import_parser import parse_file, generate_template
from app.utils.settings import load_settings, save_settings

_DETAIL = "detail"   # Qt.UserRole marker for step-detail rows

# Legacy builds shipped this as the hardcoded default Preconditions value and
# auto-saved it to settings on every queue. It is now treated as blank so the
# field starts empty by default.
_LEGACY_PRECONDITIONS_DEFAULT = "User is logged in as an HR Admin and clicked Definition Wizard."


class TagPickerWidget(QWidget):
    """Tag picker with inline search box and dropdown.
    Typing in the search box filters available tags; clicking a result
    adds it as a blue chip. Clicking × on a chip removes it."""

    _CHIP = (
        "QPushButton { background: #0078d4; color: white; border-radius: 3px; "
        "padding: 2px 8px; font-size: 12px; border: none; margin: 1px; }"
        "QPushButton:hover { background: #106ebe; }"
    )

    def __init__(self, parent=None):
        super().__init__(parent)
        self._available: list = []
        self._selected: list = []
        self._frame_qss = (
            "#tagInputFrame { border: 1px solid #ccc; border-radius: 4px; background: white; }"
        )
        self._search_qss = (
            "QLineEdit { border: none; background: transparent; font-size: 12px; }"
        )
        self._list_qss = (
            "QListWidget { border: 1px solid #ccc; border-top: none; "
            "border-bottom-left-radius: 4px; border-bottom-right-radius: 4px; "
            "background: white; outline: none; }"
            "QListWidget::item { padding: 5px 8px; }"
            "QListWidget::item:hover { background: #e8f0fe; }"
            "QListWidget::item:selected { background: #0078d4; color: white; }"
        )
        self._build_ui()

    def _build_ui(self):
        vlay = QVBoxLayout(self)
        vlay.setContentsMargins(0, 0, 0, 0)
        vlay.setSpacing(0)

        # Input frame: selected-tag chips + search QLineEdit
        self._input_frame = QFrame()
        self._input_frame.setObjectName("tagInputFrame")
        self._input_frame.setStyleSheet(self._frame_qss)
        self._input_frame.setMinimumHeight(36)

        self._chips_hbox = QHBoxLayout(self._input_frame)
        self._chips_hbox.setContentsMargins(4, 3, 6, 3)
        self._chips_hbox.setSpacing(4)

        self._search = QLineEdit()
        self._search.setPlaceholderText("Search tags…")
        self._search.setStyleSheet(self._search_qss)
        self._search.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self._search.textChanged.connect(self._on_text_changed)
        self._chips_hbox.addWidget(self._search)

        vlay.addWidget(self._input_frame)

        # Dropdown list — shown only when there is filtered text
        self._dropdown = QListWidget()
        self._dropdown.setStyleSheet(self._list_qss)
        self._dropdown.setMaximumHeight(160)
        self._dropdown.setVisible(False)
        self._dropdown.setCursor(QCursor(Qt.PointingHandCursor))
        self._dropdown.itemClicked.connect(self._on_item_clicked)
        vlay.addWidget(self._dropdown)

    def set_available_tags(self, tags: list):
        self._available = list(tags)
        self._selected.clear()
        self._rebuild_chips()
        self._search.clear()
        self._dropdown.setVisible(False)

    def _rebuild_chips(self):
        """Reconstruct chip buttons inside the input frame."""
        while self._chips_hbox.count():
            item = self._chips_hbox.takeAt(0)
            w = item.widget()
            if w is not None and w is not self._search:
                w.deleteLater()
        for tag in self._selected:
            btn = QPushButton(f"{tag}  ×")
            btn.setStyleSheet(self._CHIP)
            btn.setCursor(QCursor(Qt.PointingHandCursor))
            btn.setSizePolicy(QSizePolicy.Fixed, QSizePolicy.Fixed)
            btn.clicked.connect(lambda checked=False, t=tag: self._remove_tag(t))
            self._chips_hbox.addWidget(btn)
        self._chips_hbox.addWidget(self._search)

    def _on_text_changed(self, text: str):
        # Semicolon-separated batch input: "Automation; Regression; 26R1;"
        # Everything before the last semicolon is treated as a committed token.
        if ";" in text:
            tokens = [tok.strip() for tok in text.split(";")]
            committed, remainder = tokens[:-1], tokens[-1]
            avail_lower = {t.lower(): t for t in self._available}
            changed = False
            for tok in committed:
                if tok and tok.lower() in avail_lower:
                    canonical = avail_lower[tok.lower()]
                    if canonical not in self._selected:
                        self._selected.append(canonical)
                        changed = True
            if changed:
                self._rebuild_chips()
            # Replace the field with just the trailing fragment, without re-triggering this handler
            self._search.blockSignals(True)
            self._search.setText(remainder)
            self._search.blockSignals(False)
            text = remainder

        if not text.strip():
            self._dropdown.setVisible(False)
            return
        ltext = text.lower()
        matches = [t for t in self._available
                   if ltext in t.lower() and t not in self._selected]
        self._dropdown.clear()
        if matches:
            for tag in matches:
                self._dropdown.addItem(tag)
            self._dropdown.setVisible(True)
        else:
            self._dropdown.setVisible(False)

    def _on_item_clicked(self, item):
        tag = item.text()
        if tag not in self._selected:
            self._selected.append(tag)
            self._rebuild_chips()
        self._search.clear()
        self._dropdown.setVisible(False)
        self._search.setFocus()

    def _remove_tag(self, tag: str):
        if tag in self._selected:
            self._selected.remove(tag)
            self._rebuild_chips()
            self._on_text_changed(self._search.text())

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._frame_qss = (
            f"#tagInputFrame {{ border: 1px solid {t['border']}; border-radius: 4px; "
            f"background: {t['tag_inner_bg']}; }}"
        )
        self._search_qss = (
            f"QLineEdit {{ border: none; background: transparent; "
            f"color: {t['text']}; font-size: 12px; }}"
        )
        self._list_qss = (
            f"QListWidget {{ border: 1px solid {t['border']}; border-top: none; "
            f"border-bottom-left-radius: 4px; border-bottom-right-radius: 4px; "
            f"background: {t['tag_inner_bg']}; outline: none; }}"
            f"QListWidget::item {{ padding: 5px 8px; color: {t['text']}; }}"
            f"QListWidget::item:hover {{ background: {t['tag_unsel_hover']}; }}"
            f"QListWidget::item:selected {{ background: {t['accent']}; color: white; }}"
        )
        self._input_frame.setStyleSheet(self._frame_qss)
        self._search.setStyleSheet(self._search_qss)
        self._dropdown.setStyleSheet(self._list_qss)

    def set_search_placeholder(self, text: str):
        self._search.setPlaceholderText(text)

    def get_tags_string(self) -> str:
        return "; ".join(self._selected)

    def clear_selection(self):
        self._selected.clear()
        self._rebuild_chips()
        self._search.clear()
        self._dropdown.setVisible(False)


class ImportWidget(QWidget):
    """Tab widget for importing test cases from an Excel or CSV file."""

    test_cases_queued = pyqtSignal(list)  # emits list[TestCase]

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._parsed_cases = []
        self._tags_loaded = False
        self._tags_loading = False
        self._build_ui()
        self._restore_override_settings()

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(12)

        # Template download row
        self._tmpl_frame = QFrame()
        self._tmpl_frame.setObjectName("tmplFrame")
        self._tmpl_frame.setStyleSheet(
            "#tmplFrame { background: #e8f4fb; border: 1px solid #b3d9f5; border-radius: 6px; }"
        )
        tmpl_layout = QHBoxLayout(self._tmpl_frame)
        tmpl_layout.setContentsMargins(16, 10, 16, 10)
        tmpl_label = QLabel(
            "<b>First time?</b> Download the Excel template, fill it in, then import it here."
        )
        tmpl_label.setWordWrap(True)
        tmpl_layout.addWidget(tmpl_label, 1)
        tmpl_btn = QPushButton("Download Template (.xlsx)")
        tmpl_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; padding: 6px 14px; }"
            "QPushButton:hover { background: #106ebe; }"
        )
        tmpl_btn.setCursor(QCursor(Qt.PointingHandCursor))
        tmpl_btn.clicked.connect(self._download_template)
        tmpl_layout.addWidget(tmpl_btn)
        layout.addWidget(self._tmpl_frame)

        # File picker
        file_row = QHBoxLayout()
        self.file_label = QLabel("No file selected")
        self.file_label.setStyleSheet("color: #666;")
        file_row.addWidget(self.file_label, 1)
        browse_btn = QPushButton("Browse…")
        browse_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 5px 14px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        browse_btn.setCursor(QCursor(Qt.PointingHandCursor))
        browse_btn.clicked.connect(self._browse_file)
        file_row.addWidget(browse_btn)
        layout.addLayout(file_row)

        # Override defaults section
        self._override_frame = QFrame()
        self._override_frame.setObjectName("overrideFrame")
        self._override_frame.setStyleSheet(
            "#overrideFrame { background: #f9f9f9; border: 1px solid #ddd; border-radius: 6px; }"
        )
        ov_outer = QVBoxLayout(self._override_frame)
        ov_outer.setContentsMargins(16, 10, 16, 10)
        ov_outer.setSpacing(10)

        # Row 1 — Automation Status, Module, Tags
        ov_row1 = QHBoxLayout()
        ov_row1.setSpacing(20)
        ov_row1.addWidget(QLabel("<b>Apply to all imported cases:</b>"))

        auto_col = QVBoxLayout()
        auto_col.setSpacing(4)
        auto_col.addWidget(QLabel("Automation Status"))
        self.automation_combo = QComboBox()
        self.automation_combo.addItems(["Not Automated", "Planned"])
        self.automation_combo.setMinimumWidth(150)
        auto_col.addWidget(self.automation_combo)
        ov_row1.addLayout(auto_col)

        mod_col = QVBoxLayout()
        mod_col.setSpacing(4)
        mod_col.addWidget(QLabel("Module"))
        self.module_edit = QComboBox()
        self.module_edit.setEditable(True)
        self.module_edit.setInsertPolicy(QComboBox.NoInsert)
        self.module_edit.lineEdit().setPlaceholderText("e.g. Authentication  (leave blank to use xlsx value)")
        self.module_edit.setMinimumWidth(220)
        mod_col.addWidget(self.module_edit)
        ov_row1.addLayout(mod_col)

        created_col = QVBoxLayout()
        created_col.setSpacing(4)
        created_col.addWidget(QLabel("Created By"))
        self.created_by_combo = QComboBox()
        self.created_by_combo.setMinimumWidth(180)
        created_col.addWidget(self.created_by_combo)
        ov_row1.addLayout(created_col)

        ov_row1.addStretch()
        ov_outer.addLayout(ov_row1)

        # Tags picker (full width)
        ov_outer.addWidget(QLabel("Tags"))
        self.tag_picker = TagPickerWidget()
        ov_outer.addWidget(self.tag_picker)

        # Row 2 — Preconditions (full width)
        ov_row2 = QHBoxLayout()
        ov_row2.setSpacing(12)
        pre_lbl = QLabel("Preconditions")
        ov_row2.addWidget(pre_lbl)
        self.preconditions_edit = QLineEdit()
        self.preconditions_edit.setPlaceholderText("Leave blank to use each file's Preconditions value")
        ov_row2.addWidget(self.preconditions_edit)
        ov_outer.addLayout(ov_row2)

        layout.addWidget(self._override_frame)

        # Warnings label
        self.warnings_label = QLabel("")
        self.warnings_label.setWordWrap(True)
        self.warnings_label.setStyleSheet(
            "background: #fffbe6; border: 1px solid #ffe58f; border-radius: 4px; "
            "padding: 6px; color: #555;"
        )
        self.warnings_label.setVisible(False)
        layout.addWidget(self.warnings_label)

        # Preview table
        # Col 0: ▶/▼ expand  Col 1: Name  Col 2: Steps  Col 3: Tags  Col 4: Module  Col 5: ✕
        preview_lbl = QLabel(
            "Preview — ▶ to expand steps, ✕ to remove before queuing:"
        )
        preview_lbl.setStyleSheet("font-weight: bold;")
        layout.addWidget(preview_lbl)

        self.preview_table = QTableWidget(0, 6)
        self.preview_table.setHorizontalHeaderLabels(
            ["", "Test Case Name", "Steps", "Tags", "Module", ""]
        )
        hh = self.preview_table.horizontalHeader()
        hh.setSectionResizeMode(0, QHeaderView.Fixed)
        hh.setSectionResizeMode(1, QHeaderView.Stretch)
        hh.setSectionResizeMode(2, QHeaderView.ResizeToContents)
        hh.setSectionResizeMode(3, QHeaderView.ResizeToContents)
        hh.setSectionResizeMode(4, QHeaderView.ResizeToContents)
        hh.setSectionResizeMode(5, QHeaderView.Fixed)
        self.preview_table.setColumnWidth(0, 34)
        self.preview_table.setColumnWidth(5, 36)
        self.preview_table.setEditTriggers(QTableWidget.NoEditTriggers)
        self.preview_table.setSelectionMode(QTableWidget.NoSelection)
        self.preview_table.setAlternatingRowColors(True)
        layout.addWidget(self.preview_table)

        # count_label and queue_btn are created here but placed into the
        # main window footer row by MainWindow._build_main_page so they sit
        # level with the "Review & Create" button.
        self.count_label = QLabel("0 test cases parsed")
        self.count_label.setStyleSheet("color: #555;")

        self.queue_btn = QPushButton("Add All to Queue")
        self.queue_btn.setFixedHeight(34)
        self.queue_btn.setEnabled(False)
        self.queue_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; "
            "font-size: 13px; padding: 0 20px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self.queue_btn.clicked.connect(self._on_queue)

    def _refresh_module_combo(self):
        from app.gui.helpers import refresh_module_combo
        refresh_module_combo(self.module_edit, self.app_state.known_module_values)

    def _refresh_created_by_combo(self):
        from app.utils.members_cache import load_cached, attach_once, TeamMemberFetcher
        tm = self.app_state.client.tm

        # Populate immediately from in-memory cache, falling back to disk cache
        if self.app_state.cached_team_members is None:
            on_disk = load_cached(tm.org_url, tm.project)
            if on_disk is not None:
                self.app_state.cached_team_members = on_disk

        if self.app_state.cached_team_members is not None:
            self._populate_created_by_combo(self.app_state.cached_team_members)
        else:
            self.created_by_combo.blockSignals(True)
            self.created_by_combo.clear()
            self.created_by_combo.addItem("Loading users…")
            self.created_by_combo.blockSignals(False)

        # Background refresh — one in-flight fetch shared across all widgets
        if self.app_state._team_members_fetcher is None:
            fetcher = TeamMemberFetcher(self.app_state.client)
            self.app_state._team_members_fetcher = fetcher
            fetcher.done.connect(self._on_members_fetched)
            fetcher.failed.connect(self._on_members_failed)
            fetcher.start()
        else:
            # Attach to the already-running fetch so we get the result too
            attach_once(self.app_state._team_members_fetcher, self._populate_created_by_combo)

    def _on_members_fetched(self, members: list):
        from app.utils.members_cache import save_to_disk
        tm = self.app_state.client.tm
        self.app_state.cached_team_members = members
        self.app_state._team_members_fetcher = None
        if members:
            save_to_disk(tm.org_url, tm.project, members)
        self._populate_created_by_combo(members)

    def _on_members_failed(self, _msg: str):
        # Keep previously cached members; clear the fetcher so a later retry can happen.
        self.app_state._team_members_fetcher = None
        self._populate_created_by_combo(self.app_state.cached_team_members or [])

    def _populate_created_by_combo(self, members: list):
        cur = self.created_by_combo.currentText()
        self.created_by_combo.blockSignals(True)
        self.created_by_combo.clear()
        self.created_by_combo.addItem("(Current User)")
        for user in members:
            display = user.get("displayName", user.get("uniqueName", ""))
            unique = user.get("uniqueName", "")
            if display and unique:
                self.created_by_combo.addItem(display, unique)
        if cur and cur not in ("(Current User)", "Loading users…"):
            idx = self.created_by_combo.findText(cur)
            if idx >= 0:
                self.created_by_combo.setCurrentIndex(idx)
        self.created_by_combo.blockSignals(False)

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._tmpl_frame.setStyleSheet(
            f"#tmplFrame {{ background: {t['tmpl_bg']}; border: 1px solid {t['tmpl_border']}; border-radius: 6px; }}"
        )
        self.file_label.setStyleSheet(f"color: {t['file_lbl_color']};")
        self._override_frame.setStyleSheet(
            f"#overrideFrame {{ background: {t['surface']}; border: 1px solid {t['border']}; border-radius: 6px; }}"
        )
        self.warnings_label.setStyleSheet(
            f"background: {t['warn_bg']}; border: 1px solid {t['warn_border']}; "
            f"border-radius: 4px; padding: 6px; color: {t['text_dim']};"
        )
        self.count_label.setStyleSheet(f"color: {t['count_lbl_color']};")
        self.tag_picker.refresh_theme()
        # Restyle the per-row expand arrows so they stay visible in dark mode
        expand_style = self._expand_btn_style()
        for r in range(self.preview_table.rowCount()):
            w = self.preview_table.cellWidget(r, 0)
            if w is not None:
                w.setStyleSheet(expand_style)

    @staticmethod
    def _expand_btn_style() -> str:
        from app.utils import theme
        t = theme.tokens()
        return (
            f"QPushButton {{ background: transparent; color: {t['text']}; border: none; "
            f"font-size: 14px; font-weight: bold; }}"
            f"QPushButton:hover {{ color: {t['accent']}; }}"
        )

    def _restore_override_settings(self):
        s = load_settings()
        saved = s.get("preconditions", "")
        # Migrate the legacy hardcoded default to blank so the field is empty by default.
        if saved == _LEGACY_PRECONDITIONS_DEFAULT:
            saved = ""
        self.preconditions_edit.setText(saved)

    def showEvent(self, event):
        super().showEvent(event)
        self._refresh_module_combo()
        self._refresh_created_by_combo()
        self._load_existing_cases()
        if not self._tags_loaded and not self._tags_loading:
            self._load_tags()

    def _load_existing_cases(self):
        """Fetch the test cases already on the current PBI in the background so that
        duplicate titles can be detected and offered for update at queue time.
        Results are stored on app_state and shared with the Edit tab; nothing is
        fetched if the Edit tab already loaded them for this PBI."""
        from PyQt5.QtCore import QThreadPool
        from app.utils.worker import Worker

        pbi_id = self.app_state.pbi_id
        if not pbi_id or self.app_state.existing_cases_pbi == pbi_id:
            return
        if self.app_state.token_manager.is_expired():
            return

        extra = [r for r in (self.app_state.module_ref,) if r]
        worker = Worker(self.app_state.client.get_test_cases_for_pbi, pbi_id, extra)
        worker.signals.result.connect(
            lambda r, p=pbi_id: self._on_existing_cases_loaded(p, r)
        )
        worker.signals.error.connect(lambda _exc: None)  # silent — duplicate check just degrades
        QThreadPool.globalInstance().start(worker)

    def _on_existing_cases_loaded(self, pbi_id: int, result: tuple):
        cases, _total = result
        self.app_state.existing_cases = cases
        self.app_state.existing_cases_pbi = pbi_id

    def _load_tags(self):
        """Fetch project tags in the background; never blocks the GUI thread."""
        from PyQt5.QtCore import QThreadPool
        from app.utils.worker import Worker
        self._tags_loading = True
        self.tag_picker.set_search_placeholder("Loading tags…")
        worker = Worker(self.app_state.client.get_tags)
        worker.signals.result.connect(self._on_tags_loaded)
        worker.signals.error.connect(self._on_tags_error)
        QThreadPool.globalInstance().start(worker)

    def _on_tags_loaded(self, tags_data: list):
        self._tags_loading = False
        self._tags_loaded = True
        names = sorted({t["name"] for t in tags_data if t.get("name")})
        self.tag_picker.set_available_tags(names)
        self.tag_picker.set_search_placeholder("Search tags…")

    def _on_tags_error(self, _exc: Exception):
        # Leave _tags_loaded False so the next visit to this tab retries.
        self._tags_loading = False
        self.tag_picker.set_search_placeholder("Tags unavailable — will retry")

    # ------------------------------------------------------------------ #
    #  File handling                                                       #
    # ------------------------------------------------------------------ #

    def _download_template(self):
        save_path, _ = QFileDialog.getSaveFileName(
            self,
            "Save Template",
            str(Path.home() / "Downloads" / "test_cases_template.xlsx"),
            "Excel Files (*.xlsx)",
        )
        if not save_path:
            return
        try:
            generate_template(save_path)
            QMessageBox.information(
                self, "Template Saved",
                f"Template saved to:\n{save_path}\n\n"
                "Fill in your test cases and import the file here."
            )
        except Exception as exc:
            QMessageBox.critical(self, "Save Error", f"Could not save template:\n{exc}")

    def _browse_file(self):
        paths, _ = QFileDialog.getOpenFileNames(
            self,
            "Select Test Cases File(s)",
            str(Path.home()),
            "Spreadsheet Files (*.xlsx *.csv);;Excel Files (*.xlsx);;CSV Files (*.csv)",
        )
        if not paths:
            return
        if len(paths) == 1:
            self._load_file(paths[0])
        else:
            self._load_files(paths)

    def _load_files(self, paths: list):
        self.file_label.setText("Loading…")
        self.warnings_label.setVisible(False)
        self.preview_table.setRowCount(0)
        self._parsed_cases = []
        self.queue_btn.setEnabled(False)

        all_cases = []
        all_warnings = []
        file_names = []

        for path in paths:
            fname = os.path.basename(path)
            try:
                cases, warnings = parse_file(path)
                file_names.append(fname)
                existing_titles = {tc.title.lower() for tc in all_cases}
                for tc in cases:
                    if tc.title.lower() in existing_titles:
                        original = tc.title
                        tc.title = f"{original} ({os.path.splitext(fname)[0]})"
                        all_warnings.append(
                            f"Duplicate title renamed: '{original}' → '{tc.title}'"
                        )
                    all_cases.append(tc)
                    existing_titles.add(tc.title.lower())
                if warnings:
                    all_warnings.extend(f"[{fname}] {w}" for w in warnings)
            except Exception as exc:
                all_warnings.append(f"[{fname}] Error: {exc}")

        self._parsed_cases = all_cases
        n_files = len(file_names)
        n_cases = len(all_cases)
        self.file_label.setText(f"{n_files} file{'s' if n_files != 1 else ''} — {n_cases} test case{'s' if n_cases != 1 else ''}")

        if all_warnings:
            self.warnings_label.setText(
                "Warnings during import:\n" + "\n".join(f"• {w}" for w in all_warnings)
            )
            self.warnings_label.setVisible(True)

        for tc in all_cases:
            self._append_summary_row(tc)

        self._refresh_count()

    def _load_file(self, path: str):
        self.file_label.setText(os.path.basename(path))
        self.warnings_label.setVisible(False)
        self.preview_table.setRowCount(0)
        self._parsed_cases = []
        self.queue_btn.setEnabled(False)

        try:
            cases, warnings = parse_file(path)
        except Exception as exc:
            QMessageBox.critical(self, "Import Error", f"Could not parse file:\n\n{exc}")
            return

        self._parsed_cases = cases

        if warnings:
            self.warnings_label.setText(
                "Warnings during import:\n" + "\n".join(f"• {w}" for w in warnings)
            )
            self.warnings_label.setVisible(True)

        for tc in cases:
            self._append_summary_row(tc)

        self._refresh_count()

    # ------------------------------------------------------------------ #
    #  Row building                                                        #
    # ------------------------------------------------------------------ #

    def _append_summary_row(self, tc):
        row = self.preview_table.rowCount()
        self.preview_table.insertRow(row)

        # Col 0 — expand toggle
        expand_btn = QPushButton("▶")
        expand_btn.setFixedSize(26, 24)
        expand_btn.setCursor(QCursor(Qt.PointingHandCursor))
        expand_btn.setToolTip("Show / hide steps")
        expand_btn.setStyleSheet(self._expand_btn_style())
        expand_btn.clicked.connect(self._toggle_expand)
        self.preview_table.setCellWidget(row, 0, expand_btn)

        # Cols 1–4 — data (no UserRole = summary row)
        self.preview_table.setItem(row, 1, QTableWidgetItem(tc.title))
        self.preview_table.setItem(row, 2, QTableWidgetItem(str(len(tc.steps))))
        self.preview_table.setItem(row, 3, QTableWidgetItem(tc.tags or "—"))
        self.preview_table.setItem(row, 4, QTableWidgetItem(tc.module_value or "—"))

        # Col 5 — remove button
        remove_btn = QPushButton("✕")
        remove_btn.setFixedSize(26, 22)
        remove_btn.setCursor(QCursor(Qt.PointingHandCursor))
        remove_btn.setStyleSheet(
            "QPushButton { background: #c42b1c; color: white; border-radius: 3px; "
            "font-size: 11px; font-weight: bold; }"
            "QPushButton:hover { background: #a4261a; }"
        )
        remove_btn.clicked.connect(self._remove_case)
        _rm_wrap = QWidget()
        _rm_layout = QHBoxLayout(_rm_wrap)
        _rm_layout.setContentsMargins(0, 0, 0, 0)
        _rm_layout.setAlignment(Qt.AlignCenter)
        _rm_layout.addWidget(remove_btn)
        self.preview_table.setCellWidget(row, 5, _rm_wrap)

    def _insert_detail_rows(self, summary_row: int, tc):
        """Insert one step row per step immediately after summary_row."""
        from app.utils import theme as _theme
        _t = _theme.tokens()
        detail_bg = QColor(_t["detail_row_bg"])
        action_fg = QColor(_t["text"])
        expected_fg = QColor(_t["text_dim"])
        insert_at = summary_row + 1

        for i, step in enumerate(tc.steps):
            r = insert_at + i
            self.preview_table.insertRow(r)
            self.preview_table.setRowHeight(r, 22)

            # Col 0 — indent spacer
            spacer = QTableWidgetItem("")
            spacer.setBackground(detail_bg)
            self.preview_table.setItem(r, 0, spacer)

            # Cols 1–2 merged — step action (UserRole marks this as a detail row)
            action_item = QTableWidgetItem(f"  Step {i + 1}:  {step.action}")
            action_item.setData(Qt.UserRole, _DETAIL)
            action_item.setBackground(detail_bg)
            action_item.setForeground(action_fg)
            self.preview_table.setItem(r, 1, action_item)
            self.preview_table.setSpan(r, 1, 1, 2)

            # Cols 3–4 merged — expected result
            exp_text = step.expected if step.expected else ""
            exp_item = QTableWidgetItem(exp_text)
            exp_item.setData(Qt.UserRole, _DETAIL)
            exp_item.setBackground(detail_bg)
            exp_item.setForeground(expected_fg)
            self.preview_table.setItem(r, 3, exp_item)
            self.preview_table.setSpan(r, 3, 1, 2)

            # Col 5 — spacer
            end_spacer = QTableWidgetItem("")
            end_spacer.setBackground(detail_bg)
            self.preview_table.setItem(r, 5, end_spacer)

    def _remove_detail_rows(self, summary_row: int):
        """Remove all consecutive detail rows that follow summary_row."""
        while True:
            next_row = summary_row + 1
            if next_row >= self.preview_table.rowCount():
                break
            if not self._is_detail_row(next_row):
                break
            self.preview_table.removeRow(next_row)

    # ------------------------------------------------------------------ #
    #  Expand / collapse                                                   #
    # ------------------------------------------------------------------ #

    def _toggle_expand(self):
        btn = self.sender()
        summary_row = -1
        for r in range(self.preview_table.rowCount()):
            if self.preview_table.cellWidget(r, 0) is btn:
                summary_row = r
                break
        if summary_row == -1:
            return

        next_row = summary_row + 1
        already_expanded = (
            next_row < self.preview_table.rowCount()
            and self._is_detail_row(next_row)
        )

        if already_expanded:
            self._remove_detail_rows(summary_row)
            btn.setText("▶")
        else:
            tc_idx = self._tc_index_for_row(summary_row)
            self._insert_detail_rows(summary_row, self._parsed_cases[tc_idx])
            btn.setText("▼")

    # ------------------------------------------------------------------ #
    #  Remove case                                                         #
    # ------------------------------------------------------------------ #

    def _remove_case(self):
        btn = self.sender()
        wrapper = btn.parent()
        for row in range(self.preview_table.rowCount()):
            if self.preview_table.cellWidget(row, 5) is wrapper:
                # Collapse detail rows first so index arithmetic stays correct
                self._remove_detail_rows(row)
                tc_idx = self._tc_index_for_row(row)
                self.preview_table.removeRow(row)
                del self._parsed_cases[tc_idx]
                self._refresh_count()
                break

    # ------------------------------------------------------------------ #
    #  Helpers                                                             #
    # ------------------------------------------------------------------ #

    def _is_detail_row(self, row: int) -> bool:
        item = self.preview_table.item(row, 1)
        return item is not None and item.data(Qt.UserRole) == _DETAIL

    def _tc_index_for_row(self, row: int) -> int:
        """Count how many summary rows appear before this row."""
        count = 0
        for r in range(row):
            if not self._is_detail_row(r):
                count += 1
        return count

    def _refresh_count(self):
        n = len(self._parsed_cases)
        self.count_label.setText(f"{n} test case{'s' if n != 1 else ''} parsed")
        self.queue_btn.setEnabled(n > 0)

    # ------------------------------------------------------------------ #
    #  Queue                                                               #
    # ------------------------------------------------------------------ #

    def _on_queue(self):
        if not self._parsed_cases:
            return

        auto_status = self.automation_combo.currentText()
        module_val = self.module_edit.currentText().strip()
        tags_val = self.tag_picker.get_tags_string()
        preconditions_val = self.preconditions_edit.text().strip()

        created_by = ""
        cur_text = self.created_by_combo.currentText()
        if cur_text != "(Current User)":
            created_by = self.created_by_combo.currentData()

        save_settings({"preconditions": preconditions_val})

        for tc in self._parsed_cases:
            tc.automation_status = auto_status
            if module_val:
                tc.module_value = module_val
            if tags_val:
                tc.tags = tags_val
            if preconditions_val:
                tc.preconditions = preconditions_val
            tc.created_by = created_by

        # MainWindow clears the preview via on_queue_accepted() only if the
        # cases were actually added (the duplicate-title dialog may reject them).
        self.test_cases_queued.emit(list(self._parsed_cases))

    def on_queue_accepted(self):
        """Called by MainWindow after the emitted cases were added to the queue."""
        n = len(self._parsed_cases)
        self.preview_table.setRowCount(0)
        self._parsed_cases = []
        self.file_label.setText("No file selected")
        self.tag_picker.clear_selection()
        self._refresh_count()
        from app.gui.helpers import status_message
        status_message(
            self,
            f"{n} test case{'s' if n != 1 else ''} added to the queue — "
            "click 'Review && Create' below when ready."
        )
