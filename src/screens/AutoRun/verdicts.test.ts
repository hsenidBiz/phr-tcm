// The result buckets the Auto Run screen filters and counts by. The run
// report counts the same way in Rust; both read the same table, so a
// change to one side that the other does not share fails a suite.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { countBuckets, failingStep, lastResultFor, lastResults, matchesFilter, resultBucket } from "./verdicts";

const FIXTURE = join(
  dirname(fileURLToPath(import.meta.url)),
  "../../../src-tauri/tests/fixtures/verdict_buckets.json",
);
const table = JSON.parse(readFileSync(FIXTURE, "utf-8")) as {
  cases: { verdict: string; proposed: string; bucket: string }[];
};

test("every verdict/proposed pair lands in the bucket the shared table says (the Rust report reads it too)", () => {
  expect(table.cases.length).toBeGreaterThan(10);
  for (const row of table.cases) {
    expect([row, resultBucket(row)]).toEqual([row, row.bucket]);
  }
});

test("a confirmed verdict overrides the proposal", () => {
  expect(resultBucket({ verdict: "Passed", proposed: "Failed" })).toBe("Passed");
  expect(resultBucket({ verdict: "", proposed: "Failed" })).toBe("Failed");
  // A supervised run's case has no `proposed` at all.
  expect(resultBucket({ verdict: "" })).toBe("Not run");
});

test("counts cover every case once, and All matches everything", () => {
  const cases = [
    { verdict: "", proposed: "Passed" },
    { verdict: "Failed", proposed: "Passed" },
    { verdict: "", proposed: "Blocked" },
    { verdict: "", proposed: "" },
    { verdict: "Passed" },
  ];
  expect(countBuckets(cases)).toEqual({ Passed: 2, Failed: 1, Blocked: 1, "Not run": 1 });
  expect(cases.filter((c) => matchesFilter(c, "All"))).toHaveLength(5);
  expect(cases.filter((c) => matchesFilter(c, "Passed"))).toHaveLength(2);
});

// A run as lastResults reads it: when it started and what each case came to.
const run = (started_at: string, ...cases: { case_id: number; verdict: string; proposed?: string }[]) => ({
  started_at,
  cases,
});

test("a case's last result is the one from the newest run that holds it", () => {
  const older = run("1000", { case_id: 1, verdict: "Passed" });
  const newer = run("2000", { case_id: 1, verdict: "Failed" });
  // Either order in the list: the clock decides, not the position.
  expect(lastResults([older, newer]).get(1)).toBe("Failed");
  expect(lastResults([newer, older]).get(1)).toBe("Failed");
});

test("a case missing from the newest run keeps its older run's result", () => {
  const older = run("1000", { case_id: 1, verdict: "Blocked" }, { case_id: 2, verdict: "Passed" });
  const newer = run("2000", { case_id: 2, verdict: "Failed" });
  const last = lastResults([newer, older]);
  expect(last.get(1)).toBe("Blocked");
  expect(last.get(2)).toBe("Failed");
});

test("a confirmed verdict beats the proposal, and no word at all is Not run", () => {
  const last = lastResults([
    run(
      "1000",
      { case_id: 1, verdict: "Passed", proposed: "Failed" },
      { case_id: 2, verdict: "", proposed: "Blocked" },
      { case_id: 3, verdict: "", proposed: "" },
    ),
  ]);
  expect(last.get(1)).toBe("Passed");
  expect(last.get(2)).toBe("Blocked");
  // Neither a verdict nor a proposal: nothing was said about it.
  expect(lastResultFor(last, 3)).toBe("Not run");
});

test("a run stopped before a case does not override an older real result", () => {
  const older = run("1000", { case_id: 1, verdict: "Passed" }, { case_id: 2, verdict: "Failed" });
  // The newer run holds both cases but only ever reached case 2.
  const stopped = run(
    "2000",
    { case_id: 1, verdict: "", proposed: "" },
    { case_id: 2, verdict: "", proposed: "Passed" },
  );
  const last = lastResults([older, stopped]);
  expect(last.get(1)).toBe("Passed");
  expect(last.get(2)).toBe("Passed");
  // No run reached it at all.
  expect(lastResultFor(lastResults([stopped, run("3000", { case_id: 9, verdict: "" })]), 9)).toBe(
    "Not run",
  );
});

test("a case in no run is Not run, and started_at is compared as a number", () => {
  const last = lastResults([
    run("999", { case_id: 1, verdict: "Passed" }),
    // As text "1000" < "999"; as epoch milliseconds it is the newer run.
    run("1000", { case_id: 1, verdict: "Failed" }),
  ]);
  expect(last.get(1)).toBe("Failed");
  expect(lastResultFor(last, 77)).toBe("Not run");
  expect(lastResultFor(last, 1)).toBe("Failed");
  expect(lastResults([]).size).toBe(0);
});

test("the failing step is the first case step with an action that ran and failed", () => {
  const ok = (detail = "ok") => ({ ok: true, detail });
  const bad = (detail = "button not found") => ({ ok: false, detail });
  const skipped = { ok: false, detail: "not run: an earlier step of this case failed" };
  expect(
    failingStep({
      steps: [
        { step_number: 0, outcomes: [ok("signed in")] },
        { step_number: 1, outcomes: [ok()] },
        { step_number: 3, outcomes: [skipped] },
        { step_number: 2, outcomes: [ok(), bad()] },
      ],
    }),
  ).toBe(2);
  // Every step passed, or was only skipped: no failing step.
  expect(failingStep({ steps: [{ step_number: 1, outcomes: [ok()] }, { step_number: 2, outcomes: [skipped] }] })).toBeNull();
  // Blocked before step 1: no step ran at all.
  expect(failingStep({ steps: [] })).toBeNull();
  // The sign-in and the trip to the module are not the case's own steps.
  expect(
    failingStep({
      steps: [
        { step_number: 0, outcomes: [bad("could not sign in")] },
        { step_number: -1, outcomes: [bad("module not found")] },
      ],
    }),
  ).toBeNull();
});
