// Where Tailwind looks for class names. Left to find them itself it walks
// every file in the project that git does not ignore - once a stray build
// folder of 384,000 files, which kept `tauri dev` on the loading screen for
// over four minutes on every cold start. These pin it to the app's own
// source; XiodUI's parts are added one file at a time in xiod-theme.css.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";

const css = readFileSync(resolve(__dirname, "index.css"), "utf8");

test("Tailwind scans the app's source, not the whole project", () => {
  expect(css).toMatch(/@import\s+"tailwindcss"\s+source\("\.\.\/src"\);/);
  expect(css).not.toMatch(/@import\s+"tailwindcss"\s*;/);
});

test("the page shell is scanned too, and XiodUI only part by part", () => {
  expect(css).toContain('@source "../index.html";');
  // The whole package would bring every one of its components' utilities
  // in, used or not (about 215 KB of CSS).
  expect(css).not.toMatch(/@source\s+"[^"]*xiod-ui\/dist"/);
});
