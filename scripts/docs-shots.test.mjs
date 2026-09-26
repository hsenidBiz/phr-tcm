// scripts/docs-shots.mjs end to end, for the paths that must work with no
// app and whatever state the help content is in: a leftover settings
// snapshot is dealt with BEFORE the content is loaded, so an interrupted
// owner can always get their settings back - even mid-edit, with the
// content broken.
//
// Each run gets its own temp folder (the snapshot lives in the OS temp
// folder) and a preload that makes loading the content (`import("vite")`)
// fail loudly. None of these runs reaches the point of connecting to an
// app, so they are safe with the owner's dev app open.

import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { afterEach, beforeEach, expect, test } from "vitest";

const script = resolve(dirname(fileURLToPath(import.meta.url)), "docs-shots.mjs");
const BACKUP_NAME = "tcm-docs-shots-settings-backup.json";

let dir;
let preload;
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "docs-shots-test-"));
  preload = join(dir, "no-content.mjs");
  writeFileSync(
    preload,
    `import { registerHooks } from "node:module";
registerHooks({
  resolve(specifier, context, next) {
    if (specifier === "vite") throw new Error("CONTENT WAS LOADED");
    return next(specifier, context);
  },
});
`,
  );
});
afterEach(() => rmSync(dir, { recursive: true, force: true }));

function run(...args) {
  const r = spawnSync(process.execPath, ["--import", pathToFileURL(preload).href, script, ...args], {
    env: { ...process.env, TEMP: dir, TMP: dir, TMPDIR: dir },
    encoding: "utf8",
    timeout: 30_000,
  });
  return { code: r.status, out: `${r.stdout}${r.stderr}` };
}

const writeBackup = (at) =>
  writeFileSync(join(dir, BACKUP_NAME), JSON.stringify({ at, url: "http://localhost:1420/", storage: { k: "v" } }));

test("the preload does stop the content loading (so the tests below mean something)", () => {
  const r = run("--validate");
  expect(r.code).toBe(1);
  expect(r.out).toContain("CONTENT WAS LOADED");
});

test("--restore with nothing to restore says so, without loading the content", () => {
  const r = run("--restore");
  expect(r.out).not.toContain("CONTENT WAS LOADED");
  expect(r.out).toContain("Nothing to restore");
  expect(r.code).toBe(0);
});

test("--restore of a day-old snapshot asks for --force, without loading the content", () => {
  writeBackup(Date.now() - 2 * 24 * 60 * 60 * 1000);
  const r = run("--restore");
  expect(r.out).not.toContain("CONTENT WAS LOADED");
  expect(r.out).toContain("taken 2 days ago");
  expect(r.out).toContain("--restore --force");
  expect(r.code).toBe(1);
  expect(existsSync(join(dir, BACKUP_NAME))).toBe(true);
});

test("a capture refuses while a snapshot is left over, without loading the content", () => {
  writeBackup(Date.now() - 10 * 60 * 1000);
  const r = run();
  expect(r.out).not.toContain("CONTENT WAS LOADED");
  expect(r.out).toContain("An earlier capture (10 minutes ago)");
  expect(r.code).toBe(1);
});

test("an unreadable snapshot is named, without loading the content", () => {
  writeFileSync(join(dir, BACKUP_NAME), "{ not json");
  const r = run("--restore");
  expect(r.out).not.toContain("CONTENT WAS LOADED");
  expect(r.out).toContain("cannot be read");
  expect(r.code).toBe(1);
});
