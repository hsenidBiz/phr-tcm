// The result buckets the Auto Run screen filters and counts by. The run
// report counts the same way in Rust; both read the same table, so a
// change to one side that the other does not share fails a suite.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { countBuckets, matchesFilter, resultBucket } from "./verdicts";

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
