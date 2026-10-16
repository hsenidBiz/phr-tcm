// The sidebar's "newly enabled" marks: what a person's own switch-on marks,
// what clears a mark, and that the marks outlive a restart.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test, vi } from "vitest";
import { ALL_MODULES, hydrateModules, resetModulesStore, setModule, type Module } from "./modules";
import {
  NEW_NAV_KEY,
  clearNewNav,
  isNewNav,
  moduleNavItem,
  newNavSnapshot,
  noteModuleSwitch,
  pruneNewNav,
  resetNewNavForTest,
} from "./newNav";

afterEach(() => {
  clearMocks();
  localStorage.clear();
  resetModulesStore();
  resetNewNavForTest();
  vi.restoreAllMocks();
});

const set = (...m: Module[]) => new Set<Module>(m);

test("each module marks the row it reveals", () => {
  expect(moduleNavItem("db_access")).toBe("ai");
  expect(moduleNavItem("writing_style")).toBe("ai");
  expect(moduleNavItem("auto_run")).toBe("autorun");
  expect(moduleNavItem("api_templates")).toBe("apitemplates");
  expect(moduleNavItem("playwright")).toBe("playwright");
  expect(moduleNavItem("crystal_reports")).toBe("crystal-reports");
});

test("switching a module on marks its row, and only that row", () => {
  noteModuleSwitch(set("db_access"), set("db_access", "auto_run"));
  expect(isNewNav("autorun")).toBe(true);
  expect(isNewNav("ai")).toBe(false);
  expect(isNewNav("apitemplates")).toBe(false);

  noteModuleSwitch(set("db_access", "auto_run"), set("db_access", "auto_run", "playwright"));
  expect(isNewNav("playwright")).toBe(true);

  noteModuleSwitch(set(), set("writing_style"));
  expect(isNewNav("ai")).toBe(true);
});

test("switching a module off takes its mark away, and the cascade takes the rest", () => {
  noteModuleSwitch(set("db_access"), set("db_access", "auto_run"));
  noteModuleSwitch(set("db_access", "auto_run"), set("db_access", "auto_run", "api_templates"));
  expect(isNewNav("apitemplates")).toBe(true);
  // Rust answers Auto Run off with API Templates gone too.
  noteModuleSwitch(set("db_access", "auto_run", "api_templates"), set("db_access"));
  expect(isNewNav("autorun")).toBe(false);
  expect(isNewNav("apitemplates")).toBe(false);
});

test("AI Bridge keeps its mark while a module that marked it is still on", () => {
  noteModuleSwitch(set(), set("db_access"));
  noteModuleSwitch(set("db_access"), set("db_access", "writing_style"));
  noteModuleSwitch(set("db_access", "writing_style"), set("db_access"));
  expect(isNewNav("ai")).toBe(true);
  noteModuleSwitch(set("db_access"), set());
  expect(isNewNav("ai")).toBe(false);
});

test("opening the row clears its mark", () => {
  noteModuleSwitch(set("db_access"), set("db_access", "auto_run"));
  clearNewNav("autorun");
  expect(isNewNav("autorun")).toBe(false);
  expect(JSON.parse(localStorage.getItem(NEW_NAV_KEY) ?? "{}")).toEqual({});
});

test("marks persist through storage, as across a restart", () => {
  noteModuleSwitch(set("db_access"), set("db_access", "auto_run"));
  resetNewNavForTest();
  expect(isNewNav("autorun")).toBe(true);
  clearNewNav("autorun");
  resetNewNavForTest();
  expect(isNewNav("autorun")).toBe(false);
});

test("unreadable storage reads as no marks", () => {
  localStorage.setItem(NEW_NAV_KEY, "{not json");
  expect(newNavSnapshot()).toEqual({});
  resetNewNavForTest();
  localStorage.setItem(NEW_NAV_KEY, JSON.stringify({ autorun: "auto_run", ai: [3] }));
  expect(newNavSnapshot()).toEqual({});
});

test("storage that throws never breaks a switch", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  expect(() => noteModuleSwitch(set("db_access"), set("db_access", "auto_run"))).not.toThrow();
  // Session-only, but still marked.
  expect(isNewNav("autorun")).toBe(true);
  expect(() => clearNewNav("autorun")).not.toThrow();
});

test("hydration from Rust never marks anything", async () => {
  mockIPC((cmd) => (cmd === "modules_get" ? [...ALL_MODULES] : undefined));
  await hydrateModules();
  expect(newNavSnapshot()).toEqual({});
});

test("a switch that does not come from the Modules page does not mark", async () => {
  mockIPC((cmd) => (cmd === "modules_set" ? ["db_access", "auto_run"] : undefined));
  await setModule("auto_run", true);
  expect(newNavSnapshot()).toEqual({});
});

test("pruning drops marks whose module is off, and never adds one", () => {
  noteModuleSwitch(set("db_access"), set("db_access", "auto_run"));
  pruneNewNav(set("db_access", "playwright"));
  expect(newNavSnapshot()).toEqual({});
});
