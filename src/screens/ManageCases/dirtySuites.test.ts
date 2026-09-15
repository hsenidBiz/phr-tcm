import { afterEach, expect, test } from "vitest";
import { dirtyRanks, markSuiteDirty } from "./dirtySuites";

afterEach(() => {
  for (const id of [...dirtyRanks().keys()]) markSuiteDirty(id, false);
});

test("ranks are the order suites became dirty, and close up when one is saved", () => {
  markSuiteDirty(91, true);
  markSuiteDirty(92, true);
  expect(dirtyRanks().get(91)).toBe(0);
  expect(dirtyRanks().get(92)).toBe(1);

  // Saving the first one lets the second take its place - the sticky bars
  // must never leave a gap, or sit on top of each other.
  markSuiteDirty(91, false);
  expect(dirtyRanks().has(91)).toBe(false);
  expect(dirtyRanks().get(92)).toBe(0);
});

test("marking the same suite twice does not move it", () => {
  markSuiteDirty(91, true);
  markSuiteDirty(92, true);
  markSuiteDirty(91, true);
  expect(dirtyRanks().get(91)).toBe(0);
  expect(dirtyRanks().get(92)).toBe(1);
});
