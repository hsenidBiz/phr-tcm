import { expect, test } from "vitest";
import { keepUploaded } from "./queueUploaded";
import { keysFor } from "./fileSync";
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
const ids = (list: TestCase[]) => list.map((c) => c.update_id);
const made = (index: number, id: number) => ({ index, action: "created", id });
const updated = (index: number) => ({ index, action: "updated", id: null });
const bad = (index: number) => ({ index, action: "failed", id: null });

/** Nothing leaves the queue: the user removes rows when they are done. */
test("every row stays, created ones gain their new work item id", () => {
  const sent = [tc("A"), tc("B"), tc("C", { update_id: 7 })];
  const { queue, unmatched, uploadedIds } = keepUploaded(sent, sent, [made(0, 901), bad(1), updated(2)]);
  expect(titles(queue)).toEqual(["A", "B", "C"]);
  // A kept created row WITHOUT its id is one Upload away from a duplicate.
  expect(ids(queue)).toEqual([901, null, 7]);
  expect(unmatched).toBe(0);
  expect([...uploadedIds].sort()).toEqual([7, 901]);
});

/** The failed rows, keyed against the queue as it now stands - the one the
 * rows are rendered from - so the ring lands on the row that failed. */
test("failed keys are numbered over the stamped queue", () => {
  // A created "Login works" ahead of a failed one: the created row's key
  // becomes id:901, so the failed row is now the FIRST t:login works.
  // Keys numbered over the sent list would say #2 and ring nothing.
  const sent = [tc("Login works"), tc("Login works")];
  const { queue, failed } = keepUploaded(sent, sent, [made(0, 901), bad(1)]);
  const keys = keysFor(queue);
  expect([...failed]).toEqual([keys[1]]);
});

/**
 * The aliasing case that shipped in 1.17.3, now in stamping form: numbering
 * the SUCCEEDED subset shifted every later occurrence, so the id landed on
 * a failed draft and the created one kept no id.
 */
test("a failure ahead of a same-titled success does not shift the numbering", () => {
  const sent = [tc("Login works"), tc("Login works"), tc("Login works")];
  const { queue } = keepUploaded(sent, sent, [bad(0), made(1, 901), bad(2)]);
  expect(ids(queue)).toEqual([null, 901, null]);
});

test("every occurrence of a repeated title gets its own id", () => {
  const sent = [tc("Dup"), tc("Dup"), tc("Other")];
  const { queue } = keepUploaded(sent, sent, [made(0, 1), made(1, 2), bad(2)]);
  expect(ids(queue)).toEqual([1, 2, null]);
});

/** A watched file saved mid-upload hands back a NEW object for the case it
 * changed. It is still the case that was created, and still needs the id. */
test("a draft replaced by a mid-upload file sync is still recognised", () => {
  const sent = [tc("A"), tc("B")];
  const live = [tc("A"), sent[1]];
  expect(live[0]).not.toBe(sent[0]);
  const { queue, unmatched } = keepUploaded(sent, live, [made(0, 901), bad(1)]);
  expect(ids(queue)).toEqual([901, null]);
  expect(unmatched).toBe(0);
});

/** An index only means something against the list that was sent. */
test("a reordered queue stamps the right row, not the right position", () => {
  const sent = [tc("A"), tc("B")];
  const live = [sent[1], sent[0]];
  const { queue } = keepUploaded(sent, live, [made(0, 901)]);
  expect(titles(queue)).toEqual(["B", "A"]);
  expect(ids(queue)).toEqual([null, 901]);
});

test("a case added mid-upload is left exactly as it was", () => {
  const sent = [tc("A")];
  const added = tc("New");
  const live = [added, sent[0]];
  const { queue } = keepUploaded(sent, live, [made(0, 901)]);
  expect(queue[0]).toBe(added);
  expect(ids(queue)).toEqual([null, 901]);
});

/** A created case whose row cannot be found is REPORTED: if it is still in
 * the queue under another title, it has no id and would be created again. */
test("a created case with no row to stamp is reported", () => {
  const sent = [tc("A"), tc("B")];
  const live = [sent[1]]; // A was removed by hand mid-upload
  const { queue, unmatched } = keepUploaded(sent, live, [made(0, 901), made(1, 902)]);
  expect(ids(queue)).toEqual([902]);
  expect(unmatched).toBe(1);
});

test("a created result with no id is reported, never silently kept as new", () => {
  const sent = [tc("A")];
  const { queue, unmatched } = keepUploaded(sent, sent, [{ index: 0, action: "created", id: null }]);
  expect(ids(queue)).toEqual([null]);
  expect(unmatched).toBe(1);
});

test("same title, different work item id, are told apart", () => {
  const sent = [tc("Same", { update_id: 1 }), tc("Same", { update_id: 2 })];
  const { failed, uploadedIds } = keepUploaded(sent, sent, [bad(0), updated(1)]);
  expect([...failed]).toEqual(["id:1"]);
  expect([...uploadedIds]).toEqual([2]);
});

test("rows are untouched objects when nothing about them changed", () => {
  const sent = [tc("A", { update_id: 5 }), tc("B")];
  const { queue } = keepUploaded(sent, sent, [updated(0), bad(1)]);
  expect(queue[0]).toBe(sent[0]);
  expect(queue[1]).toBe(sent[1]);
});

test("a result index with no matching sent row is ignored rather than crashing", () => {
  const sent = [tc("A")];
  const { queue, failed, unmatched } = keepUploaded(sent, sent, [bad(7)]);
  expect(queue).toEqual(sent);
  expect(failed.size).toBe(0);
  expect(unmatched).toBe(0);
});
