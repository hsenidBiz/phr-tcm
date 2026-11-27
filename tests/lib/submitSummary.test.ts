import { expect, test } from "vitest";
import type { SubmitItemResult } from "../../src/bindings";
import { summariseSubmit } from "../../src/lib/submitSummary";

const r = (over: Partial<SubmitItemResult> & { index: number }): SubmitItemResult => ({
  title: `Case ${over.index}`,
  action: "updated",
  id: 100 + over.index,
  error: null,
  ...over,
});

test("the headline counts what happened, in the words the buttons use", () => {
  const s = summariseSubmit([
    r({ index: 0, action: "created", id: 900 }),
    r({ index: 1, action: "updated" }),
    r({ index: 2, action: "updated" }),
  ]);
  expect(s.created).toBe(1);
  expect(s.updated).toBe(2);
  expect(s.failed).toBe(0);
  expect(s.headline).toBe("3 test cases uploaded - 1 created, 2 updated");
});

test("one of a kind is singular", () => {
  expect(summariseSubmit([r({ index: 0, action: "created" })]).headline).toBe(
    "1 test case uploaded - 1 created",
  );
});

// A failure is the part someone has to act on, so it is never folded into
// the uploaded count - those cases were NOT uploaded.
test("failures are counted apart from the uploaded total", () => {
  const s = summariseSubmit([
    r({ index: 0, action: "created" }),
    r({ index: 1, action: "failed", id: null, error: "boom" }),
  ]);
  expect(s.created).toBe(1);
  expect(s.failed).toBe(1);
  expect(s.headline).toBe("1 test case uploaded - 1 created, 1 failed");
});

test("a submit where everything failed says so without claiming an upload", () => {
  const s = summariseSubmit([r({ index: 0, action: "failed", id: null, error: "boom" })]);
  expect(s.headline).toBe("Nothing uploaded - 1 failed");
});

// A case that was written and then hit trouble afterwards (the suite link,
// say) exists in Azure DevOps. It counts as uploaded, and its own row
// carries the warning - the headline must not call it a failure.
test("a written case that reported an error afterwards still counts as uploaded", () => {
  const s = summariseSubmit([r({ index: 0, action: "updated", error: "suite link failed" })]);
  expect(s.failed).toBe(0);
  expect(s.updated).toBe(1);
  expect(s.headline).toBe("1 test case uploaded - 1 updated");
});

// "unknown" may or may not exist in Azure DevOps: neither uploaded nor failed.
test("an unknown outcome is counted apart, never as uploaded or failed", () => {
  const s = summariseSubmit([
    r({ index: 0, action: "created" }),
    r({ index: 1, action: "unknown", id: null, error: "Outcome unknown" }),
  ]);
  expect(s.unknown).toBe(1);
  expect(s.failed).toBe(0);
  expect(s.headline).toBe("1 test case uploaded - 1 created, 1 unknown");
});

// 64 cases refused for the same missing field is one line, not 64.
test("identical failure reasons are grouped, most common first", () => {
  const s = summariseSubmit([
    ...Array.from({ length: 64 }, (_, i) => r({ index: i, action: "failed", id: null, error: "Module is required" })),
    r({ index: 64, action: "failed", id: null, error: "Test Type must be one of the allowed values" }),
    r({ index: 65, action: "failed", id: null, error: null }),
    r({ index: 66, action: "created" }),
  ]);
  expect(s.failures).toEqual([
    "64 failed: Module is required",
    "1 failed: Test Type must be one of the allowed values",
    "1 failed: Azure DevOps gave no reason",
  ]);
  expect(summariseSubmit([r({ index: 0, action: "created" })]).failures).toEqual([]);
});
