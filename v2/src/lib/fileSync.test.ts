import { expect, test } from "vitest";
import type { TestCase } from "../bindings";
import {
  caseKey,
  changedFields,
  countBy,
  fileName,
  loadWatches,
  ownerPaths,
  withoutFileCases,
  patchWatch,
  syncFromFile,
  syncNotification,
} from "./fileSync";

const tc = (title: string, over: Partial<TestCase> = {}): TestCase => ({
  title,
  steps: [{ action: "do", expected: "ok" }],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  comment: "",
  ...over,
});

test("an added case is appended and reported", () => {
  const r = syncFromFile([tc("A")], [tc("A")], [tc("A"), tc("B")]);
  expect(r.queue.map((c) => c.title)).toEqual(["A", "B"]);
  expect(r.changes).toEqual([
    // `full` is the case AS IT NOW STANDS, so the report can show it end
    // to end rather than only the fragment that moved.
    { kind: "added", key: "t:b", title: "B", fields: [], steps: [], full: tc("B") },
  ]);
});

test("an edited case is replaced in place, keeping its position", () => {
  const before = [tc("A"), tc("B"), tc("C")];
  const edited = tc("B", { steps: [{ action: "do", expected: "ok" }, { action: "then", expected: "done" }] });
  const r = syncFromFile(before, before, [tc("A"), edited, tc("C")]);

  expect(r.queue.map((c) => c.title)).toEqual(["A", "B", "C"]); // no reshuffle
  expect(r.queue[1].steps).toHaveLength(2);
  expect(r.changes).toEqual([
    {
      kind: "changed",
      key: "t:b",
      title: "B",
      fields: [],
      // A step added at the end, reported as the step itself rather than
      // as the words "Steps (1 → 2)".
      steps: [{ index: 1, kind: "added", new: { action: "then", expected: "done" } }],
      // The post-sync case, not the one the queue held before it.
      full: edited,
    },
  ]);
});

test("a case the file dropped is removed", () => {
  const before = [tc("A"), tc("B")];
  const r = syncFromFile(before, before, [tc("A")]);
  expect(r.queue.map((c) => c.title)).toEqual(["A"]);
  expect(countBy(r.changes, "removed")).toBe(1);
});

// The invariant the whole module exists for.
test("cases the file never owned are left completely alone", () => {
  const fromFile = [tc("A")];
  const manual = tc("Typed by hand");
  const r = syncFromFile([...fromFile, manual], fromFile, []); // file emptied

  expect(r.queue).toEqual([manual]);
  expect(countBy(r.changes, "removed")).toBe(1); // only A
  expect(r.changes.every((c) => c.title !== "Typed by hand")).toBe(true);
});

test("an in-app edit that the file overwrites is reported as changed", () => {
  // The user retitled nothing but set a module in-app; the file wins, and
  // the report says so rather than silently reverting it.
  const fileCase = tc("A");
  const editedInApp = tc("A", { module_value: "Payments" });
  const r = syncFromFile([editedInApp], [fileCase], [fileCase]);
  expect(r.changes).toEqual([
    {
      kind: "changed",
      key: "t:a",
      title: "A",
      // Old is what the QUEUE showed, new is what the file just imposed -
      // so this reads as "your in-app Payments is being cleared", which is
      // the thing the user needs to notice.
      fields: [{ name: "Module", old: "Payments", new: "" }],
      steps: [],
      full: fileCase,
    },
  ]);
  expect(r.queue[0].module_value).toBe("");
});

test("an unchanged file produces no changes at all", () => {
  const cases = [tc("A"), tc("B", { update_id: 7 })];
  const r = syncFromFile(cases, cases, cases.map((c) => ({ ...c })));
  expect(r.changes).toEqual([]);
});

test("update_id is the identity when present, so a retitle is one change", () => {
  const before = [tc("Old name", { update_id: 42 })];
  const after = [tc("New name", { update_id: 42 })];
  const r = syncFromFile(before, before, after);
  expect(r.changes).toEqual([
    {
      kind: "changed",
      key: "id:42",
      title: "New name",
      fields: [{ name: "Title", old: "Old name", new: "New name" }],
      steps: [],
      full: after[0],
    },
  ]);
  expect(r.queue).toHaveLength(1);
});

test("without an id, a retitle reads as remove + add - and both are shown", () => {
  const before = [tc("Old name")];
  const r = syncFromFile(before, before, [tc("New name")]);
  expect(countBy(r.changes, "removed")).toBe(1);
  expect(countBy(r.changes, "added")).toBe(1);
  expect(r.queue.map((c) => c.title)).toEqual(["New name"]);
});

