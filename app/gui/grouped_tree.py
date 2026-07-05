"""A folder tree that auto-groups test cases by title pattern.

Drop-in companion to the flat ``QListWidget`` on the Edit and Run Tests screens:
when the user turns "Smart Grouping" on, the screen hides its list and shows one
of these instead. Folders come from :func:`app.utils.grouping.group_indices`;
each case row carries an opaque *payload* the host screen supplies (a case dict
for Run Tests, a ``self._cases`` index for Edit), so selection maps straight back
to the host's own model.

Selecting a folder means "all its (visible) cases" — :meth:`selected_payloads`
expands folder selections — so the existing bulk-edit / add-to-session flows work
unchanged whether the user ticks a folder or individual rows.
"""
from PyQt5.QtCore import Qt, pyqtSignal
from PyQt5.QtGui import QBrush
from PyQt5.QtWidgets import QAbstractItemView, QTreeWidget, QTreeWidgetItem

from app.gui.delegates import StatusTintDelegate
from app.utils import icons
from app.utils.grouping import group_indices

# Item data roles (kept clear of Qt's built-ins by offsetting from UserRole).
_PAYLOAD_ROLE = Qt.UserRole          # case rows: the host's opaque payload
_KIND_ROLE = Qt.UserRole + 1         # "group" | "case"
_NAME_ROLE = Qt.UserRole + 2         # group rows: the bare display name


class GroupedCaseTree(QTreeWidget):
    """Two-level tree (folder → cases) built from ``(title, label, payload)``
    entries. Reuses :class:`StatusTintDelegate` so status tints and the shared
    hover behave exactly like the flat lists."""

    # Emitted (with the double-clicked item) so hosts can add/edit on double
    # click without reaching into the tree internals.
    itemActivatedPayload = pyqtSignal(object)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setHeaderHidden(True)
        self.setIndentation(14)
        self.setUniformRowHeights(True)
        self.setAlternatingRowColors(False)
        self.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.setItemDelegate(StatusTintDelegate(self))
        self._entries = []   # [(title, label, payload)]
        self.itemDoubleClicked.connect(
            lambda it, _c: self.itemActivatedPayload.emit(it))

    # -- building ------------------------------------------------------------

    def populate(self, entries):
        """Rebuild the folder tree from ``entries`` (iterable of
        ``(title, label, payload)``). Folders start expanded."""
        self._entries = list(entries)
        self.clear()
        titles = [e[0] for e in self._entries]
        folder_icon = icons.icon("folder", size=14)
        for name, idxs in group_indices(titles):
            display = name or "Ungrouped"
            folder = QTreeWidgetItem(self, [f"{display}  ({len(idxs)})"])
            folder.setIcon(0, folder_icon)
            folder.setData(0, _KIND_ROLE, "group")
            folder.setData(0, _NAME_ROLE, display)
            fnt = folder.font(0)
            fnt.setBold(True)
            folder.setFont(0, fnt)
            for i in idxs:
                _title, label, payload = self._entries[i]
                child = QTreeWidgetItem(folder, [label])
                child.setData(0, _KIND_ROLE, "case")
                child.setData(0, _PAYLOAD_ROLE, payload)
            folder.setExpanded(True)

    # -- selection -----------------------------------------------------------

    def payloads_for_item(self, item):
        """The payload(s) an item represents: a case row → itself; a folder →
        all its currently-visible case rows."""
        if item is None:
            return []
        if item.data(0, _KIND_ROLE) == "group":
            return [item.child(r).data(0, _PAYLOAD_ROLE)
                    for r in range(item.childCount())
                    if not item.child(r).isHidden()]
        payload = item.data(0, _PAYLOAD_ROLE)
        return [] if payload is None else [payload]

    def selected_payloads(self):
        """Every selected case payload, folders expanded to their cases,
        de-duplicated, in top-to-bottom tree order."""
        seen = set()
        out = []
        for item in self._iter_in_order():
            if not item.isSelected():
                continue
            for payload in self.payloads_for_item(item):
                try:
                    key = hash(payload)
                except TypeError:
                    key = id(payload)
                if key not in seen:
                    seen.add(key)
                    out.append(payload)
        return out

    def select_only_payload(self, payload):
        """Select exactly the case row carrying `payload`, clearing any other
        selection. Used to restore a selection after a guarded switch is
        cancelled. No-op if the payload isn't currently in the tree."""
        self.clearSelection()
        for item in self._iter_in_order():
            if (item.data(0, _KIND_ROLE) == "case"
                    and item.data(0, _PAYLOAD_ROLE) == payload):
                item.setSelected(True)
                self.setCurrentItem(item)
                return

    def _iter_in_order(self):
        """Folders then their children, in visual order — so selection results
        read the same way the tree looks."""
        for gi in range(self.topLevelItemCount()):
            folder = self.topLevelItem(gi)
            yield folder
            for r in range(folder.childCount()):
                yield folder.child(r)

    # -- filtering / tinting -------------------------------------------------

    def apply_predicate(self, visible_pred):
        """Hide case rows where ``visible_pred(payload)`` is falsey; hide folders
        left with no visible children; relabel folder counts to ``(visible of
        total)`` when filtered. Returns the total visible case count."""
        total_visible = 0
        for gi in range(self.topLevelItemCount()):
            folder = self.topLevelItem(gi)
            visible = 0
            for r in range(folder.childCount()):
                child = folder.child(r)
                show = bool(visible_pred(child.data(0, _PAYLOAD_ROLE)))
                child.setHidden(not show)
                if show:
                    visible += 1
            folder.setHidden(visible == 0)
            total = folder.childCount()
            name = folder.data(0, _NAME_ROLE)
            folder.setText(
                0, f"{name}  ({total})" if visible == total
                else f"{name}  ({visible} of {total})")
            total_visible += visible
        return total_visible

    def apply_tint(self, tint_fn):
        """Set each case row's background from ``tint_fn(payload) -> QColor|None``
        (None leaves the row untinted). The delegate paints it as a status tint."""
        for item in self._iter_in_order():
            if item.data(0, _KIND_ROLE) != "case":
                continue
            col = tint_fn(item.data(0, _PAYLOAD_ROLE))
            if col is not None:
                item.setBackground(0, QBrush(col))

    def update_label(self, match_pred, new_label):
        """Relabel the first case row whose payload matches ``match_pred`` — used
        after an in-place save so the tree text stays in sync without a rebuild."""
        for item in self._iter_in_order():
            if (item.data(0, _KIND_ROLE) == "case"
                    and match_pred(item.data(0, _PAYLOAD_ROLE))):
                item.setText(0, new_label)
                return
