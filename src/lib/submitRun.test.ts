import { afterEach, expect, test } from "vitest";
import { submitFinished, submitPhaseSnapshot, submitProgressed, submitStarted } from "./submitRun";

afterEach(() => {
  const p = submitPhaseSnapshot();
  if (p) submitFinished(p.run);
});

/// Rust runs one submit at a time and refuses a second. The second attempt
/// used to overwrite the running submit's phase, and then clear it when
/// Rust refused - the running upload's bar vanished mid-write.
test("a second submit cannot take over the running one's progress", () => {
  const first = submitStarted("acme", 42, 10);
  expect(first).toEqual(expect.any(Number));
  submitProgressed(3, 10, "Login works");

  expect(submitStarted("acme", 7, 5)).toBeNull();
  expect(submitStarted("acme", 42, 2)).toBeNull();
  expect(submitPhaseSnapshot()).toMatchObject({ org: "acme", pbiId: 42, done: 3, total: 10 });
});

test("finishing clears only the run that finished", () => {
  const run = submitStarted("acme", 42, 10)!;
  submitFinished(run + 1);
  expect(submitPhaseSnapshot()).toMatchObject({ pbiId: 42 });
  submitFinished(run);
  expect(submitPhaseSnapshot()).toBeNull();
  // And the slot is free again.
  expect(submitStarted("acme", 7, 1)).toEqual(expect.any(Number));
});
