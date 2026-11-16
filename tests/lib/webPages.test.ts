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

/** The code with comments removed, so prose ("let the page...") never counts. */
function stripComments(src: string): string {
  return src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|\s)\/\/.*$/gm, "$1");
}

/**
 * A `new RegExp('pattern', 'flags')` call whose pattern AND flags are both
 * plain quoted strings - nothing else. Blanking only this exact shape (not
 * `new RegExp(...)` in general) is deliberate: that string is content, not
 * syntax. Unlike a literal `/…/u` regex or a bare `\p{}` escape, which fail
 * to PARSE on an engine old enough to lack them (before the script using
 * them ever runs), a plain string only risks failing when the RegExp
 * constructor reads it - and only a call with the shape below ever gets
 * that risk waived, because `unguardedUFlagCalls` (further down) still
 * requires it to sit inside a `try`. A template literal, a spread, an
 * arrow, or an unquoted flags argument does not match this shape, so it is
 * left in place for the checks below to catch.
 */
const GUARDED_REGEXP_CALL = /new RegExp\(\s*'(?:[^'\\]|\\.)*'\s*,\s*'[gimsuy]*'\s*\)/g;

function code(src: string): string {
  return stripComments(src).replace(GUARDED_REGEXP_CALL, "new RegExp()");
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

/** Every `try { ... }` block's span in `src`, as [index of "{", index just past the matching "}"]. */
function tryBlockRanges(src: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  const re = /\btry\s*\{/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(src))) {
    const open = m.index + m[0].length - 1;
    let depth = 1, i = open + 1;
    for (; i < src.length && depth > 0; i++) {
      if (src[i] === "{") depth++;
      else if (src[i] === "}") depth--;
    }
    ranges.push([open, i]);
  }
  return ranges;
}

/**
 * `GUARDED_REGEXP_CALL` blanks a `u`-flag call the same as any other guarded
 * one, which would hide the one thing that actually makes it safe: on an
 * engine that throws building it, the call must be inside a `try`, or the
 * page stops dead instead of falling back. This counts one that is not.
 */
function unguardedUFlagCalls(strippedSrc: string): number {
  const ranges = tryBlockRanges(strippedSrc);
  const re = /new RegExp\(\s*'(?:[^'\\]|\\.)*'\s*,\s*'([gimsuy]*)'\s*\)/g;
  let count = 0, m: RegExpExecArray | null;
  while ((m = re.exec(strippedSrc))) {
    if (!m[1].includes("u")) continue;
    if (!ranges.some(([start, end]) => m!.index >= start && m!.index < end)) count++;
  }
  return count;
}

function findings(rawSrc: string): string[] {
  const stripped = stripComments(rawSrc);
  const found = NOT_ES5.filter(([, re]) => re.test(code(rawSrc))).map(([name]) => name);
  if (unguardedUFlagCalls(stripped) > 0) found.push("unguarded u-flag RegExp");
  return found;
}

describe("page scripts are ES5", () => {
  test("there are scripts to check", () => {
    expect(scripts.length).toBeGreaterThan(0);
  });
  test.each(scripts)("%s", (file) => {
    expect(findings(readFileSync(resolve(webDir, file), "utf8"))).toEqual([]);
  });
});

// The narrow `new RegExp('pattern', 'flags')` exemption above used to blank
// everything up to the call's first ")" - hiding a template literal, a
// spread or an arrow used to build the pattern, and an unguarded `u`-flag
// call, all inside what looked like the same "safe" shape.
describe("the ES5 gate itself", () => {
  test("a template literal inside new RegExp(...) is still caught", () => {
    expect(findings("var r = new RegExp(`^${x}$`, 'u');")).toContain("template literal");
  });
  test("an arrow function inside new RegExp(...) is still caught", () => {
    expect(findings("var r = new RegExp(parts.map(p => p).join(''), 'u');")).toContain("arrow function");
  });
  test("an unguarded u-flag RegExp is still caught", () => {
    expect(findings("var r = new RegExp('[\\\\p{L}]', 'u');")).toContain("unguarded u-flag RegExp");
  });
  test("the same call guarded by a try block is not flagged", () => {
    expect(findings("try { var r = new RegExp('[\\\\p{L}]', 'u'); } catch (e) {}")).toEqual([]);
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
