import { expect, test } from "vitest";
import type { TestCase } from "../bindings";
import { VAGUE_EXPECTED, duplicateWarning, qualityWarnings, titleSimilarity, validateCase } from "./validate";

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

test("a Shared Steps reference needs no action", () => {
  expect(validateCase({ ...ok, steps: [{ action: "", expected: "", shared: 812 }] })).toBeNull();
  expect(validateCase({ ...ok, steps: [{ action: "", expected: "" }] })).toMatch(/Step 1/);
});

test("duplicate titles warn only for new cases", () => {
  expect(duplicateWarning(ok, [{ id: 1, title: "t" }])).toMatch(/duplicate/);
  expect(duplicateWarning(ok, [{ id: 1, title: "other" }])).toBeNull();
  expect(duplicateWarning({ ...ok, update_id: 5 }, [{ id: 1, title: "t" }])).toBeNull();
});

/// The same case in different words gets flagged; the reviewer decides.
test("a reworded title is named as a near-duplicate, with the case it resembles", () => {
  const tc = { ...ok, title: "Manager can approve overtime request" };
  const warn = duplicateWarning(tc, [
    { id: 144398, title: "Approve overtime request as manager" },
    { id: 144399, title: "Export payslip as PDF" },
  ]);
  expect(warn).toMatch(/#144398/);
  expect(warn).toMatch(/"Approve overtime request as manager"/);
  expect(warn).toMatch(/\d+% similar/);
  expect(warn).toMatch(/set its TestCaseID/);
});

/// The one-word-apart siblings are LEGITIMATELY different cases. A warning
/// that fires on these trains people to ignore it - staying silent here is
/// the whole calibration of the threshold.
test("genuinely distinct sibling cases stay silent", () => {
  const existing = [{ id: 1, title: "Approve overtime request" }];
  expect(duplicateWarning({ ...ok, title: "Reject overtime request" }, existing)).toBeNull();
  expect(
    duplicateWarning({ ...ok, title: "Export payslip as CSV" }, [
      { id: 2, title: "Export payslip as PDF" },
    ]),
  ).toBeNull();
});

test("similarity is order-insensitive and ignores filler words", () => {
  expect(titleSimilarity("Manager can approve overtime", "Approve overtime as manager")).toBe(1);
  expect(titleSimilarity("Approve overtime request", "Reject overtime request")).toBeLessThan(0.8);
  // Short titles are compared whole - filler removal must not hollow a
  // three-word title into a set that agrees with everything.
  expect(titleSimilarity("Can it be", "Should that then")).toBe(0);
});

/// An explicit update never warns, however similar - similar to the case
/// it IS updating is the expected state.
test("near-duplicate stays quiet for explicit updates and picks the best match", () => {
  const tc = { ...ok, title: "Manager can approve overtime request", update_id: 7 };
  expect(duplicateWarning(tc, [{ id: 1, title: "Approve overtime request as manager" }])).toBeNull();

  const best = duplicateWarning({ ...ok, title: "Manager can approve overtime request" }, [
    { id: 10, title: "Manager approve overtime request flow" },
    { id: 11, title: "Approve overtime request as manager" },
  ]);
  // Both clear the bar; the message must name the closer one, not the first.
  expect(best).toMatch(/#11/);
});

const good: TestCase = { ...ok, steps: [{ action: "Click Save", expected: "The Save button is enabled" }] };

test("quality: a clean case has no warnings", () => {
  expect(qualityWarnings(good)).toEqual([]);
});

test("quality: a step with an action but no expected result is named", () => {
  const tc = {
    ...good,
    steps: [good.steps[0], { action: "Click Next", expected: "  " }],
  };
  expect(qualityWarnings(tc)).toEqual(["Step 2 has no expected result"]);
});

test("quality: an empty action is left to validateCase, not double-reported", () => {
  const tc = { ...good, steps: [{ action: " ", expected: "" }] };
  expect(qualityWarnings(tc)).toEqual([]);
  expect(qualityWarnings({ ...good, steps: [{ action: "", expected: "", shared: 7 }] })).toEqual([]);
});

test("quality: a vague expected result says nothing to check", () => {
  const tc = { ...good, steps: [good.steps[0], { action: "Save", expected: "Works." }] };
  expect(qualityWarnings(tc)).toEqual([`Step 2's expected result says nothing to check: "Works."`]);
  for (const v of VAGUE_EXPECTED) {
    const w = qualityWarnings({ ...good, steps: [{ action: "Do", expected: ` ${v.toUpperCase()}. ` }] });
    expect(w).toHaveLength(1);
  }
  expect(qualityWarnings({ ...good, steps: [{ action: "Do", expected: "It works and the total updates" }] })).toEqual([]);
});

test("quality: an earlier state with no preconditions is flagged", () => {
  const tc = { ...good, steps: [{ action: "Open the record from the above", expected: "Shown" }] };
  expect(qualityWarnings(tc)).toEqual([
    "Step 1 refers to an earlier state but the case has no preconditions",
  ]);
  expect(qualityWarnings({ ...tc, preconditions: "A record exists" })).toEqual([]);
  expect(qualityWarnings({ ...good, steps: [{ action: "Open it as above", expected: "Shown" }] })).toHaveLength(1);
  expect(qualityWarnings({ ...good, steps: [{ action: "Repeat the previous step", expected: "Shown" }] })).toHaveLength(1);
  // Only step 1 is checked.
  expect(
    qualityWarnings({ ...good, steps: [good.steps[0], { action: "Redo the above", expected: "Shown" }] }),
  ).toEqual([]);
});
