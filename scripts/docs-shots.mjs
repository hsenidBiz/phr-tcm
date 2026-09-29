// `npm run docs:shots`: captures every documented screen of the help site,
// in both themes, from the running dev app, and records where each
// documented control sits on its shot.
//
//   docs-site/shots/light/<shot>.jpg, docs-site/shots/dark/<shot>.jpg
//   docs-site/shots/positions.json   { [shot]: { size: {w,h}, controls: { [control]: {x,y,w,h} } } }
//
// Main-window shots are taken at 1440x900; a shot that sets `size` (the
// runner window) is taken at that size, its real one.
//
// The review page and its Test map are pages the app writes for the
// browser, so a `reviewPage` step has the running app write them from a
// fixed sample (src/dev/reviewSample.ts) and opens them in Microsoft Edge
// at the shot's size - Edge ships with Windows, and Playwright drives it
// the same way it drives the app's own WebView2.
//
// The shots and controls come from the help content itself
// (docs-site/src/content/), loaded through Vite, so what is documented and
// what is captured cannot drift apart. A control that cannot be found on
// its shot fails the run by name, and nothing is written.
//
// The app must be running with `npm run docs:dev` (the dev app with
// WebView2's CDP port open). The run drives the OWNER'S dev profile: it
// switches on sample data, capture mode and a theme through localStorage.
// So before it changes anything it snapshots every localStorage entry (also
// to a file in the temp folder, in case the process is killed outright),
// and it puts every entry back exactly - on success, on failure and on
// Ctrl+C - then reloads the app. It never clears localStorage wholesale.
// No sign-in is needed, or wanted: signed out, nothing reaches Azure
// DevOps, so no real data can land in a shot.
//
//   --validate  check the content and positions.json only (no app needed)
//   --dry-run   walk every route and find every control, light theme only, save nothing
//   --only a,b  capture just these shots (both themes) and update only their
//               entries in positions.json; the snapshot/restore is the same
//   --restore   put the settings back from an interrupted run's snapshot file
//               (decided before the content is loaded, so it works even
//               while the content is broken mid-edit)

import { existsSync, mkdirSync, readFileSync, rmSync, unlinkSync, writeFileSync, copyFileSync, readdirSync, mkdtempSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { setTimeout as sleep } from "node:timers/promises";
import { chromium } from "playwright-core";
import {
  CDP_URL,
  DRIFT_PX,
  KEYS,
  MAIN_SIZE,
  NO_SIGN_IN_NOTICE,
  PASSES,
  RELOAD_NOTICE,
  SETTLE_MS,
  SHOT_SCALE,
  START_APP_HINT,
  START_APP_MESSAGE,
  USAGE,
  boxDrift,
  boxInShot,
  formatMissing,
  locatorFor,
  mergePositions,
  parseArgs,
  pickShots,
  passStorage,
  planIsEmpty,
  preflight,
  restoreInOrder,
  restorePlan,
  roundBox,
  runStep,
  serializePositions,
  shotSize,
  viewportOf,
  visibleOnly,
} from "./docs-shots-lib.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const docsRoot = join(repo, "docs-site");
const shotsDir = join(docsRoot, "shots");
const BACKUP = join(tmpdir(), "tcm-docs-shots-settings-backup.json");

/** How long to leave the app running after the restore, so WebView2 has
 *  written it to disk (it batches local storage writes a few seconds). */
const RESTORE_FLUSH_MS = 10_000;

class Interrupted extends Error {}

// ------------------------------------------------------------- content

/** The registry and its problems, through Vite (the content is TypeScript). */
async function loadContent() {
  const { createServer } = await import("vite");
  const server = await createServer({
    configFile: false,
    root: docsRoot,
    appType: "custom",
    logLevel: "error",
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
  });
  try {
    const { screens } = await server.ssrLoadModule("/src/content/index.ts");
    const { validateContent, positionsProblems } = await server.ssrLoadModule("/src/validate.ts");
    return { screens, problems: validateContent({ screens }), positionsProblems };
  } finally {
    await server.close();
  }
}

// ------------------------------------------------------------- the app

const allPages = (browser) => browser.contexts().flatMap((c) => c.pages());
const isRunner = (page) => page.url().includes("#runner");

async function readStorage(page) {
  return page.evaluate(() => {
    const out = {};
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k !== null) out[k] = localStorage.getItem(k) ?? "";
    }
    return out;
  });
}

