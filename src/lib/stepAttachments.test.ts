import { expect, test } from "vitest";
import type { Step, TestCase } from "../bindings";
import { withSteps } from "./stepAttachments";

const base = (steps: Step[], removed?: string[]): TestCase => ({
  title: "t",
  steps,
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  removed_attachments: removed,
});

const held: Step = { action: "a", expected: "b", attachments: [{ id: "g1", name: "old.png" }] };
const plain: Step = { action: "c", expected: "d" };

test("deleting a step that holds an existing attachment records it for removal", () => {
  const out = withSteps(base([held, plain]), [plain]);
  expect(out.removed_attachments).toEqual(["g1"]);
  expect(out.steps).toEqual([plain]);
});

test("dropping a not-yet-uploaded chip records nothing", () => {
  const draft: Step = { ...plain, attachments: [{ path: "C:\a.png", name: "a.png" }] };
  const out = withSteps(base([draft]), [{ ...draft, attachments: [] }]);
  expect(out.removed_attachments ?? []).toEqual([]);
});

test("reordering keeps the removal list as it was", () => {
  const out = withSteps(base([held, plain], ["g9"]), [plain, held]);
  expect(out.removed_attachments).toEqual(["g9"]);
});

test("the same id is never listed twice, and one carried again is dropped from the list", () => {
  const once = withSteps(base([held, plain]), [plain]);
  const twice = withSteps({ ...once, steps: [held, plain] }, [plain]);
  expect(twice.removed_attachments).toEqual(["g1"]);
  const back = withSteps(once, [held, plain]);
  expect(back.removed_attachments ?? []).toEqual([]);
});
