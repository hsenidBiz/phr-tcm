import { expect, test } from "vitest";
import type { SubmitItemResult, TestCase, TestCaseFull } from "../bindings";
import { diffCase } from "./caseDiff";
import { hasTesterNotes, testerNotes } from "./testerNotes";

const tc = (title: string, over: Partial<TestCase> = {}): TestCase => ({
  title,
  steps: [
    { action: "Open the page", expected: "Page shown" },
    { action: "Click Save", expected: "" },
  ],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  ...over,
});

const server = (id: number, c: TestCase, over: Partial<TestCaseFull> = {}): TestCaseFull => ({
  id,
  title: c.title,
  tags: c.tags,
  automation_status: c.automation_status,
  steps_xml: "",
  steps: c.steps,
  step_ids: c.steps.map((_, i) => String(i + 2)),
  module_value: c.module_value,
  preconditions: c.preconditions,
  ...over,
});

const r = (index: number, action: string, id: number | null, title = ""): SubmitItemResult => ({
  index,
  title,
  action,
  id,
  error: action === "failed" ? "boom" : null,
});

test("an updated case leads with its id, then says what changed", () => {
  const queued = tc("Save a draft", {
    update_id: 154650,
    steps: [
      { action: "Open the page", expected: "Page shown with the grid" },
      { action: "Click Save", expected: "" },
      { action: "Reload", expected: "Draft is kept" },
    ],
  });
  const before = server(154650, tc("Save draft", { update_id: 154650 }));
  const diff = diffCase(queued, before);

  const text = testerNotes({ pbiId: 42, sent: [queued], results: [r(0, "updated", 154650)], diffs: [diff] });
  expect(text).toBe(
    [
      "Test case changes for PBI #42",
      "",
      "Updated (1)",
      "#154650  Save a draft",
      '  - Title: "Save draft" -> "Save a draft"',
      "  - Step 1 changed",
      '      Expected: "Page shown" -> "Page shown with the grid"',
      '  - Step 3 added: "Reload"',
      '      Expected: "Draft is kept"',
    ].join("\n"),
  );
});

test("a removed step and an emptied field read plainly", () => {
  const queued = tc("Login", { update_id: 7, steps: [{ action: "Open the page", expected: "Page shown" }], preconditions: "" });
  const before = server(7, tc("Login", { preconditions: "Signed out" }), { tags: "smoke" });
  const diff = diffCase({ ...queued, tags: "regression" }, before);
  const text = testerNotes({ pbiId: 1, sent: [{ ...queued, tags: "regression" }], results: [r(0, "updated", 7)], diffs: [diff] });
  expect(text).toContain('  - Tags: "smoke" -> "regression"');
  expect(text).toContain('  - Step 2 removed: "Click Save"');
  // A blank Preconditions leaves the server's value alone - no change to report.
  expect(text).not.toContain("Preconditions");
});

/// The step-type repair is not something a tester re-runs a case for.
test("a case whose only change is its step types is left out", () => {
  const queued = tc("Typed", { update_id: 9 });
  const xml = `<steps><step id="2" type="ActionStep"></step><step id="3" type="ActionStep"></step></steps>`;
  const diff = diffCase(queued, server(9, queued, { steps_xml: xml }));
  expect(diff.noop).toBe(false);
  expect(diff.steps.retyped).toBe(1);

  const text = testerNotes({ pbiId: 1, sent: [queued], results: [r(0, "updated", 9)], diffs: [diff] });
  expect(text).not.toContain("#9");
  expect(text).toContain("No test case changes to report.");
  expect(hasTesterNotes([r(0, "updated", 9)], [diff])).toBe(false);
});

test("a step-type repair riding on a real change reports only the real change", () => {
  const queued = tc("Typed", { update_id: 9, title: "Typed and renamed" });
  const xml = `<steps><step id="2" type="ActionStep"></step><step id="3" type="ActionStep"></step></steps>`;
  const diff = diffCase(queued, server(9, tc("Typed"), { steps_xml: xml }));
  const text = testerNotes({ pbiId: 1, sent: [queued], results: [r(0, "updated", 9)], diffs: [diff] });
  expect(text).toContain('  - Title: "Typed" -> "Typed and renamed"');
  expect(text).not.toMatch(/step type/i);
});

test("new cases are listed by id and title; failed ones are not listed", () => {
  const sent = [tc("Brand new"), tc("Broken")];
  const text = testerNotes({
    pbiId: 42,
    sent,
    results: [r(0, "created", 155001), r(1, "failed", null)],
    diffs: [null, null],
  });
  expect(text).toBe(["Test case changes for PBI #42", "", "New (1)", "#155001  Brand new"].join("\n"));
  expect(text).not.toContain("Broken");
});

test("an update whose before-state could not be read is still listed, saying so", () => {
  const sent = [tc("Unknown", { update_id: 88 })];
  const text = testerNotes({ pbiId: 3, sent, results: [r(0, "updated", 88)], diffs: [null] });
  expect(text).toContain("#88  Unknown\n  - Updated (details unavailable)");
  expect(hasTesterNotes([r(0, "updated", 88)], [null])).toBe(true);
});

test("multi-line values are flattened onto one line", () => {
  const queued = tc("Steps", { update_id: 5, steps: [{ action: "Open\nthe page", expected: "Page shown" }, { action: "Click Save", expected: "" }] });
  const diff = diffCase(queued, server(5, tc("Steps")));
  const text = testerNotes({ pbiId: 1, sent: [queued], results: [r(0, "updated", 5)], diffs: [diff] });
  expect(text).toContain('      Action: "Open the page" -> "Open / the page"');
});

test("updated and new counts are both given, updates first", () => {
  const up = tc("Old one", { update_id: 1, title: "Old one, renamed" });
  const diff = diffCase(up, server(1, tc("Old one")));
  const text = testerNotes({
    pbiId: 2,
    sent: [tc("Fresh"), up],
    results: [r(0, "created", 10), r(1, "updated", 1)],
    diffs: [null, diff],
  });
  expect(text.indexOf("Updated (1)")).toBeLessThan(text.indexOf("New (1)"));
  expect(hasTesterNotes([r(0, "created", 10)], [null])).toBe(true);
});
