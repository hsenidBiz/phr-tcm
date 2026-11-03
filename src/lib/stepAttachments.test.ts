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
  const draft: Step = { ...plain, attachments: [{ path: "C:\\a.png", name: "a.png" }] };
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

import { stampSaved } from "./stepAttachments";

const sentCase = (): TestCase =>
  base(
    [
      plain,
      { action: "x", expected: "y", attachments: [{ path: "C:\\shots\\a.png", name: "a.png", size: 5 }, { id: "g1", name: "old.png" }] },
    ],
    ["g9"],
  );
const done = [{ step: 1, path: "C:\\shots\\a.png", id: "gnew", name: "a.png" }];

test("a saved file's path is swapped for its id, keeping its size", () => {
  const out = stampSaved(sentCase(), sentCase(), done, true);
  expect(out.steps[1].attachments).toEqual([{ id: "gnew", name: "a.png", size: 5 }, { id: "g1", name: "old.png" }]);
});

test("removals the save applied are cleared; ones typed since stay", () => {
  const current = { ...sentCase(), removed_attachments: ["g9", "g2"] };
  expect(stampSaved(current, sentCase(), done, true).removed_attachments).toEqual(["g2"]);
  expect(stampSaved(sentCase(), sentCase(), done, true).removed_attachments).toBeUndefined();
});

test("when the save said it could not apply attachment work, removals are kept for the next try", () => {
  expect(stampSaved(sentCase(), sentCase(), [], false).removed_attachments).toEqual(["g9"]);
});
