"""A steps grid with keyboard-first entry, shared by the Manual Entry and Edit
Test Cases screens.

- **Enter** advances Action → Expected → next row, appending a new row when you
  press Enter past the last row — whether or not the cell is being edited — so a
  whole case's steps can be typed without touching the mouse.
- **Ctrl+V** pastes tab/newline text (e.g. two columns from Excel) as multiple
  steps from the current row down.
- **Ctrl+D** duplicates the current step.

The table stays a dumb grid: it asks its host (via signals) to add / paste /
duplicate rows, because each screen builds rows differently (Edit has a "#"
column + a per-row remove button; Manual Entry numbers via the vertical header).
The Action/Expected column indices differ between screens too, so they're
constructor parameters.
"""

from PyQt5.QtWidgets import (
    QTableWidget, QApplication, QAbstractItemDelegate, QAbstractItemView,
)
from PyQt5.QtCore import Qt, QTimer, pyqtSignal
from PyQt5.QtGui import QKeySequence

from app.utils.steps_paste import parse_step_rows

__all__ = ["StepsTable", "parse_step_rows"]


class StepsTable(QTableWidget):
    """See module docstring. `action_col` / `expected_col` are the column indices
    used for keyboard advancing (Manual Entry: 0/1; Edit: 1/2)."""

    request_add_row = pyqtSignal()            # append one empty step row
    request_paste = pyqtSignal(int, list)     # (start_row, [(action, expected)])
    request_duplicate = pyqtSignal(int)       # duplicate this row index

    def __init__(self, rows, cols, action_col=1, expected_col=2, parent=None):
        super().__init__(rows, cols, parent)
        self._action_col = action_col
        self._expected_col = expected_col

    def closeEditor(self, editor, hint):
        # Enter *while editing* a cell: Qt commits with SubmitModelCache. Advance
        # after the old editor finishes closing (deferred) so opening the next
        # editor doesn't fight the teardown. Tab/Esc/focus-out keep Qt defaults.
        row, col = self.currentRow(), self.currentColumn()
        super().closeEditor(editor, hint)
        if hint == QAbstractItemDelegate.SubmitModelCache:
            QTimer.singleShot(0, lambda: self._advance(row, col))

    def keyPressEvent(self, e):
        if e.matches(QKeySequence.Paste):
            pairs = parse_step_rows(QApplication.clipboard().text())
            if pairs:
                self.request_paste.emit(max(self.currentRow(), 0), pairs)
                return
        if e.key() == Qt.Key_D and e.modifiers() == Qt.ControlModifier:
            if self.currentRow() >= 0:
                self.request_duplicate.emit(self.currentRow())
                return
        # Enter on a *selected but not-editing* cell (the editing case is handled
        # by closeEditor). Without this, pressing Enter on a committed cell did
        # nothing — which read as "Enter doesn't add a step".
        if (e.key() in (Qt.Key_Return, Qt.Key_Enter) and not e.modifiers()
                and self.state() != QAbstractItemView.EditingState
                and self.currentRow() >= 0):
            self._advance(self.currentRow(), self.currentColumn())
            return
        super().keyPressEvent(e)

    def _advance(self, row, col):
        """Move to the next input: Action → Expected on the same row, otherwise
        the next row's Action — appending a new step when we're on the last row."""
        if col == self._action_col:
            self._edit(row, self._expected_col)
            return
        if row >= self.rowCount() - 1:
            self.request_add_row.emit()   # host appends synchronously
        self._edit(min(row + 1, self.rowCount() - 1), self._action_col)

    def _edit(self, row, col):
        if 0 <= row < self.rowCount():
            self.setCurrentCell(row, col)
            item = self.item(row, col)
            if item is not None:
                self.editItem(item)
