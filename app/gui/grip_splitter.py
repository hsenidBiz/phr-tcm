"""A QSplitter whose drag handles show a clearly visible dotted grip.

Qt's default splitter handle is a barely-there texture that all but disappears
in dark mode. `GripSplitter` paints a centred column/row of dots in the theme's
dim colour (brighter on hover), so the draggable dividers are obvious.
"""
from PyQt5.QtWidgets import QSplitter, QSplitterHandle
from PyQt5.QtCore import Qt, QPointF
from PyQt5.QtGui import QPainter, QColor


class _GripHandle(QSplitterHandle):
    def __init__(self, orientation, parent):
        super().__init__(orientation, parent)
        self._hover = False

    def enterEvent(self, event):
        self._hover = True
        self.update()
        super().enterEvent(event)

    def leaveEvent(self, event):
        self._hover = False
        self.update()
        super().leaveEvent(event)

    def paintEvent(self, _event):
        from app.utils import theme
        t = theme.tokens()
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        p.setPen(Qt.NoPen)
        p.setBrush(QColor(t["text_dim"] if self._hover else t["text_dim2"]))
        r = self.rect()
        d, gap, n = 3.0, 6.0, 5  # dot diameter, spacing, count
        if self.orientation() == Qt.Horizontal:
            # vertical divider -> stack the dots vertically
            cx = r.center().x()
            start_y = r.center().y() - (n - 1) * gap / 2.0
            for i in range(n):
                p.drawEllipse(QPointF(cx, start_y + i * gap), d / 2, d / 2)
        else:
            cy = r.center().y()
            start_x = r.center().x() - (n - 1) * gap / 2.0
            for i in range(n):
                p.drawEllipse(QPointF(start_x + i * gap, cy), d / 2, d / 2)


class GripSplitter(QSplitter):
    def __init__(self, orientation, parent=None):
        super().__init__(orientation, parent)
        self.setHandleWidth(8)

    def createHandle(self) -> QSplitterHandle:
        return _GripHandle(self.orientation(), self)
