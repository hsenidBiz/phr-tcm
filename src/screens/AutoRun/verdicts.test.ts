// The result buckets the Auto Run screen filters and counts by. The run
// report counts the same way in Rust; both read the same table, so a
// change to one side that the other does not share fails a suite.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { countBuckets, lastResultFor, lastResults, matchesFilter, resultBucket } from "./verdicts";

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
  expect(last.get(3)).toBe("Not run");
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
