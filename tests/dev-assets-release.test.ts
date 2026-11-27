// The sample video in src/dev/ is only loaded by dev builds, but Vite still
// emits the file it imports; vite.config.ts drops it from release builds.
// The config is read as text: importing it pulls Vite (esbuild) into jsdom,
// where esbuild refuses to start.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

test("vite.config registers the plugin that keeps src/dev assets out of release builds", () => {
  const config = readFileSync(join(ROOT, "vite.config.ts"), "utf8");
  expect(config).toContain('name: "dev-assets-out-of-release"');
  expect(config).toContain('apply: "build"');
  expect(config).toMatch(/plugins:\s*\[[^\]]*devAssetsOutOfRelease/);
  expect(config).toContain("generateBundle");
});

test("a built dist has no sample-data assets", () => {
  const assets = join(ROOT, "dist/assets");
  if (!existsSync(assets)) return;
  expect(readdirSync(assets).filter((f) => f.startsWith("demo-"))).toEqual([]);
});
