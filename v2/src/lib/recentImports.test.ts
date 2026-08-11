import { afterEach, expect, test } from "vitest";
import {
  forgetRecentImport,
  loadRecentImports,
  recordRecentImport,
} from "./recentImports";

afterEach(() => localStorage.clear());

test("imports are remembered newest first and re-imports move up, not duplicate", () => {
  recordRecentImport("C:\\work\\a.json", 1);
  recordRecentImport("C:\\work\\b.json", 2);
  recordRecentImport("C:\\work\\a.json", 3);
  expect(loadRecentImports().map((e) => e.path)).toEqual([
    "C:\\work\\a.json",
    "C:\\work\\b.json",
  ]);
});

test("the list is capped at eight, dropping the oldest", () => {
  for (let i = 1; i <= 10; i++) recordRecentImport(`C:\\work\\${i}.json`, i);
  const paths = loadRecentImports().map((e) => e.path);
  expect(paths).toHaveLength(8);
  expect(paths[0]).toBe("C:\\work\\10.json");
  expect(paths).not.toContain("C:\\work\\1.json");
  expect(paths).not.toContain("C:\\work\\2.json");
});

test("forgetting removes exactly one path", () => {
  recordRecentImport("C:\\work\\a.json", 1);
  recordRecentImport("C:\\work\\b.json", 2);
  forgetRecentImport("C:\\work\\a.json");
  expect(loadRecentImports().map((e) => e.path)).toEqual(["C:\\work\\b.json"]);
});

test("garbage in storage reads as an empty list, not a crash", () => {
  localStorage.setItem("tcm-v2-recent-imports", "{ not json");
  expect(loadRecentImports()).toEqual([]);
  localStorage.setItem("tcm-v2-recent-imports", JSON.stringify([{ nope: 1 }, null, "x"]));
  expect(loadRecentImports()).toEqual([]);
});
