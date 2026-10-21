import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test, vi } from "vitest";
import {
  ALL_MODULES,
  hydrateModules,
  modulesHydratedSnapshot,
  modulesSnapshot,
  resetModulesStore,
  sectionModule,
  sectionShown,
  setModule,
  shouldLeaveHidden,
  subscribeModules,
} from "./modules";

afterEach(() => {
  clearMocks();
  resetModulesStore();
  localStorage.removeItem("tcm-v2-dev-capture");
});

test("nothing is on, and nothing is hydrated, until Rust has answered", () => {
  expect(modulesSnapshot().size).toBe(0);
  expect(modulesHydratedSnapshot()).toBe(false);
});

test("hydrating reads the modules from Rust and flips hydrated", async () => {
  mockIPC((cmd) => (cmd === "modules_get" ? ["db_access", "writing_style"] : undefined));
  await hydrateModules();
  expect([...modulesSnapshot()].sort()).toEqual(["db_access", "writing_style"]);
  expect(modulesHydratedSnapshot()).toBe(true);
});

test("without the app behind it, hydrating leaves none on but still flips hydrated", async () => {
  mockIPC(() => {
    throw new Error("no IPC here");
  });
  await hydrateModules();
  expect(modulesSnapshot().size).toBe(0);
  expect(modulesHydratedSnapshot()).toBe(true);
});

test("setting a module publishes Rust's answer, cascade included, and tells subscribers", async () => {
  mockIPC((cmd) => (cmd === "modules_get" ? ["db_access", "auto_run", "playwright"] : cmd === "modules_set" ? ["writing_style"] : undefined));
  await hydrateModules();
  const seen = vi.fn();
  const off = subscribeModules(seen);
  await setModule("db_access", false);
  // Rust answered that Auto Run and Playwright went with it.
  expect([...modulesSnapshot()]).toEqual(["writing_style"]);
  expect(seen).toHaveBeenCalledTimes(1);
  off();
});

test("a refusal from Rust is thrown and changes nothing", async () => {
  mockIPC((cmd) => {
    if (cmd === "modules_get") return ["db_access", "auto_run"];
    if (cmd === "modules_set") throw "Auto Run can't be switched off while a run is going - stop the run first.";
    return undefined;
  });
  await hydrateModules();
  await expect(setModule("auto_run", false)).rejects.toThrow("stop the run first");
  expect([...modulesSnapshot()].sort()).toEqual(["auto_run", "db_access"]);
});

test("a read that answers after a save does not undo it", async () => {
  let answer!: (v: string[]) => void;
  const pending = new Promise<string[]>((r) => {
    answer = r;
  });
  mockIPC((cmd) => (cmd === "modules_get" ? pending : cmd === "modules_set" ? ["db_access"] : undefined));
  const read = hydrateModules();
  await setModule("db_access", true);
  answer([]);
  await read;
  expect([...modulesSnapshot()]).toEqual(["db_access"]);
});

test("capture mode reports every module on but the held-back one, hydrated, whatever Rust says", () => {
  localStorage.setItem("tcm-v2-dev-capture", "on");
  expect([...modulesSnapshot()].sort()).toEqual(ALL_MODULES.filter((m) => m !== "crystal_reports").sort());
  expect(modulesSnapshot().has("crystal_reports")).toBe(false);
  expect(modulesHydratedSnapshot()).toBe(true);
});

// Rust's answer is the effective set: a module saved on but held back on a
// machine that has not unlocked it is simply absent, so no section shows.
test("a held-back module saved on shows no section while Rust leaves it out", async () => {
  mockIPC((cmd) => (cmd === "modules_get" ? ["db_access"] : undefined));
  await hydrateModules();
  expect(sectionShown("crystal-reports")).toBe(false);
  mockIPC((cmd) => (cmd === "modules_get" ? ["db_access", "crystal_reports"] : undefined));
  await hydrateModules();
  expect(sectionShown("crystal-reports")).toBe(true);
});

test("each section belongs to its module", () => {
  expect(sectionModule("autorun")).toBe("auto_run");
  expect(sectionModule("apitemplates")).toBe("api_templates");
  expect(sectionModule("playwright")).toBe("playwright");
  expect(sectionModule("crystal-reports")).toBe("crystal_reports");
  expect(sectionModule("manual")).toBeNull();
  expect(sectionModule("settings")).toBeNull();
});

test("a section hides with its module and no other", () => {
  const some = new Set(["db_access", "auto_run"] as const);
  expect(sectionShown("autorun", some)).toBe(true);
  expect(sectionShown("apitemplates", some)).toBe(false);
  expect(sectionShown("playwright", some)).toBe(false);
  expect(sectionShown("crystal-reports", some)).toBe(false);
  expect(sectionShown("crystal-reports", new Set(["crystal_reports"] as const))).toBe(true);
  expect(sectionShown("ai", new Set())).toBe(true);
});

// A saved last tab of an off module is redirected only after the modules
// have loaded - before that the set is empty, the same as a fresh install.
test("the redirect waits for hydration before it fires", () => {
  expect(shouldLeaveHidden("autorun", false, false)).toBe(false);
  expect(shouldLeaveHidden("autorun", true, false)).toBe(false);
  expect(shouldLeaveHidden("autorun", false, true)).toBe(true);
  expect(shouldLeaveHidden("autorun", true, true)).toBe(false);
  expect(shouldLeaveHidden("manual", false, true)).toBe(false);
});

test("the redirect leaves API Templates, Playwright and Crystal Reports too, once hydration settles", () => {
  for (const s of ["apitemplates", "playwright", "crystal-reports"]) {
    expect(shouldLeaveHidden(s, false, true)).toBe(true);
    expect(shouldLeaveHidden(s, false, false)).toBe(false);
    expect(shouldLeaveHidden(s, true, true)).toBe(false);
  }
  expect(shouldLeaveHidden("ai", false, true)).toBe(false);
});

// A save that fails while the first read is still in flight must not leave
// the store unhydrated for good (the saved-tab redirect would never fire).
test("a refused save during the first hydrate still ends hydrated, with Rust's answer", async () => {
  let answer!: (v: string[]) => void;
  const pending = new Promise<string[]>((r) => {
    answer = r;
  });
  let reads = 0;
  mockIPC((cmd) => {
    if (cmd === "modules_get") {
      reads += 1;
      return reads === 1 ? pending : ["db_access"];
    }
    if (cmd === "modules_set") throw "refused";
    return undefined;
  });
  const read = hydrateModules();
  await expect(setModule("auto_run", true)).rejects.toThrow("refused");
  answer([]);
  await read;
  expect(modulesHydratedSnapshot()).toBe(true);
  expect([...modulesSnapshot()]).toEqual(["db_access"]);
});
