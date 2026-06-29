"""A compact iOS-style on/off toggle switch.

Reusable QAbstractButton: checkable, emits `toggled(bool)`, animates the knob
when switched. Colours are theme-set via `set_colors()` so it adapts to dark/
light mode. Used for the Import screen's opt-in override section.
"""
from PyQt5.QtWidgets import QAbstractButton
from PyQt5.QtCore import Qt, QPropertyAnimation, pyqtProperty, QRectF, QSize, QEasingCurve
from PyQt5.QtGui import QPainter, QColor


class ToggleSwitch(QAbstractButton):
    def __init__(self, parent=None, width=42, height=22):
        super().__init__(parent)
        self.setCheckable(True)
        self.setCursor(Qt.PointingHandCursor)
        self._w, self._h, self._margin = width, height, 2.0
        self.setFixedSize(width, height)
        self._pos = self._margin
        self._on = QColor("#0078d4")
        self._off = QColor("#c4c4c4")
        self._knob = QColor("#ffffff")
        self._anim = QPropertyAnimation(self, b"knobPos", self)
        self._anim.setDuration(130)
        self._anim.setEasingCurve(QEasingCurve.InOutCubic)
        self.toggled.connect(self._on_toggled)

    def _end_pos(self, checked: bool) -> float:
        d = self._h - 2 * self._margin
        return (self._w - self._margin - d) if checked else self._margin

    def _on_toggled(self, checked: bool):
        self._anim.stop()
        self._anim.setStartValue(self._pos)
        self._anim.setEndValue(self._end_pos(checked))
        self._anim.start()

    def set_colors(self, on, off, knob="#ffffff"):
        self._on, self._off, self._knob = QColor(on), QColor(off), QColor(knob)
        self.update()

    def sizeHint(self):
        return QSize(self._w, self._h)

    @pyqtProperty(float)
    def knobPos(self):
        return self._pos

    @knobPos.setter
    def knobPos(self, v):
        self._pos = v
        self.update()

    def showEvent(self, event):
        # Snap the knob to the current state on first show (no animation).
        self._pos = self._end_pos(self.isChecked())
        super().showEvent(event)

    def paintEvent(self, _event):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        p.setPen(Qt.NoPen)
        r = self._h / 2.0
        p.setBrush(self._on if self.isChecked() else self._off)
        p.drawRoundedRect(QRectF(0, 0, self._w, self._h), r, r)
        d = self._h - 2 * self._margin
        p.setBrush(self._knob)
        p.drawEllipse(QRectF(self._pos, self._margin, d, d))
