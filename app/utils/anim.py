from PyQt5.QtWidgets import QWidget, QGraphicsOpacityEffect
from PyQt5.QtCore import Qt, QTimer, QPropertyAnimation, QEasingCurve
from PyQt5.QtGui import QPainter, QColor, QPen


class Spinner(QWidget):
    """Indeterminate arc spinner. Call start()/stop() to control animation."""

    def __init__(self, parent=None, size=24, color="#0078d4", line_width=3):
        super().__init__(parent)
        self._angle = 0
        self._color = QColor(color)
        self._width = line_width
        self.setFixedSize(size, size)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self._timer = QTimer(self)
        self._timer.setInterval(16)
        self._timer.timeout.connect(self._tick)

    def _tick(self):
        self._angle = (self._angle + 6) % 360
        self.update()

    def set_color(self, color: str):
        self._color = QColor(color)
        self.update()

    def start(self):
        self.setVisible(True)
        self._timer.start()

    def stop(self):
        self._timer.stop()
        self.setVisible(False)

    def paintEvent(self, event):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        half = self._width + 1
        r = self.rect().adjusted(half, half, -half, -half)

        track = QColor(self._color)
        track.setAlpha(35)
        p.setPen(QPen(track, self._width, Qt.SolidLine, Qt.RoundCap))
        p.drawEllipse(r)

        p.setPen(QPen(self._color, self._width, Qt.SolidLine, Qt.RoundCap))
        p.drawArc(r, (90 - self._angle) * 16, -100 * 16)


def fade_in(widget: QWidget, duration: int = 180):
    """Fade widget from transparent to fully opaque."""
    effect = QGraphicsOpacityEffect(widget)
    effect.setOpacity(0.0)
    widget.setGraphicsEffect(effect)
    anim = QPropertyAnimation(effect, b"opacity", widget)
    anim.setDuration(duration)
    anim.setStartValue(0.0)
    anim.setEndValue(1.0)
    anim.setEasingCurve(QEasingCurve.OutCubic)
    anim.start(QPropertyAnimation.DeleteWhenStopped)
    return anim
