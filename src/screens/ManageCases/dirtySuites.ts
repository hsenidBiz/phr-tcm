import { useSyncExternalStore } from "react";

/**
 * Which suites hold an unsaved order, in the order they got one.
 *
 * Suite Management can have several suites open at once, and each renders
 * its own sticky Apply order / Reset bar. Two bars pinned to the same
 * corner would cover each other, so each one asks for its rank here and
 * offsets itself by that much. Module scope, not context: the bar needs a
 * number, not a re-render of the whole screen.
 */
const order: number[] = [];
const listeners = new Set<() => void>();
let snapshot = new Map<number, number>();

function rebuild(): void {
  snapshot = new Map(order.map((id, i) => [id, i]));
  for (const l of listeners) l();
}

export function markSuiteDirty(suiteId: number, dirty: boolean): void {
  const at = order.indexOf(suiteId);
  if (dirty) {
    if (at >= 0) return; // already listed: keep its place
    order.push(suiteId);
  } else {
    if (at < 0) return;
    order.splice(at, 1);
  }
  rebuild();
}

/** The current ranks, newest last. Stable between changes, so it is safe
 * as a `useSyncExternalStore` snapshot. */
export function dirtyRanks(): Map<number, number> {
  return snapshot;
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

/** This suite's stacking rank, or null when its order is saved. */
export function useDirtyRank(suiteId: number): number | null {
  const ranks = useSyncExternalStore(subscribe, dirtyRanks, dirtyRanks);
  return ranks.get(suiteId) ?? null;
}
