"""My Work — a fast board + inline editor for the work items assigned to you.

POC Phase 1: read-only board — one WIQL round-trip (assigned to me, most
recently changed first) + one batched field GET, grouped into To Do / Doing /
Done columns by each state's process *category*.

POC Phase 2: full inline editing. Selecting a card opens it in a right-hand
editor (title, state, assignee, priority, iteration/area, tags, description,
remaining/completed work). Saves send ONLY the changed fields and merge the
server's response back locally — no refetch. Changing State applies instantly
(the one-click transition ADO's web UI makes you work for). Comments load
lazily per card; a minimal quick-create files a Bug/Task with just a title.

Toggled from anywhere with Ctrl+Shift+M (see MainWindow._toggle_mywork).
"""

import html
import time
import webbrowser
from urllib.parse import quote

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QGridLayout, QLabel, QPushButton,
    QListWidget, QListWidgetItem, QLineEdit, QComboBox, QPlainTextEdit, QTextEdit,
    QMessageBox, QScrollArea, QFrame, QStyledItemDelegate, QStyle, QMenu,
)
from PyQt5.QtCore import Qt, QThreadPool, QTimer, QRect, QSize, pyqtSignal
from PyQt5.QtGui import (
    QCursor, QColor, QPalette, QPainter, QPen, QFont, QPixmap, QPainterPath,
)

from app.gui.delegates import HoverTrackerMixin

from app.utils.worker import Worker
from app.utils import theme
from app.utils.anim import Spinner
from app.utils.richtext import html_to_markdown, markdown_to_html
from app.utils.xml_builder import html_to_text
from app.models.work_item import WorkItem, WORK_ITEM_FIELDS, COLUMNS
from app.gui.checkable_combo import CheckableComboBox
from app.gui.tag_completer import TagLineEdit
from app.gui import frameless

# Test artifacts are managed in the app's normal mode — keep the board about
# actual work (stories, bugs, tasks, …).
_EXCLUDED_TYPES = ("Test Case", "Test Suite", "Test Plan",
                   "Shared Steps", "Shared Parameter")

_MAX_ITEMS = 500   # WIQL $top cap — personal boards stay far below this

# Rich-text fields are heavy, so the board fetch skips them; the editor fetches
# them lazily for the selected card only.
_DESC_FIELDS = ["System.Description", "Microsoft.VSTS.TCM.ReproSteps"]

# The mode-switch pill on the Work Manager header — enlarged, fully rounded
# (radius = half the fixed height). Kept in a constant so the build and
# refresh_theme paths stay in sync.
_SWITCH_PILL_H = 34
_SWITCH_PILL_EXTRA = "border-radius: 17px; padding: 0 24px; font-size: 14px;"

# Deterministic avatar colours for comment authors. No profile image is fetched;
# initials on a coloured disc read as "a person" the way ADO's web comments do.
_AVATAR_COLORS = ["#e15b64", "#d99e2b", "#2aa5e0", "#9a74d8", "#e0873c",
                  "#4caf7d", "#c65b9a", "#5b8ad9"]


def _initials(name: str) -> str:
    parts = [p for p in (name or "").replace(",", " ").split() if p]
    if not parts:
        return "?"
    if len(parts) == 1:
        return parts[0][0].upper()
    return (parts[0][0] + parts[-1][0]).upper()


def _avatar_pixmap(name: str, size: int) -> QPixmap:
    """A round initials avatar; the colour is a stable function of the name."""
    pm = QPixmap(size, size)
    pm.fill(Qt.transparent)
    p = QPainter(pm)
    p.setRenderHint(QPainter.Antialiasing, True)
    idx = sum(ord(ch) for ch in (name or "?")) % len(_AVATAR_COLORS)
    p.setBrush(QColor(_AVATAR_COLORS[idx]))
    p.setPen(Qt.NoPen)
    p.drawEllipse(0, 0, size, size)
    f = QFont()
    f.setPixelSize(int(size * 0.42))
    f.setBold(True)
    p.setFont(f)
    p.setPen(QColor("#ffffff"))
    p.drawText(pm.rect(), Qt.AlignCenter, _initials(name))
    p.end()
    return pm


def _round_pixmap(src: QPixmap, size: int) -> QPixmap:
    """Centre-crop `src` into a circular `size`x`size` avatar."""
    scaled = src.scaled(size, size, Qt.KeepAspectRatioByExpanding,
                        Qt.SmoothTransformation)
    out = QPixmap(size, size)
    out.fill(Qt.transparent)
    p = QPainter(out)
    p.setRenderHint(QPainter.Antialiasing, True)
    path = QPainterPath()
    path.addEllipse(0, 0, size, size)
    p.setClipPath(path)
    x = (scaled.width() - size) // 2
    y = (scaled.height() - size) // 2
    p.drawPixmap(-x, -y, scaled)
    p.end()
    return out


def _friendly_when(iso: str) -> str:
    """Human 'commented ...' suffix: today / yesterday / weekday / '19 Jun'."""
    from datetime import datetime, timezone
    if not iso:
        return ""
    try:
        dt = datetime.fromisoformat(iso.strip().replace("Z", "+00:00"))
    except Exception:
        return iso[:10]
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    now = datetime.now(timezone.utc)
    days = (now.date() - dt.astimezone(timezone.utc).date()).days
    if days <= 0:
        return "today"
    if days == 1:
        return "yesterday"
    if days < 7:
        return dt.strftime("%A")
    if dt.year == now.year:
        return f"{dt.day} {dt.strftime('%b')}"
    return f"{dt.day} {dt.strftime('%b %Y')}"


class _ResizableTextEdit(QTextEdit):
    """A rich-text QTextEdit the user can resize vertically by dragging a grip in
    the bottom-right corner, like an HTML <textarea>. The grip is painted as three
    small ticks; dragging it changes the widget's height. Rich text (Markdown, set
    via setMarkdown) renders in place so ADO descriptions keep their formatting."""

    _GRIP = 16

    def __init__(self, parent=None, min_height=90, start_height=112):
        super().__init__(parent)
        self._min_h = min_height
        self._drag_from = None
        self._drag_h0 = 0
        self.setFixedHeight(start_height)
        self.viewport().setMouseTracking(True)

    def _on_grip(self, pos) -> bool:
        vp = self.viewport()
        return (pos.x() >= vp.width() - self._GRIP
                and pos.y() >= vp.height() - self._GRIP)

    def mousePressEvent(self, e):
        if e.button() == Qt.LeftButton and self._on_grip(e.pos()):
            self._drag_from = e.globalPos().y()
            self._drag_h0 = self.height()
            e.accept()
            return
        super().mousePressEvent(e)

    def mouseMoveEvent(self, e):
        if self._drag_from is not None:
            dy = e.globalPos().y() - self._drag_from
            self.setFixedHeight(max(self._min_h, self._drag_h0 + dy))
            e.accept()
            return
        self.viewport().setCursor(
            Qt.SizeVerCursor if self._on_grip(e.pos()) else Qt.IBeamCursor)
        super().mouseMoveEvent(e)

    def mouseReleaseEvent(self, e):
        if self._drag_from is not None:
            self._drag_from = None
            e.accept()
            return
        super().mouseReleaseEvent(e)

    def paintEvent(self, e):
        super().paintEvent(e)
        vp = self.viewport()
        p = QPainter(vp)
        pen = QPen(QColor(theme.tokens()["text_dim2"]))
        pen.setWidth(1)
        p.setPen(pen)
        w, h = vp.width(), vp.height()
        for off in (3, 7, 11):
            p.drawLine(w - off, h - 3, w - 3, h - off)
        p.end()


class _CommentEntry(QWidget):
    """One comment rendered like a comment: round avatar + bold author +
    'commented <when>' + the body text. Re-themeable in place."""

    def __init__(self, comment: dict, parent=None):
        super().__init__(parent)
        self._name = (comment.get("created_by") or "").strip() or "Unknown"
        self._when = _friendly_when(comment.get("created_date", ""))
        body = html_to_text(comment.get("text", "") or "").strip()

        row = QHBoxLayout(self)
        row.setContentsMargins(0, 8, 0, 8)
        row.setSpacing(8)
        self._avatar = QLabel()
        self._avatar.setFixedSize(30, 30)
        self._avatar.setPixmap(_avatar_pixmap(self._name, 30))
        self._avatar.setStyleSheet("background: transparent; border: none;")
        row.addWidget(self._avatar, 0, Qt.AlignTop)

        col = QVBoxLayout()
        col.setContentsMargins(0, 0, 0, 0)
        col.setSpacing(2)
        self._head = QLabel()
        self._head.setTextFormat(Qt.RichText)
        self._body = QLabel(body or "—")
        self._body.setWordWrap(True)
        self._body.setTextInteractionFlags(Qt.TextSelectableByMouse)
        col.addWidget(self._head)
        col.addWidget(self._body)
        row.addLayout(col, 1)
        self.apply_theme()

    def apply_theme(self):
        t = theme.tokens()
        self._head.setText(
            f"<span style='color:{t['text']}'><b>{html.escape(self._name)}</b>"
            f"</span> <span style='color:{t['text_dim2']}'>commented "
            f"<u>{html.escape(self._when)}</u></span>")
        self._head.setStyleSheet(
            "background: transparent; border: none; font-size: 12px;")
        self._body.setStyleSheet(
            f"color:{t['text']}; background: transparent; border: none; "
            f"font-size: 12px;")

    def set_avatar(self, pixmap: QPixmap):
        """Swap the initials disc for the author's real profile photo."""
        self._avatar.setPixmap(pixmap)

_COMPLETED = "Microsoft.VSTS.Scheduling.CompletedWork"
_REMAINING = "Microsoft.VSTS.Scheduling.RemainingWork"
_ORIGINAL = "Microsoft.VSTS.Scheduling.OriginalEstimate"
_ACTIVITY = "Microsoft.VSTS.Common.Activity"

# Dropping a card on a column moves the item to the FIRST state of these
# categories (in order) defined for its type — process-discovered, never
# hardcoded state names. Dropping on Done prefers a truly-closed (Completed)
# state, falling back to Resolved for processes that only reach Resolved.
_COLUMN_CATEGORIES = {
    "To Do": ("Proposed",),
    "Doing": ("InProgress",),
    "Done": ("Completed", "Resolved"),
}


# Work-item-type → (icon name, badge colour). Colours echo ADO's own type
# accents but nudged for contrast on both themes. Unknown types fall back to a
# neutral document icon.
_TYPE_META = {
    "Bug": ("bug", "#e15b64"),
    "Task": ("square-check", "#d99e2b"),
    "Product Backlog Item": ("bookmark", "#2aa5e0"),
    "User Story": ("bookmark", "#2aa5e0"),
    "Issue": ("bookmark", "#2aa5e0"),
    "Feature": ("star", "#9a74d8"),
    "Epic": ("flag", "#e0873c"),
}
_DEFAULT_TYPE_ICON = "file-text"

# Extra item-data roles (per card) the delegate reads.
_STATE_HEX_ROLE = Qt.UserRole + 1     # the state's ADO colour, hex without '#'


