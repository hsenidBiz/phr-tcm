import { afterEach, expect, test, vi } from "vitest";
import {
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