test("the returned snapshot is what the NEXT edit compares against", () => {
  const first = syncFromFile([], [], [tc("A")]);
  // Same file re-read with no edits: nothing should move.
  const second = syncFromFile(first.queue, first.snapshot, [tc("A")]);
  expect(second.changes).toEqual([]);
});

test("changedFields reports both sides of every field the queue shows", () => {
  const a = tc("A");
  const b = tc("B", {
    tags: "smoke",
    automation_status: "Planned",
    module_value: "Payments",
    preconditions: "Signed in",
    update_id: 9,
  });
  // Names AND values: "Title changed" still means opening the file to see
  // what it changed to, which is the whole complaint this answers.
  expect(changedFields(a, b)).toEqual([
    { name: "Title", old: "A", new: "B" },
    { name: "Tags", old: "", new: "smoke" },
    { name: "Automation status", old: "Not Automated", new: "Planned" },
    { name: "Module", old: "", new: "Payments" },
    { name: "Preconditions", old: "", new: "Signed in" },
    { name: "Work item id", old: "", new: "9" },
  ]);
});

test("an assistant filling in reviewer notes is a reported change", () => {
  const notes = "## Source\n\nSpec 3.2, AC-4.";
  const before = tc("A");
  const after = tc("A", { reviewer_notes: notes });
  expect(changedFields(before, after)).toEqual([
    { name: "Reviewer notes", old: "", new: notes },
  ]);
});

test("whitespace-only edits are not changes", () => {
  expect(changedFields(tc("A"), tc("  A  ", { tags: "  " }))).toEqual([]);
});

test("caseKey is case-insensitive on titles", () => {
  expect(caseKey(tc("Login Works"))).toBe(caseKey(tc("login works")));
});

test("fileName handles both separators", () => {
  expect(fileName("C:\\tmp\\cases.json")).toBe("cases.json");
  expect(fileName("/home/a/cases.json")).toBe("cases.json");
});

const watch = (path: string, snapshot: TestCase[], comment = "") => ({
  path,
  stamp: "s",
  snapshot,
  comment,
});

test("each case is attributed to the file that contributed it", () => {
  const queue = [tc("A"), tc("B"), tc("typed by hand")];
  const owners = ownerPaths(queue, [
    watch("C:/w/one.json", [tc("A")]),
    watch("C:/w/two.json", [tc("B")]),
  ]);
  expect(owners).toEqual(["C:/w/one.json", "C:/w/two.json", ""]);
});

// A comment has to be written into exactly one file. When two files hold
// the same title the app cannot tell them apart, so the first importer
// keeps it - the same order withoutFileCases uses.
test("a case claimed by two files belongs to the one imported first", () => {
  const owners = ownerPaths(
    [tc("Shared")],
    [watch("C:/w/first.json", [tc("Shared")]), watch("C:/w/second.json", [tc("Shared")])],
  );
  expect(owners).toEqual(["C:/w/first.json"]);
});

test("an updated case is attributed by its work item id, not its title", () => {
  const owners = ownerPaths(
    [tc("Renamed in the app", { update_id: 7 })],
    [watch("C:/w/one.json", [tc("Original title", { update_id: 7 })])],
  );
  expect(owners).toEqual(["C:/w/one.json"]);
});

test("patchWatch touches one file and leaves the rest alone", () => {
  const list = [watch("a.json", [], "old"), watch("b.json", [], "keep")];
  const next = patchWatch(list, "a.json", { comment: "new", stamp: "s2" });
  expect(next[0]).toMatchObject({ comment: "new", stamp: "s2" });
  expect(next[1]).toBe(list[1]);
  // An unknown path is a no-op, not an insert.
  expect(patchWatch(list, "gone.json", { comment: "x" })).toHaveLength(2);
});

test("the notification counts what moved, not what it was called", () => {
  const n = syncNotification("login-cases.json", [
    { kind: "added", key: "t:a", title: "A", fields: [], steps: [], full: tc("A") },
    { kind: "added", key: "t:b", title: "B", fields: [], steps: [], full: tc("B") },
    {
      kind: "changed",
      key: "t:c",
      title: "C",
      fields: [{ name: "Title", old: "c", new: "C" }],
      steps: [],
      full: tc("C"),
    },
  ]);
  expect(n.title).toBe("login-cases.json was updated");
  expect(n.body).toContain("2 added");
  expect(n.body).toContain("1 changed");
  // Nothing removed, so it isn't mentioned at all.
  expect(n.body).not.toContain("removed");
});

