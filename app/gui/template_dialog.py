from PyQt5.QtWidgets import (
    QDialog, QVBoxLayout, QHBoxLayout, QLabel, QListWidget,
    QPushButton, QInputDialog, QMessageBox
)
from PyQt5.QtCore import pyqtSignal
from PyQt5.QtGui import QCursor
from PyQt5.QtCore import Qt

from app.utils.settings import load_settings, save_settings


class TemplateDialog(QDialog):
    """Dialog for saving, applying, renaming, and deleting test case templates."""

    applied = pyqtSignal(dict)  # emits template dict when user clicks Apply

    def __init__(self, current_form_state: dict | None = None, parent=None):
        super().__init__(parent)
        self._current_form_state = current_form_state
        self.setWindowTitle("Test Case Templates")
        self.setMinimumWidth(440)
        self.setMinimumHeight(340)
        self._build_ui()
        self._load_templates()

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setSpacing(10)

        layout.addWidget(QLabel("Saved templates  (double-click to apply):"))

        self._list = QListWidget()
        self._list.setAlternatingRowColors(True)
        self._list.itemSelectionChanged.connect(self._on_selection_changed)
        self._list.itemDoubleClicked.connect(self._on_apply)
        layout.addWidget(self._list)

        btn_row = QHBoxLayout()

        self._save_btn = QPushButton("💾  Save Current Form")
        self._save_btn.setEnabled(self._current_form_state is not None)
        self._save_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._save_btn.clicked.connect(self._on_save_current)
        btn_row.addWidget(self._save_btn)

        self._rename_btn = QPushButton("✎  Rename")
        self._rename_btn.setEnabled(False)
        self._rename_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._rename_btn.clicked.connect(self._on_rename)
        btn_row.addWidget(self._rename_btn)

        self._delete_btn = QPushButton("✕  Delete")
        self._delete_btn.setEnabled(False)
        self._delete_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._delete_btn.clicked.connect(self._on_delete)
        btn_row.addWidget(self._delete_btn)

        btn_row.addStretch()

        self._apply_btn = QPushButton("Apply →")
        self._apply_btn.setEnabled(False)
        self._apply_btn.setFixedHeight(34)
        self._apply_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._apply_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; padding: 0 16px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self._apply_btn.clicked.connect(self._on_apply)
        btn_row.addWidget(self._apply_btn)

        layout.addLayout(btn_row)

    def _load_templates(self):
        self._templates = list(load_settings().get("templates", []))
        self._list.clear()
        for t in self._templates:
            self._list.addItem(t.get("name", "(unnamed)"))

    def _on_selection_changed(self):
        has_sel = bool(self._list.selectedItems())
        self._apply_btn.setEnabled(has_sel)
        self._rename_btn.setEnabled(has_sel)
        self._delete_btn.setEnabled(has_sel)

    def _on_apply(self):
        row = self._list.currentRow()
        if row < 0 or row >= len(self._templates):
            return
        self.applied.emit(dict(self._templates[row]))
        self.accept()

    def _on_save_current(self):
        if self._current_form_state is None:
            return
        name, ok = QInputDialog.getText(self, "Save Template", "Template name:")
        if not ok or not name.strip():
            return
        name = name.strip()
        for t in self._templates:
            if t.get("name", "").lower() == name.lower():
                QMessageBox.warning(
                    self, "Duplicate Name",
                    f"A template named '{name}' already exists. Choose a different name."
                )
                return
        tmpl = dict(self._current_form_state)
        tmpl["name"] = name
        self._templates.append(tmpl)
        save_settings({"templates": self._templates})
        self._list.addItem(name)

    def _on_rename(self):
        row = self._list.currentRow()
        if row < 0 or row >= len(self._templates):
            return
        old_name = self._templates[row].get("name", "")
        name, ok = QInputDialog.getText(self, "Rename Template", "New name:", text=old_name)
        if not ok or not name.strip():
            return
        name = name.strip()
        self._templates[row]["name"] = name
        save_settings({"templates": self._templates})
        self._list.item(row).setText(name)

    def _on_delete(self):
        row = self._list.currentRow()
        if row < 0 or row >= len(self._templates):
            return
        name = self._templates[row].get("name", "")
        reply = QMessageBox.question(
            self, "Delete Template",
            f"Delete template '{name}'?",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No,
        )
        if reply == QMessageBox.Yes:
            del self._templates[row]
            save_settings({"templates": self._templates})
            self._list.takeItem(row)
            self._on_selection_changed()
