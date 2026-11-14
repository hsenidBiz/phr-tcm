/**
 * Click, Ctrl/Cmd-click and Shift-click selection over a list of rows, as
 * View Test Cases, Update Test Cases, Run Tests and the Import Test Cases
 * queue all do it. One implementation, so a rule changes in one place.
 *
 * - A plain click selects that one row (clicking the only selected row again
 *   clears it) - or, with `plainClick: "toggle"`, flips just that row.
 * - Ctrl/Cmd-click flips one row and leaves the rest alone.
 * - Shift-click takes the rows from the anchor to the clicked one, in the
 *   order `order` gives (what is on screen, filtered and grouped). Ctrl+Shift
 *   adds that range to the selection instead of replacing it. A range never
 *   moves the anchor. With no anchor, or an anchor that is no longer in the
 *   order, a Shift-click is an ordinary click and becomes the anchor.
 * - Every click that is not a range moves the anchor to the clicked row.
 *
 * The caller owns the selected set (a kept screen set, a plain useState...)
 * and hands in only its setter: the hook never needs to read it, because
 * every change goes through the setter's updater form. The anchor lives in a
 * ref here, so the returned handlers keep one identity for memoised rows; a
 * screen that keeps its anchor across tab changes passes its state pair as
 * `anchor` and the ref follows it.
 *
 * Keys: `useRowKeys` turns arrow keys and Space into the same `select` call
 * with the modifiers a click would carry. A list with its own key handling
 * uses `extendTo` for Shift+arrow (the selection runs from the anchor to the
 * row reached) and `select` with `toggle` for Space.
 */
import { useCallback, useRef, type Dispatch, type SetStateAction } from "react";

/** What the modifiers of a click ask for. */
export type ClickHow = { range: boolean; toggle: boolean };

/** Shift is a range, Ctrl or Cmd a toggle - read off a mouse event, or the
 * stand-in `useRowKeys` builds. */
export function clickHow(e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }): ClickHow {
  return { range: e.shiftKey, toggle: e.ctrlKey || e.metaKey };
}

export type RowSelectionOptions = {
  /** What a click with no modifier does. `"replace"` selects only that row
   * (and clears it when it was the only one); `"toggle"` flips that row and
   * keeps the others, as a click on a checkbox would. Default `"replace"`. */
  plainClick?: "replace" | "toggle";
  /** What a Shift-click range does to the rows already selected. `"replace"`
   * makes the range the selection (Ctrl+Shift still adds it); `"add"` always
   * adds it. Default `"replace"`. */
  shiftRange?: "replace" | "add";
  /** The rows a plain click judges "the only selected row" among, when the
   * selected set can hold keys this list does not show. Without it the whole
   * set is counted. */
  soleAmong?: readonly unknown[];
};

/** The keys from `from` to `to` inclusive, in `order`, whichever comes first;
 * null when either is not in `order`. */
export function rangeBetween<K>(order: readonly K[], from: K, to: K): K[] | null {
  const a = order.indexOf(from);
  const b = order.indexOf(to);
  if (a < 0 || b < 0) return null;
  return order.slice(Math.min(a, b), Math.max(a, b) + 1);
}

/** `sel` with `key` flipped. */
export function toggleKey<K>(sel: ReadonlySet<K>, key: K): Set<K> {
  const next = new Set(sel);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  return next;
}

/** A plain click: just `key`, or nothing when it was already the only one. */
export function replaceWithKey<K>(sel: ReadonlySet<K>, key: K, among?: readonly unknown[]): Set<K> {
  const count = among ? among.filter((k) => sel.has(k as K)).length : sel.size;
  return count === 1 && sel.has(key) ? new Set() : new Set([key]);
}

/** A group header's tick: every key when any is missing, none when all are
 * already selected. */
export function toggleMany<K>(sel: ReadonlySet<K>, keys: readonly K[]): Set<K> {
  const all = keys.every((k) => sel.has(k));
  const next = new Set(sel);
  for (const k of keys) {
    if (all) next.delete(k);
    else next.add(k);
  }
  return next;
}

/** The selection after a click on `key` that is not a range. */
export function selectionAfterClick<K>(
  sel: ReadonlySet<K>,
  key: K,
  how: ClickHow,
  plainClick: "replace" | "toggle" = "replace",
  among?: readonly unknown[],
): Set<K> {
  return how.toggle || plainClick === "toggle" ? toggleKey(sel, key) : replaceWithKey(sel, key, among);
}

/** The selection after a Shift-click that found its `range`. */
export function selectionAfterRange<K>(
  sel: ReadonlySet<K>,
  range: readonly K[],
  how: ClickHow,
  mode: "replace" | "add" = "replace",
): Set<K> {
  return mode === "add" || how.toggle ? new Set([...sel, ...range]) : new Set(range);
}

export function useRowSelection<K extends number | string>({
  order,
  setSelected,
  anchor: keptAnchor,
  plainClick = "replace",
  shiftRange = "replace",
  soleAmong,
}: {
  /** Every row's key in the order on screen: what a Shift range runs over.
   * Rows the user cannot see should be left out. */
  order: readonly K[];
  setSelected: Dispatch<SetStateAction<Set<K>>>;
  /** The anchor as a state pair, for a screen that keeps it across tab
   * changes. Without it the anchor is held in the hook alone. */
  anchor?: readonly [K | null, Dispatch<SetStateAction<K | null>>];
} & RowSelectionOptions) {
  const anchorRef = useRef<K | null>(null);
  if (keptAnchor) anchorRef.current = keptAnchor[0];

  // Read through a ref so the handlers below never change identity.
  const latest = useRef({ order, setSelected, keptAnchor, plainClick, shiftRange, soleAmong });
  latest.current = { order, setSelected, keptAnchor, plainClick, shiftRange, soleAmong };

  const setAnchor = useCallback((key: K | null) => {
    anchorRef.current = key;
    latest.current.keptAnchor?.[1](key);
  }, []);

  /** A click, or a key press that stands for one, on `key`. */
  const select = useCallback(
    (key: K, how: ClickHow) => {
      const { order, setSelected, plainClick, shiftRange, soleAmong } = latest.current;
      const anchor = anchorRef.current;
      const run = how.range && anchor != null ? rangeBetween(order, anchor, key) : null;
      if (run) {
        setSelected((s) => selectionAfterRange(s, run, how, shiftRange));
        return;
      }
      setSelected((s) => selectionAfterClick(s, key, how, plainClick, soleAmong));
      setAnchor(key);
    },
    [setAnchor],
  );

  /** A group header's tick: select all of `keys`, or clear them when they all
   * are. The first becomes the anchor. */
  const toggleGroup = useCallback(
    (keys: readonly K[]) => {
      if (keys.length === 0) return;
      latest.current.setSelected((s) => toggleMany(s, keys));
      setAnchor(keys[0]);
    },
    [setAnchor],
  );

  /** Select exactly `keys` (Select all). The first becomes the anchor. */
  const selectOnly = useCallback(
    (keys: readonly K[]) => {
      if (keys.length === 0) return;
      latest.current.setSelected(new Set(keys));
      setAnchor(keys[0]);
    },
    [setAnchor],
  );

  /** Shift plus an arrow key: the selection runs from the anchor to `to`.
   * With no anchor yet, the row the key was pressed in (`from`) is it. */
  const extendTo = useCallback(
    (from: K, to: K) => {
      if (anchorRef.current == null) setAnchor(from);
      select(to, { range: true, toggle: false });
    },
    [select, setAnchor],
  );

  return { select, toggleGroup, selectOnly, extendTo, setAnchor };
}
