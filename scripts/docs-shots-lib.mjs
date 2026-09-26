// The pure parts of scripts/docs-shots.mjs, kept apart so vitest can pin
// them (scripts/docs-shots-lib.test.mjs) without an app to drive: argument
// parsing, what to do about a leftover settings snapshot, the localStorage
// plan for a capture pass and for putting the owner's settings back, how a
// content `locate` / route step becomes a Playwright call, and how
// positions.json is written.
//
// Nothing here touches the disk, the network or a browser: every Playwright
// object arrives as a parameter.

export const CDP_PORT = 9333;
export const CDP_URL = `http://127.0.0.1:${CDP_PORT}`;

export const START_APP_MESSAGE =
  "Start the app with `npm run docs:dev`, wait for it to open, then run this again.";
/** Printed after START_APP_MESSAGE: a second copy of the dev app cannot
 *  open the debugging port the first one already holds. */
export const START_APP_HINT = "If it is already open, close any other running copy of the dev app first.";

/** No sign-in means Azure DevOps is never reached, so nothing real (the
 *  assigned-work check, which starts at sign-in) can land in a shot. */
export const NO_SIGN_IN_NOTICE = "No need to sign in: the capture runs on sample data. Leave the app signed out.";
export const RELOAD_NOTICE =
  "The app will reload several times while this runs; its settings are put back as they were at the end.";

/** The main window's shot size - docs-site/src/types.ts SHOT_WIDTH /
 *  SHOT_HEIGHT. A shot that sets `size` (the runner) is taken at that. */
export const MAIN_SIZE = { w: 1440, h: 900 };

/** @param {{ size?: { w: number, h: number } }} shot */
export const shotSize = (shot) => shot.size ?? MAIN_SIZE;

/** Playwright's viewport shape for a size. */
export const viewportOf = (size) => ({ width: size.w, height: size.h });

/** Time for transitions to finish after the network settles. */
export const SETTLE_MS = 400;

/** A settings snapshot older than this may be stale: restoring it needs
 *  --force, because it would undo whatever was changed since. */
export const STALE_BACKUP_MS = 24 * 60 * 60 * 1000;

/** Theme passes: the app theme to switch to, and the folder under
 *  docs-site/shots/ its images go in (the site's own light/dark names).
 *  Positions are recorded from the first pass only. */
export const PASSES = [
  { theme: "light", dir: "light" },
  { theme: "slate", dir: "dark" },
];

// The app's own localStorage keys (src/dev/capture.ts, src/lib/theme.ts,
// src/lib/sidebarState.ts). Change them together with the app.
export const KEYS = {
  capture: "tcm-v2-dev-capture",
  demo: "tcm-v2-dev-demo",
  themeId: "tcm-v2-theme-id",
  themeLegacy: "tcm-v2-theme",
  accent: "tcm-v2-accent",
  sidebar: "tcm-v2-sidebar",
};

/** @param {string[]} argv process.argv.slice(2) */
export function parseArgs(argv) {
  // `--only a,b` (or `--only=a,b`): shot ids, taken out before the rest.
  let only = null;
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--only" || a.startsWith("--only=")) {
      const value = a === "--only" ? argv[++i] : a.slice("--only=".length);
      const ids = (value ?? "").split(",").map((x) => x.trim()).filter(Boolean);
      if (!value || value.startsWith("--") || ids.length === 0) return { mode: "help", error: "--only needs shot ids, e.g. --only pull-requests-open" };
      only = [...new Set([...(only ?? []), ...ids])];
    } else rest.push(a);
  }
  argv = rest;
  const known = new Set(["--validate", "--dry-run", "--restore", "--force", "--help", "-h"]);
  const unknown = argv.filter((a) => !known.has(a));
  if (unknown.length) return { mode: "help", error: `Unknown option: ${unknown.join(" ")}` };
  if (argv.includes("--help") || argv.includes("-h")) return { mode: "help" };
  const modes = ["--validate", "--dry-run", "--restore"].filter((m) => argv.includes(m));
  if (modes.length > 1) return { mode: "help", error: `Pick one of ${modes.join(", ")}` };
  const mode = modes.length ? modes[0].slice(2) : "capture";
  const force = argv.includes("--force");
  if (force && mode !== "restore") return { mode: "help", error: "--force only goes with --restore" };
  if (only && mode !== "capture" && mode !== "dry-run") return { mode: "help", error: "--only goes with a capture or --dry-run" };
  const out = force ? { mode, force } : { mode };
  return only ? { ...out, only } : out;
}

/**
 * The shots a run takes: all of them, or just the ones `--only` names.
 * @template {{ shot: { id: string } }} T
 * @param {T[]} shots
 * @param {string[] | undefined} only
 * @returns {{ shots: T[], unknown: string[] }}
 */