def _type_meta(wtype: str):
    return _TYPE_META.get(wtype, (_DEFAULT_TYPE_ICON, None))


def _priority_hex(priority, tokens):
    return {1: "#e15b64", 2: "#d99e2b"}.get(priority, tokens["text_dim2"])


def _wrap_lines(fm, text, width, max_lines):
    """Greedy word-wrap `text` into at most `max_lines`, eliding the last line
    with '…' when it overflows. Used by the card delegate to draw titles."""
    words = text.split()
    lines, cur, i = [], "", 0
    while i < len(words):
        trial = words[i] if not cur else cur + " " + words[i]
        if not cur or fm.horizontalAdvance(trial) <= width:
            cur = trial
            i += 1
        else:
            lines.append(cur)
            cur = ""
            if len(lines) == max_lines:
                break
    if cur and len(lines) < max_lines:
        lines.append(cur)
        i = len(words)
    if i < len(words) and lines:          # words left over → elide the last line
        last = lines[-1]
        while last and fm.horizontalAdvance(last + "…") > width:
            last = last[:-1].rstrip()
        lines[-1] = (last + "…") if last else "…"
    return lines


class _CardDelegate(HoverTrackerMixin, QStyledItemDelegate):
    """Paints each work item as a card — a type icon + colour, its id, a wrapped
    title, a state chip and a priority badge — instead of a line of text. The
    WorkItem is on Qt.UserRole; the state colour on _STATE_HEX_ROLE."""

    _H = 90   # card slot height (a few px are the gap between cards)

    def __init__(self, view):
        super().__init__(view)
        self._init_hover(view)

    def sizeHint(self, option, index):
        return QSize(option.rect.width(), self._H)

    def paint(self, painter, option, index):
        wi = index.data(Qt.UserRole)
        if wi is None:
            return super().paint(painter, option, index)
        t = theme.tokens()
        painter.save()
        painter.setRenderHint(QPainter.Antialiasing, True)
        painter.setClipRect(option.rect)

        card = QRect(option.rect).adjusted(3, 3, -3, -4)
        selected = bool(option.state & QStyle.State_Selected)
        icon_name, type_hex = _type_meta(wi.type)
        type_col = QColor(type_hex) if type_hex else QColor(t["text_dim"])

        # Card surface + a coloured left accent bar by type.
        painter.setPen(Qt.NoPen)
        painter.setBrush(QColor(t["surface2"]))
        painter.drawRoundedRect(card, 7, 7)
        painter.save()
        clip = QRect(card)
        painter.setClipRect(clip.intersected(option.rect))
        bar = QColor(type_col)
        painter.setBrush(bar)
        painter.drawRoundedRect(QRect(card.left(), card.top(), 7, card.height()), 3, 3)
        painter.fillRect(QRect(card.left() + 3, card.top(), 4, card.height()), bar)
        painter.restore()

        if self._is_hovered(option, index) and not selected:
            painter.setBrush(self._hover_overlay_color())
            painter.setPen(Qt.NoPen)
            painter.drawRoundedRect(card, 7, 7)
        if selected:
            pen = QPen(QColor(t["accent"]))
            pen.setWidth(2)
            painter.setPen(pen)
            painter.setBrush(Qt.NoBrush)
            painter.drawRoundedRect(card.adjusted(1, 1, -1, -1), 7, 7)

        left = card.left() + 12
        right = card.right() - 10
        small = QFont(option.font)
        small.setPixelSize(11)
        title_font = QFont(option.font)
        title_font.setPixelSize(13)

        # Row 1: type icon · #id  ……  priority badge
        from app.utils import icons
        pm = icons.pixmap(icon_name, color=type_hex or t["text_dim"], size=16)
        painter.drawPixmap(left, card.top() + 9, 16, 16, pm)
        painter.setFont(small)
        painter.setPen(QColor(t["text_dim"]))
        painter.drawText(QRect(left + 22, card.top() + 8, right - (left + 22), 18),
                         Qt.AlignVCenter | Qt.AlignLeft, f"#{wi.id}")
        if wi.priority is not None:
            ptext = f"P{wi.priority}"
            pw = painter.fontMetrics().horizontalAdvance(ptext) + 2
            painter.setPen(QColor(_priority_hex(wi.priority, t)))
            painter.drawText(QRect(right - pw, card.top() + 8, pw, 18),
                             Qt.AlignVCenter | Qt.AlignRight, ptext)

        # Row 2: title, up to two wrapped lines.
        painter.setFont(title_font)
        fm = painter.fontMetrics()
        title_top = card.top() + 30
        lines = _wrap_lines(fm, wi.title, right - left, 2)
        painter.setPen(QColor(t["text"]))
        y = title_top
        for line in lines:
            painter.drawText(QRect(left, y, right - left, fm.lineSpacing()),
                             Qt.AlignLeft | Qt.AlignVCenter, line)
            y += fm.lineSpacing()

        # Row 3: state dot + name.
        state_hex = index.data(_STATE_HEX_ROLE)
        dot = QColor(f"#{state_hex}") if state_hex else QColor(t["text_dim2"])
        cy = card.bottom() - 13
        painter.setBrush(dot)
        painter.setPen(Qt.NoPen)
        painter.drawEllipse(QRect(left, cy, 8, 8))
        painter.setFont(small)
        painter.setPen(QColor(t["text_dim"]))
        painter.drawText(QRect(left + 13, cy - 4, right - (left + 13), 16),
                         Qt.AlignVCenter | Qt.AlignLeft,
                         painter.fontMetrics().elidedText(
                             wi.state, Qt.ElideRight, right - (left + 13)))
        painter.restore()


def _fmt_elapsed(seconds: float) -> str:
    sec = int(seconds)
    h, rem = divmod(sec, 3600)
    m, s = divmod(rem, 60)
    return f"{h}:{m:02d}:{s:02d}" if h else f"{m:02d}:{s:02d}"


def _log_focus_time(client, wid: int, hours: float) -> dict:
    """Worker-thread: add a focus session to Completed Work (and take it off
    Remaining Work when the item tracks one). Fresh read-modify-write so a value
    changed elsewhere is never clobbered with a stale local copy."""
    cur = client.get_work_items([wid], [_COMPLETED, _REMAINING])
    f = cur[0] if cur else {}
    fields = {_COMPLETED: round(float(f.get(_COMPLETED) or 0.0) + hours, 2)}
    rem = f.get(_REMAINING)
    if rem is not None:
        fields[_REMAINING] = max(0.0, round(float(rem) - hours, 2))
    client.update_work_item_fields(wid, fields)
    return {"fields": fields}


class _ColumnList(QListWidget):
    """A board column: draggable cards, accepts drops from OTHER columns.

    The drop is never performed by Qt — it is reported as Copy (so the source
    list doesn't delete the dragged row) and the screen transitions the item's
    State instead, then rebuilds both columns. Same philosophy as the Review
    tree's reorder: the widgets never mutate themselves out of sync."""

    drag_started = pyqtSignal(object)   # the WorkItem being dragged
    card_dropped = pyqtSignal()         # something was dropped onto this column

    def __init__(self):
        super().__init__()
        self.setDragEnabled(True)
        self.setAcceptDrops(True)
        self.setDropIndicatorShown(False)

    def startDrag(self, actions):
        it = self.currentItem()
        if it is not None:
            self.drag_started.emit(it.data(Qt.UserRole))
        super().startDrag(actions)

    def _from_other_column(self, e) -> bool:
        return isinstance(e.source(), _ColumnList) and e.source() is not self

    def dragEnterEvent(self, e):
        if self._from_other_column(e):
            e.acceptProposedAction()
        else:
            e.ignore()   # no reordering inside a column — order comes from Sort

    def dragMoveEvent(self, e):
        if self._from_other_column(e):
            e.acceptProposedAction()
        else:
            e.ignore()

    def dropEvent(self, e):
        # Report the drop as a COPY so the source never removes the dragged row;
        # the screen PATCHes System.State and rebuilds the board itself.
        e.setDropAction(Qt.CopyAction)
        e.accept()
        self.card_dropped.emit()


def _wiql_str(value: str) -> str:
    """A WIQL string literal with quotes escaped."""
    return "'" + str(value).replace("'", "''") + "'"


def _strip_path_root(path: str) -> str:
    """Display form of an Area/Iteration path with the leading project-root
    segment dropped, e.g. 'HRM\\Gamma Guardians\\Sprint 9' → 'Gamma
    Guardians\\Sprint 9'. That first segment is the project name and repeats on
    every entry, so it's noise in the dropdowns. The bare root (just the project
    name, nothing after it) is left unchanged so its item isn't blank. Only the
    shown text is shortened — callers keep the full path as the item's data."""
    parts = (path or "").split("\\", 1)
    return parts[1] if len(parts) == 2 and parts[1] else path


def _team_area_clause(field_ref: str, values: list) -> str:
    """A WIQL clause scoping to a team's area(s). Each value uses UNDER when it
    includes children on a tree field (Area/Iteration path), otherwise '='."""
    field = f"[{field_ref}]"
    tree = field_ref.endswith("AreaPath") or field_ref.endswith("IterationPath")
    parts = []
    for v in values:
        val = v.get("value") if isinstance(v, dict) else v
        if not val:
            continue
        under = tree and (v.get("includeChildren") if isinstance(v, dict) else False)
        parts.append(f"{field} {'UNDER' if under else '='} {_wiql_str(val)}")
    return " OR ".join(parts)


def _fetch_work(client, scope: dict) -> dict:
    """Worker-thread board fetch. `scope` is {"mode": "me"} (items assigned to
    me) or {"mode": "team", "team": name} (the team's board — everything under
    the team's area path(s), whoever it's assigned to). Read only."""
    excluded = ", ".join(f"'{t}'" for t in _EXCLUDED_TYPES)
    where = ["[System.TeamProject] = @project",
             f"[System.WorkItemType] NOT IN ({excluded})"]
    if scope.get("mode") == "team":
        tfv = client.get_team_field_values(scope["team"])
        vals = tfv.get("values") or ([{"value": tfv["default"], "includeChildren": True}]
                                     if tfv.get("default") else [])
        clause = _team_area_clause(tfv.get("field_ref") or "System.AreaPath", vals)
        if clause:
            where.append(f"({clause})")
    else:
        where.append("[System.AssignedTo] = @Me")
    wiql = ("SELECT [System.Id] FROM workitems WHERE "
            + " AND ".join(where)
            + " ORDER BY [System.ChangedDate] DESC")
    ids = client.query_work_items(wiql, top=_MAX_ITEMS)
    fields = client.get_work_items(ids, WORK_ITEM_FIELDS) if ids else []
    states = {}
    activities = {}
    for f in fields:
        wtype = f.get("System.WorkItemType", "")
        if wtype and wtype not in states:
            try:
                states[wtype] = {s["name"]: (s["category"], s["color"])
                                 for s in client.get_work_item_states(wtype)}
            except Exception:
                states[wtype] = {}   # unknown process — column falls back to heuristic
            try:
                activities[wtype] = client.get_field_allowed_values(wtype, _ACTIVITY)
            except Exception:
                activities[wtype] = []   # type has no Activity field
    # Project Area/Iteration trees for the editor dropdowns (client-cached, so
    # this is one network round-trip each per project regardless of board loads).
    try:
        areas = client.get_classification_paths("areas")
    except Exception:
        areas = []
    try:
        iterations = client.get_classification_paths("iterations")
    except Exception:
        iterations = []
    return {"fields": fields, "states": states, "activities": activities,
            "areas": areas, "iterations": iterations, "count": len(ids)}


