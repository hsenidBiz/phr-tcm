import { existsSync, mkdtempSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { applyToDir, mergeFeeds, parseKeep } from "./prune-releases.mjs";

const asset = (v: string) => ({
  PackageId: "AzureDevOpsTestCaseManager.V2",
  Version: v,
  Type: "Full",
  FileName: `AzureDevOpsTestCaseManager.V2-${v}-full.nupkg`,
  SHA1: "A",
  SHA256: "B",
  Size: 1,
  NotesMarkdown: "",
  NotesHtml: "",
});

test("the newest N versions survive, ordered newest first, by number not by string", () => {
  const old = ["1.9.0", "1.10.0", "1.22.1", "1.22.0", "1.21.3", "1.21.2", "1.21.1"].map(asset);
  const { kept, droppedVersions } = mergeFeeds(old, [asset("1.23.0")], 5);
  expect(kept.map((a) => a.Version)).toEqual(["1.23.0", "1.22.1", "1.22.0", "1.21.3", "1.21.2"]);
  expect(droppedVersions.sort()).toEqual(["1.10.0", "1.21.1", "1.9.0"]);
});

test("a re-pack of the same version replaces the old entry", () => {
  const fresh = { ...asset("1.22.1"), SHA1: "NEW" };
  const { kept } = mergeFeeds([asset("1.22.1"), asset("1.22.0")], [fresh], 5);
  expect(kept.filter((a) => a.Version === "1.22.1")).toHaveLength(1);
  expect(kept[0].SHA1).toBe("NEW");
});

test("an empty clone (first release) is fine", () => {
  const { kept, droppedVersions } = mergeFeeds([], [asset("1.23.0")], 5);
  expect(kept).toHaveLength(1);
  expect(droppedVersions).toEqual([]);
});

function fakeRepo(
  versions: string[],
  opts: { feed?: boolean; templateJunk?: boolean; setupExe?: boolean; missingVersions?: string[] } = {},
) {
  const { feed = true, templateJunk = true, setupExe = true, missingVersions = [] } = opts;
  const repo = mkdtempSync(join(tmpdir(), "prune-repo-"));
  mkdirSync(join(repo, ".git"));
  writeFileSync(join(repo, ".git", "HEAD"), "ref: refs/heads/main\n");
  // `missingVersions` still gets a feed entry (below) but no file on disk -
  // models a clone whose feed already names a package the clone lacks.
  for (const v of versions) {
    if (!missingVersions.includes(v)) writeFileSync(join(repo, asset(v).FileName), v);
  }
  if (setupExe) writeFileSync(join(repo, "AzureDevOpsTestCaseManager.V2-win-Setup.exe"), "old-setup");
  writeFileSync(join(repo, "README.md"), "readme");
  if (templateJunk) writeFileSync(join(repo, "src"), "template junk"); // a leftover from the DevOps template
  if (feed) writeFileSync(join(repo, "releases.win.json"), JSON.stringify({ Assets: versions.map(asset) }));
  return repo;
}

function fakePack(version: string, opts: { setupExe?: boolean } = {}) {
  const { setupExe = true } = opts;
  const pack = mkdtempSync(join(tmpdir(), "prune-pack-"));
  writeFileSync(join(pack, asset(version).FileName), version);
  if (setupExe) writeFileSync(join(pack, "AzureDevOpsTestCaseManager.V2-win-Setup.exe"), "new-setup");
  writeFileSync(join(pack, "AzureDevOpsTestCaseManager.V2-win-Portable.zip"), "zip");
  writeFileSync(join(pack, "RELEASES"), "legacy");
  writeFileSync(join(pack, "assets.win.json"), "{}");
  writeFileSync(join(pack, "releases.win.json"), JSON.stringify({ Assets: [asset(version)] }));
  return pack;
}

test("on disk: eight versions become five, the feed matches, one Setup.exe, template junk gone", () => {
  const repo = fakeRepo(["1.15.0", "1.16.0", "1.17.0", "1.18.0", "1.19.0", "1.20.0", "1.21.0", "1.22.1"]);
  const pack = fakePack("1.23.0");
  const { kept, removed } = applyToDir(repo, pack, 5);

  const names = readdirSync(repo).sort();
  const nupkgs = names.filter((n) => n.endsWith(".nupkg"));
  expect(nupkgs).toEqual([
    "AzureDevOpsTestCaseManager.V2-1.20.0-full.nupkg",
    "AzureDevOpsTestCaseManager.V2-1.21.0-full.nupkg",
    "AzureDevOpsTestCaseManager.V2-1.22.1-full.nupkg",
    "AzureDevOpsTestCaseManager.V2-1.23.0-full.nupkg",
    "AzureDevOpsTestCaseManager.V2-1.19.0-full.nupkg",
  ].sort());
  expect(names).toContain(".git");
  expect(names).toContain("README.md");
  expect(names).toContain("RELEASES");
  expect(names).toContain("AzureDevOpsTestCaseManager.V2-win-Portable.zip");
  expect(names).not.toContain("src");
  expect(names).not.toContain("assets.win.json");
  expect(readFileSync(join(repo, "AzureDevOpsTestCaseManager.V2-win-Setup.exe"), "utf8")).toBe("new-setup");

  const feed = JSON.parse(readFileSync(join(repo, "releases.win.json"), "utf8"));
  expect(feed.Assets.map((a: { Version: string }) => a.Version)).toEqual(["1.23.0", "1.22.1", "1.21.0", "1.20.0", "1.19.0"]);
  expect(kept).toHaveLength(5);
  expect(removed.filter((r) => r.endsWith(".nupkg"))).toHaveLength(4);
});

test("applyToDir: a clone with no releases.win.json at all (the first real release)", () => {
  // Not merely an empty feed -- no feed file exists yet, so readFeed must go
  // through its existsSync branch rather than parsing a file that isn't there.
  const repo = fakeRepo([], { feed: false, templateJunk: false, setupExe: false });
  const pack = fakePack("1.23.0");
  const { kept, removed } = applyToDir(repo, pack, 5);

  const names = readdirSync(repo).sort();
  expect(names).toContain(".git");
  expect(names).toContain("README.md");
  expect(names).toContain("releases.win.json");
  expect(names).toContain("AzureDevOpsTestCaseManager.V2-1.23.0-full.nupkg");
  expect(names).toContain("AzureDevOpsTestCaseManager.V2-win-Setup.exe");
  expect(names).toContain("AzureDevOpsTestCaseManager.V2-win-Portable.zip");
  expect(names).toContain("RELEASES");
  expect(names).not.toContain("assets.win.json");

  const feed = JSON.parse(readFileSync(join(repo, "releases.win.json"), "utf8"));
  expect(feed.Assets.map((a: { Version: string }) => a.Version)).toEqual(["1.23.0"]);
  expect(kept).toEqual(["AzureDevOpsTestCaseManager.V2-1.23.0-full.nupkg"]);
  expect(removed).toEqual([]);
});

test("applyToDir: retrying a pack whose version already exists in the clone overwrites the stale copy", () => {
  // Models a publish that died partway through a previous run: the feed
  // already names 1.23.0, and a package for it is already on disk, but that
  // package is the truncated/corrupt leftover from the failed attempt.
  const repo = fakeRepo(["1.21.0", "1.22.0", "1.23.0"]);
  writeFileSync(join(repo, "AzureDevOpsTestCaseManager.V2-1.23.0-full.nupkg"), "PARTIAL-CORRUPT");
  const pack = fakePack("1.23.0"); // the retry re-packs the same version fresh

  const { kept, removed } = applyToDir(repo, pack, 5);

  // The stale bytes must be replaced with the freshly-packed content, not left in place.
  expect(readFileSync(join(repo, "AzureDevOpsTestCaseManager.V2-1.23.0-full.nupkg"), "utf8")).toBe("1.23.0");
  const names = readdirSync(repo).sort();
  const nupkgs = names.filter((n) => n.endsWith(".nupkg"));
  expect(nupkgs.sort()).toEqual(
    [
      "AzureDevOpsTestCaseManager.V2-1.21.0-full.nupkg",
      "AzureDevOpsTestCaseManager.V2-1.22.0-full.nupkg",
      "AzureDevOpsTestCaseManager.V2-1.23.0-full.nupkg",
    ].sort(),
  );
  expect(kept).toContain("AzureDevOpsTestCaseManager.V2-1.23.0-full.nupkg");
  expect(removed).not.toContain("AzureDevOpsTestCaseManager.V2-1.23.0-full.nupkg");
});

test("applyToDir refuses to write a feed naming a file that is not on disk", () => {
  // The clone's own feed already names 1.22.0, but that package's file was
  // never written (e.g. lost between runs) - the invariant from the spec
  // ("a feed that names fewer packages than exist is fine; one that names
  // more is not") means this must throw rather than push a feed with a
  // dangling entry.
  const repo = fakeRepo(["1.21.0", "1.22.0", "1.22.1"], { missingVersions: ["1.22.0"] });
  const pack = fakePack("1.23.0");
  expect(() => applyToDir(repo, pack, 5)).toThrow(/1\.22\.0/);
  // Nothing was written on the way to the throw - the stale feed on disk
  // (if any) is untouched, not overwritten with a bad one.
  expect(existsSync(join(repo, "releases.win.json"))).toBe(true);
  const feed = JSON.parse(readFileSync(join(repo, "releases.win.json"), "utf8"));
  expect(feed.Assets.map((a: { Version: string }) => a.Version)).toEqual(["1.21.0", "1.22.0", "1.22.1"]);
});

test("applyToDir refuses to prune when the pack has no Setup.exe", () => {
  // `wanted` only keeps a Setup.exe by naming the one from THIS pack - a
  // pack that somehow lacks one would make the removal loop delete the
  // clone's existing Setup.exe, the first-install path the repo's README
  // points people at. This must throw before anything is deleted.
  const repo = fakeRepo(["1.22.1"]);
  const pack = fakePack("1.23.0", { setupExe: false });
  expect(() => applyToDir(repo, pack, 5)).toThrow(/Setup\.exe/i);
  // The existing installer is still there - the throw happened before the
  // removal loop, not after it had already deleted the file.
  expect(readFileSync(join(repo, "AzureDevOpsTestCaseManager.V2-win-Setup.exe"), "utf8")).toBe("old-setup");
});

test("parseKeep accepts a positive integer and rejects everything else", () => {
  expect(parseKeep("5")).toBe(5);
  expect(parseKeep("1")).toBe(1);
  expect(parseKeep("10")).toBe(10);
  expect(parseKeep("0")).toBeNull();
  expect(parseKeep("-3")).toBeNull();
  expect(parseKeep("2.5")).toBeNull();
  expect(parseKeep("abc")).toBeNull();
  expect(parseKeep(undefined)).toBeNull();
  // e.g. `--keep` with a missing value swallows the next flag as its argument
  expect(parseKeep("--pack")).toBeNull();
});
