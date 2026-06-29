"""Generate the app icon (resources/icon.png + resources/icon.ico) from the
flask glyph used in the title bar: a white flask on an accent-blue rounded
square. Re-run after changing the design:  python scripts/make_app_icon.py
"""
import os
import sys

from PyQt5.QtWidgets import QApplication
from PyQt5.QtGui import (QImage, QPainter, QColor, QLinearGradient, QBrush,
                         QPainterPath)
from PyQt5.QtCore import Qt, QRectF, QByteArray
from PyQt5.QtSvg import QSvgRenderer

_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SIZE = 256
RADIUS = 56          # rounded-square corner radius (~22% — Win11 squircle-ish)
FLASK_BOX = 140      # the flask fills a centred FLASK_BOX square


def build(size: int = SIZE) -> QImage:
    img = QImage(size, size, QImage.Format_ARGB32)
    img.fill(Qt.transparent)
    p = QPainter(img)
    p.setRenderHint(QPainter.Antialiasing, True)
    p.setRenderHint(QPainter.SmoothPixmapTransform, True)

    scale = size / SIZE
    # Accent-blue rounded-square background (transparent corners).
    path = QPainterPath()
    path.addRoundedRect(QRectF(0, 0, size, size), RADIUS * scale, RADIUS * scale)
    grad = QLinearGradient(0, 0, 0, size)
    grad.setColorAt(0.0, QColor("#1b8ae0"))
    grad.setColorAt(1.0, QColor("#0063b5"))
    p.fillPath(path, QBrush(grad))

    # White flask glyph, centred.
    with open(os.path.join(_ROOT, "resources", "icons", "flask.svg"),
              "r", encoding="utf-8") as fh:
        svg = fh.read().replace("currentColor", "#ffffff")
    renderer = QSvgRenderer(QByteArray(svg.encode("utf-8")))
    box = FLASK_BOX * scale
    off = (size - box) / 2
    renderer.render(p, QRectF(off, off, box, box))
    p.end()
    return img


def main():
    app = QApplication([])  # noqa: F841 (needed for QImage/QSvgRenderer)
    png = os.path.join(_ROOT, "resources", "icon.png")
    ico = os.path.join(_ROOT, "resources", "icon.ico")
    build(SIZE).save(png, "PNG")
    from PIL import Image
    Image.open(png).convert("RGBA").save(
        ico, sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print("wrote", png, "and", ico)


if __name__ == "__main__":
    sys.exit(main())
