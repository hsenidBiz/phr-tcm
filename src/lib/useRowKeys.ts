/**
 * Arrow keys in a list of rows (backlog A1.8), as the queue already has:
 * Up and Down move to the row above or below, Home and End to the first
 * and last, Shift extends the selection to the row reached, and Space or
 * Enter selects the focused row as a Ctrl-click does.
 *
 * One row at a time is in the Tab order (a roving tabindex), so a long
 * list is one Tab stop. The screen keeps its own click logic: keys reach
 * it through `onSelect`, with the modifiers a click would carry.
 *
 * Each row carries `data-row-key={key}`, `tabIndex={key === tabKey ? 0 : -1}`
 * and calls `onRowFocus(key)` when the row itself takes focus. A windowed
 * list mounts the rows next to the focused one, so Up and Down always find
 * their target; Home and End scroll the list's scroller to its end first.
 */
import { useCallback, useState, type KeyboardEvent } from "react";

export type RowSelectKeys = { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean };

function scrollerOf(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const y = getComputedStyle(p).overflowY;
    if (y === "auto" || y === "scroll") return p;
  }
  return null;
}

function focusRow(list: Element, key: string, edge: "start" | "end" | null): void {
  const find = () => list.querySelector<HTMLElement>(`[data-row-key="${CSS.escape(key)}"]`);
  const el = find();
  if (el) {
    el.focus();
    el.scrollIntoView?.({ block: "nearest" });
    return;
  }
  // Not mounted (the far end of a windowed list): scroll there, then focus.
  const scroller = scrollerOf(list as HTMLElement);
  if (scroller && edge) scroller.scrollTop = edge === "start" ? 0 : scroller.scrollHeight;
  requestAnimationFrame(() => find()?.focus());
}

export function useRowKeys<K extends number | string>({
  order,
  onSelect,
}: {
  /** Every row's key, in the order on screen. */
  order: readonly K[];
  /** Select `key` as a click with these modifiers would. */
  onSelect: (key: K, keys: RowSelectKeys) => void;
}) {
  const [focused, setFocused] = useState<K | null>(null);
  const tabKey = focused != null && order.includes(focused) ? focused : (order[0] ?? null);

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLElement>) => {
      const row = e.target as HTMLElement;
      // Only a row itself: keys typed in a field inside a row are the field's.
      const at = row.dataset?.rowKey == null ? -1 : order.findIndex((k) => String(k) === row.dataset.rowKey);
      if (at < 0) return;
      if (e.key === " " || e.key === "Enter") {
        e.preventDefault();
        onSelect(order[at], { shiftKey: e.shiftKey, ctrlKey: true, metaKey: false });
        return;
      }
      const to =
        e.key === "ArrowDown"
          ? Math.min(order.length - 1, at + 1)
          : e.key === "ArrowUp"
            ? Math.max(0, at - 1)
            : e.key === "Home"
              ? 0
              : e.key === "End"
                ? order.length - 1
                : -1;
      if (to < 0) return;
      e.preventDefault();
      const next = order[to];
      setFocused(next);
      if (e.shiftKey) onSelect(next, { shiftKey: true, ctrlKey: false, metaKey: false });
      focusRow(e.currentTarget, String(next), e.key === "Home" ? "start" : e.key === "End" ? "end" : null);
    },
    [order, onSelect],
  );

  return {
    /** Spread on the element that holds the rows. */
    listProps: { onKeyDown },
    tabKey,
    /** Stable, so memoized rows stay memoized. */
    onRowFocus: setFocused as (key: K) => void,
  };
}
