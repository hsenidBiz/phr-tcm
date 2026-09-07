import { beforeEach, expect, test, vi } from "vitest";

const toast = vi.hoisted(() => ({ info: vi.fn(), warning: vi.fn(), success: vi.fn() }));
vi.mock("sonner", () => ({ toast }));

import { reportUpdateCheck } from "./updateToast";

beforeEach(() => {
  toast.info.mockClear();
  toast.warning.mockClear();
  toast.success.mockClear();
});

const base = { available: null, blocked: null, failed_attempt: null };

test("a newer version is offered, not announced as done", () => {
  reportUpdateCheck({ ...base, available: "1.24.0" });
  expect(toast.info).toHaveBeenCalledTimes(1);
  expect(String(toast.info.mock.calls[0][0])).toContain("1.24.0");
  expect(toast.success).not.toHaveBeenCalled();
});

// The whole reason this module exists: "no check happened" must never be
// reported as "you are on the latest version" - that is a claim the app
// has not checked and cannot make, told to exactly the person most likely
// to be running something stale.
test("a check that could not run is never reported as up to date", () => {
  reportUpdateCheck({ ...base, blocked: "Could not reach the update feed: timeout" });
  expect(toast.warning).toHaveBeenCalledTimes(1);
  expect(String(toast.warning.mock.calls[0][0])).toContain("Could not check for updates");
  expect(toast.success).not.toHaveBeenCalled();
});

test("genuinely up to date says so", () => {
  reportUpdateCheck({ ...base });
  expect(toast.success).toHaveBeenCalledTimes(1);
  expect(toast.warning).not.toHaveBeenCalled();
});
