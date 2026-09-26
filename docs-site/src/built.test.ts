import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

// Checks the BUILT help site - the file that is committed and embedded in
// the app - not the source. It is opened from disk (file://), where
// Chromium refuses module scripts and there is no network, so it must be
// one self-contained page with a classic script.
// These are required, never skipped: the page is committed and shipped.
const BUILT = resolve(dirname(fileURLToPath(import.meta.url)), "../../src-tauri/help/index.html");
const built = existsSync(BUILT);

describe("built help site (src-tauri/help/index.html - run `npm run docs:build` to produce it)", () => {
  const html = built ? readFileSync(BUILT, "utf8") : "";

  test("exists", () => {
    expect(built).toBe(true);
  });

  test("has no module script - file:// refuses them", () => {
    expect(html).not.toMatch(/type\s*=\s*["']?module/i);
  });

  test("references nothing on the network", () => {
    expect(html).not.toMatch(/\b(?:src|href)\s*=\s*["']?\s*(?:https?:)?\/\//i);
    expect(html).not.toMatch(/url\(\s*["']?\s*(?:https?:)?\/\//i);
  });

  test("has exactly one script, inline", () => {
    const scripts = html.match(/<script\b[^>]*>/gi) ?? [];
    expect(scripts).toHaveLength(1);
    expect(scripts[0]).not.toMatch(/\bsrc\s*=/i);
  });

  test("the script actually carries the site (a build step once emptied it)", () => {
    const body = /<script\b[^>]*>([\s\S]*?)<\/script>/i.exec(html)?.[1] ?? "";
    expect(body.length).toBeGreaterThan(5000);
    expect(body).toContain("Quick start");
  });

  test("inlines its styles and fonts", () => {
    expect(html).not.toMatch(/<link\b[^>]*rel\s*=\s*["']?stylesheet/i);
    expect(html).toMatch(/<style\b/i);
    expect(html).toMatch(/font\/woff2|application\/font-woff2|data:font/i);
  });

  test("carries none of the dev-only sample content", () => {
    expect(html).not.toContain("sample-");
  });
});