async function applyPlan(page, plan) {
  await page.evaluate(({ set, remove }) => {
    for (const [k, v] of Object.entries(set)) localStorage.setItem(k, v);
    for (const k of remove) localStorage.removeItem(k);
  }, plan);
}

/** The app shell is up: the sidebar is on screen. */
async function waitForApp(page) {
  await page.getByRole("navigation").first().waitFor({ state: "visible", timeout: 30_000 });
}

async function settle(page) {
  await page.waitForLoadState("networkidle").catch(() => {});
  await sleep(SETTLE_MS);
}

/** Close every runner window the way a person would, through its own
 *  Close button, after putting away any dialog open over it (the bug form
 *  covers the button). A window that still will not close is asked to
 *  through the same window call the button makes.
 *
 *  Never close a runner page over CDP: that ends the page but leaves the
 *  app's runner WINDOW behind, empty, and the app then focuses that window
 *  instead of opening a new runner - every later runner shot fails. */
async function closeRunners(browser) {
  const closeEvent = (page) =>
    page.waitForEvent("close", { timeout: 5_000 }).then(
      () => true,
      () => false,
    );
  for (const page of allPages(browser).filter(isRunner)) {
    let closed = closeEvent(page);
    await page.keyboard.press("Escape").catch(() => {});
    const clicked = await page
      .getByRole("button", { name: "Close runner", exact: true })
      .click({ timeout: 3_000 })
      .then(
        () => true,
        () => false,
      );
    if (clicked && (await closed)) continue;
    closed = closeEvent(page);
    await page.evaluate(() => window.__TAURI_INTERNALS__.invoke("plugin:window|close", { label: "runner" })).catch(() => {});
    if (!(await closed) && !page.isClosed()) throw new Error("the runner window would not close - close it by hand, then run this again");
  }
}

/** A runner window the app still has, with no page in it: left behind by
 *  an older run that closed the page over CDP. The app would focus it
 *  instead of opening a runner, so no runner shot could be taken. */
async function hasEmptyRunnerWindow(browser, main) {
  if (allPages(browser).some(isRunner)) return false;
  const labels = await main.evaluate(() => window.__TAURI_INTERNALS__?.invoke("plugin:window|get_all_windows")).catch(() => null);
  return Array.isArray(labels) && labels.includes("runner");
}

/** Saves `page` as a JPEG at SHOT_SCALE image pixels per CSS pixel, at
 *  `size`. The layout is unchanged - only the density - so the boxes
 *  measured afterwards are the same CSS pixels as before.
 *
 *  Captured through the browser itself, not Playwright's screenshot:
 *  Playwright re-applies its own viewport emulation for a screenshot, which
 *  put the density back to 1 and saved the shot at 1440x900 again. */
async function captureSharp(page, size, path) {
  const cdp = await page.context().newCDPSession(page);
  try {
    await cdp.send("Emulation.setDeviceMetricsOverride", {
      width: size.w,
      height: size.h,
      deviceScaleFactor: SHOT_SCALE,
      mobile: false,
    });
    // Let the page re-raster at the new density before the picture.
    await sleep(SETTLE_MS);
    const { data } = await cdp.send("Page.captureScreenshot", {
      format: "jpeg",
      quality: 82,
      clip: { x: 0, y: 0, width: size.w, height: size.h, scale: 1 },
    });
    writeFileSync(path, Buffer.from(data, "base64"));
  } finally {
    await cdp.detach().catch(() => {});
  }
}

// ---- the review page ------------------------------------------------------

/** Where the sample's case file and spec are written for the app to read. */
const REVIEW_DIR = join(tmpdir(), "tcm-docs-review");
/** Edge, launched on the first review page shot and closed with the run. */
let edge = null;
/** The review page of the current shot, closed by the next reset. */
let reviewContext = null;

