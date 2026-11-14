import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import type { TestCase } from "../../bindings";
import { buildAreaGroups, visibleOrder } from "../../lib/areaGroups";
import { usePersistedStringSet } from "../../lib/collapsedGroups";

/** Group by area: a per-machine view choice, off until switched on. Only
 * how the Queue is DRAWN - the queue array, its upload order and every
 * index a row is keyed by stay exactly as they are. */
export function useAreaGrouping() {
  const [areaGrouped, setAreaGrouped] = useState(() => {
    try {
      return localStorage.getItem("tcm-v2-queue-group") === "on";
    } catch {
      return false;
    }
  });
  const [foldedAreas, toggleFoldedArea] = usePersistedStringSet("tcm-v2-queue-collapsed");
  const changeGrouped = (on: boolean) => {
    setAreaGrouped(on);
    try {
      localStorage.setItem("tcm-v2-queue-group", on ? "on" : "off");
    } catch {
      // storage unavailable -> session-only
    }
  };
  return { areaGrouped, changeGrouped, foldedAreas, toggleFoldedArea };
}

/** Which rows are selected, which row holds the grid's one Tab stop, and the
 * keyboard and click handling that moves both. */
export function useQueueSelection({
  queue,
  areaGrouped,
  foldedAreas,
  editingIdx,
}: {
  queue: TestCase[];
  areaGrouped: boolean;
  foldedAreas: ReadonlySet<string>;
  editingIdx: number | null;
}) {
  // Selection is by POSITION, like Power Rename: drafts have no id, and a
  // bulk edit can make two share a title, so the title is not an identity
  // that survives the operation being applied. Any change in queue LENGTH
  // drops the selection - after a removal or a file sync the indices point
  // at different cases, and a stale selection silently bulk-edits the
  // wrong rows.
  const [selected, setSelected] = useState<Set<number>>(new Set());
  // Positions are only meaningful while the rows exist. When the queue
  // shrinks beneath a selection (Clear queue, an import that replaces the
  // queue, a single removal) the indices past its end are dropped.
  useEffect(() => {
    setSelected((prev) => {
      if (![...prev].some((i) => i >= queue.length)) return prev;
      return new Set([...prev].filter((i) => i < queue.length));
    });
  }, [queue.length]);
  // The shift-click anchor is only ever read inside toggleSelect, so it
  // lives in a ref: that keeps the callback's identity stable, which is
  // what lets the rows below stay memoised.
  const selAnchor = useRef<number | null>(null);
  useEffect(() => {
    setSelected(new Set());
    selAnchor.current = null;
  }, [queue.length]);

  // Group by area: the tree, and the rows it leaves on screen top to
  // bottom. A Shift-click range runs over what the user SEES, so while the
  // Queue is grouped it follows this order (folded groups' cases left out)
  // rather than raw queue positions. Held in a ref for the same reason the
  // anchor is: toggleSelect has to stay stable for the memoised rows.
  const areaGroups = useMemo(() => (areaGrouped ? buildAreaGroups(queue) : null), [areaGrouped, queue]);
  const shownOrder = useRef<number[] | null>(null);
  // Every row on screen, top to bottom, grouped or flat: what the arrow
  // keys walk. The row being edited counts even inside a folded group,
  // because it is still drawn there.
  const rowsOnScreen = useMemo(
    () => (areaGroups ? visibleOrder(areaGroups, foldedAreas, editingIdx) : queue.map((_, i) => i)),
    [areaGroups, foldedAreas, editingIdx, queue],
  );
  const rowOrder = useRef<number[]>([]);
  useEffect(() => {
    shownOrder.current = areaGroups ? rowsOnScreen : null;
    rowOrder.current = rowsOnScreen;
  }, [areaGroups, rowsOnScreen]);

  // The grid's one Tab stop (a roving tabindex): the row focus was last in,
  // while it is on screen, else the first row. The arrow keys move it.
  const rowIds = useId();
  const [focusRow, setFocusRow] = useState<number | null>(null);
  useEffect(() => setFocusRow(null), [queue.length]);
  const tabRow = focusRow != null && rowsOnScreen.includes(focusRow) ? focusRow : (rowsOnScreen[0] ?? null);
  const onRowFocus = useCallback((i: number) => setFocusRow(i), []);

  // Selection works the way Update Test Cases' does: a click selects that
  // one case (and clears it if it was the only one), Ctrl/Cmd toggles one,
  // Shift takes the range from the anchor in the order on screen, and
  // Ctrl+Shift adds that range. A range leaves the anchor where it was.
  const toggleSelect = useCallback((i: number, how: { range: boolean; toggle: boolean }) => {
    const anchor = selAnchor.current;
    if (how.range && anchor != null) {
      const order = shownOrder.current;
      let range: number[] | null = null;
      if (order) {
        const from = order.indexOf(anchor);
        const to = order.indexOf(i);
        if (from >= 0 && to >= 0) range = order.slice(Math.min(from, to), Math.max(from, to) + 1);
      } else {
        range = [];
        for (let k = Math.min(anchor, i); k <= Math.max(anchor, i); k++) range.push(k);
      }
      if (range) {
        const r = range;
        setSelected((s) => (how.toggle ? new Set([...s, ...r]) : new Set(r)));
        return;
      }
    }
    if (how.toggle) {
      setSelected((s) => {
        const next = new Set(s);
        if (next.has(i)) next.delete(i);
        else next.add(i);
        return next;
      });
    } else {
      setSelected((s) => (s.size === 1 && s.has(i) ? new Set() : new Set([i])));
    }
    selAnchor.current = i;
  }, []);

  // Up and Down move to the row above or below in the order on screen
  // (folded groups skipped), Home and End to the first and last. With
  // Shift held the selection runs from the anchor to the row reached.
  const onNavigate = useCallback(
    (i: number, key: "ArrowUp" | "ArrowDown" | "Home" | "End", extend: boolean) => {
      const order = rowOrder.current;
      const at = order.indexOf(i);
      if (order.length === 0) return;
      const to =
        key === "Home"
          ? 0
          : key === "End"
            ? order.length - 1
            : Math.min(order.length - 1, Math.max(0, (at < 0 ? 0 : at) + (key === "ArrowDown" ? 1 : -1)));
      const next = order[to];
      if (extend) {
        if (selAnchor.current == null) selAnchor.current = i;
        toggleSelect(next, { range: true, toggle: false });
      }
      setFocusRow(next);
      document.getElementById(`${rowIds}-${next}-row`)?.focus();
    },
    [toggleSelect, rowIds],
  );

  return {
    selected,
    setSelected,
    selAnchor,
    areaGroups,
    rowsOnScreen,
    rowIds,
    tabRow,
    onRowFocus,
    toggleSelect,
    onNavigate,
  };
}
