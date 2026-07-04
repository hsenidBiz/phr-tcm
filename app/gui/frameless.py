"""Custom dark window chrome — a Claude Code / VS Code style title bar.

`FramelessMixin` gives any top-level window (QMainWindow, QWidget or QDialog) a
themed title bar (app glyph + title + optional minimise / maximise + close) and a
1px outer border, with native-feeling behaviour and no low-level subclassing:

* dragging the bar uses ``QWindow.startSystemMove()`` (real Windows move + Aero
  snap); double-click toggles maximise (when a maximise button is shown);
* edges resize via ``QWindow.startSystemResize()`` driven by an app-level event
  filter (frameless windows have no native resize border);
* ``nativeEvent`` clamps a maximised window to the monitor work area so it never
  covers the taskbar (multi-monitor aware).

Inherit it *before* the Qt base and call ``self.init_frameless(title, ...)`` at
the end of ``__init__`` (after the window's layout is built). For QMainWindow the
bar is installed via ``setMenuWidget``; for other widgets the existing content is
moved under the bar. Windows-only bits are guarded.
"""
import sys

from PyQt5.QtCore import Qt, QObject, QEvent, QRectF
from PyQt5.QtWidgets import (
    QWidget, QHBoxLayout, QVBoxLayout, QLabel, QPushButton, QApplication,
    QMainWindow, QDialog,
)
from PyQt5.QtGui import QPainter, QColor

from app.utils import theme
from app.utils import icons as _icons

_RESIZE_MARGIN = 8   # px from a window edge that begins a resize
_CLOSE_HOVER = "#c42b1c"
_BTN_HOVER_BG = "rgba(127, 127, 127, 0.20)"   # readable on both light & dark
_CORNER_RADIUS = 8   # matches the Win11 DWM rounded-corner radius


class _WinButton(QPushButton):
    """A flat caption button whose icon recolours on hover."""

    def __init__(self, icon_name, parent=None):
        super().__init__(parent)
        self._icon_name = icon_name
        self._base = None
        self._hover_col = None
        self._hover = False
        self.setFixedSize(46, 34)
        self.setFocusPolicy(Qt.NoFocus)
        self.setCursor(Qt.ArrowCursor)

    def set_palette(self, base_color, hover_color, hover_bg):
        self._base = base_color
        self._hover_col = hover_color
        self.setStyleSheet(
            "QPushButton { border: none; background: transparent; }"
            f"QPushButton:hover {{ background: {hover_bg}; }}"
        )
        self._apply_icon()

    def set_icon_name(self, name):
        if name != self._icon_name:
            self._icon_name = name
            self._apply_icon()

    def _apply_icon(self):
        col = self._hover_col if self._hover else self._base
        if col is not None:
            self.setIcon(_icons.icon(self._icon_name, color=col, size=14))

    def enterEvent(self, e):
        self._hover = True
        self._apply_icon()
        super().enterEvent(e)

    def leaveEvent(self, e):
        self._hover = False
        self._apply_icon()
        super().leaveEvent(e)


