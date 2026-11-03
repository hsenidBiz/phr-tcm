import { expect, test } from "vitest";
import type { TestCase, TestCaseFull } from "../bindings";
import { toTestCase, withoutStepFiles } from "./testCaseConvert";

const withFiles = {
  id: 7,
  title: "T",
  tags: "",
  automation_status: "Planned",
  steps: [
    { action: "a", expected: "e", attachments: [{ id: "g-1", name: "x.png", comment: "" }] },
    { action: "b", expected: "" },
  ],
  module_value: "",
  preconditions: "",
  priority: 2,
} as unknown as TestCaseFull;

test("withoutStepFiles drops every step's files and nothing else", () => {
  const out: TestCase = withoutStepFiles(toTestCase(withFiles));
  expect(out.steps).toEqual([
    { action: "a", expected: "e" },
    { action: "b", expected: "" },
  ]);
  expect(out.update_id).toBe(7);
  expect(out.title).toBe("T");
  expect(out.removed_attachments).toBeUndefined();
});
