import { expect, test } from "vitest";
import { pruneCreated } from "./queuePrune";
import type { TestCase } from "../bindings";

const tc = (title: string, over: Partial<TestCase> = {}): TestCase => ({
  title,
  steps: [{ action: "a", expected: "b" }],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  comment: "",
  ...over,
});

const titles = (list: TestCase[]) => list.map((c) => c.title);
const ok = (index: number) => ({ index, action: "created" });
const bad = (index: number) => ({ index, action: "failed" });

test("created drafts go, failed ones stay", () => {
  const sent = [tc("A"), tc("B"), tc("C")];
  const { queue, unmatched } = pruneCreated(sent, sent, [ok(0), bad(1), ok(2)]);
  expect(titles(queue)).toEqual(["B"]);
  expect(unmatched).toBe(0);
});

/**
 * FAILURE 3, the one that shipped in 1.17.3 and was the worst of the four.
 *
 * `keysFor` numbers repeats by position within the list it is given. The
 * previous version numbered the SUCCEEDED SUBSET and compared it against
 * the whole queue, so a same-titled row failing ahead of one that succeeded
 * shifted every later occurrence by one: it deleted the FAILED draft the
 * user still had to fix, and kept the CREATED one - which the next Create
 * would then write to Azure DevOps a second time.
 */
test("a failure ahead of a same-titled success does not shift the numbering", () => {
  const sent = [tc("Login works"), tc("Login works"), tc("Login works")];
  const { queue } = pruneCreated(sent, sent, [bad(0), ok(1), bad(2)]);

  // Two rows left - the two that failed - not "the first one, whichever
  // that was".
  expect(queue).toHaveLength(2);
  expect(queue[0]).toBe(sent[0]);
  expect(queue[1]).toBe(sent[2]);
  expect(queue).not.toContain(sent[1]);
});

test("the two-row form of the same shift", () => {
  const sent = [tc("Login works"), tc("Login works")];
  const { queue } = pruneCreated(sent, sent, [bad(0), ok(1)]);
  expect(queue).toEqual([sent[0]]);
});

/** Repeats that all succeed are all removed, one per occurrence. */
test("every occurrence of a repeated title is removed when all succeed", () => {
  const sent = [tc("Dup"), tc("Dup"), tc("Other")];
  const { queue } = pruneCreated(sent, sent, [ok(0), ok(1), bad(2)]);
  expect(titles(queue)).toEqual(["Other"]);
});

/**
 * FAILURE 2. A watched file saved mid-submit makes syncFromFile hand back a
 * NEW object for the case it changed. Reference matching missed it, the
 * created draft stayed queued, and the next Create made a duplicate.
 */
test("a draft replaced by a mid-submit file sync is still recognised", () => {
  const sent = [tc("A"), tc("B")];
  // Same content, different object - what syncFromFile returns.
  const live = [tc("A"), sent[1]];
  expect(live[0]).not.toBe(sent[0]);

  const { queue, unmatched } = pruneCreated(sent, live, [ok(0), bad(1)]);
  expect(titles(queue)).toEqual(["B"]);
  expect(unmatched).toBe(0);
});

/**
 * FAILURE 1. An index only means something against the list that was sent.
 * A sync that reorders the queue must not cause a row to be removed just
 * for sitting where a created one used to.
 */
test("a reordered queue removes the right row, not the right position", () => {
  const sent = [tc("A"), tc("B")];
  const live = [sent[1], sent[0]]; // swapped
  const { queue } = pruneCreated(sent, live, [ok(0)]); // A was created
  expect(titles(queue)).toEqual(["B"]);
});

/** A case ADDED while the submit ran is not one of the ones sent. */
test("a case added mid-submit survives", () => {
  const sent = [tc("A")];
  const live = [tc("New"), sent[0]];
  const { queue } = pruneCreated(sent, live, [ok(0)]);
  expect(titles(queue)).toEqual(["New"]);
});

/**
 * A created draft that cannot be found is REPORTED, never silently
 * forgotten - it is one Create away from a duplicate work item, and this
 * app cannot delete one.
 */
test("a created draft that is no longer in the queue is reported", () => {
  const sent = [tc("A"), tc("B")];
  const live = [sent[1]]; // A was removed by hand mid-submit
  const { queue, unmatched } = pruneCreated(sent, live, [ok(0), ok(1)]);
  expect(queue).toEqual([]);
  expect(unmatched).toBe(1);
});

/** Updates count as processed too - they are not failures. */
test("updated rows are pruned like created ones", () => {
  const sent = [tc("A", { update_id: 42 })];
  const { queue } = pruneCreated(sent, sent, [{ index: 0, action: "updated" }]);
  expect(queue).toEqual([]);
});

/** Two drafts that differ only by work item id are distinct. */
test("same title, different work item id, are told apart", () => {
  const sent = [tc("Same", { update_id: 1 }), tc("Same", { update_id: 2 })];
  const { queue } = pruneCreated(sent, sent, [bad(0), ok(1)]);
  expect(queue).toEqual([sent[0]]);
});