class TitleBar(QWidget):
    """The themed replacement for the native title bar."""

    def __init__(self, win, title, show_min=True, show_max=True, extra_widgets=None):
        super().__init__(win)
        self._win = win
        self._can_max = show_max
        self._icon_name = "flask"   # app glyph; swappable per mode (set_app_icon)
        self.setObjectName("framelessTitleBar")
        self.setFixedHeight(36)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(10, 0, 0, 0)
        lay.setSpacing(8)

        self._app_icon = QLabel()
        lay.addWidget(self._app_icon)
        self._title = QLabel(title)
        lay.addWidget(self._title)
        lay.addStretch(1)
        if extra_widgets:
            for wdg in extra_widgets:
                lay.addWidget(wdg)

        self._btn_min = _WinButton("minus") if show_min else None
        self._btn_max = _WinButton("window-maximize") if show_max else None
        self._btn_close = _WinButton("x")
        if self._btn_min is not None:
            self._btn_min.clicked.connect(self._win.showMinimized)
            lay.addWidget(self._btn_min)
        if self._btn_max is not None:
            self._btn_max.clicked.connect(self._toggle_max)
            lay.addWidget(self._btn_max)
        self._btn_close.clicked.connect(self._win.close)
        lay.addWidget(self._btn_close)

        self.refresh_theme()

    def _toggle_max(self):
        if not self._can_max:
            return
        if self._win.isMaximized():
            self._win.showNormal()
        else:
            self._win.showMaximized()

    def set_title(self, text):
        """Update the title-bar caption text."""
        self._title.setText(text)

    def set_app_icon(self, name):
        """Swap the app glyph (e.g. per app mode). Persists across theme toggles
        because refresh_theme re-renders from self._icon_name."""
        self._icon_name = name
        self._app_icon.setPixmap(
            _icons.pixmap(name, color=theme.tokens()["accent"], size=16))

    def sync_max_state(self):
        """Keep the maximise/restore glyph in step with the window state (covers
        Aero snap as well as the button)."""
        if self._btn_max is not None:
            self._btn_max.set_icon_name(
                "window-restore" if self._win.isMaximized() else "window-maximize")

    # Empty parts of the bar drag (and double-click maximises) the window.
    def mousePressEvent(self, e):
        if e.button() == Qt.LeftButton:
            wh = self._win.windowHandle()
            if wh is not None:
                wh.startSystemMove()

    def mouseDoubleClickEvent(self, e):
        if e.button() == Qt.LeftButton:
            self._toggle_max()

    def refresh_theme(self):
        t = theme.tokens()
        self.setStyleSheet(
            f"#framelessTitleBar {{ background: {t['header_bg']}; "
            f"border-bottom: 1px solid {t['border']}; }}"
        )
        self._app_icon.setPixmap(
            _icons.pixmap(self._icon_name, color=t["accent"], size=16))
        self._title.setStyleSheet(
            f"color: {t['text_dim']}; font-size: 12px; font-weight: 500;")
        if self._btn_min is not None:
            self._btn_min.set_palette(t["text_dim"], t["text"], _BTN_HOVER_BG)
        if self._btn_max is not None:
            self._btn_max.set_palette(t["text_dim"], t["text"], _BTN_HOVER_BG)
        self._btn_close.set_palette(t["text_dim"], "#ffffff", _CLOSE_HOVER)
        self.sync_max_state()


class _EdgeResizer(QObject):
    """App-level filter that resizes the active frameless window from its edges."""

    _CURSORS = {
        int(Qt.LeftEdge): Qt.SizeHorCursor,
        int(Qt.RightEdge): Qt.SizeHorCursor,
        int(Qt.TopEdge): Qt.SizeVerCursor,
        int(Qt.BottomEdge): Qt.SizeVerCursor,
        int(Qt.LeftEdge | Qt.TopEdge): Qt.SizeFDiagCursor,
        int(Qt.RightEdge | Qt.BottomEdge): Qt.SizeFDiagCursor,
        int(Qt.RightEdge | Qt.TopEdge): Qt.SizeBDiagCursor,
        int(Qt.LeftEdge | Qt.BottomEdge): Qt.SizeBDiagCursor,
    }

    def __init__(self, win, margin=_RESIZE_MARGIN):
        super().__init__(win)
        self._win = win
        self._m = margin
        self._cursor_set = False

    def _edges(self, gpos):
        w = self._win
        if not w.isVisible() or not w.isActiveWindow() or w.isMaximized():
            return Qt.Edges()
        r = w.frameGeometry()
        m = self._m
        x, y = gpos.x(), gpos.y()
        if not (r.left() - 1 <= x <= r.right() + 1 and r.top() - 1 <= y <= r.bottom() + 1):
            return Qt.Edges()
        e = Qt.Edges()
        if x <= r.left() + m:
            e |= Qt.LeftEdge
        elif x >= r.right() - m:
            e |= Qt.RightEdge
        if y <= r.top() + m:
            e |= Qt.TopEdge
        elif y >= r.bottom() - m:
            e |= Qt.BottomEdge
        return e

    def eventFilter(self, obj, ev):
        et = ev.type()
        if et == QEvent.MouseMove:
            if not (ev.buttons() & Qt.LeftButton):
                self._update_cursor(ev.globalPos())
        elif et == QEvent.MouseButtonPress and ev.button() == Qt.LeftButton:
            edges = self._edges(ev.globalPos())
            if edges:
                wh = self._win.windowHandle()
                if wh is not None and wh.startSystemResize(edges):
                    return True
        return False

    def _update_cursor(self, gpos):
        e = int(self._edges(gpos))
        if e:
            self._win.setCursor(self._CURSORS.get(e, Qt.ArrowCursor))
            self._cursor_set = True
        elif self._cursor_set:
            self._win.unsetCursor()
            self._cursor_set = False