def _fetch_teams(client) -> list:
    """Worker-thread: the project's team names (for the scope selector)."""
    return [t.get("name", "") for t in client.get_teams() if t.get("name")]


def _quick_create(client, kind: str, title: str, description: str, assign_to: str) -> dict:
    """Worker-thread create: resolve the real type (Bug adapts to the process
    via detect_bug_type), POST it, and return what the board needs for a local
    insert (id + type + the process's initial state)."""
    if kind == "Bug":
        info = client.detect_bug_type()
        wtype, desc_field = info["type"], info["repro_field"]
    else:
        wtype, desc_field = "Task", "System.Description"
    fields = {"System.Title": title}
    if description:
        fields[desc_field] = description
    if assign_to:
        fields["System.AssignedTo"] = assign_to
    res = client.create_work_item(wtype, fields)
    initial, states = "New", {}
    try:
        raw = client.get_work_item_states(wtype)
        states = {s["name"]: (s["category"], s["color"]) for s in raw}
        initial = next((s["name"] for s in raw if s["category"] == "Proposed"),
                       raw[0]["name"] if raw else "New")
    except Exception:
        pass
    return {"id": res.get("id"), "type": wtype, "state": initial,
            "title": title, "assign_to": assign_to, "states": states,
            "description": description, "desc_field": desc_field}


class _NewItemDialog(frameless.FramelessDialog):
    """Minimal quick-create: kind + title (+ optional description). No forms of
    forms — the fields ADO's dialog makes mandatory are defaulted server-side."""

    def __init__(self, parent):
        super().__init__(parent, "New work item", resizable=False)
        lay = self.content_layout
        lay.setSpacing(8)
        lay.setContentsMargins(16, 12, 16, 14)

        row = QHBoxLayout()
        row.addWidget(QLabel("Type"))
        self.kind_combo = QComboBox()
        self.kind_combo.addItem("Bug", "Bug")
        self.kind_combo.addItem("Task", "Task")
        self.kind_combo.setCursor(QCursor(Qt.PointingHandCursor))
        row.addWidget(self.kind_combo, 1)
        lay.addLayout(row)

        self.title_edit = QLineEdit()
        self.title_edit.setPlaceholderText("Title (required)")
        lay.addWidget(self.title_edit)

        self.desc_edit = QPlainTextEdit()
        self.desc_edit.setPlaceholderText("Description / repro steps (optional)")
        self.desc_edit.setFixedHeight(96)
        lay.addWidget(self.desc_edit)

        btns = QHBoxLayout()
        btns.addStretch()
        cancel = QPushButton("Cancel")
        cancel.setStyleSheet(theme.btn_neutral_qss())
        cancel.setCursor(QCursor(Qt.PointingHandCursor))
        cancel.clicked.connect(self.reject)
        btns.addWidget(cancel)
        create = QPushButton("Create")
        create.setStyleSheet(theme.btn_primary_qss("padding: 6px 18px;"))
        create.setCursor(QCursor(Qt.PointingHandCursor))
        create.clicked.connect(self._on_create)
        btns.addWidget(create)
        lay.addLayout(btns)

        theme.style_combos(self)
        theme.style_inputs(self)
        self.setMinimumWidth(420)
        self.finalize_frameless()
        self.title_edit.setFocus()

    def _on_create(self):
        if not self.title_edit.text().strip():
            QMessageBox.information(self, "Title required",
                                    "Give the work item a title first.")
            return
        self.accept()

    def values(self) -> tuple:
        return (self.kind_combo.currentData(),
                self.title_edit.text().strip(),
                self.desc_edit.toPlainText().strip())


