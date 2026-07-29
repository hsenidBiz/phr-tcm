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
  expect(diffSummary(removed)).toBe("1 step change");
});
