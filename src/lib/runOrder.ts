// Run order: pure reordering helpers shared by Run Tests, the runner and
// the Execution order modal, plus a tester's own order for one suite on this
// machine (never written to Azure DevOps - see design doc §4.3/§4.4) and
// the cross-window event that keeps Run Tests and the runner in step when
// the runner saves a new My order (design doc §5.2).

import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { RunOrderCase } from "../bindings";

export type OrderView = "suggested" | "spec" | "mine";
export type OrderKey = { org: string; planId: number; suiteId: number };

/** `order` kept where its ids are still in `spec`, duplicates dropped, then
 * every id of `spec` missing from it, in spec order. A stale order never
 * blocks a run and never hides a case (design doc §4.4). */
export function reconcile(order: readonly number[], spec: readonly number[]): number[] {
  const specIds = new Set(spec);
  const seen = new Set<number>();
  const kept: number[] = [];
  for (const id of order) {
    if (specIds.has(id) && !seen.has(id)) {
      kept.push(id);
      seen.add(id);
    }
  }
  for (const id of spec) {
    if (!seen.has(id)) {
      kept.push(id);
      seen.add(id);
    }
  }
  return kept;
}

/** `id` moved to directly after `afterId` (no-op if either is missing or
 * equal). */
export function moveAfter(order: readonly number[], id: number, afterId: number): number[] {
  if (id === afterId || !order.includes(id) || !order.includes(afterId)) {
    return [...order];
  }
  const without = order.filter((x) => x !== id);
  const afterIdx = without.indexOf(afterId);
  return [...without.slice(0, afterIdx + 1), id, ...without.slice(afterIdx + 1)];
}

/** After position `idx`, the UNMARKED cases are re-sorted by `rank` into the
 * slots unmarked cases held; marked cases and everything at or before idx
 * stay where they are. Ids not in `rank` keep their relative order at the
 * end of the unmarked slots (used by the runner to follow a My order saved
 * in Run Tests - design doc §5.2). */
export function resortUpcoming(
  list: readonly number[],
  idx: number,
  isMarked: (id: number) => boolean,
  rank: readonly number[],
): number[] {
  const result = [...list];
  const slots: number[] = [];
  const unmarked: number[] = [];
  for (let i = idx + 1; i < list.length; i++) {
    if (!isMarked(list[i])) {
      slots.push(i);
      unmarked.push(list[i]);
    }
  }
  const rankOf = new Map<number, number>();
  rank.forEach((id, i) => rankOf.set(id, i));
  // Array#sort is stable, so ids missing from rank (both Infinity) keep
  // their original relative order without a manual tie-breaker.
  const sorted = [...unmarked].sort(
    (a, b) => (rankOf.get(a) ?? Infinity) - (rankOf.get(b) ?? Infinity),
  );
  slots.forEach((pos, i) => {
    result[pos] = sorted[i];
  });
  return result;
}

/** The cases a suggested run order is saved with, in `ids` order. A case's
 * group is the started-from draft's area when the start was a tester-order
 * file (`fileGroups`), else the group it had in the saved file (`saved`),
 * else none - an empty group is never written (design doc §4.2). */
export function runOrderPayload(
  ids: readonly number[],
  saved: readonly { id: number; group?: string | null }[] | null,
  fileGroups?: ReadonlyMap<number, string>,
): RunOrderCase[] {
  const savedGroup = new Map(saved?.map((c) => [c.id, c.group]) ?? []);
  return ids.map((id) => {
    const group = fileGroups?.get(id) ?? savedGroup.get(id);
    return group ? { id, group } : { id };
  });
}

const orderKey = (k: OrderKey) => `tcm-v2-run-order:${k.org}/${k.planId}/${k.suiteId}`;
const viewKey = (k: OrderKey) => `tcm-v2-run-order-view:${k.org}/${k.planId}/${k.suiteId}`;

/** A tester's own order for this suite, on this machine only (design doc
 * §4.3). `null` when there isn't one, it can't be parsed, or storage is
 * unavailable - callers fall back to spec/suggested order, same as any
 * other stale-or-missing case. */
export function loadMyOrder(k: OrderKey): number[] | null {
  try {
    const raw = localStorage.getItem(orderKey(k));
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) && parsed.every((x) => typeof x === "number") ? parsed : null;
  } catch {
    return null;
  }
}

export function saveMyOrder(k: OrderKey, ids: readonly number[]): void {
  try {
    localStorage.setItem(orderKey(k), JSON.stringify(ids));
  } catch {
    // storage unavailable - My order is simply not offered next load
  }
  emitMyOrderChanged(k);
}

export function loadOrderView(k: OrderKey): OrderView | null {
  try {
    const raw = localStorage.getItem(viewKey(k));
    return raw === "suggested" || raw === "spec" || raw === "mine" ? raw : null;
  } catch {
    return null;
  }
}

export function saveOrderView(k: OrderKey, v: OrderView): void {
  try {
    localStorage.setItem(viewKey(k), v);
  } catch {
    // storage unavailable - the view simply isn't remembered next load
  }
}

export const MY_ORDER_EVENT = "run-order:changed";

/** Fire-and-forget Tauri event (as src/lib/runnerBus.ts does): a lost
 * notification self-heals the next time either window reads My order. */
export function emitMyOrderChanged(k: OrderKey): void {
  emit(MY_ORDER_EVENT, k).catch(() => {});
}

export function onMyOrderChanged(cb: (k: OrderKey) => void): Promise<UnlistenFn> {
  return listen<OrderKey>(MY_ORDER_EVENT, (e) => cb(e.payload));
}
