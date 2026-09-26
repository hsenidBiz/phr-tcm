// Where Tailwind looks for class names. Left to find them itself it walks
// every file in the project that git does not ignore - once a stray build
// folder of 384,000 files, which kept `tauri dev` on the loading screen for
// over four minutes on every cold start. These pin it to the app's own
// source, plus the one package whose components ship Tailwind classes.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";

const css = readFileSync(resolve(__dirname, "index.css"), "utf8");

test("Tailwind scans the app's source, not the whole project", () => {
  expect(css).toMatch(/@import\s+"tailwindcss"\s+source\("\.\.\/src"\);/);
  expect(css).not.toMatch(/@import\s+"tailwindcss"\s*;/);
});

test("the page shell and XiodUI's components are scanned too", () => {
  expect(css).toContain('@source "../index.html";');
  expect(css).toContain('@source "../node_modules/xiod-ui/dist";');
});
