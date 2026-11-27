import { expect, test } from "vitest";
import type { RuleProblem } from "../../src/bindings";
import { fieldRuleSummary, rowFieldProblems, ruleInputs } from "../../src/lib/fieldRules";
import { keepUploaded } from "../../src/lib/queueUploaded";
import type { TestCase } from "../../src/bindings";

const problem = (index: number, over: Partial<RuleProblem> = {}): RuleProblem => ({
  index,
  reference_name: "Custom.Module",
  field: "Module",
  kind: "required",
  message: "Module is required for a Test Case in this project - pick one.",
  ...over,
});

test("the summary counts the cases per field and kind", () => {
  expect(fieldRuleSummary([problem(0), problem(1), problem(2)])).toEqual([
    "3 cases need a Module before they can be sent",
  ]);
  expect(fieldRuleSummary([problem(4)])).toEqual(["1 case needs a Module before it can be sent"]);
  expect(
    fieldRuleSummary([
      problem(0),
      problem(0, { reference_name: "Custom.Type", field: "Test Type", kind: "not_allowed" }),
      problem(1, { reference_name: "Custom.Type", field: "Test Type", kind: "not_allowed" }),
    ]),
  ).toEqual([
    "1 case needs a Module before it can be sent",
    "2 cases have a Test Type that is not one of the allowed values",
  ]);
});

test("a row with two problems reads both, and a check that found none marks nothing", () => {
  const rows = rowFieldProblems({ checked: true, problems: [problem(1), problem(1, { message: "Tags is required." })] }, 3);
  expect(rows).toEqual([null, "Module is required for a Test Case in this project - pick one. Tags is required.", null]);
  expect(rowFieldProblems(null, 2)).toEqual([null, null]);
  expect(rowFieldProblems({ checked: false, problems: [] }, 2)).toEqual([null, null]);
  // An index the queue no longer has is ignored, not written past the end.
  expect(rowFieldProblems({ checked: true, problems: [problem(9)] }, 2)).toEqual([null, null]);
});

test("the rules are asked again for a changed module, not for a changed step", () => {
  const tc = (module_value: string, action = "Open"): TestCase => ({
    title: "A",
    steps: [{ action, expected: "ok" }],
    tags: "",
    automation_status: "Not Automated",
    module_value,
    preconditions: "",
    update_id: null,
  });
  expect(ruleInputs([tc("", "Open")])).toBe(ruleInputs([tc("", "Click")]));
  expect(ruleInputs([tc("")])).not.toBe(ruleInputs([tc("Payroll")]));
});

test("a failed row keeps the reason Azure DevOps gave, under the row's own key", () => {
  const sent: TestCase[] = [
    { title: "A", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null },
    { title: "B", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null },
  ];
  const kept = keepUploaded(sent, sent, [
    { index: 0, action: "failed", error: "Module is required" },
    { index: 1, action: "created", id: 9 },
  ]);
  expect(kept.failed.size).toBe(1);
  expect([...kept.failedReasons.values()]).toEqual(["Module is required"]);
});
