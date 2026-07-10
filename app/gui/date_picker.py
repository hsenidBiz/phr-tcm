"""A modern clearable date picker (custom calendar popup).

QCalendarWidget can't be styled into a contemporary calendar (rounded day
highlights, an outlined "today", a Today/Clear footer), so this module paints
its own: `ClearableDateEdit` is a read-only field + calendar/clear buttons that
opens `_CalendarPopup` — header with ‹ month year ›, an uppercase SUN–SAT row,
a custom-painted month grid, and Today/Clear links.

Visual language (theme-token driven, light + dark):
- selected day  → filled accent rounded square, white text
- today         → outlined accent rounded square, accent text
- other-month   → dimmed, still clickable
- hover         → soft fill

Public API (kept identical to the old QDateEdit-based widget): ``changed``
signal, ``clear()``, ``set_iso()``, ``is_set()``, ``iso_date()``,
``apply_theme()``.
"""

from PyQt5.QtWidgets import (
    QWidget, QFrame, QVBoxLayout, QHBoxLayout, QGridLayout, QLabel,
    QLineEdit, QPushButton, QToolButton, QApplication,
)
from PyQt5.QtCore import Qt, QDate, QPoint, QRect, QEvent, pyqtSignal
from PyQt5.QtGui import QCursor, QPainter, QColor, QPen, QFont

from app.utils import theme

_CELL_W, _CELL_H = 38, 34
_GRID_ROWS, _GRID_COLS = 6, 7
_WEEKDAYS = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"]  # Sunday-first


class _MonthGrid(QWidget):
    """The 7×6 day grid, custom-painted. Emits ``picked`` on click."""

    picked = pyqtSignal(QDate)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._month = QDate.currentDate()      # any day inside the shown month
        self._selected = None                  # QDate | None
        self._hover = -1                       # cell index under the mouse
        self.setMouseTracking(True)
        self.setFixedSize(_GRID_COLS * _CELL_W, _GRID_ROWS * _CELL_H)

    # -- state ---------------------------------------------------------
    def set_month(self, d: QDate):
        self._month = d
        self.update()

    def set_selected(self, d):
        self._selected = d
        self.update()

    def _first_cell_date(self) -> QDate:
        first = QDate(self._month.year(), self._month.month(), 1)
        return first.addDays(-(first.dayOfWeek() % 7))   # back to Sunday

    def _date_at(self, index: int) -> QDate:
        return self._first_cell_date().addDays(index)

    def _cell_at(self, pos) -> int:
        col, row = pos.x() // _CELL_W, pos.y() // _CELL_H
        if 0 <= col < _GRID_COLS and 0 <= row < _GRID_ROWS:
            return row * _GRID_COLS + col
        return -1

    # -- interaction ----------------------------------------------------
    def mouseMoveEvent(self, e):
        cell = self._cell_at(e.pos())
        if cell != self._hover:
            self._hover = cell
            self.update()

    def leaveEvent(self, e):
        self._hover = -1
        self.update()

    def mousePressEvent(self, e):
        if e.button() == Qt.LeftButton:
            cell = self._cell_at(e.pos())
            if cell >= 0:
                self.picked.emit(self._date_at(cell))

    # -- painting -------------------------------------------------------
    def paintEvent(self, e):
        t = theme.tokens()
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing, True)
        f = QFont(self.font())
        f.setPixelSize(12)
        today = QDate.currentDate()
        for i in range(_GRID_ROWS * _GRID_COLS):
            d = self._date_at(i)
            row, col = divmod(i, _GRID_COLS)
            cell = QRect(col * _CELL_W, row * _CELL_H, _CELL_W, _CELL_H)
            box = cell.adjusted(3, 3, -3, -3)
            selected = (self._selected is not None and d == self._selected)
            if not selected and i == self._hover:
                p.setPen(Qt.NoPen)
                p.setBrush(QColor(t["btn_hover"]))
                p.drawRoundedRect(box, 8, 8)
            if selected:
                p.setPen(Qt.NoPen)
                p.setBrush(QColor(t["accent"]))
                p.drawRoundedRect(box, 8, 8)
                p.setPen(QColor("#ffffff"))
                f.setBold(True)
            elif d == today:
                pen = QPen(QColor(t["accent"]))
                pen.setWidthF(1.5)
                p.setPen(pen)
                p.setBrush(Qt.NoBrush)
                p.drawRoundedRect(box, 8, 8)
                p.setPen(QColor(t["accent"]))
                f.setBold(True)
            else:
                in_month = (d.month() == self._month.month()
                            and d.year() == self._month.year())
                p.setPen(QColor(t["text"] if in_month else t["text_dim2"]))
                f.setBold(False)
            p.setFont(f)
            p.drawText(cell, Qt.AlignCenter, str(d.day()))
            f.setBold(False)
        p.end()


