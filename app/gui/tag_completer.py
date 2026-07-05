"""A tags QLineEdit with autocomplete over the project's existing tags.

Drop-in replacement for a plain QLineEdit on the semicolon-separated Tags fields:
``text()`` / ``setText()`` / ``textChanged`` behave exactly the same, so callers'
save and dirty-tracking logic is unchanged. The only addition is a popup that
suggests known tags for the segment currently being typed (the text after the
last ``;``).
"""

from PyQt5.QtWidgets import QLineEdit, QCompleter
from PyQt5.QtCore import Qt, QStringListModel


class TagLineEdit(QLineEdit):
    """Semicolon-separated tags input with per-segment autocomplete."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self._model = QStringListModel([], self)
        self._completer = QCompleter(self._model, self)
        self._completer.setCaseSensitivity(Qt.CaseInsensitive)
        self._completer.setFilterMode(Qt.MatchContains)
        self._completer.setCompletionMode(QCompleter.PopupCompletion)
        self._completer.setWidget(self)
        self._completer.activated.connect(self._insert_completion)
        # textEdited (not textChanged) so programmatic setText() never pops up.
        self.textEdited.connect(self._maybe_complete)

    def set_known_tags(self, names):
        """Supply the suggestion pool (project tag names)."""
        self._model.setStringList(sorted(set(names)))

    # -- the tag segment being typed = text after the last ';' up to cursor -- #
    def _segment_bounds(self):
        cur = self.cursorPosition()
        start = self.text().rfind(";", 0, cur) + 1   # 0 when there's no ';'
        return start, cur

    def _current_segment(self):
        start, cur = self._segment_bounds()
        return self.text()[start:cur].strip()

    def _maybe_complete(self, _text):
        if self._current_segment():
            self._completer.setCompletionPrefix(self._current_segment())
            self._completer.complete()
        else:
            self._completer.popup().hide()

    def _insert_completion(self, completion):
        start, cur = self._segment_bounds()
        text = self.text()
        seg = text[start:cur]
        lead = seg[:len(seg) - len(seg.lstrip())]   # keep a leading space, if any
        new = text[:start] + lead + completion + text[cur:]
        self.setText(new)
        self.setCursorPosition(start + len(lead) + len(completion))
