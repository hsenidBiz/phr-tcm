import { useSyncExternalStore } from "react";

/**
 * Which suites are showing their floating toolbar, in the order they
 * started showing it.
 *
 * Suite Management can have several suites open at once, and each renders
 * its own floating Apply order / Reset / Apply order from files bar - while
 * its order is unsaved, or while its own toolbar has scrolled out of view.
 * Two bars pinned to the same corner would cover each other, so each one
 * asks for its rank here and offsets itself by that much. Module scope, not
 * context: the bar needs a number, not a re-render of the whole screen.
 */
const order: number[] = [];
const listeners = new Set<() => void>();
let snapshot = new Map<number, number>();

function rebuild(): void {
  snapshot = new Map(order.map((id, i) => [id, i]));
  for (const l of listeners) l();
}

export function markSuiteFloating(suiteId: number, floating: boolean): void {
  const at = order.indexOf(suiteId);
  if (floating) {
    if (at >= 0) return; // already listed: keep its place
    order.push(suiteId);
  } else {
    if (at < 0) return;
    order.splice(at, 1);
  }
  rebuild();
}

/** The current ranks, newest last. Stable between changes, so it is safe
 * as a `useSyncExternalStore` snapshot. Read-only to callers: the live
 * `Map` is module state, and only `markSuiteFloating` may mutate it. */
export function floatRanks(): ReadonlyMap<number, number> {
  return snapshot;
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

/** This suite's stacking rank, or null when its bar is not showing. */
export function useFloatRank(suiteId: number): number | null {
  const ranks: ReadonlyMap<number, number> = useSyncExternalStore(subscribe, floatRanks, floatRanks);
  return ranks.get(suiteId) ?? null;
}
