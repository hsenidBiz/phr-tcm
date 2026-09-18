import { afterEach, expect, test } from "vitest";
import type { SubmitItemResult, TestCase } from "../bindings";
import {
  ambiguousRows,
  heldRows,
  holdFromResults,
  loadHold,
  reconciledResults,
  saveHold,
  subscribeHold,
} from "./uploadHold";

afterEach(() => localStorage.clear());

const tc = (title: string, over: Partial<TestCase> = {}): TestCase => ({
  title,
  steps: [{ action: "a", expected: "b" }],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  comment: "",
  ...over,
});
const res = (index: number, action: string, id: number | null = null): SubmitItemResult => ({
  index,
  title: "",
  action,
  id,
  error: null,
});

test("only unknown results become a hold, named by the SENT row's title", () => {
  const sent = [tc("A"), tc("B"), tc("C")];
  expect(holdFromResults([res(0, "created", 901), res(1, "unknown"), res(2, "failed")], sent, "S")).toEqual({
    since: "S",
    titles: ["B"],
  });
  expect(holdFromResults([res(0, "created", 901), res(2, "failed")], sent, "S")).toBeNull();
});

test("a hold survives a reload, per PBI, and reads back as the same object", () => {
  saveHold("acme", 42, { since: "S", titles: ["B"] });
  const a = loadHold("acme", 42);
  expect(a).toEqual({ since: "S", titles: ["B"] });
  // useSyncExternalStore needs a stable snapshot while nothing changed.
  expect(loadHold("acme", 42)).toBe(a);
  expect(localStorage.getItem("tcm-v2-upload-hold:acme/42")).not.toBeNull();
  expect(loadHold("acme", 7)).toBeNull();

  saveHold("acme", 42, null);
  expect(loadHold("acme", 42)).toBeNull();
  expect(localStorage.getItem("tcm-v2-upload-hold:acme/42")).toBeNull();
});

test("saving tells the subscribers", () => {
  let calls = 0;
  const off = subscribeHold(() => {
    calls += 1;
  });
  saveHold("acme", 42, { since: "S", titles: ["B"] });
  saveHold("acme", 42, null);
  off();
  saveHold("acme", 42, { since: "S", titles: ["B"] });
  expect(calls).toBe(2);
});

test("a stored hold that is not a hold reads as none", () => {
  localStorage.setItem("tcm-v2-upload-hold:acme/42", "{nope");
  expect(loadHold("acme", 42)).toBeNull();
  localStorage.setItem("tcm-v2-upload-hold:acme/42", JSON.stringify({ since: "S", titles: [] }));
  expect(loadHold("acme", 42)).toBeNull();
});

test("held rows are create rows, as many per title as the hold names", () => {
  const queue = [tc("B", { update_id: 5 }), tc("B"), tc("B"), tc("C")];
  expect(heldRows(queue, { since: "S", titles: ["B"] })).toEqual([false, true, false, false]);
  expect(heldRows(queue, { since: "S", titles: ["B", "B"] })).toEqual([false, true, true, false]);
  expect(heldRows(queue, null)).toEqual([false, false, false, false]);
});

test("found cases become created results on the held rows, one row each", () => {
  const queue = [tc("X"), tc("B"), tc("B")];
  const held = [false, true, true];
  expect(reconciledResults(queue, held, [{ title: "B", id: 901 }])).toEqual({
    results: [{ index: 1, title: "B", action: "created", id: 901, error: null }],
    orphans: 0,
  });
  expect(
    reconciledResults(queue, held, [
      { title: "B", id: 901 },
      { title: "B", id: 902 },
    ]).results.map((r) => r.index),
  ).toEqual([1, 2]);
  // Found, but its row is gone from the queue: reported, never dropped.
  expect(reconciledResults(queue, held, [{ title: "Gone", id: 5 }])).toEqual({ results: [], orphans: 1 });
});

// ---- deviation from brief (controller ruling): reconcile_upload cannot
// ---- tell "not found" from "ambiguous" - a title with more unclaimed
// ---- matches than rows checked is reported separately, and that row must
// ---- stay held rather than being cleared. -------------------------------

test("a hold can name which of its titles are ambiguous, not merely unknown", () => {
  saveHold("acme", 42, { since: "S", titles: ["B"], ambiguous: ["B"] });
  expect(loadHold("acme", 42)).toEqual({ since: "S", titles: ["B"], ambiguous: ["B"] });
});

test("ambiguousRows marks only the rows named by hold.ambiguous, same rule as heldRows", () => {
  const queue = [tc("B", { update_id: 5 }), tc("B"), tc("B"), tc("C")];
  expect(ambiguousRows(queue, { since: "S", titles: ["B", "B"], ambiguous: ["B"] })).toEqual([
    false,
    true,
    false,
    false,
  ]);
  expect(ambiguousRows(queue, { since: "S", titles: ["B", "B"] })).toEqual([false, false, false, false]);
  expect(ambiguousRows(queue, null)).toEqual([false, false, false, false]);
});
