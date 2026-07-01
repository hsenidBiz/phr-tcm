"""A QComboBox whose popup items carry checkboxes for multi-select.

The closed field shows a summary of the checked items (or an "all" placeholder
when none are checked); the popup stays open while you toggle items. Emits
`changed` whenever the checked set changes. Themed by `theme.style_combos`
like any other QComboBox — the internal read-only line edit is given a
transparent stylesheet so `theme.style_inputs` leaves it alone (it only styles
inputs with an empty stylesheet) and no box-in-a-box appears.
"""

from PyQt5.QtCore import Qt, QEvent, QRect, QRectF, QSize, pyqtSignal
from PyQt5.QtGui import (
    QColor, QPainter, QPainterPath, QPen, QStandardItem, QStandardItemModel,
)
from PyQt5.QtWidgets import (
    QComboBox, QListView, QStyle, QStyledItemDelegate,
)

from app.utils import theme


class _CheckItemDelegate(QStyledItemDelegate):
    """Draws a high-contrast checkbox + label for each popup item.

    The native QSS check indicator was too faint on the dark themed popup, so we
    paint the box ourselves: a filled, bordered square (theme-aware) that fills
    with the accent colour and shows a white tick when checked. Reads the theme
    live, so it stays correct across light/dark toggles."""

    _BOX = 16   # checkbox side, px
    _PAD = 9    # left inset + gap before the label

    def paint(self, painter, option, index):
        t = theme.tokens()
        painter.save()
        painter.setRenderHint(QPainter.Antialiasing, True)
        rect = option.rect

        if option.state & QStyle.State_MouseOver:
            painter.fillRect(rect, QColor(128, 128, 128, 46))

        box = QRect(rect.left() + self._PAD,
                    rect.center().y() - self._BOX // 2 + 1, self._BOX, self._BOX)
        checked = index.data(Qt.CheckStateRole) == Qt.Checked
        if checked:
            painter.setPen(Qt.NoPen)
            painter.setBrush(QColor(t["accent"]))
            painter.drawRoundedRect(box, 4, 4)
            pen = QPen(QColor("#ffffff"))
            pen.setWidthF(2.0)
            pen.setCapStyle(Qt.RoundCap)
            pen.setJoinStyle(Qt.RoundJoin)
            painter.setPen(pen)
            painter.setBrush(Qt.NoBrush)
            L, T = box.left(), box.top()
            path = QPainterPath()
            path.moveTo(L + 4, T + 8.3)
            path.lineTo(L + 6.8, T + 11.3)
            path.lineTo(L + 12.2, T + 4.7)
            painter.drawPath(path)
        else:
            painter.setBrush(QColor(t["surface"]))
            pen = QPen(QColor(t["text_dim"]))
            pen.setWidthF(1.6)
            painter.setPen(pen)
            painter.drawRoundedRect(
                QRectF(box).adjusted(0.8, 0.8, -0.8, -0.8), 4, 4)

        painter.setPen(QColor(t["text"]))
        text_rect = rect.adjusted(0, 0, -self._PAD, 0)
        text_rect.setLeft(box.right() + self._PAD)
        text = str(index.data(Qt.DisplayRole) or "")
        painter.drawText(
            text_rect, Qt.AlignVCenter | Qt.AlignLeft,
            option.fontMetrics.elidedText(text, Qt.ElideRight, text_rect.width()))
        painter.restore()

    def sizeHint(self, option, index):
        s = super().sizeHint(option, index)
        return QSize(s.width() + self._BOX + self._PAD * 2, max(s.height(), 28))


class CheckableComboBox(QComboBox):

    changed = pyqtSignal()

    def __init__(self, parent=None, all_text="All", max_labels=2):
        super().__init__(parent)
        self._all_text = all_text
        self._max_labels = max_labels

        self._model = QStandardItemModel(self)
        self.setModel(self._model)
        self.setView(QListView())
        # Custom, high-contrast checkbox rendering (native indicator was faint on
        # the dark popup). Reads the theme live, so no refresh needed on toggle.
        self.view().setItemDelegate(_CheckItemDelegate(self.view()))

        # Read-only editable field so we control the shown summary text. NoFocus +
        # transparent styling keeps it looking like a normal (non-editable) combo,
        # and the non-empty stylesheet makes theme.style_inputs skip it.
        self.setEditable(True)
        self.setInsertPolicy(QComboBox.NoInsert)
        le = self.lineEdit()
        le.setReadOnly(True)
        le.setFocusPolicy(Qt.NoFocus)
        le.setStyleSheet("background: transparent; border: none;")
        le.setCursor(Qt.PointingHandCursor)
        le.installEventFilter(self)

        self.view().viewport().installEventFilter(self)
        self._model.dataChanged.connect(lambda *_: self._update_text())
        self._update_text()

    # -- building -------------------------------------------------------- #

    def addCheckItem(self, text, data):
        item = QStandardItem(text)
        item.setData(data, Qt.UserRole)
        # Enabled + checkable, but NOT selectable — clicking toggles the check
        # (handled below) instead of selecting/closing the popup.
        item.setFlags(Qt.ItemIsEnabled | Qt.ItemIsUserCheckable)
        item.setData(Qt.Unchecked, Qt.CheckStateRole)
        self._model.appendRow(item)
        self._update_text()

    # -- state ----------------------------------------------------------- #

    def checked_data(self):
        return [self._model.item(i).data(Qt.UserRole)
                for i in range(self._model.rowCount())
                if self._model.item(i).checkState() == Qt.Checked]

    def set_checked_data(self, values):
        want = set(values)
        for i in range(self._model.rowCount()):
            it = self._model.item(i)
            it.setCheckState(Qt.Checked if it.data(Qt.UserRole) in want else Qt.Unchecked)
        self._update_text()

    def clear_checks(self):
        """Uncheck everything (no `changed` emitted — callers decide when)."""
        for i in range(self._model.rowCount()):
            self._model.item(i).setCheckState(Qt.Unchecked)
        self._update_text()

    # -- interaction ----------------------------------------------------- #

    def eventFilter(self, obj, event):
        if obj is self.lineEdit() and event.type() == QEvent.MouseButtonRelease:
            self.showPopup()   # clicking the field (not just the arrow) opens it
            return True
        if obj is self.view().viewport() and event.type() == QEvent.MouseButtonRelease:
            index = self.view().indexAt(event.pos())
            if index.isValid():
                item = self._model.itemFromIndex(index)
                item.setCheckState(
                    Qt.Unchecked if item.checkState() == Qt.Checked else Qt.Checked)
                self.changed.emit()
            return True        # keep the popup open after a toggle
        return super().eventFilter(obj, event)

    def hidePopup(self):
        # Qt may reset the editable field's text when the popup closes — restore
        # our summary afterwards.
        super().hidePopup()
        self._update_text()

    # -- display --------------------------------------------------------- #

    def _update_text(self):
        labels = [self._model.item(i).text()
                  for i in range(self._model.rowCount())
                  if self._model.item(i).checkState() == Qt.Checked]
        if not labels:
            text = self._all_text
        elif len(labels) <= self._max_labels:
            text = ", ".join(labels)
        else:
            text = f"{len(labels)} selected"
        le = self.lineEdit()
        if le is not None:
            le.setText(text)
            le.setCursorPosition(0)
