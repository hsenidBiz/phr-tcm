import { expect, test } from "vitest";
import type { SubmitItemResult, TestCase } from "../bindings";
import { failedKeys, summariseSubmit } from "./submitSummary";

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

// --- which queue rows stay behind, and why they must be found by key ----

const tc = (title: string, id?: number): TestCase =>
  ({
    title,
    update_id: id ?? null,
    steps: [{ action: "a", expected: "b" }],
    tags: "",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
  }) as unknown as TestCase;

test("failed rows are identified by key, numbered over the whole sent list", () => {
  const sent = [tc("Login works"), tc("Login works"), tc("Logout works")];
  // The SECOND "Login works" failed. Matching by title alone cannot tell
  // the two apart, which is why this goes through the same occurrence
  // numbering the prune uses.
  const keys = failedKeys(sent, [
    r({ index: 0, action: "created", id: 1 }),
    r({ index: 1, action: "failed", id: null, error: "boom" }),
    r({ index: 2, action: "created", id: 3 }),
  ]);
  expect(keys).toEqual(new Set(["t:login works#2"]));
});

test("nothing failed means nothing to mark", () => {
  expect(failedKeys([tc("A")], [r({ index: 0, action: "created", id: 1 })]).size).toBe(0);
});

test("a result index with no matching sent row is ignored rather than crashing", () => {
  expect(failedKeys([tc("A")], [r({ index: 7, action: "failed", id: null })]).size).toBe(0);
});