/** The newest file in the temp folder matching `pattern` written since
 *  `since` - the app names its pages after its own process id. */
function newestWritten(pattern, since) {
  const dir = tmpdir();
  const hits = readdirSync(dir)
    .filter((n) => pattern.test(n))
    .map((n) => ({ n, t: statSync(join(dir, n)).mtimeMs }))
    .filter((f) => f.t >= since)
    .sort((a, b) => b.t - a.t);
  return hits.length ? join(dir, hits[0].n) : null;
}

/** Have the app write its review page from the sample, then open it - or
 *  the Test map beside it - in Edge at `size`. Written afresh for every
 *  shot, so each theme pass gets the page in that pass's theme. */
async function openReview(main, kind, size) {
  edge ??= await chromium.launch({ channel: "msedge", headless: true }).catch((e) => {
    throw new Error(`Microsoft Edge could not be started for the review page: ${firstLine(e)}`);
  });
  mkdirSync(REVIEW_DIR, { recursive: true });
  const sample = await main.evaluate(async () => {
    const m = await import("/src/dev/reviewSample.ts");
    return { spec: m.REVIEW_SAMPLE_SPEC, cases: m.REVIEW_SAMPLE_CASES, shownPath: m.REVIEW_SAMPLE_SHOWN_PATH };
  });
  // The spec pane reads the spec from beside the case file.
  writeFileSync(join(REVIEW_DIR, "leave-spec.md"), sample.spec);
  writeFileSync(join(REVIEW_DIR, "leave-cases.json"), JSON.stringify(sample.cases, null, 2));
  const since = Date.now() - 1_000;
  await main.evaluate(async (dir) => {
    const m = await import("/src/dev/reviewSample.ts");
    await m.writeReviewSample(dir);
  }, REVIEW_DIR);
  const file =
    kind === "map"
      ? newestWritten(/^test-map-draft-\d+\.html$/, since)
      : newestWritten(/^test-cases-draft-\d+\.html$/, since);
  if (!file) throw new Error(`the app did not write the ${kind === "map" ? "Test map" : "review page"}`);
  await reviewContext?.close().catch(() => {});
  reviewContext = await edge.newContext({ viewport: viewportOf(size) });
  const page = await reviewContext.newPage();
  await page.goto(pathToFileURL(file).href, { waitUntil: "load" });
  // The spec pane names the file's real path, and a temp folder's path
  // names the Windows user: the shots show the sample's own instead.
  await page.evaluate((shown) => {
    for (const el of document.querySelectorAll(".spec-path")) {
      el.textContent = shown;
      el.setAttribute("title", shown);
    }
  }, sample.shownPath);
  return page;
}

/** The runner window, once the route has opened it, at the shot's size. */
async function waitForRunner(browser, size) {
  for (let i = 0; i < 75; i++) {
    const runner = allPages(browser).find(isRunner);
    if (runner) {
      await runner.waitForLoadState("load");
      await runner.setViewportSize(viewportOf(size));
      return runner;
    }
    await sleep(200);
  }
  throw new Error("the runner window did not open");
}

/** Where a control sits, or why it cannot be placed. Hidden copies (a
 *  collapsed menu, a closed drawer kept in the DOM) are not a second match. */
async function place(target, locate, size) {
  const all = locatorFor(target, locate);
  const shown = visibleOnly(all);
  const n = await shown.count();
  if (n === 0) return { reason: (await all.count()) === 0 ? "not found" : "not visible" };
  if (n > 1) return { reason: `matches ${n} visible elements, make the locate more specific` };
  const box = await shown.boundingBox();
  if (!box || !boxInShot(box, size)) return { reason: "outside the shot" };
  return { box };
}

/** Waits until the shot's controls stop moving: two readings of every box,
 *  STILL_GAP_MS apart, that agree. Content that arrives late (the app log,
 *  sample answers on a delay) or an entry animation still running would
 *  otherwise leave the picture and the measured boxes describing two
 *  different moments - and the two theme passes caught at different ones.
 *  Gives up after STILL_TRIES readings and lets the drift check judge. */
