import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test, vi } from "vitest";
import {
  extrasHydratedSnapshot,
  extrasUnlockedSnapshot,
  hydrateExtras,
  resetExtrasStore,
  setExtrasUnlocked,
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

// `unlocked` starts false the same way a locked machine reads, so a caller
// that must not act before Rust has answered reads the hydrated flag.
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

// The Extras Easter egg gates no feature any more: unlocking it moves no
// module (lib/modules.ts) - the card it reveals is the only thing it shows.
test("unlocking the extras does not touch the modules store", async () => {
  const { modulesSnapshot, resetModulesStore } = await import("./modules");
  resetModulesStore();
  mockIPC(() => null);
  await setExtrasUnlocked(true);
  expect(modulesSnapshot().size).toBe(0);
});
