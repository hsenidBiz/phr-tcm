import { afterEach, expect, test, vi } from "vitest";
import {
  dropRetiredAiSwitches,
  globalAllowedSnapshot,
  loadGlobalAllowed,
  loadScope,
  saveGlobalAllowed,
  saveScope,
  scopeSnapshot,
  subscribeAiScope,
} from "./aiScope";

afterEach(() => localStorage.clear());

test("machine-wide registration is off until Settings turns it on", () => {
  expect(loadGlobalAllowed()).toBe(false);
  const heard = vi.fn();
  const off = subscribeAiScope(heard);
  saveGlobalAllowed(true);
  expect(loadGlobalAllowed()).toBe(true);
  expect(globalAllowedSnapshot()).toBe(true);
  expect(heard).toHaveBeenCalledTimes(1);
  off();
  saveGlobalAllowed(false);
  expect(loadGlobalAllowed()).toBe(false);
  expect(heard).toHaveBeenCalledTimes(1);
});

test("the scope choice defaults to the repository and only accepts the two values", () => {
  expect(loadScope()).toBe("project");
  saveScope("global");
  expect(scopeSnapshot()).toBe("global");
  localStorage.setItem("tcm-v2-ai-scope", "elsewhere");
  expect(loadScope()).toBe("project");
});

/// The AI Bridge switches earlier versions kept are gone from the app; their
/// stored choices are dropped at start, and nothing current goes with them.
test("retired switches are dropped and the current choices are kept", () => {
  localStorage.setItem("tcm-v2-ai-show-db", "on");
  saveGlobalAllowed(true);
  saveScope("global");
  localStorage.setItem("tcm-v2-db-selected", "dev-read");

  dropRetiredAiSwitches();

  expect(localStorage.getItem("tcm-v2-ai-show-db")).toBeNull();
  expect(loadGlobalAllowed()).toBe(true);
  expect(loadScope()).toBe("global");
  expect(localStorage.getItem("tcm-v2-db-selected")).toBe("dev-read");
  // Idempotent: a second start finds nothing to do.
  dropRetiredAiSwitches();
  expect(localStorage.length).toBe(3);
});
