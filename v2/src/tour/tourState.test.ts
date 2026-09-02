import { afterEach, expect, test } from "vitest";
import {
  markTourDone,
  setTourRunning,
  subscribeTour,
  tourDone,
  tourRunningSnapshot,
} from "./tourState";

afterEach(() => {
  setTourRunning(false);
  localStorage.clear();
});

test("the tour counts as unseen until it is marked done", () => {
  expect(tourDone()).toBe(false);
  markTourDone();
  expect(tourDone()).toBe(true);
});

test("running is off by default and notifies once per change", () => {
  const seen: boolean[] = [];
  const un = subscribeTour(() => seen.push(tourRunningSnapshot()));
  expect(tourRunningSnapshot()).toBe(false);
  setTourRunning(true);
  setTourRunning(true); // same value - no second notification
  setTourRunning(false);
  un();
  setTourRunning(true); // unsubscribed - not seen
  expect(seen).toEqual([true, false]);
});