class FramelessMixin:
    """Mix in (before the Qt base) to give a top-level window the dark title bar
    + outer border. Call ``init_frameless`` at the end of ``__init__``."""

    def init_frameless(self, title, *, resizable=True, show_min=True,
                       show_max=True, extra_title_widgets=None):
        self._frameless_resizable = resizable
        self.setWindowFlags(self.windowFlags() | Qt.FramelessWindowHint)
        tb = TitleBar(self, title, show_min=show_min, show_max=show_max,
                      extra_widgets=extra_title_widgets)
        self._title_bar = tb

        if isinstance(self, QMainWindow):
            self.setMenuWidget(tb)                 # spans the top of the window
        else:
            old = self.layout()                    # move existing content below
            content = QWidget()
            if old is not None:
                content.setLayout(old)
            outer = QVBoxLayout(self)
            outer.setContentsMargins(0, 0, 0, 0)
            outer.setSpacing(0)
            outer.addWidget(tb)
            outer.addWidget(content, 1)

        self.setContentsMargins(1, 1, 1, 1)        # reserve the 1px border ring
        if resizable:
            self._edge_resizer = _EdgeResizer(self)
            QApplication.instance().installEventFilter(self._edge_resizer)
        return tb

    def refresh_frameless_theme(self):
        tb = getattr(self, "_title_bar", None)
        if tb is not None:
            tb.refresh_theme()
        self.update()                              # repaint the themed border

    def showEvent(self, event):
        super().showEvent(event)
        _enable_rounded_corners(self)   # per-HWND; re-applied on every show

    def paintEvent(self, event):
        super().paintEvent(event)
        if self.isMaximized() or self.isFullScreen():
            return
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing, True)
        p.setBrush(Qt.NoBrush)
        p.setPen(QColor(theme.tokens()["scroll_handle_hover"]))
        # Rounded to sit flush with the Win11 rounded window corners.
        r = QRectF(0.5, 0.5, self.width() - 1, self.height() - 1)
        p.drawRoundedRect(r, _CORNER_RADIUS, _CORNER_RADIUS)
        p.end()

    def changeEvent(self, event):
        if event.type() == QEvent.WindowStateChange and getattr(self, "_title_bar", None):
            self._title_bar.sync_max_state()
            if getattr(self, "_frameless_resizable", True):
                m = 0 if self.isMaximized() else 1   # no border ring when maximised
                self.setContentsMargins(m, m, m, m)
        super().changeEvent(event)

    def nativeEvent(self, eventType, message):
        # Keep a maximised frameless window inside the monitor work area so it
        # never covers the taskbar (multi-monitor aware).
        if (getattr(self, "_frameless_resizable", True)
                and sys.platform.startswith("win")
                and eventType == "windows_generic_MSG"
                and handle_getminmaxinfo(message, self)):
            return True, 0
        return super().nativeEvent(eventType, message)


class FramelessDialog(FramelessMixin, QDialog):
    """A QDialog with the dark title bar. Add content to ``content_layout`` then
    call ``finalize_frameless()`` before ``exec_()``."""

    def __init__(self, parent, title, *, resizable=True):
        super().__init__(parent)
        self._fd_title = title
        self._fd_resizable = resizable
        self.content_layout = QVBoxLayout(self)
        self.content_layout.setContentsMargins(10, 10, 10, 10)

    def finalize_frameless(self):
        self.init_frameless(self._fd_title, resizable=self._fd_resizable,
                            show_min=False, show_max=False)


