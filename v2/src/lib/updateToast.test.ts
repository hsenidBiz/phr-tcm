import { beforeEach, expect, test, vi } from "vitest";

const toast = vi.hoisted(() => ({ info: vi.fn(), warning: vi.fn(), success: vi.fn() }));
vi.mock("sonner", () => ({ toast }));

import { reportUpdateCheck, UPDATES_MOVED } from "./updateToast";

beforeEach(() => {
  toast.info.mockClear();
  toast.warning.mockClear();
  toast.success.mockClear();
});

const base = { available: null, blocked: null, failed_attempt: null, no_access: false };

test("no access is told even when there is nothing else to say", () => {
  reportUpdateCheck({ ...base, no_access: true });
  expect(toast.warning).toHaveBeenCalledTimes(1);
  const msg = String(toast.warning.mock.calls[0][0]);
  expect(msg).toContain("PHR-TCM");
  expect(msg).toContain("Redmine");
  // It is not ALSO "you are on the latest version" - that is not known.
  expect(toast.success).not.toHaveBeenCalled();
});

test("an update from the fallback and no access are both told", () => {
  reportUpdateCheck({ ...base, available: "1.23.0", no_access: true });
  expect(toast.info).toHaveBeenCalledTimes(1);
  expect(toast.warning).toHaveBeenCalledTimes(1);
});

// The reason the check failed IS the missing access, so the generic
// "could not check" line would only push the actionable sentence further
// down the toast behind two clauses of ours.
test("a check blocked BY the missing access says it once, not twice", () => {
  reportUpdateCheck({
    ...base,
    blocked: "Could not reach the update feed: you don't have access yet",
    no_access: true,
  });
  expect(toast.warning).toHaveBeenCalledTimes(1);
  const msg = String(toast.warning.mock.calls[0][0]);
  expect(msg).toBe(`${UPDATES_MOVED.title} ${UPDATES_MOVED.body}`);
  expect(msg).not.toMatch(/Could not check for updates/);
});

// A check blocked for any OTHER reason still says so - that path is the
// whole reason `blocked` exists, and losing it would tell someone running
// a stale build nothing at all.
test("a check blocked for an unrelated reason is still reported", () => {
  reportUpdateCheck({ ...base, blocked: "This build does not update itself." });
  expect(toast.warning).toHaveBeenCalledTimes(1);
  expect(String(toast.warning.mock.calls[0][0])).toContain("Could not check for updates");
  expect(toast.success).not.toHaveBeenCalled();
});

test("the notice copy names the user's next step, not the app's insides", () => {
  const text = `${UPDATES_MOVED.title} ${UPDATES_MOVED.body}`;
  expect(text).toMatch(/Redmine ticket/);
  expect(text).not.toMatch(/\b(API|feed|source|token)\b/i);
});
