import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test, vi } from "vitest";
import {
  autoRunVisible,
  extrasHydratedSnapshot,
  extrasUnlockedSnapshot,
  hydrateExtras,
  resetExtrasStore,
  setExtrasUnlocked,
  shouldLeaveAutoRun,
  subscribeExtras,
} from "./extras";

afterEach(() => {
  clearMocks();
  resetExtrasStore();
});

test("hydrating reads the switch from Rust", async () => {
  mockIPC((cmd) => (cmd === "get_extras_unlocked" ? true : undefined));
  await hydrateExtras();
  expect(extrasUnlockedSnapshot()).toBe(true);
});

test("without the app behind it, hydrating leaves it locked", async () => {
  mockIPC(() => {
    throw new Error("no IPC here");
  });
  await hydrateExtras();
  expect(extrasUnlockedSnapshot()).toBe(false);
});

test("setting it saves through Rust first, then tells every subscriber", async () => {
  const saved: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "set_extras_unlocked") saved.push(args);
    return null;
  });
  const seen = vi.fn();
  const off = subscribeExtras(seen);
  await setExtrasUnlocked(true);
  expect(saved).toEqual([{ unlocked: true }]);
  expect(extrasUnlockedSnapshot()).toBe(true);
  expect(seen).toHaveBeenCalledTimes(1);
  off();
});

test("a save Rust refuses changes nothing and says so", async () => {
  mockIPC((cmd) => {
    if (cmd === "set_extras_unlocked") throw "Could not save this setting. The app log in Settings has the details.";
    return null;
  });
  await expect(setExtrasUnlocked(true)).rejects.toThrow("Could not save this setting");
  expect(extrasUnlockedSnapshot()).toBe(false);
});

/// A read that was already in flight when the person unlocked must not put
/// the old value back when it finally answers.
test("a slow read that answers after a save does not undo it", async () => {
  let answer!: (v: boolean) => void;
  const pending = new Promise<boolean>((r) => {
    answer = r;
  });
  mockIPC((cmd) => (cmd === "get_extras_unlocked" ? pending : null));
  const read = hydrateExtras();
  await setExtrasUnlocked(true);
  answer(false);
  await read;
  expect(extrasUnlockedSnapshot()).toBe(true);
});

// Regression for M-1: on a release build, the redirect that steers away
// from Auto Run once it disappears must not fire on the very first render,
// before Rust's answer to get_extras_unlocked comes back - or a saved
// Auto Run tab on an unlocked machine gets bounced to Manual Entry on
// every restart.
test("hydration starts false and flips true once the read settles, success or not", async () => {
  expect(extrasHydratedSnapshot()).toBe(false);
  mockIPC((cmd) => (cmd === "get_extras_unlocked" ? true : undefined));
  await hydrateExtras();
  expect(extrasHydratedSnapshot()).toBe(true);
});

test("hydration flips true even when there is no IPC to answer it", async () => {
  mockIPC(() => {
    throw new Error("no IPC here");
  });
  await hydrateExtras();
  expect(extrasHydratedSnapshot()).toBe(true);
});

test("resetting the store for tests also resets the hydrated flag", async () => {
  mockIPC((cmd) => (cmd === "get_extras_unlocked" ? true : undefined));
  await hydrateExtras();
  expect(extrasHydratedSnapshot()).toBe(true);
  resetExtrasStore();
  expect(extrasHydratedSnapshot()).toBe(false);
});

test("the Auto Run redirect waits for hydration before it fires", () => {
  // Not hydrated yet: never redirect, whatever autoRunShown says right now.
  expect(shouldLeaveAutoRun("autorun", false, false)).toBe(false);
  expect(shouldLeaveAutoRun("autorun", true, false)).toBe(false);
  // Hydrated: redirect only off Auto Run, and only once it is not shown.
  expect(shouldLeaveAutoRun("autorun", false, true)).toBe(true);
  expect(shouldLeaveAutoRun("autorun", true, true)).toBe(false);
  expect(shouldLeaveAutoRun("manual", false, true)).toBe(false);
});

test("a development build shows Auto Run whatever the switch says", () => {
  expect(autoRunVisible()).toBe(true);
});

test("a release build shows Auto Run only once unlocked", async () => {
  vi.stubEnv("DEV", false);
  vi.resetModules();
  const fresh = await import("./extras");
  mockIPC(() => null);
  expect(fresh.autoRunVisible()).toBe(false);
  await fresh.setExtrasUnlocked(true);
  expect(fresh.autoRunVisible()).toBe(true);
  await fresh.setExtrasUnlocked(false);
  expect(fresh.autoRunVisible()).toBe(false);
  vi.unstubAllEnvs();
  vi.resetModules();
});