def _enable_rounded_corners(win):
    """Ask the Windows 11 DWM to round the window's corners (with the proper
    drop shadow + anti-aliasing). No-op on Windows 10 / other platforms. Applied
    on every show because the attribute is per-HWND and resets when the native
    window is recreated (e.g. a setWindowFlags / pin toggle)."""
    if not sys.platform.startswith("win"):
        return
    try:
        import ctypes
        from ctypes import wintypes
        hwnd = int(win.winId())
        if not hwnd:
            return
        DWMWA_WINDOW_CORNER_PREFERENCE = 33
        DWMWCP_ROUND = 2
        pref = ctypes.c_int(DWMWCP_ROUND)
        dwmapi = ctypes.windll.dwmapi
        dwmapi.DwmSetWindowAttribute.argtypes = [
            wintypes.HWND, wintypes.DWORD, ctypes.POINTER(ctypes.c_int), wintypes.DWORD]
        dwmapi.DwmSetWindowAttribute(
            wintypes.HWND(hwnd), DWMWA_WINDOW_CORNER_PREFERENCE,
            ctypes.byref(pref), ctypes.sizeof(pref))
    except Exception:
        pass


def _set_min_track_size(mmi, win):
    """Set ``ptMinTrackSize`` (physical px) from the window's logical minimum
    size, so a native frameless resize can't shrink below it. Our handler
    replaces Qt's own WM_GETMINMAXINFO handling, so we must set this ourselves —
    otherwise Windows lets the window shrink to a tiny default."""
    if win is None:
        return
    try:
        dpr = float(win.devicePixelRatioF())
    except Exception:
        dpr = 1.0
    min_w, min_h = win.minimumWidth(), win.minimumHeight()
    if min_w > 0:
        mmi.ptMinTrackSize.x = int(round(min_w * dpr))
    if min_h > 0:
        mmi.ptMinTrackSize.y = int(round(min_h * dpr))


def handle_getminmaxinfo(message, win=None) -> bool:
    """From a window's ``nativeEvent``: clamp a maximised frameless window to the
    monitor work area so it doesn't cover the taskbar, and enforce ``win``'s
    minimum size during a native (frameless) drag-resize. Returns True if it
    handled a WM_GETMINMAXINFO message (caller should then return (True, 0))."""
    if not sys.platform.startswith("win"):
        return False
    try:
        import ctypes
        from ctypes import wintypes

        msg = wintypes.MSG.from_address(int(message))
        if msg.message != 0x0024:        # WM_GETMINMAXINFO
            return False

        class _POINT(ctypes.Structure):
            _fields_ = [("x", wintypes.LONG), ("y", wintypes.LONG)]

        class _MINMAXINFO(ctypes.Structure):
            _fields_ = [("ptReserved", _POINT), ("ptMaxSize", _POINT),
                        ("ptMaxPosition", _POINT), ("ptMinTrackSize", _POINT),
                        ("ptMaxTrackSize", _POINT)]

        class _RECT(ctypes.Structure):
            _fields_ = [("left", wintypes.LONG), ("top", wintypes.LONG),
                        ("right", wintypes.LONG), ("bottom", wintypes.LONG)]

        class _MONITORINFO(ctypes.Structure):
            _fields_ = [("cbSize", wintypes.DWORD), ("rcMonitor", _RECT),
                        ("rcWork", _RECT), ("dwFlags", wintypes.DWORD)]

        user32 = ctypes.windll.user32
        user32.MonitorFromWindow.restype = wintypes.HANDLE
        user32.MonitorFromWindow.argtypes = [wintypes.HWND, wintypes.DWORD]
        hmon = user32.MonitorFromWindow(wintypes.HWND(msg.hWnd), 2)  # NEAREST
        if not hmon:
            return False
        mi = _MONITORINFO()
        mi.cbSize = ctypes.sizeof(_MONITORINFO)
        if not user32.GetMonitorInfoW(hmon, ctypes.byref(mi)):
            return False
        work, mon = mi.rcWork, mi.rcMonitor
        mmi = _MINMAXINFO.from_address(msg.lParam)
        mmi.ptMaxPosition.x = work.left - mon.left
        mmi.ptMaxPosition.y = work.top - mon.top
        mmi.ptMaxSize.x = work.right - work.left
        mmi.ptMaxSize.y = work.bottom - work.top
        mmi.ptMaxTrackSize.x = work.right - work.left
        mmi.ptMaxTrackSize.y = work.bottom - work.top
        _set_min_track_size(mmi, win)
        return True
    except Exception:
        return False