export function pickShots(shots, only) {
  if (!only) return { shots, unknown: [] };
  const ids = new Set(shots.map((s) => s.shot.id));
  return { shots: shots.filter((s) => only.includes(s.shot.id)), unknown: only.filter((id) => !ids.has(id)) };
}

/**
 * positions.json after a `--only` run: the shots just taken replace their
 * own entries, every other shot's entry is kept as it was.
 * @param {Record<string, unknown>} existing
 * @param {Record<string, unknown>} fresh
 */
export const mergePositions = (existing, fresh) => ({ ...existing, ...fresh });

/**
 * Put the settings back even when closing the runner fails: the runner is
 * closed first (so it cannot write over what is put back), but a runner
 * that will not close must never stop the owner's settings coming back.
 * Returns the runner problem, if there was one, for the caller to report
 * once the settings are safe.
 * @param {{ closeRunners: () => Promise<void>, putBack: () => Promise<void> }} steps
 * @returns {Promise<Error | null>}
 */
export async function restoreInOrder({ closeRunners, putBack }) {
  let runnerProblem = null;
  try {
    await closeRunners();
  } catch (e) {
    runnerProblem = e instanceof Error ? e : new Error(String(e));
  }
  await putBack();
  return runnerProblem;
}

export const USAGE = `Usage: npm run docs:shots [-- <option>]

  (no option)  Capture every documented screen in both themes into
               docs-site/shots/ and record where each control sits.
  --validate   Check the help content only; no app needed.
  --dry-run    Walk every route and find every control, light theme only;
               saves nothing.
  --restore    Put the app's settings back from an interrupted run
               (add --force for a snapshot more than a day old).
  --only <ids> Capture just these shots (comma-separated shot ids), in both
               themes, and update only their entries in positions.json.
               Also works with --dry-run.

The app must be running with \`npm run docs:dev\`.`;

/** "under a minute", "3 minutes", "5 hours", "2 days" - how long ago a
 *  snapshot was taken. */
export function formatAge(ms) {
  const unit = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const min = Math.max(0, Math.floor(ms / 60_000));
  if (min < 1) return "under a minute";
  if (min < 60) return unit(min, "minute");
  const hours = Math.floor(min / 60);
  if (hours < 48) return unit(hours, "hour");
  return unit(Math.floor(hours / 24), "day");
}

/**
 * What to do before anything else - and before the help content is even
 * loaded, so settings can always be put back while the content is broken
 * mid-edit.
 * @param {{ mode: string, force?: boolean }} args
 * @param {null | { at?: number }} backup the leftover snapshot, if any
 * @param {string} backupPath where it is (named in messages)
 * @param {number} now Date.now()
 * @returns {{ action: "content" } | { action: "restore" } | { action: "exit", code: number, message: string }}
 */
export function preflight(args, backup, backupPath, now) {
  if (args.mode === "validate") return { action: "content" };
  const age = backup && typeof backup.at === "number" ? now - backup.at : null;
  const ago = age === null ? "at an unknown time" : `${formatAge(age)} ago`;
  if (args.mode === "restore") {
    if (!backup) return { action: "exit", code: 0, message: "Nothing to restore: no interrupted run left a snapshot behind." };
    if ((age === null || age > STALE_BACKUP_MS) && !args.force) {
      return {
        action: "exit",
        code: 1,
        message:
          `The settings snapshot was taken ${ago}. Restoring it would undo anything changed in the app since.\n` +
          "To restore it anyway, run `npm run docs:shots -- --restore --force` with the app open;\n" +
          `to keep the settings as they are now, delete ${backupPath}.`,
      };
    }
    return { action: "restore" };
  }
  if (backup) {
    return {
      action: "exit",
      code: 1,
      message:
        `An earlier capture (${ago}) was interrupted before it put the app's settings back.\n` +
        "With the app open, run `npm run docs:shots -- --restore` to put them back first\n" +
        `(or delete ${backupPath} to keep the settings as they are now).`,
    };
  }
  return { action: "content" };
}

/**
 * The localStorage writes that set up one theme pass (on top of the app's
 * own demo switch, which the script flips through the app). Everything
 * that changes how a whole shot looks is pinned: the theme, the default
 * accent, the sidebar open.
 * @param {string} theme an app theme id ("light", "slate", ...)
 * @returns {{ set: Record<string, string>, remove: string[] }}
 */
export function passStorage(theme) {
  return {
    set: { [KEYS.capture]: "on", [KEYS.themeId]: theme },
    remove: [KEYS.themeLegacy, KEYS.accent, KEYS.sidebar],
  };
}

/**
 * What to write to turn `current` back into `snapshot`, exactly: keys that
 * changed or disappeared are set back, keys that did not exist are removed,
 * everything else is left alone (never a wholesale clear). Both lists are
 * sorted so the plan is deterministic.
 * @param {Record<string, string>} snapshot localStorage before the run
 * @param {Record<string, string>} current localStorage now
 * @returns {{ set: Record<string, string>, remove: string[] }}
 */
