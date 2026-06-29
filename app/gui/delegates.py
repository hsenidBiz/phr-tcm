"""Shared item-view delegates.

Hover highlighting lives in a delegate rather than a QSS ``::item:hover`` rule:
several of these views colour individual rows via ``setBackground()`` /
``setForeground()``, and adding *any* ``::item`` stylesheet rule switches the
view to ``QStyleSheetStyle``, which silently drops those item colours.

The hover changes only the row's **background** — never the text colour. We do
that by tinting the style option's ``backgroundBrush`` in ``initStyleOption``
(so the default paint fills the tinted background and then draws the text in its
own, untouched colour), not by overlaying a translucent fill on top of the
already-painted text.

The hovered row is tracked from the viewport's mouse-move (not the ``entered``
signal) so the highlight spans every column of a multi-column table and clears
correctly when the cursor moves into empty space below the last row.
"""
from PyQt5.QtCore import Qt, QEvent
from PyQt5.QtGui import QColor, QBrush
from PyQt5.QtWidgets import QStyledItemDelegate, QStyle, QStyleOptionViewItem

from app.utils import theme

_HOVER_BLEND = 0.16   # how far the row background moves toward the accent colour


class HoverTrackerMixin:
    """Tracks the row under the mouse for an item view and repaints on change,
    so a delegate subclass can tint the whole hovered row's background."""

    def _init_hover(self, view):
        self._hview = view
        self._hover_row = -1
        view.setMouseTracking(True)
        vp = view.viewport()
        if vp is not None:
            vp.setMouseTracking(True)
            vp.installEventFilter(self)

    def _set_hover_row(self, row):
        if row != getattr(self, "_hover_row", -1):
            self._hover_row = row
            self._hview.viewport().update()

    def eventFilter(self, obj, ev):
        et = ev.type()
        if et == QEvent.MouseMove:
            idx = self._hview.indexAt(ev.pos())
            self._set_hover_row(idx.row() if idx.isValid() else -1)
        elif et in (QEvent.Leave, QEvent.Hide):
            self._set_hover_row(-1)
        # Fall through so QStyledItemDelegate's editor-event handling still runs.
        return super().eventFilter(obj, ev)

    def _is_hovered(self, option, index):
        return (getattr(self, "_hover_row", -1) == index.row()
                and not (option.state & QStyle.State_Selected))

    @staticmethod
    def _row_base_color(option):
        """The row's effective background colour (item brush, else alternating
        base, else base)."""
        bb = option.backgroundBrush
        if bb is not None and bb.style() != Qt.NoBrush:
            return bb.color()
        if option.features & QStyleOptionViewItem.Alternate:
            return option.palette.alternateBase().color()
        return option.palette.base().color()

    def _apply_hover_bg(self, option):
        """Tint ONLY the background: blend the row's effective background toward
        the accent and set it as the bg brush. Text colour is left untouched."""
        base = self._row_base_color(option)
        acc = QColor(theme.tokens()["accent"])
        a = _HOVER_BLEND
        option.backgroundBrush = QBrush(QColor(
            round(base.red()   * (1 - a) + acc.red()   * a),
            round(base.green() * (1 - a) + acc.green() * a),
            round(base.blue()  * (1 - a) + acc.blue()  * a)))

    def _hover_overlay_color(self):
        """Translucent accent for delegates that fill their own background
        (painted *before* the text, so the text colour stays the same)."""
        c = QColor(theme.tokens()["accent"])
        c.setAlpha(40)
        return c


class HoverDelegate(HoverTrackerMixin, QStyledItemDelegate):
    """Default item rendering (keeps per-item background/foreground and
    alternating rows) plus a subtle background-only hover tint."""

    def __init__(self, view):
        super().__init__(view)
        self._init_hover(view)

    def initStyleOption(self, option, index):
        super().initStyleOption(option, index)
        if self._is_hovered(option, index):
            self._apply_hover_bg(option)


def apply_hover(view):
    """Give a plain item view a subtle background-only row hover highlight."""
    view.setItemDelegate(HoverDelegate(view))
    return view
