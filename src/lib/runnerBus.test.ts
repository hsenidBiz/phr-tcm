import { expect, test } from "vitest";
import type { TestPoint } from "../bindings";
import { patchPointRows, type PointRecorded } from "./runnerBus";

const row = (overrides: Partial<TestPoint> = {}): TestPoint => ({
  point_id: 1,
  test_case_id: 100,
  test_case_name: "Login works",
  config_name: "Windows",
  tester: "Avin",
  last_outcome: "",
  last_run_id: null,
  last_result_id: null,
  ...overrides,
});

const recorded = (overrides: Partial<PointRecorded> = {}): PointRecorded => ({
  org: "acme",
  project: "Web",
  planId: 5,
  suiteId: 9,
  testCaseId: 100,
  outcome: "Passed",
  runId: 77,
  resultId: 880,
  ...overrides,
});

test("a recorded outcome lands on the matching case only", () => {
  const rows = [row(), row({ point_id: 2, test_case_id: 200, last_outcome: "Failed" })];
  const next = patchPointRows(rows, recorded())!;
  expect(next[0].last_outcome).toBe("Passed");
  expect(next[0].last_run_id).toBe(77);
  expect(next[0].last_result_id).toBe(880);
  expect(next[1].last_outcome).toBe("Failed"); // untouched
});

test("a reset clears the outcome but keeps the run/result reference", () => {
  const rows = [row({ last_outcome: "Passed", last_run_id: 70, last_result_id: 800 })];
  const next = patchPointRows(rows, recorded({ outcome: "", runId: null, resultId: null }))!;
  expect(next[0].last_outcome).toBe(""); // the never-run bucket
  expect(next[0].last_run_id).toBe(70);
  expect(next[0].last_result_id).toBe(800);
});

test("undefined rows stay undefined (query not loaded yet)", () => {
  expect(patchPointRows(undefined, recorded())).toBeUndefined();
});
