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
from PyQt5.QtCore import Qt, QEvent, QPersistentModelIndex
from PyQt5.QtGui import QColor, QBrush
from PyQt5.QtWidgets import QStyledItemDelegate, QStyle, QStyleOptionViewItem

from app.utils import theme

_HOVER_BLEND = 0.16   # how far the row background moves toward the accent colour


class HoverTrackerMixin:
    """Tracks the row under the mouse for an item view and repaints on change,
    so a delegate subclass can tint the whole hovered row's background."""

    def _init_hover(self, view):
        self._hview = view
        # Track the hovered row by its column-0 index identity, not a bare row
        # number: in a QTreeWidget row() is parent-relative, so a bare row would
        # highlight every same-positioned item across sibling branches. The
        # column-0 index encodes the parent, so it uniquely names one row while
        # still spanning all columns of a flat multi-column table.
        self._hover_pidx = QPersistentModelIndex()
        view.setMouseTracking(True)
        vp = view.viewport()
        if vp is not None:
            vp.setMouseTracking(True)
            vp.installEventFilter(self)

    def _set_hover_pidx(self, pidx):
        if pidx != getattr(self, "_hover_pidx", QPersistentModelIndex()):
            self._hover_pidx = pidx
            self._hview.viewport().update()

    def eventFilter(self, obj, ev):
        et = ev.type()
        if et == QEvent.MouseMove:
            idx = self._hview.indexAt(ev.pos())
            self._set_hover_pidx(
                QPersistentModelIndex(idx.sibling(idx.row(), 0))
                if idx.isValid() else QPersistentModelIndex())
        elif et in (QEvent.Leave, QEvent.Hide):
            self._set_hover_pidx(QPersistentModelIndex())
        # Fall through so QStyledItemDelegate's editor-event handling still runs.
        return super().eventFilter(obj, ev)

    def _is_hovered(self, option, index):
        pidx = getattr(self, "_hover_pidx", QPersistentModelIndex())
        return (pidx.isValid()
                and pidx == QPersistentModelIndex(index.sibling(index.row(), 0))
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


class StatusTintDelegate(HoverTrackerMixin, QStyledItemDelegate):
    """Paints each cell's status tint (a translucent BackgroundRole colour) over
    the view background, keeping the theme's normal text colour. Done in a
    delegate because these views live under a styled QTabWidget, and
    QStyleSheetStyle otherwise ignores item background brushes set via
    setBackground(). Also overlays a subtle hover tint on the row under the
    mouse. Used by the Run Tests lists and the Test Suites points table."""

    def __init__(self, view):
        super().__init__(view)
        self._init_hover(view)

    def initStyleOption(self, option, index):
        # Plain (non-status) rows are painted by super().paint() below; tint
        # their background here so the hover never recolours the text.
        super().initStyleOption(option, index)
        if self._is_hovered(option, index):
            self._apply_hover_bg(option)

    def paint(self, painter, option, index):
        brush = index.data(Qt.BackgroundRole)
        color = brush.color() if isinstance(brush, QBrush) else None
        if (color is not None and color.alpha() > 0
                and not (option.state & QStyle.State_Selected)):
            painter.save()
            # Opaque base first, so every row of a given status is the exact same
            # shade (no alternating-row tint, no alpha stacking on repaint).
            painter.fillRect(option.rect, option.palette.base().color())
            painter.fillRect(option.rect, color)   # translucent status tint
            if self._is_hovered(option, index):
                # Tint the background *before* the text, so text colour is kept.
                painter.fillRect(option.rect, self._hover_overlay_color())
            painter.setPen(option.palette.text().color())
            rect = option.rect.adjusted(6, 0, -6, 0)
            text = str(index.data(Qt.DisplayRole) or "")
            painter.drawText(
                rect, Qt.AlignVCenter | Qt.AlignLeft,
                option.fontMetrics.elidedText(text, Qt.ElideRight, rect.width()))
            painter.restore()
        else:
            super().paint(painter, option, index)
