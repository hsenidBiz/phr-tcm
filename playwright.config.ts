import { defineConfig } from "@playwright/test";

/**
 * Visual regression only - the behavioral suite is vitest. Run with
 * `npm run visual`; refresh goldens after an INTENTIONAL look change with
 * `npm run visual:update` and commit the new PNGs alongside the change.
 *
 * Deliberately separate from `npm test` (and therefore from the release
 * gate): screenshot diffs need a real browser and a running dev server,
 * and a 2% pixel drift should block a look-change review, not a hotfix.
 */
export default defineConfig({
  testDir: "./visual",
  timeout: 60_000,
  // Screenshots must come from one machine profile to be comparable.
  workers: 1,
  fullyParallel: false,
  use: {
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 1,
  },
  expect: {
    toHaveScreenshot: {
      // Absorbs antialiasing and the odd relative-time label ("6d ago")
      // without hiding real layout drift.
      maxDiffPixelRatio: 0.02,
      animations: "disabled",
    },
  },
  webServer: {
    command: "npm run dev",
    port: 1420,
    reuseExistingServer: true,
    timeout: 60_000,
  },
});
