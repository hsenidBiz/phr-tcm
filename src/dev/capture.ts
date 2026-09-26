/**
 * DEV-ONLY CAPTURE MODE - a localStorage flag the screenshot script
 * (scripts/docs-shots.mjs) flips on before reloading, so a shot never
 * carries anything a normal install does not show: the DEV BUILD panel, an
 * unlock-gated sidebar entry (Auto Run), or an unprompted tour, "what's
 * new", update banner or session-expired modal covering the screen.
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

/** True only in a dev build with the flag set to "on". Always false the
 * moment `import.meta.env.DEV` is false, so a release build - and any code
 * path only reachable there - never depends on capture mode at all. */
export function isCaptureMode(): boolean {
  if (!import.meta.env.DEV) return false;
  try {
    return localStorage.getItem(CAPTURE_KEY) === "on";
  } catch {
    return false;
  }
}
