import { afterEach, expect, test, vi } from "vitest";
import {
  CASES_DIR,
  casesDir,
  isInsideCasesDir,
  loadWorkingDir,
  saveWorkingDir,
  subscribeWorkingDir,
  workingDirSnapshot,
} from "./workingDir";

afterEach(() => localStorage.clear());

test("unset reads as empty, and saving persists and notifies", () => {
  expect(loadWorkingDir()).toBe("");
  const heard = vi.fn();
  const off = subscribeWorkingDir(heard);
  saveWorkingDir("  D:\\repo  ");
  expect(loadWorkingDir()).toBe("D:\\repo");
  expect(workingDirSnapshot()).toBe("D:\\repo");
  expect(heard).toHaveBeenCalledTimes(1);
  off();
  saveWorkingDir("");
  expect(loadWorkingDir()).toBe("");
  expect(heard).toHaveBeenCalledTimes(1);
});

test("the cases folder sits directly under the repo", () => {
  expect(casesDir("D:\\repo")).toBe(`D:\\repo\\${CASES_DIR}`);
  expect(casesDir("D:/repo/")).toBe(`D:/repo\\${CASES_DIR}`);
});

test("inside-check ignores case and slash style, and is exact about the prefix", () => {
  expect(isInsideCasesDir("D:\\repo", "D:\\repo\\.test-cases\\login.json")).toBe(true);
  expect(isInsideCasesDir("D:\\repo", "d:/REPO/.test-cases/login.json")).toBe(true);
  expect(isInsideCasesDir("D:\\repo", "D:\\repo\\login.json")).toBe(false);
  expect(isInsideCasesDir("D:\\repo", "D:\\repo\\.test-cases-old\\login.json")).toBe(false);
});