export function restorePlan(snapshot, current) {
  const set = {};
  for (const key of Object.keys(snapshot).sort()) {
    if (current[key] !== snapshot[key]) set[key] = snapshot[key];
  }
  const remove = Object.keys(current)
    .filter((key) => !Object.prototype.hasOwnProperty.call(snapshot, key))
    .sort();
  return { set, remove };
}

/** @param {{ set: Record<string, string>, remove: string[] }} plan */
export const planIsEmpty = (plan) => Object.keys(plan.set).length === 0 && plan.remove.length === 0;

/** Readable one-line form of a locate, for failure lists. */
export function describeLocate(locate) {
  return JSON.stringify(locate);
}

const cssString = (s) => `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;

/**
 * The Playwright locator for a content `locate`, under `root` (a Page or a
 * Locator) - every match, visible or not. Wrap it in `visibleOnly` to act
 * on it.
 */
export function locatorFor(root, locate) {
  if ("role" in locate && "name" in locate) return root.getByRole(locate.role, { name: locate.name, exact: true });
  if ("role" in locate && "nameRe" in locate) return root.getByRole(locate.role, { name: new RegExp(locate.nameRe) });
  if ("label" in locate) return root.getByLabel(locate.label, { exact: true });
  if ("text" in locate) return root.getByText(locate.text, { exact: true });
  if ("testId" in locate) {
    // The app marks elements with data-testid in a few places and
    // data-tour (the interface tour's anchors) in many more.
    const v = cssString(locate.testId);
    return root.locator(`[data-testid=${v}], [data-tour=${v}]`);
  }
  throw new Error(`Unknown locate ${describeLocate(locate)}`);
}

/** Only the matches a person can see: a hidden copy of a control (a
 *  collapsed menu, a closed drawer kept in the DOM) is neither clicked nor
 *  counted as a second match. */
export const visibleOnly = (locator) => locator.filter({ visible: true });

/**
 * A sidebar item by its label, exactly. No " (N new)" badge form: capture
 * runs signed out on sample data, so a badge in a shot would mean real
 * data got in - better to fail on it than to photograph it.
 */
export function navLocator(page, label) {
  return visibleOnly(page.getByRole("navigation").getByRole("button", { name: label, exact: true })).first();
}

/**
 * Run one route step.
 * @param {{ page: any, openRunner: () => Promise<any> }} ctx `page` is the
 *   current target; `openRunner` waits for the runner window and returns it.
 * @returns {Promise<any>} the target the following steps act on
 */
export async function runStep(ctx, step) {
  const { page } = ctx;
  if ("nav" in step) await navLocator(page, step.nav).click();
  else if ("click" in step) await visibleOnly(locatorFor(page, step.click)).click();
  else if ("press" in step) await page.keyboard.press(step.press);
  else if ("waitFor" in step) await visibleOnly(locatorFor(page, step.waitFor)).waitFor({ state: "visible" });
  else if ("scrollTo" in step) await visibleOnly(locatorFor(page, step.scrollTo)).scrollIntoViewIfNeeded();
  else if ("runnerWindow" in step) return ctx.openRunner();
  else throw new Error(`Unknown step ${JSON.stringify(step)}`);
  return page;
}

/** @param {{x:number,y:number,width:number,height:number}} box */
export const roundBox = (box) => ({
  x: Math.round(box.x),
  y: Math.round(box.y),
  w: Math.round(box.width),
  h: Math.round(box.height),
});

/** A marker is drawn at the control, so its centre must be in the shot. */
export function boxInShot(box, size = MAIN_SIZE) {
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  return box.width > 0 && box.height > 0 && cx >= 0 && cy >= 0 && cx <= size.w && cy <= size.h;
}

/**
 * positions.json text: shots in sorted order, each with the size it was
 * captured at and its controls in sorted order, each box as x, y, w, h -
 * so a re-capture only shows real moves in a diff.
 * @param {Record<string, { size: {w:number,h:number}, controls: Record<string, {x:number,y:number,w:number,h:number}> }>} positions
 */
export function serializePositions(positions) {
  const out = {};
  for (const shot of Object.keys(positions).sort()) {
    const { size, controls } = positions[shot];
    const sorted = {};
    for (const control of Object.keys(controls).sort()) {
      const { x, y, w, h } = controls[control];
      sorted[control] = { x, y, w, h };
    }
    out[shot] = { size: { w: size.w, h: size.h }, controls: sorted };
  }
  return `${JSON.stringify(out, null, 2)}\n`;
}

/** The failure list printed when controls cannot be placed. */
export function formatMissing(missing) {
  return missing.map((m) => `  ${m.shot} / ${m.control} / ${describeLocate(m.locate)}: ${m.reason}`).join("\n");
}
