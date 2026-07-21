import { afterEach, expect, test } from "vitest";
import {
  CHANGELOG,
  compareVersions,
  entriesSince,
  markChangelogSeen,
  pendingChangelog,
} from "./changelog";

afterEach(() => localStorage.clear());

test("compareVersions orders numerically, not lexically", () => {
  expect(compareVersions("1.9.0", "1.10.0")).toBe(-1); // lexical would say >
  expect(compareVersions("1.10.0", "1.9.0")).toBe(1);
  expect(compareVersions("1.8.0", "1.8.0")).toBe(0);
  expect(compareVersions("2.0.0", "1.99.99")).toBe(1);
});

test("entriesSince returns only versions in (seen, current], newest first", () => {
  const between = entriesSince("1.7.2", "1.9.0");
  expect(between.map((e) => e.version)).toEqual(["1.9.0", "1.8.0"]);
  expect(entriesSince("1.9.0", "1.9.0")).toEqual([]);
});

test("fresh install records the version silently - installing is not updating", () => {
  expect(pendingChangelog("1.9.0")).toEqual([]);
  expect(localStorage.getItem("tcm-v2-changelog-seen")).toBe("1.9.0");
  // Next launch on the same version stays quiet too.
  expect(pendingChangelog("1.9.0")).toEqual([]);
});

test("after an update the entries between seen and current come back once", () => {
  markChangelogSeen("1.7.2");
  const pending = pendingChangelog("1.9.0");
  expect(pending.map((e) => e.version)).toEqual(["1.9.0", "1.8.0"]);
  // The caller marks seen on dismiss - then nothing is pending anymore.
  markChangelogSeen("1.9.0");
  expect(pendingChangelog("1.9.0")).toEqual([]);
});

test("a downgrade or dev build never shows the modal", () => {
  markChangelogSeen("1.9.0");
  expect(pendingChangelog("1.8.0")).toEqual([]);
  expect(pendingChangelog("dev")).toEqual([]);
});

test("every entry has a version, a date, and at least one item", () => {
  for (const e of CHANGELOG) {
    expect(e.version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(e.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(e.items.length).toBeGreaterThan(0);
  }
  // Newest first - the modal and Settings render in array order.
  for (let i = 1; i < CHANGELOG.length; i++) {
    expect(compareVersions(CHANGELOG[i - 1].version, CHANGELOG[i].version)).toBe(1);
  }
});
