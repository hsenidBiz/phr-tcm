"""Themed monochrome icons rendered from the bundled SVG set.

Each SVG strokes with `currentColor`; the loader substitutes the active theme's
text colour (or a caller-supplied colour) and renders to a transparent pixmap.
Results are cached per (name, colour, size). Call `clear_cache()` on theme
toggle so icons recolour.
"""
import os
import sys

from PyQt5.QtCore import Qt, QByteArray
from PyQt5.QtGui import QPixmap, QPainter, QIcon, QGuiApplication
from PyQt5.QtSvg import QSvgRenderer


def _target_dpr() -> float:
    """Oversample factor so rasterised SVG icons stay crisp under Windows
    display scaling (125/150/200%). At least 2x; higher on 3x+ monitors."""
    app = QGuiApplication.instance()
    if app is not None:
        scr = app.primaryScreen()
        if scr is not None:
            try:
                return max(float(scr.devicePixelRatio()), 2.0)
            except Exception:
                pass
    return 2.0


def _icons_dir() -> str:
    """resources/icons, resolved for both source runs and the frozen build."""
    base = getattr(sys, "_MEIPASS", None)
    if base:
        return os.path.join(base, "resources", "icons")
    # app/utils/icons.py -> repo root is two levels up.
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    return os.path.join(root, "resources", "icons")


_pix_cache: dict = {}
_icon_cache: dict = {}


def _resolve_color(color):
    if color is not None:
        return color
    from app.utils import theme
    return theme.tokens()["text"]


def pixmap(name: str, color=None, size: int = 18) -> QPixmap:
    """A tinted icon pixmap. Blank (transparent) if the SVG is missing."""
    color = _resolve_color(color)
    key = (name, color, size)
    cached = _pix_cache.get(key)
    if cached is not None:
        return cached
    path = os.path.join(_icons_dir(), f"{name}.svg")
    dpr = _target_dpr()
    phys = max(1, round(size * dpr))
    pm = QPixmap(phys, phys)
    pm.fill(Qt.transparent)
    try:
        with open(path, "r", encoding="utf-8") as fh:
            svg = fh.read().replace("currentColor", color)
        renderer = QSvgRenderer(QByteArray(svg.encode("utf-8")))
        painter = QPainter(pm)
        painter.setRenderHint(QPainter.Antialiasing, True)
        painter.setRenderHint(QPainter.SmoothPixmapTransform, True)
        renderer.render(painter)
        painter.end()
    except Exception:
        pass  # missing/invalid icon -> transparent placeholder
    pm.setDevicePixelRatio(dpr)  # tag hi-res so Qt draws it at `size` logical px
    _pix_cache[key] = pm
    return pm


def icon(name: str, color=None, size: int = 18) -> QIcon:
    """A QIcon for the named glyph, tinted to the theme (or `color`)."""
    color = _resolve_color(color)
    key = (name, color, size)
    cached = _icon_cache.get(key)
    if cached is not None:
        return cached
    ic = QIcon(pixmap(name, color, size))
    _icon_cache[key] = ic
    return ic


def clear_cache():
    """Drop cached icons so they re-render in the new theme colour."""
    _pix_cache.clear()
    _icon_cache.clear()
