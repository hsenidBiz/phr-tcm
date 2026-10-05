/**
 * DEV-ONLY CAPTURE MODE - a localStorage flag the screenshot script
 * (scripts/docs-shots.mjs) flips on before reloading, so a shot never
 * carries anything a normal install does not show: the DEV BUILD panel, the
 * hidden Extras card, or an unprompted tour, "what's new", update banner or
 * session-expired modal covering the screen. Auto Run and API Templates are
 * shown, as Enable Advanced Features shows them (lib/extras.ts).
 *
 * Unlike dev/demo.ts, this module has no module-level side effects (no
 * dataset built at import time) - it is safe as a plain, always-present
 * import from files that ship in release too (lib/extras.ts, App.tsx),
 * because `import.meta.env.DEV` is a compile-time constant: Vite folds it to
 * `false` in a `tauri build`, which makes the body below unreachable past
 * the first line and drops out under minification, the same way
 * `AUTO_RUN_DEV` and `DEV_TOOLS` already do elsewhere in this app.
 */
const CAPTURE_KEY = "tcm-v2-dev-capture";

/** `VITE_DOCS_CAPTURE` is set only by `npm run docs:dev`
 * (scripts/docs-dev.mjs), in the dev server's environment. The flag above
 * lives in the app's local storage, which a plain `tauri dev` shares, and a
 * capture stopped before WebView2 had saved its restore left the flag on:
 * the next ordinary dev session opened with sample names and no DEV panel.
 * Without the marker the flag is ignored, so that can no longer happen. */
/** True only in a dev build started by `npm run docs:dev`, with the flag
 * set to "on". Always false the moment `import.meta.env.DEV` is false, so a
 * release build - and any code path only reachable there - never depends
 * on capture mode at all. */
export function isCaptureMode(): boolean {
  if (!import.meta.env.DEV || import.meta.env.VITE_DOCS_CAPTURE !== "1") return false;
  try {
    return localStorage.getItem(CAPTURE_KEY) === "on";
  } catch {
    return false;
  }
}