class _CalendarPopup(QFrame):
    """The popup card: ‹ Month Year › header, weekday row, grid, Today/Clear."""

    date_picked = pyqtSignal(QDate)
    cleared = pyqtSignal()

    def __init__(self, parent=None):
        super().__init__(parent, Qt.Popup | Qt.FramelessWindowHint)
        self.setObjectName("calendarPopup")
        self.setAttribute(Qt.WA_StyledBackground, True)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(12, 10, 12, 8)
        lay.setSpacing(4)

        head = QHBoxLayout()
        self._prev = QToolButton()
        self._prev.setText("‹")
        self._next = QToolButton()
        self._next.setText("›")
        for b, step in ((self._prev, -1), (self._next, 1)):
            b.setCursor(QCursor(Qt.PointingHandCursor))
            b.setFixedSize(28, 28)
            b.clicked.connect(lambda _c, s=step: self._step_month(s))
        self._title = QLabel("")
        self._title.setAlignment(Qt.AlignCenter)
        head.addWidget(self._prev)
        head.addWidget(self._title, 1)
        head.addWidget(self._next)
        lay.addLayout(head)

        days = QGridLayout()
        days.setContentsMargins(0, 2, 0, 0)
        days.setSpacing(0)
        self._day_lbls = []
        for c, name in enumerate(_WEEKDAYS):
            lbl = QLabel(name)
            lbl.setAlignment(Qt.AlignCenter)
            lbl.setFixedSize(_CELL_W, 18)
            days.addWidget(lbl, 0, c)
            self._day_lbls.append(lbl)
        lay.addLayout(days)

        self._grid = _MonthGrid(self)
        self._grid.picked.connect(self._on_picked)
        lay.addWidget(self._grid)

        foot = QHBoxLayout()
        foot.setContentsMargins(0, 4, 0, 0)
        self._today_btn = QPushButton("Today")
        self._clear_btn = QPushButton("Clear")
        for b in (self._today_btn, self._clear_btn):
            b.setCursor(QCursor(Qt.PointingHandCursor))
            b.setFlat(True)
        self._today_btn.clicked.connect(
            lambda: self._on_picked(QDate.currentDate()))
        self._clear_btn.clicked.connect(self._on_clear)
        foot.addWidget(self._today_btn)
        foot.addStretch()
        foot.addWidget(self._clear_btn)
        lay.addLayout(foot)

        self._month = QDate.currentDate()
        self.apply_theme()

    # -- behaviour -------------------------------------------------------
    def open_for(self, anchor: QWidget, selected):
        """Show below `anchor`, on the selected date's month (today's if unset)."""
        self._month = selected if selected is not None else QDate.currentDate()
        self._grid.set_selected(selected)
        self._sync()
        pos = anchor.mapToGlobal(QPoint(0, anchor.height() + 4))
        screen = QApplication.screenAt(pos) or QApplication.primaryScreen()
        if screen is not None:                      # keep the card on-screen
            geo = screen.availableGeometry()
            pos.setX(min(pos.x(), geo.right() - self.sizeHint().width() - 4))
            if pos.y() + self.sizeHint().height() > geo.bottom():
                pos.setY(anchor.mapToGlobal(QPoint(0, 0)).y()
                         - self.sizeHint().height() - 4)
        self.move(pos)
        self.show()

    def _sync(self):
        self._grid.set_month(self._month)
        self._title.setText(self._month.toString("MMMM yyyy"))

    def _step_month(self, step: int):
        self._month = self._month.addMonths(step)
        self._sync()

    def _on_picked(self, d: QDate):
        self.hide()
        self.date_picked.emit(d)

    def _on_clear(self):
        self.hide()
        self.cleared.emit()

    # -- theming ---------------------------------------------------------
    def apply_theme(self):
        t = theme.tokens()
        self.setStyleSheet(
            f"#calendarPopup {{ background: {t['surface']}; "
            f"border: 1px solid {t['border']}; border-radius: 10px; }}"
            f"QToolButton {{ border: none; background: transparent; "
            f"color: {t['text_dim']}; font-size: 16px; border-radius: 6px; }}"
            f"QToolButton:hover {{ background: {t['btn_hover']}; color: {t['text']}; }}"
            f"QPushButton {{ border: none; background: transparent; "
            f"color: {t['accent']}; font-size: 12px; font-weight: 600; "
            f"padding: 4px 6px; border-radius: 5px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
        self._title.setStyleSheet(
            f"color: {t['text']}; font-size: 13px; font-weight: 700; "
            f"background: transparent; border: none;")
        for lbl in self._day_lbls:
            lbl.setStyleSheet(
                f"color: {t['text_dim2']}; font-size: 10px; font-weight: 700; "
                f"background: transparent; border: none;")
        self._grid.update()


class ClearableDateEdit(QWidget):
    """A date field with an empty ("Not set") state and the custom calendar
    popup above. Emits ``changed`` on any value change (programmatic included,
    matching the old QDateEdit behaviour the editor's dirty-tracking expects)."""

    changed = pyqtSignal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self._date = None                       # QDate | None
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(4)

        self._display = QLineEdit()
        self._display.setReadOnly(True)
        self._display.setPlaceholderText("Not set")
        self._display.setCursor(QCursor(Qt.PointingHandCursor))
        self._display.installEventFilter(self)
        lay.addWidget(self._display, 1)

        self._cal_btn = QToolButton()
        self._cal_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._cal_btn.setToolTip("Pick a date")
        self._cal_btn.setFixedSize(24, 24)
        self._cal_btn.clicked.connect(self._open_popup)
        lay.addWidget(self._cal_btn)

        self._clear_btn = QToolButton()
        self._clear_btn.setText("✕")
        self._clear_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._clear_btn.setToolTip("Clear date")
        self._clear_btn.setFixedWidth(22)
        self._clear_btn.clicked.connect(self.clear)
        lay.addWidget(self._clear_btn)

        self._popup = _CalendarPopup(self)
        self._popup.date_picked.connect(self._set_date)
        self._popup.cleared.connect(self.clear)
        self.apply_theme()

    def eventFilter(self, obj, event):
        if obj is self._display and event.type() == QEvent.MouseButtonPress:
            self._open_popup()
            return True
        return super().eventFilter(obj, event)

    def _open_popup(self):
        self._popup.open_for(self._display, self._date)

    # -- value API (unchanged from the QDateEdit-based widget) -----------
    def _set_date(self, d):
        self._date = d
        self._display.setText(d.toString("yyyy-MM-dd") if d is not None else "")
        self.changed.emit()

    def clear(self):
        self._set_date(None)

    def set_iso(self, iso: str):
        """Load a value from an ADO date string (``YYYY-MM-DD...``) or clear."""
        d = QDate.fromString((iso or "")[:10], "yyyy-MM-dd")
        self._set_date(d if d.isValid() else None)

    def is_set(self) -> bool:
        return self._date is not None

    def iso_date(self):
        """Return ``YYYY-MM-DD`` when set, else ``None``."""
        return self._date.toString("yyyy-MM-dd") if self._date is not None else None

    def apply_theme(self):
        from app.utils import icons
        t = theme.tokens()
        self._cal_btn.setIcon(icons.icon("calendar", color=t["text_dim"], size=15))
        qss = (
            f"QToolButton {{ border: none; background: transparent; "
            f"color: {t['text_dim']}; border-radius: 4px; }}"
            f"QToolButton:hover {{ background: {t['btn_hover']}; color: {t['text']}; }}")
        self._cal_btn.setStyleSheet(qss)
        self._clear_btn.setStyleSheet(qss)
        self._popup.apply_theme()
