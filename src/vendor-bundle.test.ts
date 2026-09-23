// Everything under public/vendor is third-party code the app serves to
// itself. It was read before it went in - each folder's SOURCE.txt says
// what, from where and when - and these rules keep it the way it was
// vetted: nothing in it may reach the network, storage, or the app around
// it, and all of it must run under the app's CSP.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const VENDOR = join(ROOT, "public/vendor");
const RUNNER = join(VENDOR, "runner");

function files(dir: string): string[] {
  const out: string[] = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out.push(...files(p));
    else out.push(p);
  }
  return out;
}
const rel = (p: string) => relative(VENDOR, p).replace(/\\/g, "/");
const isText = (p: string) => !p.endsWith(".png");

test("the runner bundle is exactly the files that were vetted", () => {
  expect(files(RUNNER).map(rel).sort()).toEqual(
    [
      "runner/LICENSE",
      "runner/LICENSE.chromium",
      "runner/SOURCE.txt",
      "runner/assets/default_100_percent/100-error-offline.png",
      "runner/assets/default_100_percent/100-offline-sprite.png",
      "runner/assets/default_200_percent/200-error-offline.png",
      "runner/assets/default_200_percent/200-offline-sprite.png",
      "runner/escape.js",
      "runner/index.css",
      "runner/index.html",
      "runner/index.js",
    ].sort(),
  );
});

test("no bundled file names a network address", () => {
  const hits: string[] = [];
  for (const f of files(VENDOR).filter(isText)) {
    const text = readFileSync(f, "utf8");
    for (const re of [/https?:\/\//i, /\bwss?:\/\//i, /["'(]\s*\/\/[^\s"')]+/]) {
      const m = text.match(re);
      if (m) hits.push(`${rel(f)}: ${m[0].slice(0, 60)}`);
    }
  }
  expect(hits).toEqual([]);
});

test("the game's script uses nothing that reaches the network, storage or the app", () => {
  const js = readFileSync(join(RUNNER, "index.js"), "utf8");
  const forbidden = [
    /\bfetch\s*\(/,
    /XMLHttpRequest/,
    /WebSocket/,
    /EventSource/,
    /sendBeacon/,
    /\bimportScripts\b/,
    /\bimport\s*\(/,
    /\beval\s*\(/,
    /new\s+Function\b/,
    /localStorage|sessionStorage|indexedDB|document\.cookie/,
    /\b(?:parent|top|opener)\s*\./,
    /__TAURI/,
    /postMessage/,
    /window\.open\b/,
    /serviceWorker/,
  ];
  expect(forbidden.filter((re) => re.test(js)).map(String)).toEqual([]);
});

// postMessage is how the sandboxed frame (no allow-same-origin, so no
// direct parent access) asks to be closed - see RunnerGameModal.tsx and
// escape.js's own header comment. Only that one first-party file may use
// it; the vendored game must stay exactly as vetted, which is fetch/XHR/etc
// free (the test above) AND postMessage free.
test("postMessage is used only by our own escape.js, never by the vendored game", () => {
  const ESCAPE = join(RUNNER, "escape.js");
  for (const f of files(RUNNER).filter((p) => p.endsWith(".js"))) {
    const text = readFileSync(f, "utf8");
    if (f === ESCAPE) expect(text).toMatch(/\bparent\.postMessage\s*\(/);
    else expect(text).not.toMatch(/postMessage/);
  }
});

test("the page runs under the app's CSP: no inline script, no inline handler, no other stylesheet", () => {
  const html = readFileSync(join(RUNNER, "index.html"), "utf8");
  expect(html).not.toMatch(/<script(?![^>]*\bsrc=)[^>]*>/i);
  expect(html).not.toMatch(/\son[a-z]+\s*=/i);
  expect(html).not.toMatch(/<link[^>]*href="(?!index\.css")/i);
});

test("every file the page and its stylesheet ask for is in the bundle", () => {
  const html = readFileSync(join(RUNNER, "index.html"), "utf8");
  const css = readFileSync(join(RUNNER, "index.css"), "utf8");
  const refs = [
    ...[...html.matchAll(/\b(?:src|href)="([^"]+)"/g)].map((m) => m[1]),
    ...[...css.matchAll(/url\(([^)]+)\)/g)].map((m) => m[1].trim().replace(/^["']|["']$/g, "")),
  ].filter((r) => !r.startsWith("data:"));
  expect(refs.length).toBeGreaterThan(3);
  expect(refs.filter((r) => !existsSync(join(RUNNER, r)))).toEqual([]);
});

test("the app's CSP lets the window frame its own pages and nothing else", () => {
  const conf = JSON.parse(readFileSync(join(ROOT, "src-tauri/tauri.conf.json"), "utf8"));
  for (const policy of [conf.app.security.csp, conf.app.security.devCsp] as string[]) {
    const frame = policy
      .split(";")
      .map((d) => d.trim())
      .find((d) => d.startsWith("frame-src"));
    expect(frame).toBe("frame-src 'self'");
  }
});

// The BSD notice both licences require has to actually be in the notices
// file, not just a filename check that the licences themselves are still
// bundled - an edit to the notices file could otherwise quietly drop it.
test("every line of both runner licences is reproduced in the notices file", () => {
  const notices = readFileSync(join(ROOT, "public/THIRD-PARTY-NOTICES.txt"), "utf8");
  for (const name of ["LICENSE", "LICENSE.chromium"]) {
    const lines = readFileSync(join(RUNNER, name), "utf8")
      .split(/\r?\n/)
      .filter((l) => l.trim().length > 0);
    expect(lines.length).toBeGreaterThan(0);
    for (const line of lines) expect(notices).toContain(line);
  }
});