const STILL_GAP_MS = 150;
const STILL_TRIES = 20;
async function untilStill(target, controls, size) {
  const read = async () =>
    JSON.stringify(await Promise.all(controls.map(async (c) => {
      const r = await place(target, c.locate, size).catch(() => ({}));
      return r.box ? roundBox(r.box) : null;
    })));
  let last = await read();
  for (let i = 0; i < STILL_TRIES; i++) {
    await sleep(STILL_GAP_MS);
    const now = await read();
    if (now === last) return;
    last = now;
  }
}

/** The leftover settings snapshot, if an earlier run was interrupted. */
function readBackup() {
  if (!existsSync(BACKUP)) return null;
  try {
    return JSON.parse(readFileSync(BACKUP, "utf8"));
  } catch {
    return { unreadable: true };
  }
}

const startAppMessage = () => `${START_APP_MESSAGE}\n${START_APP_HINT}`;

// ------------------------------------------------------------- run

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.mode === "help") {
    if (args.error) console.error(args.error);
    console.log(USAGE);
    return args.error ? 1 : 0;
  }

  // A leftover settings snapshot is dealt with first - before the content
  // is loaded - so `--restore` works even while the content is broken.
  const backup = args.mode === "validate" ? null : readBackup();
  if (backup?.unreadable) {
    console.error(`The settings snapshot ${BACKUP} cannot be read. Delete it, then run this again.`);
    return 1;
  }
  const pre = preflight(args, backup, BACKUP, Date.now());
  if (pre.action === "exit") {
    (pre.code ? console.error : console.log)(pre.message);
    return pre.code;
  }
  if (pre.action === "restore") return connectAndDrive("restore", [], backup);

  const { screens, problems, positionsProblems } = await loadContent();
  if (problems.length) {
    console.error(`The help content has ${problems.length} problem(s):\n${problems.map((p) => `  ${p}`).join("\n")}`);
    return 1;
  }
  const allShots = screens.flatMap((screen) => screen.shots.map((shot) => ({ screen, shot })));
  const { shots, unknown } = pickShots(allShots, args.only);
  if (unknown.length) {
    console.error(`--only names shot(s) the help content does not have: ${unknown.join(", ")}`);
    return 1;
  }
  const controlCount = screens.reduce((n, s) => n + s.controls.length, 0);
  if (args.mode === "validate") {
    const positionsFile = join(shotsDir, "positions.json");
    const stale = existsSync(positionsFile) ? positionsProblems(screens, JSON.parse(readFileSync(positionsFile, "utf8"))) : [];
    if (stale.length) {
      console.error(`positions.json is out of date (run \`npm run docs:shots\`):\n${stale.map((p) => `  ${p}`).join("\n")}`);
      return 1;
    }
    console.log(`Help content OK: ${screens.length} screen(s), ${allShots.length} shot(s), ${controlCount} control(s).`);
    return 0;
  }
  return connectAndDrive(args.mode, shots, null, Boolean(args.only));
}

async function connectAndDrive(mode, shots, backup, only = false) {
  const browser = await chromium.connectOverCDP(CDP_URL, { timeout: 5_000 }).catch(() => null);
  if (!browser) {
    console.error(startAppMessage());
    return 1;
  }
  try {
    return await drive(browser, mode, shots, backup, only);
  } finally {
    await edge?.close().catch(() => {});
    await browser.close().catch(() => {});
  }
}