test("a warnings-only sync still says something useful", () => {
  expect(syncNotification("a.json", []).body).toBe("The queue is up to date.");
});

// ------------------------------------- two cases can share a title

test("two new cases sharing a title both reach the queue", () => {
  // Reported: the second silently never arrived, and nothing said so.
  const file = [tc("Login works", { tags: "smoke" }), tc("login works ", { tags: "regression" })];
  const r = syncFromFile([], [], file);
  expect(r.queue).toHaveLength(2);
  expect(r.queue.map((c) => c.tags)).toEqual(["smoke", "regression"]);
  expect(countBy(r.changes, "added")).toBe(2);
  // Distinct keys, or the change report collapses them again.
  expect(new Set(r.changes.map((c) => c.key)).size).toBe(2);
});

test("one file case is never written over two queue rows", () => {
  // Reported: both rows became the SAME object, so creating them wrote the
  // same test case to Azure DevOps twice. The file contributed ONE of them;
  // the other was typed by hand and happens to share the title.
  const fromFile = tc("Dup", { tags: "a" });
  const typedByHand = tc("Dup", { tags: "b" });
  const r = syncFromFile([fromFile, typedByHand], [fromFile], [tc("Dup", { tags: "c" })]);
  expect(r.queue).toHaveLength(2);
  expect(r.queue[0]).not.toBe(r.queue[1]);
  // The file's row takes the update; the hand-typed one is left alone.
  expect(r.queue.map((c) => c.tags)).toEqual(["c", "b"]);
});

test("dropping one of two same-titled cases removes exactly one", () => {
  const both = [tc("Dup", { tags: "a" }), tc("Dup", { tags: "b" })];
  const r = syncFromFile(both, both, [tc("Dup", { tags: "a" })]);
  expect(r.queue).toHaveLength(1);
  expect(countBy(r.changes, "removed")).toBe(1);
});

test("ownership and removal agree about which case is which", () => {
  const a = tc("Shared", { tags: "one" });
  const b = tc("Shared", { tags: "two" });
  const watches = [watch("C:/w/first.json", [a, b])];
  expect(ownerPaths([a, b], watches)).toEqual(["C:/w/first.json", "C:/w/first.json"]);
  // Both belong to that file, so dropping it takes both.
  expect(withoutFileCases([a, b], [a, b], [])).toEqual([]);
});

/** The cast that used to stand in loadWatches trusted whatever was in
 * storage, and this shape has already changed once. A half-written or
 * older entry got as far as `snapshot.map(...)` and threw mid-render,
 * taking the Import screen with it. */
test("a malformed watch entry is dropped rather than thrown on later", () => {
  const key = "tcm-v2-watch:acme/42";
  const good = { path: "C:/w/a.json", stamp: "abc", snapshot: [] };
  localStorage.setItem(
    key,
    JSON.stringify([
      good,
      { path: "C:/w/b.json", stamp: "x" }, // no snapshot - the one that threw
      { stamp: "x", snapshot: [] }, // no path
      { path: "", stamp: "x", snapshot: [] }, // empty path
      { path: "C:/w/c.json", stamp: "x", snapshot: {} }, // snapshot not a list
      null,
      "not an object",
    ]),
  );
  expect(loadWatches("acme", 42)).toEqual([good]);

  // The pre-multi-file single-object shape still loads.
  localStorage.setItem(key, JSON.stringify(good));
  expect(loadWatches("acme", 42)).toEqual([good]);

  // And an entry from before general comments existed is still valid.
  const older = { path: "C:/w/d.json", stamp: "y", snapshot: [] };
  localStorage.setItem(key, JSON.stringify([older]));
  expect(loadWatches("acme", 42)).toEqual([older]);
  localStorage.clear();
});

test("the notification calls a fill-of-an-empty-queue a load, not additions", () => {
  const all = ["A", "B"].map((t) => ({
    kind: "added" as const,
    key: `t:${t}`,
    title: t,
    fields: [],
    steps: [],
    full: tc(t),
  }));
  const n = syncNotification("FDP.json", all, true);
  expect(n.title).toBe("FDP.json was loaded");
  expect(n.body).toBe("2 cases loaded into the queue.");
  // The same changes into a NON-empty queue keep the edit wording.
  expect(syncNotification("FDP.json", all).body).toContain("2 added");
});
