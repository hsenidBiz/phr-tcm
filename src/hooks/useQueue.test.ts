import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../lib/toast";
import { saveDraftQueue } from "./useQueue";

const tc = {
  title: "Case",
  steps: [],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
};

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

test("a queue that cannot be saved warns once, not on every change", () => {
  const warn = vi.spyOn(toast, "warning").mockImplementation(() => "" as never);
  const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new DOMException("full", "QuotaExceededError");
  });
  expect(saveDraftQueue("acme", 1, [tc])).toBe(false);
  expect(saveDraftQueue("acme", 1, [tc, tc])).toBe(false);
  expect(warn).toHaveBeenCalledTimes(1);

  // Saving works again: the next failure warns again.
  setItem.mockRestore();
  expect(saveDraftQueue("acme", 1, [tc])).toBe(true);
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new DOMException("full", "QuotaExceededError");
  });
  saveDraftQueue("acme", 1, [tc]);
  expect(warn).toHaveBeenCalledTimes(2);
});
