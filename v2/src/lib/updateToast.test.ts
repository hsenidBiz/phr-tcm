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

test("the notice copy names the user's next step, not the app's insides", () => {
  const text = `${UPDATES_MOVED.title} ${UPDATES_MOVED.body}`;
  expect(text).toMatch(/Redmine ticket/);
  expect(text).not.toMatch(/\b(API|feed|source|token)\b/i);
});
