// The stale-build guard's fingerprint. `npm run docs:build` writes this
// hash of everything the built page is made from into the page itself
// (<meta name="help-source">), and guard.test.ts recomputes it: change the
// content, the site code, the styles or a screenshot without re-running
// docs:build, and the gate fails instead of shipping the old page.
//
// Node-only (the build config and the tests use it); the site never
// imports it, so it is not in the bundle.

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

/** Text is hashed with LF line endings, so a checkout with CRLF (git's
 *  autocrlf on Windows) gives the same hash as one with LF. */
const TEXT = /\.(?:ts|css|html|json)$/i;

function walk(dir: string): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? walk(path) : [path];
  });
}

/** Every file the built page is made from, relative to docs-site/, with
 *  forward slashes, sorted. Tests and the dev-only design preview (src/dev)
 *  are left out: neither reaches the build. Files outside docs-site/ that
 *  the page imports are listed by hand: the app icon, and the runner's size
 *  (src/lib/runnerSize.ts, which the runner shots are sized and framed by).
 *  The @fontsource woff2 files inlined into the page are not hashed: their
 *  versions are pinned by package-lock.json, so they only change with a
 *  dependency update, which is followed by a docs:build anyway. */
export function helpSourceFiles(docsRoot: string): string[] {
  const rel = (p: string) => relative(docsRoot, p).split(sep).join("/");
  const files = [
    join(docsRoot, "index.html"),
    join(docsRoot, "vite.config.ts"),
    join(docsRoot, "..", "src-tauri", "icons", "64x64.png"),
    join(docsRoot, "..", "src", "lib", "runnerSize.ts"),
    ...walk(join(docsRoot, "src")),
    ...walk(join(docsRoot, "shots")),
  ].map(rel);
  return files.filter((f) => !f.endsWith(".test.ts") && !f.startsWith("src/dev/")).sort();
}

/** The hash of every file `helpSourceFiles` lists: path, length and bytes. */
export function helpSourceHash(docsRoot: string): string {
  const hash = createHash("sha256");
  for (const file of helpSourceFiles(docsRoot)) {
    let bytes = readFileSync(join(docsRoot, file));
    if (TEXT.test(file)) bytes = Buffer.from(bytes.toString("utf8").replace(/\r\n/g, "\n"), "utf8");
    hash.update(`${file}\0${bytes.length}\0`).update(bytes);
  }
  return hash.digest("hex");
}

/** The meta tag the build writes into <head>, and how the test reads it. */
export const SOURCE_META = "help-source";
export const readSourceMeta = (html: string): string | null =>
  new RegExp(`<meta name="${SOURCE_META}" content="([0-9a-f]{64})"`).exec(html)?.[1] ?? null;
