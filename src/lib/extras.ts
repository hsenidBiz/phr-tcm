// This machine's optional extras switch, as the webview sees it. Rust owns
// it (src-tauri/src/extras.rs: saved in the app data dir, read by the AI
// bridge); this mirrors it for the screens. Turned on by a key sequence on
// the Settings screen (lib/extrasSequence.ts), off by that section's Reset
// to default. While it is on, a release build shows Auto Run - its tab and
// its AI tools - the way a development build always does.
//
// Beside it, Settings' Enable Advanced Features switch (`advanced`), saved
// in the same Rust file. It shows the same things - Auto Run, API Templates
// and their AI tools - but not the Extras card, which follows `unlocked`
// alone. One listener set for both: a change to either can change what is
// shown.
import { useSyncExternalStore } from "react";
import { commands } from "../bindings";
import { isCaptureMode } from "../dev/capture";

/** True in `tauri dev` and vitest, false in `tauri build`: a development
 * build shows Auto Run whatever the switch says. Read once at module load. */
export const AUTO_RUN_DEV: boolean = import.meta.env.DEV;

let unlocked = false;
let advanced = false;
/** Bumped by every save, so a read that started before it cannot put the
 * old value back when it answers late. One per flag: a save of one must not
 * discard a read of the other. */
let generation = 0;
let advancedGeneration = 0;
const listeners = new Set<() => void>();

/** True once `hydrateExtras` has settled at least once, success or not.
 * `unlocked` starts false the same way a locked machine would read, so
 * nothing that only checks its value can tell "locked" from "still
 * waiting on Rust's answer" - this is that third state, for callers (the
 * App shell's Auto Run redirect) that must not act before it flips. */
let hydrated = false;
const hydratedListeners = new Set<() => void>();

function notify(): void {
  for (const l of [...listeners]) l();
}

function publish(next: boolean): void {
  if (next === unlocked) return;
  unlocked = next;
  notify();
}

function publishAdvanced(next: boolean): void {
  if (next === advanced) return;
  advanced = next;
  notify();
}

function publishHydrated(): void {
  if (hydrated) return;
  hydrated = true;
  for (const l of [...hydratedListeners]) l();
}

/** Fires on a change to either flag. */
export function subscribeExtras(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function subscribeExtrasHydrated(cb: () => void): () => void {
  hydratedListeners.add(cb);
  return () => {
    hydratedListeners.delete(cb);
  };
}

export function extrasUnlockedSnapshot(): boolean {
  return unlocked;
}

export function extrasHydratedSnapshot(): boolean {
  return hydrated;
}

export function advancedFeaturesSnapshot(): boolean {
  return advanced;
}

/** Whether Auto Run, API Templates and their AI tools are on for this
 * machine, before the build kind and capture mode are applied: either
 * flag. Mirrors Rust's `extras::features_on`. */
export function featuresOnSnapshot(): boolean {
  return unlocked || advanced;
}

/** Ask Rust for both flags. Outside the app (a test without IPC, a browser
 * preview) the answer is simply "as it was" - off, from a fresh start.
 * Either way, `hydrated` flips once this settles. */
export async function hydrateExtras(): Promise<void> {
  const started = generation;
  const startedAdvanced = advancedGeneration;
  const readUnlocked = (async () => {
    try {
      const on = await commands.getExtrasUnlocked();
      if (started === generation) publish(Boolean(on));
    } catch {
      // no IPC here
    }
  })();
  const readAdvanced = (async () => {
    try {
      const on = await commands.getAdvancedFeatures();
      if (startedAdvanced === advancedGeneration) publishAdvanced(Boolean(on));
    } catch {
      // no IPC here
    }
  })();
  await Promise.all([readUnlocked, readAdvanced]);
  if (started === generation && startedAdvanced === advancedGeneration) publishHydrated();
}

/** Save through Rust, then tell every screen. Throws - changing nothing -
 * when the save fails. */
export async function setExtrasUnlocked(on: boolean): Promise<void> {
  generation += 1;
  const r = await commands.setExtrasUnlocked(on);
  if (r.status === "error") throw new Error(r.error);
  publish(on);
}

/** Save Enable Advanced Features through Rust, then tell every screen.
 * Throws - changing nothing - when the save fails. */
export async function setAdvancedFeatures(on: boolean): Promise<void> {
  advancedGeneration += 1;
  const r = await commands.setAdvancedFeatures(on);
  if (r.status === "error") throw new Error(r.error);
  publishAdvanced(on);
}

/** Whether Auto Run is shown right now: always in a development build, and
 * in a release build while this machine's extras are unlocked or Enable
 * Advanced Features is on. Capture mode (only ever a development build)
 * shows it too: the help site documents Auto Run and API Templates as
 * Enable Advanced Features shows them. The Extras card, which follows the
 * hidden switch alone, stays out of capture mode on its own gate. */
export function autoRunVisible(): boolean {
  if (isCaptureMode()) return true;
  return AUTO_RUN_DEV || featuresOnSnapshot();
}

/** The hidden extras switch ALONE - only for what belongs to it (the Extras
 * card). Anything that asks "is Auto Run shown" reads `useAutoRunVisible`. */
export function useExtrasUnlocked(): boolean {
  return useSyncExternalStore(subscribeExtras, extrasUnlockedSnapshot);
}

export function useAdvancedFeatures(): boolean {
  return useSyncExternalStore(subscribeExtras, advancedFeaturesSnapshot);
}

export function useExtrasHydrated(): boolean {
  return useSyncExternalStore(subscribeExtrasHydrated, extrasHydratedSnapshot);
}

export function useAutoRunVisible(): boolean {
  const on = useSyncExternalStore(subscribeExtras, featuresOnSnapshot);
  // Capture mode shows Auto Run, as Enable Advanced Features does: the help
  // site documents it.
  if (isCaptureMode()) return true;
  return AUTO_RUN_DEV || on;
}

/** The tabs offered exactly where Auto Run is: Auto Run itself, and API
 * Templates, whose tools and runs are gated the same way. */
const HIDDEN_WITH_AUTO_RUN = new Set(["autorun", "apitemplates"]);

/** Whether the App shell's redirect off a tab that stopped being offered
 * while it was open (Auto Run or API Templates) should fire right now.
 * False until hydration settles, so a release build with one of them saved
 * as the tab is not bounced to Manual Entry before Rust's answer to
 * `get_extras_unlocked` arrives. */
export function shouldLeaveHidden(section: string, shown: boolean, hydrated: boolean): boolean {
  return hydrated && !shown && HIDDEN_WITH_AUTO_RUN.has(section);
}

/** Tests only: back to locked and un-hydrated, without telling anyone. */
export function resetExtrasStore(): void {
  unlocked = false;
  advanced = false;
  generation = 0;
  advancedGeneration = 0;
  hydrated = false;
}