async function drive(browser, mode, shots, backup, only) {
  const main = allPages(browser).find((p) => !isRunner(p) && /^https?:/.test(p.url()));
  if (!main) {
    console.error(`${startAppMessage()}\n(The app is reachable, but its main window is not.)`);
    return 1;
  }
  // Playwright's 30 s default makes one wrong locate cost half a minute.
  browser.contexts().forEach((c) => c.setDefaultTimeout(15_000));

  // ---- the snapshot, and how everything is put back
  let snapshot;
  let homeUrl;
  if (mode === "restore") {
    ({ storage: snapshot, url: homeUrl } = backup);
  } else {
    if (allPages(browser).some(isRunner)) {
      console.error("Close the test runner window first, then run this again.");
      return 1;
    }
    if (await hasEmptyRunnerWindow(browser, main)) {
      console.error(
        "The app still has an empty test runner window from an earlier run, so no runner could be opened.\n" +
          "Close that window (or stop `npm run docs:dev` and start it again), then run this again.",
      );
      return 1;
    }
    console.log(`${NO_SIGN_IN_NOTICE}\n${RELOAD_NOTICE}\n`);
    homeUrl = main.url();
    snapshot = await readStorage(main);
    writeFileSync(BACKUP, JSON.stringify({ at: Date.now(), url: homeUrl, storage: snapshot }));
  }

  let stopping = false;
  let restoring = null;
  const restore = () =>
    (restoring ??= (async () => {
      stopping = true;
      // The runner is closed first, but a runner that will not close must
      // never keep the settings from coming back: restoreInOrder puts them
      // back whatever closing it did, and the problem is reported after.
      const runnerProblem = await restoreInOrder({ closeRunners: () => closeRunners(browser), putBack });
      if (runnerProblem) console.error(`Settings restored, but ${runnerProblem.message}.`);
    })());

  async function putBack() {
    // Put the entries back from a same-origin page that runs none of the
    // app, so nothing the app does while unloading can write over them.
    // Vite serves /@vite/client as a plain script document.
    let inert = true;
    try {
      await main.goto(new URL("/@vite/client", homeUrl).href, { waitUntil: "load" });
    } catch {
      inert = false;
    }
    // A write the app made just before it was left can land after the
    // first pass here (seen live: a sample project's field choice came
    // back once), so keep going until the entries stay put for a moment.
    let left;
    for (let attempt = 0; attempt < 10; attempt++) {
      left = restorePlan(snapshot, await readStorage(main));
      if (planIsEmpty(left)) {
        await sleep(750);
        left = restorePlan(snapshot, await readStorage(main));
        if (planIsEmpty(left)) break;
      }
      await applyPlan(main, left);
      await sleep(300);
    }
    left = restorePlan(snapshot, await readStorage(main));
    if (!planIsEmpty(left)) {
      const why = inert ? "" : " (the app could not be left for a blank page, so it may have kept writing)";
      throw new Error(`could not put back: ${[...Object.keys(left.set), ...left.remove].join(", ")}${why}`);
    }
    if (inert) await main.goto(homeUrl, { waitUntil: "load" });
    else await main.reload({ waitUntil: "load" });
    // The shot-size viewport is an emulation that ends when this script
    // disconnects; clearing it here as well is belt and braces.
    const cdp = await main.context().newCDPSession(main).catch(() => null);
    await cdp?.send("Emulation.clearDeviceMetricsOverride").catch(() => {});
    await cdp?.detach().catch(() => {});
    // WebView2 writes local storage to disk in batches, seconds after a
    // change. An app stopped straight after a capture (docs:dev's terminal
    // closed, its task killed) lost the whole restore that way, and the
    // next `tauri dev` opened in capture mode with the capture's theme.
    // Wait out the batch before saying the settings are back.
    console.log("Saving the restored settings...");
    await sleep(RESTORE_FLUSH_MS);
    // The settings are back: a snapshot file that will not delete is a
    // warning, never a failed restore.
    try {
      unlinkSync(BACKUP);
    } catch (e) {
      console.error(`Settings restored, but ${BACKUP} could not be deleted (${e.message}). Delete it by hand.`);
    }
  }

  if (mode === "restore") {
    await restore();
    console.log("The app's settings are back as they were.");
    return 0;
  }

  // Shots are taken into a temp folder and only copied into docs-site/shots/
  // once every control was found, so a failed run leaves the last good set.
  const staging = mode === "capture" ? mkdtempSync(join(tmpdir(), "tcm-docs-shots-")) : null;
  const dropStaging = () => staging && rmSync(staging, { recursive: true, force: true });

  const onSignal = async () => {
    if (restoring) {
      console.error("\nStill putting the app's settings back - one moment.");
      return;
    }
    console.error("\nStopping - putting the app's settings back...");
    try {
      await restore();
      console.error("Settings restored.");
      dropStaging();
      process.exit(130);
    } catch (e) {
      console.error(`Could not restore the settings: ${e.message}\nRun \`npm run docs:shots -- --restore\` with the app open.`);
      dropStaging();
      process.exit(1);
    }
  };
  const signals = ["SIGINT", "SIGTERM", "SIGHUP"];
  for (const s of signals) process.on(s, onSignal);

  const guard = () => {
    if (stopping) throw new Interrupted("stopped");
  };

  let code = 1;
  try {
    code = await capture({ browser, main, mode, shots, staging, guard, only });
  } catch (e) {
    // Once a signal has started the restore, whatever the capture was
    // doing fails as the page moves under it - that is not news.
    if (!(e instanceof Interrupted) && !stopping) console.error(`Capture failed: ${e.message ?? e}`);
  } finally {
    try {
      await restore();
      console.log("The app's settings are back as they were.");
    } catch (e) {
      console.error(`Could not restore the settings: ${e.message}\nRun \`npm run docs:shots -- --restore\` with the app open.`);
      code = 1;
    }
    for (const s of signals) process.off(s, onSignal);
    dropStaging();
  }
  return code;
}

