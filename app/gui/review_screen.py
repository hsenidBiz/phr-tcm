from pathlib import Path

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton,
    QTreeWidget, QTreeWidgetItem, QFrame, QMessageBox,
    QFileDialog, QShortcut
)
from PyQt5.QtCore import Qt, pyqtSignal
from PyQt5.QtGui import QFont, QColor, QBrush, QCursor, QKeySequence


class ReviewScreen(QWidget):
    confirmed = pyqtSignal()
    back_requested = pyqtSignal()
    queue_changed = pyqtSignal()  # emitted after any remove/reorder

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._build_ui()

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(40, 30, 40, 30)
        layout.setSpacing(14)

        title = QLabel("Review & Confirm")
        font = QFont()
        font.setPointSize(16)
        font.setBold(True)
        title.setFont(font)
        layout.addWidget(title)

        self.summary_label = QLabel("")
        self.summary_label.setWordWrap(True)
        layout.addWidget(self.summary_label)

        # Warning banner
        self._warn_frame = QFrame()
        self._warn_frame.setObjectName("warnFrame")
        self._warn_frame.setStyleSheet(
            "#warnFrame { background: #fff3cd; border: 1px solid #ffc107; border-radius: 6px; }"
        )
        warn_layout = QHBoxLayout(self._warn_frame)
        warn_layout.setContentsMargins(14, 10, 14, 10)
        warn_icon = QLabel("⚠️")
        warn_icon.setStyleSheet("font-size: 20px; border: none;")
        warn_layout.addWidget(warn_icon)
        self.warn_text = QLabel("")
        self.warn_text.setWordWrap(True)
        self.warn_text.setStyleSheet(
            "color: #856404; font-size: 13px; font-weight: bold; border: none;"
        )
        warn_layout.addWidget(self.warn_text, 1)
        layout.addWidget(self._warn_frame)

        # Tree view of all queued test cases
        self.tree = QTreeWidget()
        self.tree.setHeaderLabels(["Test Case / Step", "Details"])
        self.tree.setColumnWidth(0, 340)
        self.tree.setAlternatingRowColors(True)
        self.tree.setEditTriggers(QTreeWidget.NoEditTriggers)
        self.tree.setSelectionMode(QTreeWidget.ExtendedSelection)
        self.tree.itemSelectionChanged.connect(self._on_tree_selection_changed)
        layout.addWidget(self.tree)

        # Delete key shortcut on the tree
        del_sc = QShortcut(QKeySequence(Qt.Key_Delete), self.tree)
        del_sc.setContext(Qt.WidgetShortcut)
        del_sc.activated.connect(self._on_remove)

        # Module/config info
        self.config_label = QLabel("")
        self.config_label.setStyleSheet("color: #555; font-size: 11px;")
        layout.addWidget(self.config_label)

        # Buttons
        btn_row = QHBoxLayout()

        self.back_btn = QPushButton("← Back")
        self.back_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 7px 20px; font-size: 13px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self.back_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.back_btn.clicked.connect(self.back_requested)
        btn_row.addWidget(self.back_btn)
        btn_row.addSpacing(8)

        # Up / Down / Remove buttons
        _arrow_style = (
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 7px 12px; font-size: 13px; }"
            "QPushButton:hover { background: #e0e0e0; }"
            "QPushButton:disabled { color: #aaa; }"
        )
        self.move_up_btn = QPushButton("↑")
        self.move_up_btn.setEnabled(False)
        self.move_up_btn.setStyleSheet(_arrow_style)
        self.move_up_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.move_up_btn.setToolTip("Move selected test case up")
        self.move_up_btn.clicked.connect(self._on_move_up)
        btn_row.addWidget(self.move_up_btn)

        self.move_down_btn = QPushButton("↓")
        self.move_down_btn.setEnabled(False)
        self.move_down_btn.setStyleSheet(_arrow_style)
        self.move_down_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.move_down_btn.setToolTip("Move selected test case down")
        self.move_down_btn.clicked.connect(self._on_move_down)
        btn_row.addWidget(self.move_down_btn)
        btn_row.addSpacing(4)

        self.remove_selected_btn = QPushButton("✕ Remove")
        self.remove_selected_btn.setEnabled(False)
        self.remove_selected_btn.setStyleSheet(
            "QPushButton { background: #fde8e8; border: 1px solid #e88b8b; "
            "border-radius: 4px; padding: 7px 14px; font-size: 13px; }"
            "QPushButton:hover { background: #f8d0d0; }"
            "QPushButton:disabled { color: #aaa; background: #f5f5f5; border-color: #ddd; }"
        )
        self.remove_selected_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.remove_selected_btn.setToolTip("Remove selected test case(s) from queue (Delete)")
        self.remove_selected_btn.clicked.connect(self._on_remove)
        btn_row.addWidget(self.remove_selected_btn)
        btn_row.addSpacing(4)

        self.clear_all_btn = QPushButton("🗑 Clear All")
        self.clear_all_btn.setEnabled(False)
        self.clear_all_btn.setStyleSheet(
            "QPushButton { background: #fde8e8; border: 1px solid #e88b8b; "
            "border-radius: 4px; padding: 7px 14px; font-size: 13px; }"
            "QPushButton:hover { background: #f8d0d0; }"
            "QPushButton:disabled { color: #aaa; background: #f5f5f5; border-color: #ddd; }"
        )
        self.clear_all_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.clear_all_btn.setToolTip("Remove all test cases from the queue")
        self.clear_all_btn.clicked.connect(self._on_clear_all)
        btn_row.addWidget(self.clear_all_btn)

        btn_row.addStretch()

        self.export_queue_btn = QPushButton("Export Queue (.xlsx)")
        self.export_queue_btn.setEnabled(False)
        self.export_queue_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; padding: 7px 14px; font-size: 13px; }"
            "QPushButton:hover { background: #e0e0e0; }"
            "QPushButton:disabled { color: #aaa; }"
        )
        self.export_queue_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.export_queue_btn.clicked.connect(self._on_export_queue)
        btn_row.addWidget(self.export_queue_btn)
        btn_row.addSpacing(8)

        from app.utils import theme
        self.create_btn = QPushButton("Create All Test Cases")
        self.create_btn.setFixedHeight(40)
        self.create_btn.setStyleSheet(
            theme.btn_danger_qss(
                "border-radius: 4px; font-size: 14px; font-weight: bold; padding: 0 28px;"
            )
        )
        self.create_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.create_btn.clicked.connect(self._on_create)
        btn_row.addWidget(self.create_btn)
        layout.addLayout(btn_row)

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        self._warn_frame.setStyleSheet(
            f"#warnFrame {{ background: {t['review_warn_bg']}; "
            f"border: 1px solid {t['review_warn_border']}; border-radius: 6px; }}"
        )
        self.warn_text.setStyleSheet(
            f"color: {t['review_warn_text']}; font-size: 13px; font-weight: bold; border: none;"
        )
        self.config_label.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        _btn_style = (
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 7px 20px; font-size: 13px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
        self.back_btn.setStyleSheet(_btn_style)
        self.move_up_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 7px 12px; font-size: 13px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
            f"QPushButton:disabled {{ color: {t['text_dim2']}; }}"
        )
        self.move_down_btn.setStyleSheet(self.move_up_btn.styleSheet())
        self.export_queue_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; padding: 7px 14px; font-size: 13px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
            f"QPushButton:disabled {{ color: {t['text_dim2']}; }}"
        )
        self.remove_selected_btn.setStyleSheet(
            f"QPushButton {{ background: {t['red_btn_bg']}; border: 1px solid {t['red_btn_border']}; "
            f"border-radius: 4px; padding: 7px 14px; font-size: 13px; color: {t['text']}; }}"
            f"QPushButton:hover {{ background: {t['red_btn_hover']}; }}"
            f"QPushButton:disabled {{ color: {t['text_dim2']}; background: {t['surface2']}; "
            f"border-color: {t['border']}; }}"
        )
        self.clear_all_btn.setStyleSheet(self.remove_selected_btn.styleSheet())
        self.create_btn.setStyleSheet(
            theme.btn_danger_qss(
                "border-radius: 4px; font-size: 14px; font-weight: bold; padding: 0 28px;"
            )
        )

    def on_enter(self):
        """Refresh display when this screen becomes active."""
        queue = self.app_state.queue
        n = len(queue)

        self._update_summary(n)
        module_info = (
            f"Module field: {self.app_state.module_ref}"
            if self.app_state.module_ref
            else "Module field: not configured (will be skipped)"
        )
        self.config_label.setText(
            f"Organisation: {self.app_state.token_manager.org_url}  |  "
            f"Project: {self.app_state.token_manager.project}  |  {module_info}"
        )
        self._rebuild_tree()
        self.refresh_expiry_state()

    def refresh_expiry_state(self):
        """Sync the Create button and warning banner with the current token state."""
        expired = self.app_state.token_manager.is_expired()
        n = len(self.app_state.queue)
        self.create_btn.setEnabled(n > 0 and not expired)
        if expired:
            self.warn_text.setText(
                "Your Azure DevOps session has expired. Go back and sign in again "
                "before creating test cases."
            )
            self.create_btn.setToolTip("Session has expired — sign in again to continue")
        else:
            self._update_summary(n)
            self.create_btn.setToolTip("")

    # ------------------------------------------------------------------ #
    #  Tree helpers                                                        #
    # ------------------------------------------------------------------ #

    def _display_name_for(self, unique_name: str) -> str:
        """Return the display name for a uniqueName, falling back to the uniqueName itself."""
        members = self.app_state.cached_team_members or []
        for m in members:
            if m.get("uniqueName", "").lower() == unique_name.lower():
                return m.get("displayName") or unique_name
        return unique_name

    def _rebuild_tree(self):
        from app.utils import theme as _theme
        _t = _theme.tokens()
        title_color = QColor(_t["tree_title"])
        meta_color = QColor(_t["tree_meta"])

        self.tree.clear()
        queue = self.app_state.queue
        for tc in queue:
            tc_item = QTreeWidgetItem(self.tree)
            step_summary = f"{len(tc.steps)} step{'s' if len(tc.steps) != 1 else ''}"
            if tc.update_id:
                step_summary += f"  ·  ↻ updates existing #{tc.update_id}"
            tc_item.setText(0, tc.title)
            tc_item.setText(1, step_summary)
            tc_item.setForeground(0, QBrush(title_color))

            meta_item = QTreeWidgetItem(tc_item)
            meta_item.setText(0, "   Metadata")
            parts = [f"Status: {tc.automation_status}"]
            if tc.tags:
                parts.append(f"Tags: {tc.tags}")
            if tc.module_value:
                parts.append(f"Module: {tc.module_value}")
            if tc.created_by:
                parts.append(f"Created By: {self._display_name_for(tc.created_by)}")
            else:
                parts.append("Created By: (current user)")
            meta_item.setText(1, "  |  ".join(parts))
            meta_item.setForeground(0, QBrush(meta_color))
            meta_item.setForeground(1, QBrush(meta_color))

            for i, step in enumerate(tc.steps):
                step_item = QTreeWidgetItem(tc_item)
                step_item.setText(
                    0,
                    f"   Step {i + 1}: {step.action[:60]}{'…' if len(step.action) > 60 else ''}"
                )
                step_item.setText(1, step.expected[:80] if step.expected else "(no expected result)")
                step_item.setForeground(1, QBrush(meta_color))

            tc_item.setExpanded(True)

        n = len(queue)
        expired = self.app_state.token_manager.is_expired()
        self.create_btn.setEnabled(n > 0 and not expired)
        self.export_queue_btn.setEnabled(n > 0)
        self.clear_all_btn.setEnabled(n > 0)
        self._update_action_btns()

    def _update_summary(self, n: int):
        n_updates = sum(1 for tc in self.app_state.queue if tc.update_id)
        n_creates = n - n_updates

        if n_updates and n_creates:
            action = (
                f"create <b>{n_creates} Test Case{'s' if n_creates != 1 else ''}</b> "
                f"and update <b>{n_updates} existing</b>"
            )
        elif n_updates:
            action = f"update <b>{n_updates} existing Test Case{'s' if n_updates != 1 else ''}</b>"
        else:
            action = f"create <b>{n_creates} Test Case{'s' if n_creates != 1 else ''}</b>"

        self.summary_label.setText(
            f"You are about to {action} for PBI <b>#{self.app_state.pbi_id}</b>: "
            f"{self.app_state.pbi_title}"
        )

        if n_updates:
            self.warn_text.setText(
                f"This will create {n_creates} and update {n_updates} Test Case work item(s) "
                "in Azure DevOps. Updates overwrite the existing steps and fields with the "
                "queued values. This action cannot be undone. Review carefully before clicking Create."
            )
        else:
            self.warn_text.setText(
                f"This will create {n} Test Case work item{'s' if n != 1 else ''} in Azure DevOps. "
                "This action cannot be undone. Review carefully before clicking Create."
            )

    # ------------------------------------------------------------------ #
    #  Selection & action button state                                     #
    # ------------------------------------------------------------------ #

    def _selected_root_indices(self) -> list:
        """Queue indices of all selected top-level items (selecting a step
        or metadata row counts as selecting its parent test case)."""
        root = self.tree.invisibleRootItem()
        indices = set()
        for item in self.tree.selectedItems():
            while item.parent():
                item = item.parent()
            for i in range(root.childCount()):
                if root.child(i) is item:
                    indices.add(i)
                    break
        return sorted(indices)

    def _selected_root_index(self) -> int:
        """Queue index when exactly one test case is selected, else -1."""
        indices = self._selected_root_indices()
        return indices[0] if len(indices) == 1 else -1

    def _on_tree_selection_changed(self):
        self._update_action_btns()

    def _update_action_btns(self):
        indices = self._selected_root_indices()
        idx = self._selected_root_index()
        n = len(self.app_state.queue)
        self.remove_selected_btn.setEnabled(bool(indices))
        # Reordering only applies to a single selected case
        self.move_up_btn.setEnabled(idx > 0)
        self.move_down_btn.setEnabled(0 <= idx < n - 1)

    # ------------------------------------------------------------------ #
    #  Remove / reorder                                                    #
    # ------------------------------------------------------------------ #

    def _on_remove(self):
        indices = self._selected_root_indices()
        if not indices:
            return
        for idx in reversed(indices):
            self.app_state.queue.pop(idx)
        n = len(self.app_state.queue)
        self._update_summary(n)
        self._rebuild_tree()
        self.queue_changed.emit()

    def _on_clear_all(self):
        n = len(self.app_state.queue)
        if n == 0:
            return
        reply = QMessageBox.question(
            self, "Clear Queue",
            f"Remove all {n} test case{'s' if n != 1 else ''} from the queue?\n\n"
            "This only clears the queue in this app — nothing in Azure DevOps is affected.",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No,
        )
        if reply != QMessageBox.Yes:
            return
        self.app_state.queue.clear()
        self._update_summary(0)
        self._rebuild_tree()
        self.queue_changed.emit()

    def _on_move_up(self):
        idx = self._selected_root_index()
        if idx <= 0:
            return
        q = self.app_state.queue
        q[idx - 1], q[idx] = q[idx], q[idx - 1]
        self._rebuild_tree()
        root = self.tree.invisibleRootItem()
        if root.childCount() > idx - 1:
            self.tree.setCurrentItem(root.child(idx - 1))
        self.queue_changed.emit()

    def _on_move_down(self):
        idx = self._selected_root_index()
        q = self.app_state.queue
        if idx < 0 or idx >= len(q) - 1:
            return
        q[idx], q[idx + 1] = q[idx + 1], q[idx]
        self._rebuild_tree()
        root = self.tree.invisibleRootItem()
        if root.childCount() > idx + 1:
            self.tree.setCurrentItem(root.child(idx + 1))
        self.queue_changed.emit()

    # ------------------------------------------------------------------ #
    #  Export queue                                                        #
    # ------------------------------------------------------------------ #

    def _on_export_queue(self):
        if not self.app_state.queue:
            return
        path, _ = QFileDialog.getSaveFileName(
            self, "Export Queue",
            str(Path.home() / "Downloads" / "test_cases_queue.xlsx"),
            "Excel Files (*.xlsx)",
        )
        if not path:
            return
        try:
            from app.utils.import_parser import export_queue_to_excel
            export_queue_to_excel(self.app_state.queue, path)
            QMessageBox.information(self, "Exported", f"Queue exported to:\n{path}")
        except Exception as exc:
            QMessageBox.critical(self, "Export Error", f"Could not export queue:\n{exc}")

    # ------------------------------------------------------------------ #
    #  Confirm create                                                      #
    # ------------------------------------------------------------------ #

    def _on_create(self):
        n = len(self.app_state.queue)
        n_updates = sum(1 for tc in self.app_state.queue if tc.update_id)
        n_creates = n - n_updates

        if n_updates and n_creates:
            what = f"Create {n_creates} and update {n_updates} Test Case(s)"
        elif n_updates:
            what = f"Update {n_updates} existing Test Case{'s' if n_updates != 1 else ''}"
        else:
            what = f"Create {n_creates} Test Case{'s' if n_creates != 1 else ''}"

        reply = QMessageBox.question(
            self,
            "Confirm Creation",
            f"{what} for PBI #{self.app_state.pbi_id}?\n\nThis cannot be undone.",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No,
        )
        if reply == QMessageBox.Yes:
            self.confirmed.emit()
