import { afterEach, expect, test } from "vitest";
import {
  CHANGELOG,
  changelogFor,
  compareVersions,
  entriesSince,
  isBetaVersion,
  markChangelogSeen,
  pendingChangelog,
  type ChangelogEntry,
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

/** The shape every changelog must have: a stable (`X.Y.Z`) or beta
 * (`X.Y.Z-beta.N`) version, a date, at least one item, newest first. */
function expectWellFormed(entries: readonly ChangelogEntry[]) {
  for (const e of entries) {
    expect(e.version).toMatch(/^\d+\.\d+\.\d+(-beta\.\d+)?$/);
    expect(e.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(e.items.length).toBeGreaterThan(0);
  }
  // Newest first - the modal and Settings render in array order.
  for (let i = 1; i < entries.length; i++) {
    expect(
      compareVersions(entries[i - 1].version, entries[i].version),
      `${entries[i - 1].version} is listed above ${entries[i].version}`,
    ).toBe(1);
  }
}

test("every entry has a version, a date, and at least one item", () => {
  expectWellFormed(CHANGELOG);
});

test("a beta's entry passes the same checks, between the stables around it", () => {
  const entry = (version: string): ChangelogEntry => ({ version, date: "2026-09-27", items: ["x"] });
  expectWellFormed([entry("1.26.0"), entry("1.26.0-beta.2"), entry("1.26.0-beta.1"), entry("1.25.31")]);
  // A beta listed above the stable it leads to is out of order.
  expect(() => expectWellFormed([entry("1.26.0-beta.1"), entry("1.26.0")])).toThrow();
  // Only the beta suffix is allowed on top of X.Y.Z.
  expect(() => expectWellFormed([entry("1.26.0-rc.1")])).toThrow();
  expect(() => expectWellFormed([entry("1.26")])).toThrow();
});

test("compareVersions orders betas below the stable they lead to", () => {
  const order = ["1.25.31", "1.26.0-beta.1", "1.26.0-beta.2", "1.26.0-beta.10", "1.26.0", "1.26.1-beta.1"];
  for (let i = 0; i < order.length - 1; i++) {
    expect(compareVersions(order[i], order[i + 1]), `${order[i]} < ${order[i + 1]}`).toBe(-1);
    expect(compareVersions(order[i + 1], order[i])).toBe(1);
  }
  expect(compareVersions("1.26.0-beta.2", "1.26.0-beta.2")).toBe(0);
});

test("a dev build still compares equal-ish, so What's new stays shut", () => {
  expect(compareVersions("dev", "dev")).toBe(0);
  expect(compareVersions("dev", "0.0.0")).toBe(0);
});

test("isBetaVersion", () => {
  expect(isBetaVersion("1.26.0-beta.1")).toBe(true);
  expect(isBetaVersion("1.26.0")).toBe(false);
  expect(isBetaVersion("dev")).toBe(false);
});

test("a release build lists only releases; a beta build keeps the betas", () => {
  const stable = changelogFor("2.0.7");
  expect(stable.length).toBeGreaterThan(0);
  expect(stable.some((e) => isBetaVersion(e.version))).toBe(false);
  expect(stable.map((e) => e.version)).toEqual(
    CHANGELOG.filter((e) => !isBetaVersion(e.version)).map((e) => e.version),
  );
  expect(changelogFor("2.0.5-beta.3")).toEqual(CHANGELOG);
});

test("updating to a release shows no beta entries, updating to a beta does", () => {
  markChangelogSeen("2.0.3");
  expect(pendingChangelog("2.0.4").map((e) => e.version)).toEqual(["2.0.4"]);
  markChangelogSeen("2.0.3");
  expect(pendingChangelog("2.0.4-beta.3").map((e) => e.version)).toEqual([
    "2.0.4-beta.3",
    "2.0.4-beta.2",
    "2.0.4-beta.1",
  ]);
  // A beta tester moving on to the release sees the release's own entry.
  markChangelogSeen("2.0.5-beta.6");
  expect(pendingChangelog("2.0.6").map((e) => e.version)).toEqual(["2.0.6"]);
});

test("a release's entry carries what its betas changed", () => {
  const items = (v: string) => CHANGELOG.find((e) => e.version === v)!.items.join(" ");
  expect(items("2.0.4")).toMatch(/spec order/);
  expect(items("2.0.4")).toMatch(/Review page/);
  expect(items("2.0.4")).toMatch(/interface tour/);
  expect(items("2.0.6")).toMatch(/Collapse all/);
});
