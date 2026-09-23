import { afterEach, expect, test, vi } from "vitest";
import {
  globalAllowedSnapshot,
  loadGlobalAllowed,
  loadScope,
  loadShowPhrx,
  saveGlobalAllowed,
  saveScope,
  saveShowPhrx,
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

test("the PHR X option is off by default and only ON is stored", () => {
  localStorage.clear();
  expect(loadShowPhrx()).toBe(false);
  saveShowPhrx(true);
  expect(localStorage.getItem("tcm-v2-ai-show-phrx")).toBe("on");
  expect(loadShowPhrx()).toBe(true);
  saveShowPhrx(false);
  expect(localStorage.getItem("tcm-v2-ai-show-phrx")).toBeNull();
  expect(loadShowPhrx()).toBe(false);
});

test("the old show-db key no longer switches anything on", () => {
  localStorage.clear();
  localStorage.setItem("tcm-v2-ai-show-db", "on");
  expect(loadShowPhrx()).toBe(false);
});
