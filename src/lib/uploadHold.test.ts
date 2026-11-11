import { afterEach, expect, test } from "vitest";
import type { SubmitItemResult, TestCase } from "../bindings";
import {
  ambiguousRows,
  checkExcludeIds,
  claimsForCheck,
  heldRows,
  holdFromJournal,
  holdFromResults,
  holdSignature,
  loadHold,
  mergeHolds,
  narrowHold,
  reconciledResults,
  saveHold,
  subscribeHold,
  withClaimedFiles,
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
    ids: [901],
    sigs: [holdSignature(sent[1])],
  });
  expect(holdFromResults([res(0, "created", 901), res(2, "failed")], sent, "S")).toBeNull();
});

test("a hold survives a reload, per PBI, and reads back as the same object", () => {
  saveHold("acme", 42, { since: "S", titles: ["B"] });
  const a = loadHold("acme", 42);
  expect(a).toEqual({ since: "S", titles: ["B"], ids: [] });
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
  expect(loadHold("acme", 42)).toEqual({ since: "S", titles: ["B"], ambiguous: ["B"], ids: [] });
});

// ---- final review: a Check must never claim an id that is already
// ---- accounted for - one this upload reported, or one linked to the PBI
// ---- before the upload began. -------------------------------------------

test("the hold records the PBI's earlier cases and every id this upload reported", () => {
  const sent = [tc("A"), tc("B"), tc("C", { update_id: 77 }), tc("D")];
  expect(
    holdFromResults(
      [res(0, "created", 901), res(1, "unknown"), res(2, "updated", 77), res(3, "failed")],
      sent,
      "S",
      [50, 51, 77],
    ),
  ).toEqual({ since: "S", titles: ["B"], ids: [50, 51, 77, 901], sigs: [holdSignature(sent[1])] });
});

test("a hold stored before ids were recorded still loads, with no ids", () => {
  localStorage.setItem("tcm-v2-upload-hold:acme/42", JSON.stringify({ since: "S", titles: ["B"] }));
  expect(loadHold("acme", 42)).toEqual({ since: "S", titles: ["B"], ids: [] });
  localStorage.setItem("tcm-v2-upload-hold:acme/42", JSON.stringify({ since: "S", titles: ["B"], ids: ["x"] }));
  expect(loadHold("acme", 42)).toBeNull();
});

test("a Check excludes the hold's ids and every update id in the queue", () => {
  const queue = [tc("A", { update_id: 901 }), tc("B"), tc("C", { update_id: 300 })];
  expect(checkExcludeIds({ since: "S", titles: ["B"], ids: [50, 901] }, queue)).toEqual([50, 901, 300]);
  expect(checkExcludeIds({ since: "S", titles: ["B"] }, queue)).toEqual([901, 300]);
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

/// Two drafts share a title. Only the second came back unknown, so the
/// hold must mark THAT row - first-come used to mark the first - and keep
/// marking it after a re-sort.
test("a hold marks the same-titled row that was sent, not merely the first one", () => {
  const a = tc("Login works", { steps: [{ action: "Open A", expected: "" }] });
  const b = tc("Login works", { steps: [{ action: "Open B", expected: "" }] });
  const hold = holdFromResults([res(0, "failed"), res(1, "unknown")], [a, b], "S")!;
  expect(heldRows([a, b], hold)).toEqual([false, true]);
  expect(heldRows([b, a], hold)).toEqual([true, false]);
  expect(ambiguousRows([a, b], { ...hold, ambiguous: ["Login works"] })).toEqual([false, true]);
});

/// A hold saved by an older version has no `sigs`; it must still load and
/// mark its rows by title rather than be dropped.
test("a hold stored before signatures existed still marks its rows by title", () => {
  localStorage.setItem("tcm-v2-upload-hold:acme/42", JSON.stringify({ since: "S", titles: ["B"] }));
  const hold = loadHold("acme", 42);
  expect(hold?.sigs).toBeUndefined();
  expect(heldRows([tc("A"), tc("B"), tc("B")], hold)).toEqual([false, true, false]);
});

test("a Check keeps each still-held row's own signature", () => {
  const h = { since: "S", titles: ["A", "B", "B"], ids: [1], sigs: ["sa", "sb1", "sb2"] };
  expect(narrowHold(h, ["B"], [7])).toEqual({
    since: "S",
    titles: ["B", "B"],
    ambiguous: ["B", "B"],
    ids: [1, 7],
    sigs: ["sb1", "sb2"],
  });
  expect(narrowHold(h, [], [7])).toBeNull();
});

test("a claimed create's step files are asked for only when its row carries local paths", () => {
  const withPath = tc("A", { steps: [{ action: "a", expected: "b", attachments: [{ path: "C:/x/a.png", name: "a.png" }] }] });
  const stored = tc("B", { steps: [{ action: "a", expected: "b", attachments: [{ id: "g", name: "b.png" }] }] });
  const queue = [withPath, stored, tc("C")];
  const results = [res(0, "created", 901), res(1, "created", 902), res(2, "created", 903)];
  const claims = claimsForCheck(queue, results);
  expect(claims.map((c) => c.id)).toEqual([901]);
  const merged = withClaimedFiles(results, [
    { id: 901, attached: [{ step: 0, path: "C:/x/a.png", id: "g-1", name: "a.png" }] },
  ]);
  expect(merged[0].attached).toEqual([{ step: 0, path: "C:/x/a.png", id: "g-1", name: "a.png" }]);
  expect(merged[1].attached ?? []).toEqual([]);
});

test("a journal becomes an unknown hold", () => {
  expect(
    holdFromJournal({ organization: "acme", project: "Web", pbi_id: 42, since: "2026-11-11T10:00:00Z", titles: ["A", "B"], sigs: ["sa", "sb"], ids: [7] }),
  ).toEqual({ since: "2026-11-11T10:00:00Z", titles: ["A", "B"], ids: [7], sigs: ["sa", "sb"] });
});

test("a journal whose signatures do not line up has none", () => {
  const h = holdFromJournal({ organization: "acme", project: "Web", pbi_id: 42, since: "s", titles: ["A", "B"], sigs: [], ids: [] });
  expect(h.sigs).toBeUndefined();
  expect(h.ambiguous).toBeUndefined();
});

test("merging takes the earlier since, every id and each row once", () => {
  const existing = { since: "2026-11-11T10:05:00.000Z", titles: ["A"], ids: [1], sigs: ["sa"], ambiguous: ["A"] };
  const add = { since: "2026-11-11T10:00:00Z", titles: ["A", "B"], ids: [7], sigs: ["sa", "sb"] };
  expect(mergeHolds(existing, add)).toEqual({ since: "2026-11-11T10:00:00Z", titles: ["A", "B"], ids: [1, 7], sigs: ["sa", "sb"], ambiguous: ["A"] });
});

test("merging with nothing standing returns the new hold", () => {
  const add = { since: "s", titles: ["A"], ids: [] };
  expect(mergeHolds(null, add)).toBe(add);
});

test("two rows sharing a title stay two", () => {
  expect(
    mergeHolds({ since: "s", titles: ["A", "A"], ids: [], sigs: ["s1", "s2"] }, { since: "s", titles: ["A"], ids: [], sigs: ["s1"] }).titles,
  ).toEqual(["A", "A"]);
});

test("a hold without signatures merges by title", () => {
  const m = mergeHolds({ since: "s", titles: ["A"], ids: [] }, { since: "s", titles: ["A", "B"], ids: [], sigs: ["x", "y"] });
  expect(m.titles).toEqual(["A", "B"]);
  expect(m.sigs).toBeUndefined();
});
