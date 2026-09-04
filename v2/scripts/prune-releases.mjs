// Merge a fresh `vpk pack` into a clone of the DevOps releases repo and
// keep only the newest N versions.
//
// `vpk pack` writes a releases.win.json that lists ONLY the version it just
// packed, so copying its output over the clone would throw away every
// older version's feed entry - and an install one version behind would then
// find nothing to update from. This merges the two feeds instead, prunes
// the packages the merged feed no longer names, and deletes anything else
// in the clone (the DevOps template's src/, docs/ and .gitignore included)
// so the repo holds exactly what the feed describes, plus README.md.
//
// Usage: node prune-releases.mjs --repo <clone> --pack <v2/Releases> --keep 5

import { copyFileSync, existsSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const FEED = "releases.win.json";
/** Files the pack produces that are published as-is, latest only. */
const LATEST_ONLY = (name) => /-Setup\.exe$/i.test(name) || /-Portable\.zip$/i.test(name) || name === "RELEASES";

function parseVersion(v) {
  return v.split(".").map((n) => Number.parseInt(n, 10) || 0);
}

function compareDesc(a, b) {
  const pa = parseVersion(a), pb = parseVersion(b);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pb[i] ?? 0) - (pa[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

/** Newest `keep` versions across both feeds; the pack's entry wins a tie. */
export function mergeFeeds(oldAssets, newAssets, keep) {
  const byKey = new Map();
  for (const a of oldAssets) byKey.set(`${a.Version}|${a.Type}`, a);
  for (const a of newAssets) byKey.set(`${a.Version}|${a.Type}`, a);
  const versions = [...new Set([...byKey.values()].map((a) => a.Version))].sort(compareDesc);
  const keptVersions = new Set(versions.slice(0, keep));
  const kept = [...byKey.values()]
    .filter((a) => keptVersions.has(a.Version))
    .sort((x, y) => compareDesc(x.Version, y.Version) || x.Type.localeCompare(y.Type));
  const droppedVersions = versions.filter((v) => !keptVersions.has(v));
  return { kept, droppedVersions };
}

function readFeed(dir) {
  const p = join(dir, FEED);
  return existsSync(p) ? JSON.parse(readFileSync(p, "utf8")).Assets ?? [] : [];
}

export function applyToDir(repoDir, packDir, keep) {
  const { kept } = mergeFeeds(readFeed(repoDir), readFeed(packDir), keep);
  const keepNames = new Set(kept.map((a) => a.FileName));
  const packNames = readdirSync(packDir);

  // Copy what the pack made and the feed still names, plus the latest-only files.
  for (const name of packNames) {
    if (keepNames.has(name) || LATEST_ONLY(name)) copyFileSync(join(packDir, name), join(repoDir, name));
  }
  const wanted = new Set([...keepNames, ...packNames.filter(LATEST_ONLY), FEED, "README.md", ".git"]);

  const removed = [];
  for (const name of readdirSync(repoDir)) {
    if (wanted.has(name)) continue;
    rmSync(join(repoDir, name), { recursive: true, force: true });
    removed.push(name);
  }
  writeFileSync(join(repoDir, FEED), JSON.stringify({ Assets: kept }));
  return { kept: kept.map((a) => a.FileName), removed };
}

// --- CLI -------------------------------------------------------------------
if (process.argv[1] && import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/").replace(/^.*\//, ""))) {
  const arg = (flag, dflt) => {
    const i = process.argv.indexOf(flag);
    return i >= 0 ? process.argv[i + 1] : dflt;
  };
  const repo = arg("--repo"), pack = arg("--pack"), keep = Number(arg("--keep", "5"));
  if (!repo || !pack || !statSync(repo).isDirectory() || !statSync(pack).isDirectory()) {
    console.error("usage: node prune-releases.mjs --repo <clone> --pack <Releases dir> --keep 5");
    process.exit(2);
  }
  const r = applyToDir(repo, pack, keep);
  console.log(`kept ${r.kept.length} package(s); removed ${r.removed.length} file(s)`);
}
