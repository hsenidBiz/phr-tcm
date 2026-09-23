import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, expect, test, vi } from "vitest";
import {
  clearMyOrder,
  emitMyOrderChanged,
  loadMyOrder,
  loadOrderView,
  moveAfter,
  MY_ORDER_EVENT,
  onMyOrderChanged,
  reconcile,
  resortUpcoming,
  saveMyOrder,
  saveOrderView,
  type OrderKey,
} from "./runOrder";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const key: OrderKey = { org: "acme", planId: 5, suiteId: 91 };

// --- reconcile ---------------------------------------------------------

test("reconcile: cases added since go at the end, in spec order", () => {
  expect(reconcile([1, 2], [1, 2, 3, 4])).toEqual([1, 2, 3, 4]);
});

test("reconcile: cases no longer in the suite are dropped", () => {
  expect(reconcile([1, 2, 3], [1, 3])).toEqual([1, 3]);
});

test("reconcile: duplicate ids in the stored order are dropped", () => {
  expect(reconcile([1, 1, 2], [1, 2])).toEqual([1, 2]);
});

test("reconcile: empty order falls back to spec order", () => {
  expect(reconcile([], [3, 1, 2])).toEqual([3, 1, 2]);
});

test("reconcile: empty spec reconciles to empty", () => {
  expect(reconcile([1, 2, 3], [])).toEqual([]);
});

// --- moveAfter -----------------------------------------------------------

test("moveAfter: moves an id to the middle of the list", () => {
  expect(moveAfter([1, 2, 3, 4], 4, 1)).toEqual([1, 4, 2, 3]);
});

test("moveAfter: moves an id to the end of the list", () => {
  expect(moveAfter([1, 2, 3, 4], 1, 4)).toEqual([2, 3, 4, 1]);
});

test("moveAfter: a missing id is a no-op", () => {
  expect(moveAfter([1, 2, 3], 99, 1)).toEqual([1, 2, 3]);
  expect(moveAfter([1, 2, 3], 1, 99)).toEqual([1, 2, 3]);
});

test("moveAfter: moving an id after itself is a no-op", () => {
  expect(moveAfter([1, 2, 3], 2, 2)).toEqual([1, 2, 3]);
});

// --- resortUpcoming --------------------------------------------------------

test("resortUpcoming: the prefix at or before idx never moves", () => {
  const list = [1, 2, 3, 4, 5];
  const result = resortUpcoming(list, 1, () => false, [5, 4, 3]);
  expect(result[0]).toBe(1);
  expect(result[1]).toBe(2);
});

test("resortUpcoming: marked cases after idx stay in their slot", () => {
  // 3 is marked (already run) - it must not move even though rank puts 5 first.
  const result = resortUpcoming([1, 2, 3, 4, 5], 1, (id) => id === 3, [5, 4]);
  expect(result).toEqual([1, 2, 3, 5, 4]);
});

test("resortUpcoming: unmarked cases are re-sorted by rank into the slots unmarked cases held", () => {
  const result = resortUpcoming([1, 2, 3, 4, 5], 1, () => false, [5, 3, 4]);
  expect(result).toEqual([1, 2, 5, 3, 4]);
});

test("resortUpcoming: ids missing from rank keep their relative order at the end", () => {
  // 3 and 5 aren't in rank; they keep 3-before-5 (their original order),
  // placed after the ranked 4.
  const result = resortUpcoming([1, 2, 3, 4, 5], 1, () => false, [4]);
  expect(result).toEqual([1, 2, 4, 3, 5]);
});

// --- My order / view round-trip -------------------------------------------

test("My order round-trips under the exact key", () => {
  expect(loadMyOrder(key)).toBeNull();
  saveMyOrder(key, [3, 1, 2]);
  expect(localStorage.getItem("tcm-v2-run-order:acme/5/91")).toBe(JSON.stringify([3, 1, 2]));
  expect(loadMyOrder(key)).toEqual([3, 1, 2]);
});

test("clearMyOrder removes the stored order", () => {
  saveMyOrder(key, [1, 2]);
  clearMyOrder(key);
  expect(loadMyOrder(key)).toBeNull();
});

test("the order view round-trips under the exact key", () => {
  expect(loadOrderView(key)).toBeNull();
  saveOrderView(key, "mine");
  expect(localStorage.getItem("tcm-v2-run-order-view:acme/5/91")).toBe("mine");
  expect(loadOrderView(key)).toBe("mine");
});

test("a storage failure on load never throws - it reads as absent", () => {
  const spy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("storage disabled");
  });
  expect(loadMyOrder(key)).toBeNull();
  expect(loadOrderView(key)).toBeNull();
  spy.mockRestore();
});

test("a storage failure on save never throws", () => {
  const spy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("quota exceeded");
  });
  expect(() => saveMyOrder(key, [1, 2])).not.toThrow();
  expect(() => saveOrderView(key, "spec")).not.toThrow();
  spy.mockRestore();
});

// --- cross-window change event ---------------------------------------------

test("saving My order emits the change event with the key", async () => {
  mockIPC(() => null, { shouldMockEvents: true });
  const cb = vi.fn();
  const unlisten = await onMyOrderChanged(cb);
  saveMyOrder(key, [1, 2]);
  await vi.waitFor(() => expect(cb).toHaveBeenCalledWith(key));
  unlisten();
});

test("clearing My order emits the change event with the key", async () => {
  mockIPC(() => null, { shouldMockEvents: true });
  const cb = vi.fn();
  const unlisten = await onMyOrderChanged(cb);
  clearMyOrder(key);
  await vi.waitFor(() => expect(cb).toHaveBeenCalledWith(key));
  unlisten();
});

test("emitMyOrderChanged never throws outside Tauri", () => {
  expect(() => emitMyOrderChanged(key)).not.toThrow();
});

test("the event name is the one every window agrees on", () => {
  expect(MY_ORDER_EVENT).toBe("run-order:changed");
});
