import { afterEach, expect, test } from "vitest";
import { floatRanks, markSuiteFloating } from "./floatingSuites";

afterEach(() => {
  for (const id of [...floatRanks().keys()]) markSuiteFloating(id, false);
});

test("ranks are the order bars started showing, and close up when one goes", () => {
  markSuiteFloating(91, true);
  markSuiteFloating(92, true);
  expect(floatRanks().get(91)).toBe(0);
  expect(floatRanks().get(92)).toBe(1);

  // Hiding the first one lets the second take its place - the sticky bars
  // must never leave a gap, or sit on top of each other.
  markSuiteFloating(91, false);
  expect(floatRanks().has(91)).toBe(false);
  expect(floatRanks().get(92)).toBe(0);
});

test("marking the same suite twice does not move it", () => {
  markSuiteFloating(91, true);
  markSuiteFloating(92, true);
  markSuiteFloating(91, true);
  expect(floatRanks().get(91)).toBe(0);
  expect(floatRanks().get(92)).toBe(1);
});
