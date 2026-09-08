// E2E smoke against the REAL packed binary: launches the exe with WebView2's
// CDP port exposed, connects with playwright-core, asserts the app shell
// renders (sign-in view - tokens are memory-only so a fresh process is
// always signed out), then exits cleanly.
//
// Usage: node scripts/e2e-smoke.mjs [path-to-exe]
// Default exe: src-tauri/target/release/v2.exe

import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import { chromium } from "playwright-core";

const exe = process.argv[2] ?? "src-tauri/target/release/v2.exe";
const port = 9333;

const child = spawn(exe, [], {
  env: {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
  },
  stdio: "ignore",
  detached: false,
});

let failed = null;
try {
  // Wait for the CDP endpoint to come up.
  let browser = null;
  for (let i = 0; i < 40 && !browser; i++) {
    await sleep(500);
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`).catch(() => null);
  }
  if (!browser) throw new Error("CDP endpoint never came up - is WebView2 honoring the port?");

  const contexts = browser.contexts();
  const pages = contexts.flatMap((c) => c.pages());
  if (pages.length === 0) throw new Error("No pages exposed over CDP.");
  const page = pages[0];

  await page.waitForSelector("text=Sign in with Microsoft", { timeout: 15000 });
  const title = await page.textContent("h1");
  if (!title || !title.includes("Test Case Manager")) {
    throw new Error(`Unexpected shell heading: ${title}`);
  }
  console.log("SMOKE OK: shell rendered, sign-in view present.");
  await browser.close();
} catch (e) {
  failed = e;
} finally {
  child.kill();
}

if (failed) {
  console.error(`SMOKE FAILED: ${failed.message ?? failed}`);
  process.exit(1);
}
