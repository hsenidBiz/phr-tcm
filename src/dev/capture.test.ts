import { afterEach, expect, test, vi } from "vitest";
import { isCaptureMode } from "./capture";

const CAPTURE_KEY = "tcm-v2-dev-capture";

afterEach(() => {
  localStorage.clear();
  vi.unstubAllEnvs();
});

test("false when the flag is unset, in a dev build", () => {
  vi.stubEnv("DEV", true);
  expect(isCaptureMode()).toBe(false);
});

test("false when the flag is anything other than \"on\"", () => {
  vi.stubEnv("DEV", true);
  localStorage.setItem(CAPTURE_KEY, "off");
  expect(isCaptureMode()).toBe(false);
});

test("true when the flag is \"on\", in a dev build", () => {
  vi.stubEnv("DEV", true);
  localStorage.setItem(CAPTURE_KEY, "on");
  expect(isCaptureMode()).toBe(true);
});

// The whole point: a release build (`import.meta.env.DEV` false) must never
// honour the flag, whatever localStorage happens to hold.
test("false when DEV is false, even with the flag on", () => {
  vi.stubEnv("DEV", false);
  localStorage.setItem(CAPTURE_KEY, "on");
  expect(isCaptureMode()).toBe(false);
});

test("false when localStorage throws (no storage available)", () => {
  vi.stubEnv("DEV", true);
  const spy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("no storage");
  });
  expect(isCaptureMode()).toBe(false);
  spy.mockRestore();
});
