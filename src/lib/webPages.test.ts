/**
 * The pages under src-tauri/web open in whatever browser the user has, as
 * plain files: their scripts are ES5, and their colours come from the
 * palette the app writes into each page.
 */
import { readdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const webDir = resolve(here, "../../src-tauri/web");
const scripts = readdirSync(webDir).filter((f) => f.endsWith(".js"));

/**
 * The code with comments removed, so prose ("let the page...") never counts,
 * and with a pattern string handed to `new RegExp(...)` blanked out too. That
 * string is content, not syntax: unlike a literal `/…/u` regex or a bare
 * `\p{}` escape, which fail to PARSE on an engine old enough to lack them
 * (before the script using them ever runs), a string built at runtime only
 * risks failing when the RegExp constructor reads it - which is exactly why
 * that call is guarded by a try/catch (see cases-specs.js's UNICODE_BASE).
 */
function code(src: string): string {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/(^|\s)\/\/.*$/gm, "$1")
    .replace(/new RegExp\([^)]*\)/g, "new RegExp()");
}

const NOT_ES5: Array<[string, RegExp]> = [
  ["arrow function", /=>/],
  ["template literal", /`/],
  ["let", /\blet\s+[A-Za-z_$[{]/],
  ["const", /\bconst\s+[A-Za-z_$[{]/],
  ["class", /\bclass\s+[A-Za-z_$]/],
  ["spread / rest", /\.\.\.[A-Za-z_$[{(]/],
  ["for...of", /\bfor\s*\([^;)]*\bof\b/],
  ["Unicode property escape", /\\p\{/],
  ["u-flag regex", /\/[gimsy]*u[gimsy]*(?=\s*[.,;)\]}])/],
];

describe("page scripts are ES5", () => {
  test("there are scripts to check", () => {
    expect(scripts.length).toBeGreaterThan(0);
  });
  test.each(scripts)("%s", (file) => {
    const src = code(readFileSync(resolve(webDir, file), "utf8"));
    expect(NOT_ES5.filter(([, re]) => re.test(src)).map(([name]) => name)).toEqual([]);
  });
});

test("cases-page.css takes shadows and muted text from the palette", () => {
  const css = readFileSync(resolve(webDir, "cases-page.css"), "utf8");
  expect(css).not.toMatch(/rgba\(/);
  expect(css).not.toMatch(/#444\b/);
  expect(css).toMatch(/box-shadow: var\(--shadow\)/);
});

test("cases-page.css lets the search bar wrap and long words break", () => {
  const css = readFileSync(resolve(webDir, "cases-page.css"), "utf8");
  expect(css).toMatch(/\.searchbar \{[^}]*flex-wrap: wrap/);
  expect(css).toMatch(/\.case h2 \{[^}]*overflow-wrap: anywhere/);
  expect(css).toMatch(/\ntd \{[^}]*overflow-wrap: anywhere/);
  expect(css).toMatch(/\.side \{[^}]*overflow: auto/);
  expect(css).toMatch(/\.specs \{[^}]*min-height: 240px/);
});
