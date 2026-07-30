import { expect, test } from "vitest";
import type { TestCase, TestCaseFull } from "../bindings";
import { diffCase, diffSummary } from "./caseDiff";

function queued(over: Partial<TestCase> = {}): TestCase {
  return {
    title: "Login works",
    steps: [{ action: "Open", expected: "Shown" }],
    tags: "smoke",
    automation_status: "Planned",
    module_value: "Auth",
    preconditions: "User exists",
    update_id: 201,
    ...over,
  };
}

function current(over: Partial<TestCaseFull> = {}): TestCaseFull {
  return {
    id: 201,
    title: "Login works",
    tags: "smoke",
    automation_status: "Planned",
    steps_xml: "",
    steps: [{ action: "Open", expected: "Shown" }],
    step_ids: ["2"],
    module_value: "Auth",
    preconditions: "User exists",
    ...over,
  };
}

test("identical case is a no-op", () => {
  const d = diffCase(queued(), current());
  expect(d.noop).toBe(true);
  expect(d.fields).toEqual([]);
  expect(d.blankSkipped).toEqual([]);
  expect(diffSummary(d)).toBe("");
});

test("changed fields report old -> new", () => {
  const d = diffCase(
    queued({ title: "Login works v2", automation_status: "Not Automated" }),
    current(),
  );
  expect(d.noop).toBe(false);
  expect(d.fields).toEqual([
    { name: "Title", old: "Login works", new: "Login works v2" },
    { name: "Automation status", old: "Planned", new: "Not Automated" },
  ]);
});

test("blank queued values are skipped, not treated as changes", () => {
  const d = diffCase(
    queued({ tags: "", module_value: "", preconditions: "" }),
    current(),
  );
  expect(d.noop).toBe(true); // blank-skip means nothing changes
  expect(d.blankSkipped).toEqual(["Tags", "Module", "Preconditions"]);
});

test("blank queued value with blank server value is not noted", () => {
  const d = diffCase(queued({ module_value: "" }), current({ module_value: "" }));
  expect(d.blankSkipped).toEqual([]);
});

test("tags compare as an order-insensitive set", () => {
  const d = diffCase(
    queued({ tags: "regression; smoke" }),
    current({ tags: "smoke; regression" }),
  );
  expect(d.noop).toBe(true);
});

test("steps diff positionally: changed, added, removed", () => {
  const changed = diffCase(
    queued({ steps: [{ action: "Open page", expected: "Shown" }] }),
    current(),
  );
  expect(changed.steps.changed).toBe(1);
  expect(changed.noop).toBe(false);
  // Detail carries both sides so the review can render a -/+ diff.
  expect(changed.steps.detail[0]).toEqual({
    index: 0,
    kind: "changed",
    old: { action: "Open", expected: "Shown" },
    new: { action: "Open page", expected: "Shown" },
  });

  const added = diffCase(
    queued({
      steps: [
        { action: "Open", expected: "Shown" },
        { action: "Submit", expected: "" },
      ],
    }),
    current(),
  );
  expect(added.steps.added).toBe(1);

  const removed = diffCase(
    queued({ steps: [] }),
    current(),
  );
  expect(removed.steps.removed).toBe(1);
  // Phrased as an invitation, because the summary IS the button that
  // opens the diff - "1 step change" read as a statement and left the
  // detail undiscovered.
  expect(diffSummary(removed)).toBe("Click to view 1 step changing");
});

test("a project with no Module field is not promised a Module change", () => {
  // bestMatch returns null when the project has no such custom field, and
  // the update then skips it entirely - so showing it as pending would be a
  // change the submit cannot make.
  const q = queued({ module_value: "Payments" });
  const c = current({ module_value: "Auth" });

  const withField = diffCase(q, c, { moduleRef: "Custom.Module", preconditionsRef: "Custom.Pre" });
  expect(withField.fields.map((f) => f.name)).toContain("Module");

  const without = diffCase(q, c, { moduleRef: null, preconditionsRef: null });
  expect(without.fields.map((f) => f.name)).not.toContain("Module");
  // Not blank-skipped either - that would claim a blank is preserving a
  // server value, which is a different statement.
  expect(without.blankSkipped).not.toContain("Module");
});

test("preconditions are hidden the same way, and tags are never affected", () => {
  const q = queued({ preconditions: "Logged in", tags: "smoke" });
  const c = current({ preconditions: "Logged out", tags: "regression" });
  const d = diffCase(q, c, { moduleRef: null, preconditionsRef: null });
  expect(d.fields.map((f) => f.name)).not.toContain("Preconditions");
  // Tags is a built-in field and always writes.
  expect(d.fields.map((f) => f.name)).toContain("Tags");
});
