// CSP verification against the REAL packed binary, extending the e2e-smoke
// pattern: launch v2.exe with CDP exposed, then prove four things.
//
//  1. The app boots to the sign-in view. That alone proves IPC survives the
//     CSP - the view only renders after auth_status resolves over
//     http://ipc.localhost, which connect-src must now permit.
//  2. No CSP violation reached the console during boot.
//  3. The policy is actually ENFORCING: an injected inline <script> must
//     not execute (script-src 'self'), and an injected external <img> to a
//     disallowed origin must be refused.
//  4. Runtime-injected <style> still works (Astryx does this for syntax
//     highlighting), and the bundled Fira fonts loaded - i.e. the policy is
//     not silently breaking styling.
//
// Usage: node csp-verify.mjs <path-to-exe>

import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import { chromium } from "playwright-core";

const exe = process.argv[2];
if (!exe) {
  console.error("usage: node csp-verify.mjs <path-to-exe>");
  process.exit(2);
}
const port = 9334;

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
  let browser = null;
  for (let i = 0; i < 40 && !browser; i++) {
    await sleep(500);
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`).catch(() => null);
  }
  if (!browser) throw new Error("CDP endpoint never came up.");

  const pages = browser.contexts().flatMap((c) => c.pages());
  if (pages.length === 0) throw new Error("No pages exposed over CDP.");
  const page = pages[0];

  // Collect every console message from now on. Boot-time violations that
  // happened before we attached are caught separately by the style/font
  // assertions below - a blocked stylesheet or font leaves visible traces.
  const consoleMessages = [];
  page.on("console", (m) => consoleMessages.push(`${m.type()}: ${m.text()}`));

  // (1) Shell boots => IPC works under connect-src.
  await page.waitForSelector("text=Sign in with Microsoft", { timeout: 20000 });

  // (4a) Fonts actually load - a blocked font-src fails these. `load`, not
  // `check` after `ready`: @font-face faces fetch lazily, and a view with
  // no mono text on it never requests Fira Code at all, so `check` says
  // false for a face the CSP would have allowed perfectly well.
  const fonts = await page.evaluate(async () => {
    await Promise.all([
      document.fonts.load("16px 'Fira Sans'"),
      document.fonts.load("16px 'Fira Code'"),
    ]);
    return {
      sans: document.fonts.check("16px 'Fira Sans'"),
      mono: document.fonts.check("16px 'Fira Code'"),
    };
  });
  if (!fonts.sans || !fonts.mono) {
    throw new Error(`Fonts did not load under the CSP: ${JSON.stringify(fonts)}`);
  }

  // (4b) A runtime-injected <style> must apply (Astryx injects these).
  const styleApplied = await page.evaluate(() => {
    const probe = document.createElement("div");
    probe.id = "csp-style-probe";
    document.body.appendChild(probe);
    const s = document.createElement("style");
    s.textContent = "#csp-style-probe { margin-left: 7px; }";
    document.head.appendChild(s);
    const applied = getComputedStyle(probe).marginLeft === "7px";
    probe.remove();
    s.remove();
    return applied;
  });
  if (!styleApplied) {
    throw new Error(
      "Runtime-injected <style> was BLOCKED - Astryx syntax highlighting would break. " +
        "Check that Tauri did not append a nonce to style-src (dangerousDisableAssetCspModification).",
    );
  }

  // (3a) An injected inline <script> must NOT run. This is the whole point
  // of the policy: markup that becomes an element must still not become
  // code. (page.evaluate itself runs via CDP and is exempt - the probe is
  // the <script> ELEMENT it creates, which is subject to the page's CSP.)
  const inlineScriptRan = await page.evaluate(() => {
    delete window.__csp_probe;
    const s = document.createElement("script");
    s.textContent = "window.__csp_probe = 1;";
    document.head.appendChild(s);
    const ran = window.__csp_probe === 1;
    s.remove();
    return ran;
  });
  if (inlineScriptRan) {
    throw new Error("Inline <script> EXECUTED - the CSP is not enforcing script-src.");
  }

  // (3b) An <img> pointed at a disallowed origin must be refused without a
  // network attempt: CSP rejection fires the error event with no fetch.
  const remoteImgBlocked = await page.evaluate(
    () =>
      new Promise((resolve) => {
        const img = document.createElement("img");
        // .invalid never resolves, so if CSP let it through we would hang
        // on DNS - the 3s fallback treats that as "not refused by CSP".
        const t = setTimeout(() => { img.remove(); resolve(false); }, 3000);
        img.onerror = () => { clearTimeout(t); img.remove(); resolve(true); };
        img.onload = () => { clearTimeout(t); img.remove(); resolve(false); };
        img.src = "https://csp-probe.invalid/x.png";
        document.body.appendChild(img);
      }),
  );
  if (!remoteImgBlocked) {
    throw new Error("Remote <img> was not refused - img-src is not enforcing.");
  }

  // (2) The probes above SHOULD have produced exactly their own violation
  // reports; anything else logged is the app tripping over its own policy.
  const violations = consoleMessages.filter((m) => /Content.Security.Policy|Refused to/i.test(m));
  const expected = violations.filter((m) => /__csp_probe|csp-probe\.invalid|inline script/i.test(m));
  const unexpected = violations.filter((m) => !expected.includes(m));
  if (unexpected.length > 0) {
    throw new Error(`Unexpected CSP violations during normal use:\n${unexpected.join("\n")}`);
  }

  console.log("CSP VERIFY OK:");
  console.log("  boot + IPC under connect-src: yes");
  console.log("  Fira Sans / Fira Code loaded: yes");
  console.log("  runtime <style> injection allowed: yes");
  console.log("  inline <script> blocked: yes");
  console.log("  remote <img> blocked: yes");
  console.log(`  probe violations reported by webview: ${expected.length}`);
  await browser.close();
} catch (e) {
  failed = e;
} finally {
  child.kill();
}

if (failed) {
  console.error(`CSP VERIFY FAILED: ${failed.message ?? failed}`);
  process.exit(1);
}
