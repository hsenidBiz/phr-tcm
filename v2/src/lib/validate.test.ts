import { expect, test } from "vitest";
import type { TestCase } from "../bindings";
import { duplicateWarning, validateCase } from "./validate";

const ok: TestCase = {
  title: "T",
  steps: [{ action: "Do", expected: "" }],
  tags: "",
  automation_status: "Planned",
  module_value: "",
  preconditions: "",
  update_id: null,
};

test("mirrors the Rust is_valid rules", () => {
  expect(validateCase(ok)).toBeNull();
  expect(validateCase({ ...ok, title: "  " })).toMatch(/Title is required/);
  expect(validateCase({ ...ok, title: "X".repeat(300) })).toMatch(/max 255/);
  expect(validateCase({ ...ok, steps: [] })).toMatch(/At least one step/);
  expect(validateCase({ ...ok, steps: [{ action: " ", expected: "x" }] })).toMatch(/Step 1/);
  expect(validateCase({ ...ok, automation_status: "Automated" })).toMatch(/Invalid automation/);
  expect(validateCase({ ...ok, tags: "a, b" })).toMatch(/semicolons/);
});

test("duplicate titles warn only for new cases", () => {
  expect(duplicateWarning(ok, ["t"])).toMatch(/duplicate/);
  expect(duplicateWarning(ok, ["other"])).toBeNull();
  expect(duplicateWarning({ ...ok, update_id: 5 }, ["t"])).toBeNull();
});
