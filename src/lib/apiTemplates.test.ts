import { afterEach, expect, test, vi } from "vitest";

import { apiWritesSnapshot, loadApiWrites, saveApiWrites, subscribeApiWrites } from "./apiTemplates";

afterEach(() => localStorage.clear());

/// Off is the only default a switch like this may have, and it has to be
/// off for both ways a profile can arrive with nothing stored: never
/// configured, and cleared. The same rule as the database write switch
/// (dbServer.ts).
test("proving and running API templates is off until it is switched on", () => {
  expect(loadApiWrites()).toBe(false);
  expect(apiWritesSnapshot()).toBe(false);

  saveApiWrites(true);
  expect(localStorage.getItem("tcm-v2-api-writes")).toBe("1");
  expect(loadApiWrites()).toBe(true);
  expect(apiWritesSnapshot()).toBe(true);

  saveApiWrites(false);
  // Absent rather than "0": a cleared profile then reads exactly the same
  // as a fresh one.
  expect(localStorage.getItem("tcm-v2-api-writes")).toBeNull();
  expect(loadApiWrites()).toBe(false);
});

test("saving notifies subscribers, so App can re-push the bridge context", () => {
  const seen = vi.fn();
  const stop = subscribeApiWrites(seen);

  saveApiWrites(true);
  expect(seen).toHaveBeenCalledTimes(1);

  saveApiWrites(false);
  expect(seen).toHaveBeenCalledTimes(2);

  stop();
  saveApiWrites(true);
  expect(seen).toHaveBeenCalledTimes(2);
});