class MyWorkScreen(QWidget):
    """Kanban-style view + inline editor for the work items assigned to the
    signed-in user in the current project."""

    switch_to_test_cases = pyqtSignal()   # header button -> MainWindow toggles mode

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._items: list[WorkItem] = []
        self._states_by_type: dict = {}     # {type: {state: (category, color)}}
        self._activities_by_type: dict = {}  # {type: [allowed Activity values]}
        self._areas: list = []               # project Area paths (dropdown)
        self._iterations: list = []          # project Iteration paths (dropdown)
        self._loaded_key = None             # (org_url, project) the items belong to
        self._loading = False
        self._current: WorkItem | None = None
        self._dirty = False
        self._suspend = False               # True while programmatically filling
        self._selecting = False             # True while syncing list selections
        self._saving = False
        self._desc_field = "System.Description"   # field the shown text came from
        self._desc_original = ""
        self._comments_cache: dict = {}     # {work_item_id: [comment dicts]}
        self._avatar_cache: dict = {}       # {avatar_url: rounded QPixmap}
        self._avatar_pending: dict = {}     # {avatar_url: [_CommentEntry awaiting]}
        self._drag_wi = None                # WorkItem mid-drag between columns
        # Focus timer: {"id", "title", "accum" (sec), "run_started" (monotonic
        # while running, None while paused)}. Persisted so it survives restarts.
        self._focus: dict | None = None
        self._focus_ticks = 0
        self._focus_restored = False
        self._focus_tick = QTimer(self)
        self._focus_tick.setInterval(1000)
        self._focus_tick.timeout.connect(self._on_focus_tick)
        # Board scope: {"mode": "me"} or {"mode": "team", "team": name}.
        self._scope = {"mode": "me"}
        self._teams_key = None               # (org, project) the team list is for
        # Locally hidden work-item ids (never sent to ADO); loaded per org.
        self._hidden: set = set()
        self._build_ui()

    # ------------------------------------------------------------------ #
    #  Lifecycle / loading                                                #
    # ------------------------------------------------------------------ #

    def on_enter(self):
        """Called when the mode is toggled on — (re)load if the org/project
        changed since the last load (first entry included)."""
        tm = self.app_state.token_manager
        key = (tm.org_url, tm.project)
        if key != self._teams_key:
            # New org/project — a team from the previous project no longer
            # applies, so fall back to personal scope before (re)loading.
            self._scope = {"mode": "me"}
            self._reset_scope_combo()
        if key != self._loaded_key and not self._loading:
            self.refresh()
        self._load_teams()
        self._refresh_members()
        # A focus timer left running when the app closed comes back PAUSED with
        # its recorded time (offline hours are never counted) — resume or stop.
        if not self._focus_restored:
            self._focus_restored = True
            from app.utils.settings import load_focus_timer
            saved = load_focus_timer()
            if saved and saved.get("id") and self._focus is None:
                self._focus = {"id": saved["id"], "title": saved.get("title", ""),
                               "accum": float(saved.get("accum", 0.0)),
                               "run_started": None}
                self._update_focus_ui()
                # _on_loaded clears the status when the (async) board load
                # finishes — keep the restore note alive across it.
                self._focus_note = (
                    f"Focus timer on #{saved['id']} restored (paused) — resume it "
                    "or stop it to log the time.")
                self._status_lbl.setText(self._focus_note)

    def refresh(self):
        if self._loading:
            return
        tm = self.app_state.token_manager
        key = (tm.org_url, tm.project)
        scope = dict(self._scope)
        self._loading = True
        self._refresh_btn.setEnabled(False)
        self._status_lbl.setText(
            f"Loading the {scope['team']} board…" if scope.get("mode") == "team"
            else "Loading your work items…")
        self._spinner.start()
        worker = Worker(_fetch_work, self.app_state.client, scope)
        worker.signals.result.connect(lambda res, k=key: self._on_loaded(k, res))
        worker.signals.error.connect(self._on_error)
        QThreadPool.globalInstance().start(worker)

    # ---- team scope --------------------------------------------------------

    def _load_teams(self):
        """Populate the scope selector with the project's teams (once per
        org/project). Failure just leaves 'My work' as the only option."""
        tm = self.app_state.token_manager
        key = (tm.org_url, tm.project)
        if key == self._teams_key:
            return
        self._teams_key = key
        worker = Worker(_fetch_teams, self.app_state.client)
        worker.signals.result.connect(lambda names, k=key: self._on_teams(k, names))
        worker.signals.error.connect(lambda _exc: None)
        QThreadPool.globalInstance().start(worker)

    def _reset_scope_combo(self):
        """Collapse the scope selector back to just 'My work' (used on a project
        switch, before the new team list arrives)."""
        self._scope_combo.blockSignals(True)
        self._scope_combo.clear()
        self._scope_combo.addItem("My work", None)
        self._scope_combo.setCurrentIndex(0)
        self._scope_combo.blockSignals(False)

    def _on_teams(self, key, names):
        if key != self._teams_key:
            return
        cur = self._scope_combo.currentData()
        self._scope_combo.blockSignals(True)
        self._scope_combo.clear()
        self._scope_combo.addItem("My work", None)
        for name in sorted(names):
            self._scope_combo.addItem(f"Team: {name}", name)
        idx = self._scope_combo.findData(cur)
        self._scope_combo.setCurrentIndex(idx if idx >= 0 else 0)
        self._scope_combo.blockSignals(False)

    def _on_scope_changed(self, _idx):
        team = self._scope_combo.currentData()
        new_scope = {"mode": "team", "team": team} if team else {"mode": "me"}
        if new_scope == self._scope:
            return
        self._scope = new_scope
        self._loaded_key = None   # force reload for the new scope
        self.refresh()

    def _on_loaded(self, key, result):
        self._loading = False
        self._spinner.stop()
        self._refresh_btn.setEnabled(True)
        # Preserve a just-restored focus-timer note; otherwise clear "Loading…".
        self._status_lbl.setText(getattr(self, "_focus_note", None) or "")
        self._focus_note = None
        self._loaded_key = key
        self._items = [WorkItem(f) for f in result.get("fields", [])]
        self._states_by_type = result.get("states", {})
        self._activities_by_type = result.get("activities", {})
        self._areas = result.get("areas", [])
        self._iterations = result.get("iterations", [])
        self._comments_cache.clear()
        from app.utils.settings import load_hidden_work_items
        self._hidden = load_hidden_work_items(key[0])   # key = (org_url, project)
        self._show_placeholder()
        self._repopulate_type_filter()
        self._rebuild()
        self._update_hidden_btn()
        from app.gui.helpers import fetch_project_tags
        fetch_project_tags(self.app_state, self._tags_edit.set_known_tags)

    def _on_error(self, exc):
        self._loading = False
        self._spinner.stop()
        self._refresh_btn.setEnabled(True)
        self._status_lbl.setText(f"Could not load work items: {exc}")

    # ------------------------------------------------------------------ #
    #  UI                                                                 #
    # ------------------------------------------------------------------ #

    def _build_ui(self):
        from app.utils import icons
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 14, 16, 12)
        layout.setSpacing(8)

        # Header: title · scope · count · spinner · focus · new · refresh
        hdr = QHBoxLayout()
        self._title_lbl = QLabel("<b>Work Manager</b>")
        self._title_lbl.setStyleSheet("font-size: 16px;")
        hdr.addWidget(self._title_lbl)
        hdr.addSpacing(10)
        self._scope_combo = QComboBox()
        self._scope_combo.setToolTip("Show your items, or a whole team's board")
        self._scope_combo.addItem("My work", None)
        self._scope_combo.setMinimumWidth(150)
        self._scope_combo.setCursor(QCursor(Qt.PointingHandCursor))
        self._scope_combo.currentIndexChanged.connect(self._on_scope_changed)
        hdr.addWidget(self._scope_combo)
        hdr.addSpacing(10)
        self._count_lbl = QLabel("")
        hdr.addWidget(self._count_lbl)
        self._spinner = Spinner(size=16, line_width=2)
        self._spinner.stop()   # hidden until a load starts
        hdr.addWidget(self._spinner)
        hdr.addSpacing(12)
        # Focus timer pill: "Now: #123 · 12:34" + pause/resume + stop-and-log
        self._focus_frame = QFrame()
        self._focus_frame.setObjectName("focusFrame")
        ff = QHBoxLayout(self._focus_frame)
        ff.setContentsMargins(10, 3, 4, 3)
        ff.setSpacing(6)
        self._focus_lbl = QLabel("")
        ff.addWidget(self._focus_lbl)
        self._focus_pause_btn = QPushButton()
        self._focus_pause_btn.setFixedSize(24, 24)
        self._focus_pause_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._focus_pause_btn.clicked.connect(self._toggle_focus_pause)
        ff.addWidget(self._focus_pause_btn)
        self._focus_stop_btn = QPushButton()
        self._focus_stop_btn.setFixedSize(24, 24)
        self._focus_stop_btn.setToolTip(
            "Stop the focus timer and log the time to Completed Work")
        self._focus_stop_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._focus_stop_btn.clicked.connect(lambda: self._stop_focus(log=True))
        ff.addWidget(self._focus_stop_btn)
        self._focus_frame.setVisible(False)
        hdr.addWidget(self._focus_frame)
        hdr.addStretch()
        self._switch_btn = QPushButton("Test Case Manager")
        self._switch_btn.setIcon(icons.icon("switch", color=theme.tokens()["accent"], size=16))
        self._switch_btn.setFixedHeight(_SWITCH_PILL_H)
        self._switch_btn.setStyleSheet(theme.btn_pill_accent_qss(_SWITCH_PILL_EXTRA))
        self._switch_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._switch_btn.setToolTip("Switch back to Test Case Manager (Ctrl+Shift+M)")
        self._switch_btn.clicked.connect(lambda: self.switch_to_test_cases.emit())
        hdr.addWidget(self._switch_btn, 0, Qt.AlignVCenter)
        hdr.addSpacing(8)
        self._new_btn = QPushButton("New item")
        self._new_btn.setIcon(icons.icon("plus", size=15))
        self._new_btn.setStyleSheet(theme.btn_neutral_qss())
        self._new_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._new_btn.clicked.connect(self._on_new_item)
        hdr.addWidget(self._new_btn)
        # "Hidden (N)" — only shown when items are hidden; opens the unhide menu.
        self._hidden_btn = QPushButton("Hidden")
        self._hidden_btn.setIcon(icons.icon("minus", size=15))
        self._hidden_btn.setStyleSheet(theme.btn_neutral_qss())
        self._hidden_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._hidden_btn.setToolTip("Show and unhide items you've hidden")
        self._hidden_btn.clicked.connect(self._show_hidden_menu)
        self._hidden_btn.setVisible(False)
        hdr.addWidget(self._hidden_btn)
        self._refresh_btn = QPushButton("Refresh")
        self._refresh_btn.setIcon(icons.icon("refresh", size=15))
        self._refresh_btn.setStyleSheet(theme.btn_neutral_qss())
        self._refresh_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._refresh_btn.clicked.connect(self.refresh)
        hdr.addWidget(self._refresh_btn)
        layout.addLayout(hdr)

        self._status_lbl = QLabel("")
        layout.addWidget(self._status_lbl)

        # Filter bar: search · type multi-select · sort · clear
        bar = QHBoxLayout()
        bar.setSpacing(6)
        self._search = QLineEdit()
        self._search.setPlaceholderText("Search by ID or title…")
        self._search.setClearButtonEnabled(True)
        self._search.textChanged.connect(lambda _t: self._on_filter_changed())
        bar.addWidget(self._search, 2)
        self._type_combo = CheckableComboBox(all_text="All types")
        self._type_combo.setToolTip("Filter by work item type — tick one or more")
        self._type_combo.changed.connect(self._on_filter_changed)
        bar.addWidget(self._type_combo, 1)
        self._sort_combo = QComboBox()
        self._sort_combo.setToolTip("Sort the cards inside each column")
        for label, data in (
            ("Recently changed", None), ("Priority", "priority"),
            ("Title A–Z", "az"), ("Title Z–A", "za"),
        ):
            self._sort_combo.addItem(label, data)
        self._sort_combo.currentIndexChanged.connect(lambda _i: self._rebuild())
        bar.addWidget(self._sort_combo, 1)
        self._clear_btn = QPushButton()
        self._clear_btn.setIcon(icons.icon("x", size=13))
        self._clear_btn.setToolTip("Clear all filters")
        self._clear_btn.setFixedSize(28, 28)
        self._clear_btn.setStyleSheet(theme.btn_ghost_qss("padding: 4px;"))
        self._clear_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._clear_btn.clicked.connect(self._clear_filters)
        self._clear_btn.setVisible(False)
        bar.addWidget(self._clear_btn)
        for cb in (self._type_combo, self._sort_combo):
            cb.setMinimumWidth(120)
            cb.setCursor(QCursor(Qt.PointingHandCursor))
        layout.addLayout(bar)

        # Board (left) | detail editor (right)
        from app.gui.grip_splitter import GripSplitter
        split = GripSplitter(Qt.Horizontal)

        board_widget = QWidget()
        board = QHBoxLayout(board_widget)
        board.setContentsMargins(0, 0, 0, 0)
        board.setSpacing(10)
        self._col_labels = {}
        self._col_lists = {}
        for col in COLUMNS:
            col_v = QVBoxLayout()
            col_v.setSpacing(4)
            lbl = QLabel(f"<b>{col}</b>")
            self._col_labels[col] = lbl
            col_v.addWidget(lbl)
            lst = _ColumnList()
            lst.setAlternatingRowColors(False)
            lst.setSelectionMode(QListWidget.SingleSelection)
            lst.setUniformItemSizes(True)
            lst.setSpacing(0)
            lst.setItemDelegate(_CardDelegate(lst))   # paint items as cards
            lst.itemDoubleClicked.connect(self._open_in_browser)
            lst.itemSelectionChanged.connect(
                lambda _=None, s=lst: self._on_card_selected(s))
            lst.drag_started.connect(self._on_drag_started)
            lst.card_dropped.connect(lambda c=col: self._on_card_dropped(c))
            lst.setContextMenuPolicy(Qt.CustomContextMenu)
            lst.customContextMenuRequested.connect(
                lambda pos, li=lst: self._on_card_context_menu(li, pos))
            self._col_lists[col] = lst
            col_v.addWidget(lst, 1)
            board.addLayout(col_v, 1)
        split.addWidget(board_widget)

        split.addWidget(self._build_detail_panel())
        split.setStretchFactor(0, 3)
        split.setStretchFactor(1, 2)
        split.setSizes([620, 360])
        self._splitter = split
        layout.addWidget(split, 1)

        self._empty_lbl = QLabel("")
        self._empty_lbl.setAlignment(Qt.AlignCenter)
        self._empty_lbl.setVisible(False)
        layout.addWidget(self._empty_lbl)

        self.refresh_theme()

    def _build_detail_panel(self) -> QWidget:
        """The right-hand editor: a scrollable form + comments, hidden behind a
        placeholder until a card is selected."""
        from app.utils import icons
        panel = QWidget()
        outer = QVBoxLayout(panel)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(0)

        self._no_sel_lbl = QLabel("Select a card to view and edit it")
        self._no_sel_lbl.setAlignment(Qt.AlignCenter)
        outer.addWidget(self._no_sel_lbl, 1)

        self._detail_scroll = QScrollArea()
        self._detail_scroll.setWidgetResizable(True)
        self._detail_scroll.setFrameShape(QFrame.NoFrame)
        self._detail_scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        form_host = QWidget()
        # Palette-based background (never a stylesheet on an ancestor of the
        # comment list — the QStyleSheetStyle gotcha).
        for w in (self._detail_scroll.viewport(), form_host):
            w.setBackgroundRole(QPalette.Window)
            w.setAutoFillBackground(True)
        v = QVBoxLayout(form_host)
        v.setContentsMargins(10, 2, 4, 8)
        v.setSpacing(6)

        # Header: "#123 · Bug" + open in browser
        head = QHBoxLayout()
        self._detail_id_lbl = QLabel("")
        head.addWidget(self._detail_id_lbl)
        head.addStretch()
        self._focus_btn = QPushButton("Focus")
        self._focus_btn.setIcon(icons.icon("play", size=14))
        self._focus_btn.setToolTip(
            "Start a focus timer on this item — stopping it logs the time to "
            "Completed Work")
        self._focus_btn.setStyleSheet(theme.btn_ghost_qss())
        self._focus_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._focus_btn.clicked.connect(self._start_focus_current)
        head.addWidget(self._focus_btn)
        self._open_btn = QPushButton("Open in browser")
        self._open_btn.setIcon(icons.icon("external-link", size=14))
        self._open_btn.setStyleSheet(theme.btn_ghost_qss())
        self._open_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._open_btn.clicked.connect(self._open_current_in_browser)
        head.addWidget(self._open_btn)
        v.addLayout(head)

        self._title_edit = QLineEdit()
        self._title_edit.setPlaceholderText("Title")
        self._title_edit.textChanged.connect(self._on_edit)
        v.addWidget(self._title_edit)

        grid = QGridLayout()
        grid.setHorizontalSpacing(8)
        grid.setVerticalSpacing(6)
        self._form_labels = []

        def _lbl(text):
            lab = QLabel(text)
            self._form_labels.append(lab)
            return lab

        # State applies INSTANTLY on change (see _on_state_changed);
        # everything else batches into Save.
        grid.addWidget(_lbl("State"), 0, 0)
        self._state_combo = QComboBox()
        self._state_combo.setToolTip("Changing the state saves immediately")
        self._state_combo.currentIndexChanged.connect(self._on_state_changed)
        grid.addWidget(self._state_combo, 0, 1)
        grid.addWidget(_lbl("Priority"), 0, 2)
        self._priority_combo = QComboBox()
        self._priority_combo.addItem("—", None)
        for p in (1, 2, 3, 4):
            self._priority_combo.addItem(str(p), p)
        self._priority_combo.currentIndexChanged.connect(self._on_edit)
        grid.addWidget(self._priority_combo, 0, 3)

        grid.addWidget(_lbl("Assigned to"), 1, 0)
        self._assigned_combo = QComboBox()
        self._assigned_combo.addItem("Unassigned", "")
        self._assigned_combo.currentIndexChanged.connect(self._on_edit)
        grid.addWidget(self._assigned_combo, 1, 1, 1, 3)

        grid.addWidget(_lbl("Iteration"), 2, 0)
        self._iteration_combo = QComboBox()
        self._iteration_combo.setToolTip("Select from the project's iterations")
        self._iteration_combo.currentIndexChanged.connect(self._on_edit)
        grid.addWidget(self._iteration_combo, 2, 1, 1, 3)

        grid.addWidget(_lbl("Area"), 3, 0)
        self._area_combo = QComboBox()
        self._area_combo.setToolTip("Select from the project's areas")
        self._area_combo.currentIndexChanged.connect(self._on_edit)
        grid.addWidget(self._area_combo, 3, 1, 1, 3)

        grid.addWidget(_lbl("Tags"), 4, 0)
        self._tags_edit = TagLineEdit()
        self._tags_edit.setPlaceholderText("tag1; tag2")
        self._tags_edit.textChanged.connect(self._on_edit)
        grid.addWidget(self._tags_edit, 4, 1, 1, 3)

        grid.addWidget(_lbl("Remaining"), 5, 0)
        self._remaining_edit = QLineEdit()
        self._remaining_edit.setPlaceholderText("hours")
        self._remaining_edit.textChanged.connect(self._on_edit)
        grid.addWidget(self._remaining_edit, 5, 1)
        grid.addWidget(_lbl("Completed"), 5, 2)
        self._completed_edit = QLineEdit()
        self._completed_edit.setPlaceholderText("hours")
        self._completed_edit.textChanged.connect(self._on_edit)
        grid.addWidget(self._completed_edit, 5, 3)

        grid.addWidget(_lbl("Estimate"), 6, 0)
        self._estimate_edit = QLineEdit()
        self._estimate_edit.setPlaceholderText("hours")
        self._estimate_edit.setToolTip("Original Estimate (hours)")
        self._estimate_edit.textChanged.connect(self._on_edit)
        grid.addWidget(self._estimate_edit, 6, 1)
        grid.addWidget(_lbl("Activity"), 6, 2)
        self._activity_combo = QComboBox()
        self._activity_combo.setToolTip("Activity (Development, Testing, …)")
        self._activity_combo.currentIndexChanged.connect(self._on_edit)
        grid.addWidget(self._activity_combo, 6, 3)
        v.addLayout(grid)

        self._desc_lbl = _lbl("Description")
        v.addWidget(self._desc_lbl)
        self._desc_edit = _ResizableTextEdit(min_height=90, start_height=112)
        self._desc_edit.setToolTip("Drag the bottom-right corner to resize")
        self._desc_edit.textChanged.connect(self._on_edit)
        v.addWidget(self._desc_edit)

        save_row = QHBoxLayout()
        self._save_status = QLabel("")
        self._save_status.setWordWrap(True)
        save_row.addWidget(self._save_status, 1)
        self._save_btn = QPushButton("Save")
        self._save_btn.setIcon(icons.icon("check", color="white", size=14))
        self._save_btn.setEnabled(False)
        self._save_btn.setStyleSheet(theme.btn_primary_qss("padding: 6px 20px;"))
        self._save_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._save_btn.clicked.connect(self._on_save)
        save_row.addWidget(self._save_btn)
        v.addLayout(save_row)

        self._comments_lbl = _lbl("Comments")
        v.addWidget(self._comments_lbl)
        # Comments render as avatar + author + "commented <when>" + body inside a
        # bounded, self-scrolling column. Palette-based background only — never a
        # stylesheet on this scroll area's ancestors (the QStyleSheetStyle gotcha
        # noted above for the old list).
        self._comments_scroll = QScrollArea()
        self._comments_scroll.setWidgetResizable(True)
        self._comments_scroll.setFrameShape(QFrame.NoFrame)
        self._comments_scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self._comments_scroll.setMinimumHeight(120)
        self._comments_scroll.setMaximumHeight(230)
        self._comments_host = QWidget()
        for w in (self._comments_scroll.viewport(), self._comments_host):
            w.setBackgroundRole(QPalette.Window)
            w.setAutoFillBackground(True)
        self._comments_layout = QVBoxLayout(self._comments_host)
        self._comments_layout.setContentsMargins(2, 0, 8, 0)
        self._comments_layout.setSpacing(0)
        self._comments_layout.addStretch()
        self._comments_scroll.setWidget(self._comments_host)
        v.addWidget(self._comments_scroll)
        self._comment_box = QPlainTextEdit()
        self._comment_box.setPlaceholderText("Write a comment…")
        self._comment_box.setFixedHeight(54)
        v.addWidget(self._comment_box)
        comment_row = QHBoxLayout()
        comment_row.addStretch()
        self._comment_btn = QPushButton("Add comment")
        self._comment_btn.setStyleSheet(theme.btn_neutral_qss())
        self._comment_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._comment_btn.clicked.connect(self._on_add_comment)
        comment_row.addWidget(self._comment_btn)
        v.addLayout(comment_row)
        v.addStretch()

        self._detail_scroll.setWidget(form_host)
        self._detail_scroll.setVisible(False)
        outer.addWidget(self._detail_scroll, 1)
        return panel

    # ------------------------------------------------------------------ #
    #  Filters / board build                                              #
    # ------------------------------------------------------------------ #

    def _repopulate_type_filter(self):
        """Fill the type multi-select from the loaded items, keeping any ticks
        that still apply."""
        keep = set(self._type_combo.checked_data())
        self._type_combo._model.removeRows(0, self._type_combo._model.rowCount())
        types = sorted({wi.type for wi in self._items if wi.type})
        for t in types:
            self._type_combo.addCheckItem(t, t)
        self._type_combo.set_checked_data([t for t in keep if t in types])

    def _filters_active(self) -> bool:
        return bool(self._search.text().strip() or self._type_combo.checked_data())

    def _on_filter_changed(self):
        self._rebuild()
        self._clear_btn.setVisible(self._filters_active())

    def _clear_filters(self):
        self._search.blockSignals(True)
        self._search.clear()
        self._search.blockSignals(False)
        self._type_combo.clear_checks()
        self._on_filter_changed()

    def _visible_items(self) -> list:
        """Board items minus the ones the user has locally hidden."""
        return [wi for wi in self._items if wi.id not in self._hidden]

    def _filtered_sorted(self) -> list:
        query = self._search.text().strip().lower()
        types = set(self._type_combo.checked_data())
        items = [
            wi for wi in self._visible_items()
            if (not types or wi.type in types)
            and (not query or query in wi.title.lower() or query == str(wi.id))
        ]
        mode = self._sort_combo.currentData()
        if mode == "priority":   # lower number = more urgent; unset last
            items.sort(key=lambda w: (w.priority is None, w.priority or 0))
        elif mode in ("az", "za"):
            items.sort(key=lambda w: w.title.lower(), reverse=(mode == "za"))
        return items   # default: WIQL order (recently changed first)

    def _state_hex(self, wi: WorkItem) -> str:
        """The item's state colour as a bare hex string ('' if the process didn't
        supply one) — stashed per card for the delegate's state dot."""
        entry = (self._states_by_type.get(wi.type) or {}).get(wi.state)
        if entry and entry[1]:
            return str(entry[1]).lstrip("#")
        return ""

    def _cat_map(self) -> dict:
        """{type: {state: category}} for WorkItem.column (strip the colours)."""
        return {t: {s: v[0] for s, v in m.items()}
                for t, m in self._states_by_type.items()}

    def _rebuild(self):
        self._selecting = True   # clear() fires selectionChanged — don't react
        try:
            for lst in self._col_lists.values():
                lst.clear()
            counts = {col: 0 for col in COLUMNS}
            cat_map = self._cat_map()
            shown = 0
            for wi in self._filtered_sorted():
                col = wi.column(cat_map)
                if col is None:   # Removed — hidden
                    continue
                # Item text (the title) is kept only for keyboard type-search;
                # the card delegate draws the actual layout from the WorkItem.
                item = QListWidgetItem(wi.title)
                item.setData(Qt.UserRole, wi)
                item.setData(_STATE_HEX_ROLE, self._state_hex(wi))
                tip = f"#{wi.id}  ·  {wi.type}  ·  {wi.state}"
                if wi.priority is not None:
                    tip += f"  ·  P{wi.priority}"
                tip += (f"\n{wi.title}"
                        "\n\nClick to edit here · drag to another column to change "
                        "state · double-click to open in Azure DevOps")
                item.setToolTip(tip)
                self._col_lists[col].addItem(item)
                counts[col] += 1
                shown += 1
            for col, lbl in self._col_labels.items():
                lbl.setText(f"<b>{col}</b>  <span style='color:{theme.tokens()['text_dim2']}'>"
                            f"{counts[col]}</span>")
            total = len(self._visible_items())   # excludes locally hidden items
            hidden = sum(1 for wi in self._items if wi.id in self._hidden)
            if self._filters_active() and total:
                self._count_lbl.setText(f"{shown} of {total} items")
            else:
                self._count_lbl.setText(f"{total} item{'s' if total != 1 else ''}"
                                        if self._loaded_key else "")
            self._empty_lbl.setVisible(self._loaded_key is not None and total == 0)
            if total == 0 and hidden:
                self._empty_lbl.setText(
                    f"All {hidden} item{'s' if hidden != 1 else ''} here are hidden — "
                    "use “Hidden” to bring them back.")
            elif total == 0:
                self._empty_lbl.setText(
                    f"No work items on the {self._scope['team']} board."
                    if self._scope.get("mode") == "team"
                    else "No work items are assigned to you in this project.")
            else:
                self._empty_lbl.setText("")
            # Keep the edited card highlighted after a rebuild.
            if self._current is not None:
                self._select_card(self._current.id)
        finally:
            self._selecting = False

    def _select_card(self, wid):
        """Silently select the card for a work-item id (if visible)."""
        for lst in self._col_lists.values():
            for r in range(lst.count()):
                it = lst.item(r)
                w = it.data(Qt.UserRole)
                if w is not None and w.id == wid:
                    lst.setCurrentItem(it)
                    return

    # ------------------------------------------------------------------ #
    #  Selection → detail editor                                          #
    # ------------------------------------------------------------------ #

    def _on_card_selected(self, src_list):
        if self._selecting:
            return
        items = src_list.selectedItems()
        if not items:
            return
        wi = items[0].data(Qt.UserRole)
        if wi is None:
            return
        if (self._dirty and self._current is not None
                and wi.id != self._current.id):
            reply = QMessageBox.question(
                self, "Discard changes?",
                f"#{self._current.id} has unsaved changes. Discard them?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No)
            if reply != QMessageBox.Yes:
                self._selecting = True
                try:
                    src_list.clearSelection()
                    self._select_card(self._current.id)
                finally:
                    self._selecting = False
                return
        self._selecting = True
        try:
            for lst in self._col_lists.values():
                if lst is not src_list:
                    lst.clearSelection()
        finally:
            self._selecting = False
        self._load_detail(wi)

    def _show_placeholder(self):
        self._current = None
        self._dirty = False
        self._detail_scroll.setVisible(False)
        self._no_sel_lbl.setVisible(True)

    def _load_detail(self, wi: WorkItem):
        self._current = wi
        self._suspend = True
        try:
            self._detail_id_lbl.setText(
                f"<b>#{wi.id}</b>  <span style='color:{theme.tokens()['text_dim']}'>"
                f"{wi.type}</span>")
            self._title_edit.setText(wi.title)

            # Legal states for this item's type (fall back to just its own state).
            self._state_combo.clear()
            names = list((self._states_by_type.get(wi.type) or {}).keys()) or [wi.state]
            if wi.state and wi.state not in names:
                names.insert(0, wi.state)
            for n in names:
                self._state_combo.addItem(n)
            self._state_combo.setCurrentText(wi.state)

            self._apply_assignee(wi)

            idx = self._priority_combo.findData(wi.priority)
            self._priority_combo.setCurrentIndex(idx if idx >= 0 else 0)
            self._fill_path_combo(self._iteration_combo, self._iterations,
                                  wi.iteration_path)
            self._fill_path_combo(self._area_combo, self._areas, wi.area_path)
            self._tags_edit.setText(wi.tags)
            self._remaining_edit.setText(
                "" if wi.remaining_work is None else str(wi.remaining_work))
            self._completed_edit.setText(
                "" if wi.completed_work is None else str(wi.completed_work))
            self._estimate_edit.setText(
                "" if wi.original_estimate is None else str(wi.original_estimate))
            self._populate_activity(wi)

            self._load_description(wi)
            self._save_status.setText("")
            self._no_sel_lbl.setVisible(False)
            self._detail_scroll.setVisible(True)
        finally:
            self._suspend = False
        self._set_dirty(False)
        self._sync_focus_btn()
        self._load_comments(wi.id)

    def _apply_assignee(self, wi: WorkItem):
        """Fill the assignee combo from the shared members cache and select the
        item's assignee (adding them if the cache doesn't know them yet)."""
        members = self.app_state.cached_team_members or []
        combo = self._assigned_combo
        combo.blockSignals(True)
        combo.clear()
        combo.addItem("Unassigned", "")
        for user in members:
            display = user.get("displayName", user.get("uniqueName", ""))
            unique = user.get("uniqueName", "")
            if display and unique:
                combo.addItem(display, unique)
        unique = wi.assigned_to_unique
        if unique:
            idx = combo.findData(unique)
            if idx < 0:
                combo.addItem(wi.assigned_to or unique, unique)
                idx = combo.findData(unique)
            combo.setCurrentIndex(idx)
        else:
            combo.setCurrentIndex(0)
        combo.blockSignals(False)

    def _refresh_members(self):
        """Warm the shared team-members cache (used by the assignee combo)."""
        from app.gui.helpers import refresh_team_members
        refresh_team_members(self.app_state, self._on_members,
                             self._on_members_fetched, self._on_members_failed)

    def _on_members(self, members):
        if self._current is not None and not self._dirty:
            self._suspend = True
            try:
                self._apply_assignee(self._current)
            finally:
                self._suspend = False

    def _on_members_fetched(self, members):
        from app.gui.helpers import store_fetched_members
        store_fetched_members(self.app_state, members)
        self._on_members(members)

    def _on_members_failed(self, _msg):
        self.app_state._team_members_fetcher = None

    # -- description (lazy: rich-text fields are skipped by the board fetch) --

    def _load_description(self, wi: WorkItem):
        primary = ("Microsoft.VSTS.TCM.ReproSteps" if wi.type == "Bug"
                   else "System.Description")
        fallback = ("System.Description" if primary != "System.Description"
                    else "Microsoft.VSTS.TCM.ReproSteps")
        if wi.fields.get("_desc_fetched"):
            self._show_description(wi, primary, fallback)
            return
        self._desc_edit.setPlainText("")
        self._desc_edit.setEnabled(False)
        self._desc_edit.setPlaceholderText("Loading description…")
        worker = Worker(self.app_state.client.get_work_items, [wi.id], _DESC_FIELDS)
        worker.signals.result.connect(lambda res, w=wi: self._on_description(w, res))
        worker.signals.error.connect(lambda _exc: self._desc_edit.setEnabled(True))
        QThreadPool.globalInstance().start(worker)

    def _on_description(self, wi: WorkItem, result: list):
        fetched = result[0] if result else {}
        for ref in _DESC_FIELDS:
            wi.fields[ref] = fetched.get(ref, "")
        wi.fields["_desc_fetched"] = True
        if self._current is not None and self._current.id == wi.id:
            primary = ("Microsoft.VSTS.TCM.ReproSteps" if wi.type == "Bug"
                       else "System.Description")
            fallback = ("System.Description" if primary != "System.Description"
                        else "Microsoft.VSTS.TCM.ReproSteps")
            self._show_description(wi, primary, fallback)

    def _show_description(self, wi: WorkItem, primary: str, fallback: str):
        """Render the primary rich-text field as Markdown (headings, bold, tables
        all show); fall back to the other when the primary is empty. Edits write
        back to whichever was shown, re-serialised to HTML on save."""
        raw_primary = wi.fields.get(primary, "") or ""
        raw_fallback = wi.fields.get(fallback, "") or ""
        self._desc_field = primary if (raw_primary or not raw_fallback) else fallback
        md = html_to_markdown(raw_primary or raw_fallback)
        self._suspend = True
        try:
            self._desc_edit.setEnabled(True)
            self._desc_edit.setPlaceholderText("Description")
            self._desc_edit.setMarkdown(md)
        finally:
            self._suspend = False
        # Baseline off the widget's own normalised Markdown so an untouched
        # description never looks "changed" (setMarkdown/toMarkdown round-trip).
        self._desc_original = self._desc_edit.toMarkdown()

    # ------------------------------------------------------------------ #
    #  Editing / saving                                                   #
    # ------------------------------------------------------------------ #

    def _on_edit(self, *_a):
        if not self._suspend:
            self._set_dirty(True)

    def _set_dirty(self, dirty: bool):
        self._dirty = dirty
        self._save_btn.setEnabled(dirty and not self._saving)

    @staticmethod
    def _parse_hours(text: str):
        """float or None for a Remaining/Completed box; raises ValueError."""
        text = text.strip()
        if not text:
            return None
        return float(text)

    def _fill_path_combo(self, combo, values, current):
        """Fill an Area/Iteration combo with the project's discovered paths,
        selecting the item's current value (inserted if discovery somehow lacks
        it, so nothing is silently lost). Called under _suspend."""
        combo.clear()
        vals = list(values)
        if current and current not in vals:
            vals.insert(0, current)
        for v in vals:
            # Show the path without the repeated project-root prefix; keep the
            # full path as the item data (that's what the Save diff/PATCH uses).
            combo.addItem(_strip_path_root(v), v)
            combo.setItemData(combo.count() - 1, v, Qt.ToolTipRole)  # full path on hover
        idx = combo.findData(current)
        combo.setCurrentIndex(idx if idx >= 0 else 0)

    def _populate_activity(self, wi: WorkItem):
        """Fill the Activity combo with the type's allowed values (blank first),
        selecting the item's current value. Disabled for types with no Activity
        field. Called under _suspend, so signals won't mark the form dirty."""
        combo = self._activity_combo
        combo.clear()
        combo.addItem("—", "")
        values = self._activities_by_type.get(wi.type, []) or []
        for val in values:
            combo.addItem(val, val)
        cur = wi.activity
        if cur and cur not in values:
            combo.addItem(cur, cur)   # honour a value outside the discovered list
        idx = combo.findData(cur or "")
        combo.setCurrentIndex(idx if idx >= 0 else 0)
        combo.setEnabled(bool(values) or bool(cur))

    def _collect_changes(self) -> dict:
        """Diff the form against the current item — only what changed is sent
        (so untouched fields can never be clobbered, and processes without a
        given field never see it). State is EXCLUDED: it saves instantly."""
        wi = self._current
        changes = {}
        title = self._title_edit.text().strip()
        if title and title != wi.title:
            changes["System.Title"] = title
        unique = self._assigned_combo.currentData()
        if unique is not None and unique != wi.assigned_to_unique:
            changes["System.AssignedTo"] = unique   # "" unassigns
        prio = self._priority_combo.currentData()
        if prio is not None and prio != wi.priority:
            changes["Microsoft.VSTS.Common.Priority"] = prio
        iteration = self._iteration_combo.currentData() or ""
        if iteration and iteration != wi.iteration_path:
            changes["System.IterationPath"] = iteration
        area = self._area_combo.currentData() or ""
        if area and area != wi.area_path:
            changes["System.AreaPath"] = area
        tags = self._tags_edit.text().strip()
        if tags != wi.tags:
            changes["System.Tags"] = tags
        # The editor holds Markdown; ADO's field is HTML, so serialise on save.
        # Diff on Markdown (stable) but send HTML.
        desc = self._desc_edit.toMarkdown()
        if self._desc_edit.isEnabled() and desc != self._desc_original:
            changes[self._desc_field] = markdown_to_html(desc)
        remaining = self._parse_hours(self._remaining_edit.text())
        if remaining is not None and remaining != wi.remaining_work:
            changes["Microsoft.VSTS.Scheduling.RemainingWork"] = remaining
        completed = self._parse_hours(self._completed_edit.text())
        if completed is not None and completed != wi.completed_work:
            changes["Microsoft.VSTS.Scheduling.CompletedWork"] = completed
        estimate = self._parse_hours(self._estimate_edit.text())
        if estimate is not None and estimate != wi.original_estimate:
            changes[_ORIGINAL] = estimate
        activity = self._activity_combo.currentData() or ""
        if activity != wi.activity:
            changes[_ACTIVITY] = activity   # "" clears the Activity
        return changes

    def _on_save(self):
        if self._current is None or self._saving:
            return
        try:
            changes = self._collect_changes()
        except ValueError:
            self._set_save_status("Remaining / Completed must be numbers (hours).",
                                  error=True)
            return
        if not changes:
            self._set_save_status("No changes to save.")
            self._set_dirty(False)
            return
        self._saving = True
        self._save_btn.setEnabled(False)
        self._save_btn.setText("Saving…")
        self._set_save_status("")
        wid = self._current.id
        worker = Worker(self.app_state.client.update_work_item_fields, wid, changes)
        worker.signals.result.connect(
            lambda data, w=wid, ch=changes: self._on_saved(w, ch, data))
        worker.signals.error.connect(self._on_save_error)
        QThreadPool.globalInstance().start(worker)

    def _on_saved(self, wid, changes: dict, data: dict):
        self._saving = False
        self._save_btn.setText("Save")
        wi = next((w for w in self._items if w.id == wid), None)
        if wi is not None:
            # Optimistic local merge: server response is the source of truth
            # (rules may have adjusted fields); fall back to our own changes.
            fresh = (data or {}).get("fields") or {}
            wi.fields.update(fresh or changes)
            wi.fields["_id"] = wid
            if self._desc_field in changes:
                self._desc_original = self._desc_edit.toMarkdown()
        self._set_dirty(False)
        self._set_save_status("Saved ✓", ok=True)
        self._rebuild()   # title/priority on the card may have changed

    def _on_save_error(self, exc):
        self._saving = False
        self._save_btn.setText("Save")
        self._set_dirty(True)   # nothing was lost — keep Save armed
        self._set_save_status(
            f"Could not save: {exc}  —  use 'Open in browser' for fields with "
            "process rules.", error=True)

    # -- instant state transition ---------------------------------------- #

    def _on_state_changed(self, _idx):
        """The State combo saves immediately — the quickest, most common action.
        Other pending form edits stay pending (only System.State is sent)."""
        if self._suspend or self._current is None:
            return
        new_state = self._state_combo.currentText()
        if not new_state or new_state == self._current.state:
            return
        self._transition_state(self._current.id, new_state)

    def _transition_state(self, wid, new_state):
        """PATCH only System.State — shared by the editor's State combo and
        drag-drop between columns."""
        self._state_combo.setEnabled(False)
        self._set_save_status(f"Moving #{wid} to {new_state}…")
        worker = Worker(self.app_state.client.update_work_item_fields,
                        wid, {"System.State": new_state})
        worker.signals.result.connect(
            lambda data, w=wid, s=new_state: self._on_state_saved(w, s, data))
        worker.signals.error.connect(lambda exc, w=wid: self._on_state_error(w, exc))
        QThreadPool.globalInstance().start(worker)

    def _on_state_saved(self, wid, new_state, data):
        self._state_combo.setEnabled(True)
        wi = next((w for w in self._items if w.id == wid), None)
        if wi is not None:
            fresh = (data or {}).get("fields") or {}
            wi.fields.update(fresh or {"System.State": new_state})
            wi.fields["_id"] = wid
            # A drag on the item open in the editor must sync its State combo.
            if self._current is not None and self._current.id == wid:
                self._suspend = True
                try:
                    self._state_combo.setCurrentText(wi.state)
                finally:
                    self._suspend = False
        self._set_save_status(f"State → {new_state} ✓", ok=True)
        self._rebuild()   # the card moves column / retints

    def _on_state_error(self, wid, exc):
        self._state_combo.setEnabled(True)
        wi = next((w for w in self._items if w.id == wid), None)
        if wi is not None and self._current is not None and self._current.id == wid:
            self._suspend = True
            try:
                self._state_combo.setCurrentText(wi.state)   # revert
            finally:
                self._suspend = False
        self._set_save_status(
            f"Could not change state: {exc}  —  ADO may require extra fields "
            "for this transition; use 'Open in browser'.", error=True)

    def _set_save_status(self, text: str, ok: bool = False, error: bool = False):
        t = theme.tokens()
        color = t["ok"] if ok else (t["error"] if error else t["text_dim"])
        self._save_status.setStyleSheet(f"color: {color}; font-size: 11px;")
        self._save_status.setText(text)

    # ------------------------------------------------------------------ #
    #  Drag-drop between columns → state transition                       #
    # ------------------------------------------------------------------ #

    def _on_drag_started(self, wi):
        self._drag_wi = wi

    def _state_for_column(self, wi: WorkItem, col: str):
        """The state a drop on `col` should move `wi` to: the first state of the
        column's category defined for the item's type (the API lists states in
        workflow order). None when the process defines no such state."""
        states = self._states_by_type.get(wi.type) or {}
        for cat in _COLUMN_CATEGORIES.get(col, ()):
            for name, (category, _color) in states.items():
                if category == cat:
                    return name
        return None

    def _on_card_dropped(self, col: str):
        wi, self._drag_wi = self._drag_wi, None
        if wi is None:
            return
        if wi.column(self._cat_map()) == col:
            return   # dropped back on its own column
        new_state = self._state_for_column(wi, col)
        if not new_state:
            self._set_save_status(
                f"{wi.type} has no {col} state in this process.", error=True)
            return
        self._transition_state(wi.id, new_state)

    # ------------------------------------------------------------------ #
    #  Focus timer — logs elapsed time to Completed Work                  #
    # ------------------------------------------------------------------ #

    def _focus_elapsed(self) -> float:
        if not self._focus:
            return 0.0
        elapsed = self._focus["accum"]
        if self._focus.get("run_started") is not None:
            elapsed += time.monotonic() - self._focus["run_started"]
        return elapsed

    def _start_focus_current(self):
        if self._current is not None:
            self._start_focus(self._current)

    def _start_focus(self, wi: WorkItem):
        if self._focus and self._focus["id"] == wi.id:
            if self._focus.get("run_started") is None:
                self._toggle_focus_pause()   # same item, paused → resume
            return
        if self._focus:
            self._stop_focus(log=True)       # switching focus logs the old item
        self._focus = {"id": wi.id, "title": wi.title, "accum": 0.0,
                       "run_started": time.monotonic()}
        self._focus_ticks = 0
        self._focus_tick.start()
        self._persist_focus()
        self._update_focus_ui()

    def _toggle_focus_pause(self):
        if not self._focus:
            return
        if self._focus.get("run_started") is not None:
            self._focus["accum"] += time.monotonic() - self._focus["run_started"]
            self._focus["run_started"] = None
            self._focus_tick.stop()
        else:
            self._focus["run_started"] = time.monotonic()
            self._focus_tick.start()
        self._persist_focus()
        self._update_focus_ui()

    def _stop_focus(self, log: bool = True):
        """Stop the timer; when `log`, add the elapsed hours to the item's
        Completed Work (fresh read-modify-write on a worker)."""
        if not self._focus:
            return
        focus = self._focus
        hours = round(self._focus_elapsed() / 3600.0, 2)
        self._focus = None
        self._focus_tick.stop()
        from app.utils.settings import clear_focus_timer
        clear_focus_timer()
        self._update_focus_ui()
        if not log:
            return
        if hours < 0.01:
            self._status_lbl.setText("Focus session under a minute — nothing logged.")
            return
        self._status_lbl.setText(f"Logging {hours:g}h to #{focus['id']}…")
        worker = Worker(_log_focus_time, self.app_state.client, focus["id"], hours)
        worker.signals.result.connect(
            lambda res, f=focus, h=hours: self._on_time_logged(f, h, res))
        worker.signals.error.connect(
            lambda exc, f=focus, h=hours: self._on_time_log_error(f, h, exc))
        QThreadPool.globalInstance().start(worker)

    def _on_time_logged(self, focus, hours, res: dict):
        wid = focus["id"]
        wi = next((w for w in self._items if w.id == wid), None)
        if wi is not None:
            wi.fields.update((res or {}).get("fields", {}))
            wi.fields["_id"] = wid
            # Reflect the new numbers in the editor if the item is on screen
            # (and the user isn't mid-edit).
            if self._current is not None and self._current.id == wid and not self._dirty:
                self._suspend = True
                try:
                    self._remaining_edit.setText(
                        "" if wi.remaining_work is None else str(wi.remaining_work))
                    self._completed_edit.setText(
                        "" if wi.completed_work is None else str(wi.completed_work))
                finally:
                    self._suspend = False
        self._status_lbl.setText(f"Logged {hours:g}h to #{wid} ✓")

    def _on_time_log_error(self, focus, hours, exc):
        # The time is not silently lost — the message carries the hours so they
        # can be added in the browser (some processes don't track Completed Work).
        self._status_lbl.setText(
            f"Could not log {hours:g}h to #{focus['id']}: {exc} — add it in the "
            "browser if this process tracks Completed Work.")

    def _on_focus_tick(self):
        self._update_focus_label()
        self._focus_ticks += 1
        if self._focus_ticks % 30 == 0:
            self._persist_focus()   # survives a crash/kill at ≤30s granularity

    def _persist_focus(self):
        from app.utils.settings import save_focus_timer
        if self._focus:
            save_focus_timer({"id": self._focus["id"], "title": self._focus["title"],
                              "accum": self._focus_elapsed()})

    def _update_focus_label(self):
        if self._focus:
            self._focus_lbl.setText(
                f"Now: <b>#{self._focus['id']}</b> · {_fmt_elapsed(self._focus_elapsed())}")

    def _update_focus_ui(self):
        from app.utils import icons
        has = self._focus is not None
        self._focus_frame.setVisible(has)
        if has:
            running = self._focus.get("run_started") is not None
            self._focus_pause_btn.setIcon(icons.icon("pause" if running else "play", size=13))
            self._focus_pause_btn.setToolTip(
                "Pause the focus timer" if running else "Resume the focus timer")
            self._focus_stop_btn.setIcon(icons.icon("stop", size=13))
            self._update_focus_label()
        self._sync_focus_btn()

    def _sync_focus_btn(self):
        """The editor's Focus button reflects the shown item's timer state."""
        if self._current is None:
            return
        focused = self._focus is not None and self._focus["id"] == self._current.id
        running = focused and self._focus.get("run_started") is not None
        if running:
            self._focus_btn.setText("Focusing…")
            self._focus_btn.setEnabled(False)
        elif focused:
            self._focus_btn.setText("Resume focus")
            self._focus_btn.setEnabled(True)
        else:
            self._focus_btn.setText("Focus")
            self._focus_btn.setEnabled(True)

    # ------------------------------------------------------------------ #
    #  Comments                                                           #
    # ------------------------------------------------------------------ #

    def _load_comments(self, wid):
        cached = self._comments_cache.get(wid)
        if cached is not None:
            self._render_comments(cached)
            return
        self._show_comment_message("Loading comments…")
        worker = Worker(self.app_state.client.get_work_item_comments, wid)
        worker.signals.result.connect(lambda res, w=wid: self._on_comments(w, res))
        worker.signals.error.connect(
            lambda _exc, w=wid: self._on_comments(w, []))
        QThreadPool.globalInstance().start(worker)

    def _on_comments(self, wid, comments: list):
        self._comments_cache[wid] = comments
        if self._current is not None and self._current.id == wid:
            self._render_comments(comments)

    def _clear_comments(self):
        while self._comments_layout.count():
            item = self._comments_layout.takeAt(0)
            w = item.widget()
            if w is not None:
                w.deleteLater()

    def _show_comment_message(self, text: str):
        self._clear_comments()
        t = theme.tokens()
        lab = QLabel(text)
        lab.setStyleSheet(
            f"color:{t['text_dim2']}; background: transparent; font-size: 12px;")
        self._comments_layout.addWidget(lab)
        self._comments_layout.addStretch()

    def _render_comments(self, comments: list):
        if not comments:
            self._show_comment_message("No comments yet.")
            return
        self._clear_comments()
        for i, c in enumerate(comments):
            if i:
                sep = QFrame()
                sep.setFixedHeight(1)
                sep.setAutoFillBackground(True)
                sep.setStyleSheet(f"background: {theme.tokens()['border']};")
                self._comments_layout.addWidget(sep)
            entry = _CommentEntry(c)
            self._comments_layout.addWidget(entry)
            self._request_avatar(entry, c.get("avatar_url", ""))
        self._comments_layout.addStretch()

    def _request_avatar(self, entry, url: str):
        """Load the author's real profile photo for `entry`, keeping the initials
        avatar until it arrives. Coalesces concurrent requests for the same URL
        and caches the rounded result so re-renders / theme toggles never
        refetch."""
        if not url:
            return
        cached = self._avatar_cache.get(url)
        if cached is not None:
            entry.set_avatar(cached)
            return
        waiters = self._avatar_pending.get(url)
        if waiters is not None:
            waiters.append(entry)   # a fetch for this author is already in flight
            return
        self._avatar_pending[url] = [entry]
        worker = Worker(self.app_state.client.get_avatar_image, url)
        worker.signals.result.connect(lambda data, u=url: self._on_avatar(u, data))
        worker.signals.error.connect(lambda _exc, u=url: self._on_avatar(u, None))
        QThreadPool.globalInstance().start(worker)

    def _on_avatar(self, url: str, data):
        waiters = self._avatar_pending.pop(url, [])
        if not data:
            return   # leave the initials fallback in place
        pm = QPixmap()
        if not pm.loadFromData(data):
            return
        rounded = _round_pixmap(pm, 30)
        self._avatar_cache[url] = rounded
        for entry in waiters:
            try:
                entry.set_avatar(rounded)
            except RuntimeError:
                pass   # entry was replaced by a re-render before the image loaded

    def _on_add_comment(self):
        if self._current is None:
            return
        text = self._comment_box.toPlainText().strip()
        if not text:
            return
        wid = self._current.id
        self._comment_btn.setEnabled(False)
        self._comment_btn.setText("Adding…")
        worker = Worker(self.app_state.client.add_work_item_comment, wid, text)
        worker.signals.result.connect(lambda res, w=wid: self._on_comment_added(w, res))
        worker.signals.error.connect(self._on_comment_error)
        QThreadPool.globalInstance().start(worker)

    def _on_comment_added(self, wid, res: dict):
        self._comment_btn.setEnabled(True)
        self._comment_btn.setText("Add comment")
        self._comment_box.clear()
        from datetime import datetime, timezone
        entry = {
            "id": (res or {}).get("id"),
            "text": (res or {}).get("text", ""),
            "created_by": "You",
            "created_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        }
        self._comments_cache.setdefault(wid, []).insert(0, entry)
        if self._current is not None and self._current.id == wid:
            self._render_comments(self._comments_cache[wid])

    def _on_comment_error(self, exc):
        self._comment_btn.setEnabled(True)
        self._comment_btn.setText("Add comment")
        self._set_save_status(f"Could not add comment: {exc}", error=True)

    # ------------------------------------------------------------------ #
    #  Quick create                                                       #
    # ------------------------------------------------------------------ #

    def _on_new_item(self):
        dlg = _NewItemDialog(self)
        if not dlg.exec_():
            return
        kind, title, desc = dlg.values()
        assign_to = ""
        try:
            assign_to = self.app_state.token_manager.get_current_upn() or ""
        except Exception:
            pass
        self._new_btn.setEnabled(False)
        self._status_lbl.setText(f"Creating {kind.lower()}…")
        worker = Worker(_quick_create, self.app_state.client,
                        kind, title, desc, assign_to)
        worker.signals.result.connect(self._on_created)
        worker.signals.error.connect(self._on_create_error)
        QThreadPool.globalInstance().start(worker)

    def _on_created(self, res: dict):
        self._new_btn.setEnabled(True)
        self._status_lbl.setText("")
        who = res.get("assign_to", "")
        fields = {
            "_id": res.get("id"),
            "System.Title": res.get("title", ""),
            "System.WorkItemType": res.get("type", ""),
            "System.State": res.get("state", "New"),
            "_desc_fetched": True,
        }
        if res.get("description"):
            fields[res.get("desc_field", "System.Description")] = res["description"]
        if who:
            fields["System.AssignedTo"] = {"uniqueName": who, "displayName": who}
        # New local item at the top (WIQL order is recently-changed-first) +
        # make sure its type's states are known for column/tint/state-combo.
        if res.get("states") and res.get("type") not in self._states_by_type:
            self._states_by_type[res["type"]] = res["states"]
        wi = WorkItem(fields)
        self._items.insert(0, wi)
        self._current = wi          # _rebuild re-selects the current card silently
        self._repopulate_type_filter()
        self._rebuild()
        self._load_detail(wi)
        self._set_save_status(f"Created #{wi.id} ✓", ok=True)

    def _on_create_error(self, exc):
        self._new_btn.setEnabled(True)
        self._status_lbl.setText("")
        QMessageBox.critical(self, "Could not create",
                             f"The work item was not created:\n\n{exc}")

    # ------------------------------------------------------------------ #
    #  Actions                                                            #
    # ------------------------------------------------------------------ #

    def _web_url(self, wid) -> str:
        tm = self.app_state.token_manager
        return f"{tm.org_url}/{quote(tm.project)}/_workitems/edit/{wid}"

    def _open_in_browser(self, item):
        wi = item.data(Qt.UserRole)
        if wi is not None and wi.id is not None:
            webbrowser.open(self._web_url(wi.id))

    def _open_current_in_browser(self):
        if self._current is not None and self._current.id is not None:
            webbrowser.open(self._web_url(self._current.id))

    # ------------------------------------------------------------------ #
    #  Hide / unhide (local only — never touches ADO)                     #
    # ------------------------------------------------------------------ #

    def _on_card_context_menu(self, lst, pos):
        """Right-click a card → open in browser / hide it."""
        item = lst.itemAt(pos)
        if item is None:
            return
        wi = item.data(Qt.UserRole)
        if wi is None or wi.id is None:
            return
        menu = QMenu(self)
        open_act = menu.addAction("Open in Azure DevOps")
        menu.addSeparator()
        hide_act = menu.addAction(f"Hide #{wi.id}")
        act = menu.exec_(lst.viewport().mapToGlobal(pos))
        if act == open_act:
            webbrowser.open(self._web_url(wi.id))
        elif act == hide_act:
            self._hide_item(wi)

    def _hide_item(self, wi: WorkItem):
        self._hidden.add(wi.id)
        self._persist_hidden()
        # If the hidden card was open in the editor, clear the editor.
        if self._current is not None and self._current.id == wi.id:
            self._current = None
            self._show_placeholder()
        self._rebuild()
        self._update_hidden_btn()
        self._status_lbl.setText(f"Hid #{wi.id} — undo it from “Hidden”.")

    def _unhide(self, wid):
        self._hidden.discard(wid)
        self._persist_hidden()
        self._rebuild()
        self._update_hidden_btn()

    def _unhide_all(self):
        self._hidden.clear()
        self._persist_hidden()
        self._rebuild()
        self._update_hidden_btn()

    def _persist_hidden(self):
        from app.utils.settings import save_hidden_work_items
        save_hidden_work_items(self.app_state.token_manager.org_url, self._hidden)

    def _update_hidden_btn(self):
        n = len(self._hidden)
        self._hidden_btn.setVisible(n > 0)
        self._hidden_btn.setText(f"Hidden ({n})" if n else "Hidden")

    def _show_hidden_menu(self):
        """Menu listing hidden items (title if known, else just the id) with a
        per-item unhide plus 'Unhide all'."""
        if not self._hidden:
            return
        titles = {wi.id: wi.title for wi in self._items}
        menu = QMenu(self)
        header = menu.addAction(f"Hidden items ({len(self._hidden)})")
        header.setEnabled(False)
        menu.addSeparator()
        for wid in sorted(self._hidden):
            title = titles.get(wid, "")
            label = f"#{wid}  {title}" if title else f"#{wid}"
            if len(label) > 60:
                label = label[:57] + "…"
            act = menu.addAction(label)
            act.triggered.connect(lambda _=False, w=wid: self._unhide(w))
        menu.addSeparator()
        menu.addAction("Unhide all").triggered.connect(self._unhide_all)
        menu.exec_(QCursor.pos())

    # ------------------------------------------------------------------ #
    #  Theme                                                              #
    # ------------------------------------------------------------------ #

    def refresh_theme(self):
        from app.utils import icons
        t = theme.tokens()
        self._count_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 12px;")
        self._switch_btn.setStyleSheet(theme.btn_pill_accent_qss(_SWITCH_PILL_EXTRA))
        self._switch_btn.setIcon(icons.icon("switch", color=t["accent"], size=16))
        self._status_lbl.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        self._empty_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 13px;")
        self._no_sel_lbl.setStyleSheet(f"color: {t['text_dim2']}; font-size: 13px;")
        self._spinner.set_color(t["accent"])
        self._refresh_btn.setStyleSheet(theme.btn_neutral_qss())
        self._refresh_btn.setIcon(icons.icon("refresh", size=15))
        self._new_btn.setStyleSheet(theme.btn_neutral_qss())
        self._new_btn.setIcon(icons.icon("plus", size=15))
        self._hidden_btn.setStyleSheet(theme.btn_neutral_qss())
        self._hidden_btn.setIcon(icons.icon("minus", size=15))
        self._clear_btn.setStyleSheet(theme.btn_ghost_qss("padding: 4px;"))
        self._clear_btn.setIcon(icons.icon("x", size=13))
        self._open_btn.setStyleSheet(theme.btn_ghost_qss())
        self._open_btn.setIcon(icons.icon("external-link", size=14))
        self._focus_btn.setStyleSheet(theme.btn_ghost_qss())
        self._focus_btn.setIcon(icons.icon("play", size=14))
        self._focus_frame.setStyleSheet(
            f"#focusFrame {{ background: {t['surface2']}; "
            f"border: 1px solid {t['border']}; border-radius: 12px; }}")
        self._focus_lbl.setStyleSheet(
            f"color: {t['text']}; font-size: 12px; background: transparent; border: none;")
        for b in (self._focus_pause_btn, self._focus_stop_btn):
            b.setStyleSheet(theme.btn_ghost_qss("padding: 2px;"))
        if self._focus:
            self._update_focus_ui()   # re-tint the pause/stop icons
        self._save_btn.setStyleSheet(theme.btn_primary_qss("padding: 6px 20px;"))
        self._save_btn.setIcon(icons.icon("check", color="white", size=14))
        self._comment_btn.setStyleSheet(theme.btn_neutral_qss())
        for lab in self._form_labels:
            lab.setStyleSheet(f"color: {t['text_dim']}; font-size: 11px;")
        if self._items:
            self._rebuild()   # column headers embed a theme colour
        if self._current is not None:
            self._detail_id_lbl.setText(
                f"<b>#{self._current.id}</b>  <span style='color:{t['text_dim']}'>"
                f"{self._current.type}</span>")
            cached = self._comments_cache.get(self._current.id)
            if cached is not None:
                self._render_comments(cached)   # rebuild entries with new tokens
