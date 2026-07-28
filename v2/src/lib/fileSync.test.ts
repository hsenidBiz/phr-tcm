import { expect, test } from "vitest";
import type { TestCase } from "../bindings";
import {
  caseKey,
  changedFields,
  countBy,
  fileName,
  ownerPaths,
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
  expect(r.changes).toEqual([{ kind: "added", key: "t:b", title: "B", fields: [] }]);
});

test("an edited case is replaced in place, keeping its position", () => {
  const before = [tc("A"), tc("B"), tc("C")];
  const edited = tc("B", { steps: [{ action: "do", expected: "ok" }, { action: "then", expected: "done" }] });
  const r = syncFromFile(before, before, [tc("A"), edited, tc("C")]);

  expect(r.queue.map((c) => c.title)).toEqual(["A", "B", "C"]); // no reshuffle
  expect(r.queue[1].steps).toHaveLength(2);
  expect(r.changes).toEqual([
    { kind: "changed", key: "t:b", title: "B", fields: ["Steps (1 → 2)"] },
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
  expect(r.changes).toEqual([{ kind: "changed", key: "t:a", title: "A", fields: ["Module"] }]);
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
    { kind: "changed", key: "id:42", title: "New name", fields: ["Title"] },
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

test("changedFields names every field the queue shows", () => {
  const a = tc("A");
  const b = tc("B", {
    tags: "smoke",
    automation_status: "Planned",
    module_value: "Payments",
    preconditions: "Signed in",
    update_id: 9,
  });
  expect(changedFields(a, b)).toEqual([
    "Title",
    "Tags",
    "Automation status",
    "Module",
    "Preconditions",
    "Work item id",
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
    { kind: "added", key: "t:a", title: "A", fields: [] },
    { kind: "added", key: "t:b", title: "B", fields: [] },
    { kind: "changed", key: "t:c", title: "C", fields: ["Title"] },
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
