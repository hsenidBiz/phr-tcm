import { afterEach, expect, test, vi } from "vitest";
import {
  addRepository,
  CASES_DIR,
  casesDir,
  isInsideCasesDir,
  loadCurrentPath,
  loadRepositories,
  loadWorkingDir,
  removeRepository,
  repositoriesSnapshot,
  saveWorkingDir,
  setCurrentRepository,
  setRepositoryEnabled,
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

/// The single value from before the list becomes its first entry, on and
/// current - nobody loses the repository they had set.
test("a pre-list working directory migrates into the list", () => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
  expect(loadRepositories()).toEqual([{ path: "D:\\repo", enabled: true }]);
  expect(loadCurrentPath()).toBe("D:\\repo");
  expect(loadWorkingDir()).toBe("D:\\repo");
  expect(localStorage.getItem("tcm-v2-working-dir")).toBeNull();
});

test("adding dedupes by path, switches on, and makes current", () => {
  addRepository("D:\\repo");
  addRepository("E:\\other");
  expect(loadCurrentPath()).toBe("E:\\other");
  setRepositoryEnabled("D:\\repo", false);
  // Same folder, different spelling: no third entry, and it comes back on
  // and current.
  addRepository("d:/REPO/");
  expect(loadRepositories()).toEqual([
    { path: "D:\\repo", enabled: true },
    { path: "E:\\other", enabled: true },
  ]);
  expect(loadCurrentPath()).toBe("D:\\repo");
});

test("the working dir is the current repository only while it is on", () => {
  addRepository("D:\\repo");
  addRepository("E:\\other");
  setCurrentRepository("D:\\repo");
  expect(loadWorkingDir()).toBe("D:\\repo");
  setRepositoryEnabled("D:\\repo", false);
  expect(loadWorkingDir()).toBe("");
  expect(loadCurrentPath()).toBe("D:\\repo");
  setRepositoryEnabled("D:\\repo", true);
  expect(loadWorkingDir()).toBe("D:\\repo");
  setCurrentRepository("Z:\\unknown");
  expect(loadCurrentPath()).toBe("D:\\repo");
});

test("removing the current repository deselects; removing another does not", () => {
  addRepository("D:\\repo");
  addRepository("E:\\other");
  removeRepository("D:\\repo");
  expect(loadRepositories()).toEqual([{ path: "E:\\other", enabled: true }]);
  expect(loadCurrentPath()).toBe("E:\\other");
  removeRepository("E:\\other");
  expect(loadRepositories()).toEqual([]);
  expect(loadWorkingDir()).toBe("");
});

test("the list snapshot keeps its identity while nothing changed", () => {
  addRepository("D:\\repo");
  const a = repositoriesSnapshot();
  expect(repositoriesSnapshot()).toBe(a);
  addRepository("E:\\other");
  expect(repositoriesSnapshot()).not.toBe(a);
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
