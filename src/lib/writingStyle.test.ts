import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test } from "vitest";
import { migrateOldTrialSwitch } from "./writingStyle";

const OLD_KEY = "tcm-v2-risk-tiered-guide";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function mocks(saved: { enabled: boolean; text: string }, onSave: (style: unknown) => unknown = () => null) {
  const saves: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "writing_style_get") return saved;
    if (cmd === "writing_style_save") {
      const style = (args as { style: unknown }).style;
      saves.push(style);
      return onSave(style);
    }
  });
  return saves;
}

test("an old trial switch that was on saves the style switched on once, then drops the key", async () => {
  localStorage.setItem(OLD_KEY, "1");
  const saves = mocks({ enabled: false, text: "## Trial rules" });

  expect(await migrateOldTrialSwitch()).toBe(true);
  expect(saves).toEqual([{ enabled: true, text: "## Trial rules" }]);
  expect(localStorage.getItem(OLD_KEY)).toBeNull();

  // Once: the key is gone, so a second start saves nothing.
  expect(await migrateOldTrialSwitch()).toBe(false);
  expect(saves).toHaveLength(1);
});

test("no old trial switch: nothing is read or saved", async () => {
  const saves = mocks({ enabled: false, text: "## Trial rules" });
  expect(await migrateOldTrialSwitch()).toBe(false);
  expect(saves).toEqual([]);
});

test("an old key that is not on is dropped without saving", async () => {
  localStorage.setItem(OLD_KEY, "0");
  const saves = mocks({ enabled: false, text: "## Trial rules" });
  expect(await migrateOldTrialSwitch()).toBe(false);
  expect(saves).toEqual([]);
  expect(localStorage.getItem(OLD_KEY)).toBeNull();
});

test("a style already on is left as it is, and the key still goes", async () => {
  localStorage.setItem(OLD_KEY, "1");
  const saves = mocks({ enabled: true, text: "## Mine" });
  expect(await migrateOldTrialSwitch()).toBe(true);
  expect(saves).toEqual([]);
  expect(localStorage.getItem(OLD_KEY)).toBeNull();
});

test("a failed save keeps the key, so the next start tries again", async () => {
  localStorage.setItem(OLD_KEY, "1");
  mocks({ enabled: false, text: "## Trial rules" }, () => {
    throw "The writing style could not be saved. Settings → Logs has the details.";
  });
  expect(await migrateOldTrialSwitch()).toBe(false);
  expect(localStorage.getItem(OLD_KEY)).toBe("1");
});