async function capture({ browser, main, mode, shots, staging, guard, only }) {
  const passes = mode === "dry-run" ? PASSES.slice(0, 1) : PASSES;
  const positions = {};
  const missing = [];
  const routeFailures = [];

  /** Back to a known start: no runner window, a fresh load of the app. */
  const reset = async () => {
    guard();
    await reviewContext?.close().catch(() => {});
    reviewContext = null;
    await closeRunners(browser);
    await main.reload({ waitUntil: "load" });
    await waitForApp(main);
  };

  for (const [passIndex, pass] of passes.entries()) {
    guard();
    console.log(`${pass.dir} (${pass.theme})`);
    await applyPlan(main, passStorage(pass.theme));
    guard();
    // Sample data goes on through the app's own switch, which also selects
    // a sample PBI (and reloads). Entering it from here keeps the script on
    // the app's exact keys instead of a copy of them.
    const sampleOn = await main.evaluate((k) => localStorage.getItem(k) === "on", KEYS.demo);
    if (!sampleOn) {
      const loaded = main.waitForEvent("load", { timeout: 30_000 });
      try {
        await main.evaluate(async () => {
          const m = await import("/src/dev/demo.ts");
          setTimeout(() => m.toggleDemoMode(), 0);
        });
      } catch (e) {
        loaded.catch(() => {});
        throw new Error(`could not switch the app to sample data: ${firstLine(e)}`);
      }
      await loaded;
    }
    for (const { screen, shot } of shots) {
      // Routes start in the main window at its own size; a shot of the
      // main window that sets a size is taken at that, and one that ends in
      // the runner gets the size on the runner (waitForRunner).
      const size = shotSize(shot);
      const inRunner = shot.route.some((step) => "runnerWindow" in step);
      await main.setViewportSize(viewportOf(inRunner ? MAIN_SIZE : size));
      await reset();
      const controls = screen.controls.filter((c) => c.shot === shot.id);
      let target = main;
      try {
        for (const [i, step] of shot.route.entries()) {
          guard();
          try {
            target = await runStep(
              {
                page: target,
                openRunner: () => waitForRunner(browser, size),
                openReview: (kind) => openReview(main, kind, size),
              },
              step,
            );
          } catch (e) {
            if (e instanceof Interrupted) throw e;
            throw new Error(`route step ${i + 1} ${JSON.stringify(step)}: ${firstLine(e)}`);
          }
        }
      } catch (e) {
        if (e instanceof Interrupted) throw e;
        routeFailures.push(`  ${shot.id}: ${e.message}`);
        console.log(`  ${shot.id}  FAILED`);
        continue;
      }
      guard();

      // Shoot once everything the shot documents has drawn (sample data
      // answers after a short delay, which network idle does not see).
      await Promise.all(
        controls.map((c) => visibleOnly(locatorFor(target, c.locate)).first().waitFor({ state: "visible", timeout: 10_000 }).catch(() => {})),
      );
      // The pointer stays where the route last clicked, and a tooltip or a
      // hover style there would land in the shot: park it on the window's
      // top-left corner (the title bar / the runner's name), where nothing
      // reacts to it.
      await target.mouse.move(2, 2).catch(() => {});
      await settle(target);
      await untilStill(target, controls, size);
      let placed = 0;
      if (passIndex === 0) positions[shot.id] = { size, controls: {} };
      for (const control of controls) {
        const r = await place(target, control.locate, size);
        if (!r.box) {
          missing.push({ shot: shot.id, control: control.id, locate: control.locate, reason: `${r.reason} (${pass.dir})` });
          continue;
        }
        placed++;
        // Layout is the same in both themes: record from the first pass,
        // and hold the others to it. A box that moved between passes means
        // the shots differ (a "just now" that became "2m ago" once moved a
        // whole row), and the markers would be off in one theme.
        const box = roundBox(r.box);
        if (passIndex === 0) positions[shot.id].controls[control.id] = box;
        else {
          const first = positions[shot.id]?.controls[control.id];
          if (first && boxDrift(first, box) > DRIFT_PX) {
            missing.push({
              shot: shot.id,
              control: control.id,
              locate: control.locate,
              reason: `moved between passes: ${JSON.stringify(first)} in ${passes[0].dir}, ${JSON.stringify(box)} in ${pass.dir}`,
            });
          }
        }
      }
      // Photographed AFTER the boxes are measured. Leaving the capture's
      // density override also drops Playwright's viewport (it is set the
      // same way), and the page falls back to its real window. The app's
      // window is the shot's size anyway, but headless Edge's is 1410x805:
      // measured after the picture, the review page had re-laid itself out
      // narrower and every box sat 30 px beside what the picture shows. The
      // override itself changes only the density, so boxes measured now
      // describe exactly what it photographs.
      guard();
      if (staging) {
        const dir = join(staging, pass.dir);
        mkdirSync(dir, { recursive: true });
        await captureSharp(target, size, join(dir, `${shot.id}.jpg`));
      }

      console.log(`  ${shot.id}  ${placed}/${controls.length} controls`);
    }
  }

  if (routeFailures.length || missing.length) {
    if (routeFailures.length) console.error(`\nThese shots could not be reached:\n${routeFailures.join("\n")}`);
    if (missing.length) console.error(`\nThese controls could not be placed (shot / control / locate):\n${formatMissing(missing)}`);
    console.error(mode === "dry-run" ? "\nDry run: nothing was saved." : "\nNothing was saved.");
    return 1;
  }

  if (mode === "dry-run") {
    console.log(`\nDry run OK: ${shots.length} shot(s) reached, every control found. Nothing was saved.`);
    return 0;
  }

  // Everything was found: publish the shots and the positions together.
  for (const pass of passes) {
    const from = join(staging, pass.dir);
    if (!existsSync(from)) continue;
    const to = join(shotsDir, pass.dir);
    mkdirSync(to, { recursive: true });
    for (const f of readdirSync(from)) copyFileSync(join(from, f), join(to, f));
  }
  // A `--only` run replaces just its own shots' entries; a full run writes
  // the whole file (which also drops entries of shots no longer documented).
  const positionsFile = join(shotsDir, "positions.json");
  const partial = only && existsSync(positionsFile);
  const merged = partial ? mergePositions(JSON.parse(readFileSync(positionsFile, "utf8")), positions) : positions;
  writeFileSync(positionsFile, serializePositions(merged));
  console.log(`\nSaved ${shots.length} shot(s) per theme and ${partial ? "their entries in " : ""}positions.json.`);
  return 0;
}

const firstLine = (e) => String(e?.message ?? e).split("\n")[0];

process.exitCode = await main().catch((e) => {
  console.error(e?.stack ?? e);
  return 1;
});
