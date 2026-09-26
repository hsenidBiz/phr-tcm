// This machine's optional extras switch, as the webview sees it. Rust owns
// it (src-tauri/src/extras.rs: saved in the app data dir, read by the AI
// bridge); this mirrors it for the screens. Turned on by a key sequence on
// the Settings screen (lib/extrasSequence.ts), off by that section's Reset
// to default. While it is on, a release build shows Auto Run - its tab and
// its AI tools - the way a development build always does.
import { useSyncExternalStore } from "react";
import { commands } from "../bindings";
import { isCaptureMode } from "../dev/capture";

/** True in `tauri dev` and vitest, false in `tauri build`: a development
 * build shows Auto Run whatever the switch says. Read once at module load. */
export const AUTO_RUN_DEV: boolean = import.meta.env.DEV;

let unlocked = false;
/** Bumped by every save, so a read that started before it cannot put the
 * old value back when it answers late. */
let generation = 0;
const listeners = new Set<() => void>();

/** True once `hydrateExtras` has settled at least once, success or not.
 * `unlocked` starts false the same way a locked machine would read, so
 * nothing that only checks its value can tell "locked" from "still
 * waiting on Rust's answer" - this is that third state, for callers (the
 * App shell's Auto Run redirect) that must not act before it flips. */
let hydrated = false;
const hydratedListeners = new Set<() => void>();

function publish(next: boolean): void {
  if (next === unlocked) return;
  unlocked = next;
  for (const l of [...listeners]) l();
}

function publishHydrated(): void {
  if (hydrated) return;
  hydrated = true;
  for (const l of [...hydratedListeners]) l();
}

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

/** Ask Rust. Outside the app (a test without IPC, a browser preview) the
 * answer is simply "as it was" - locked, from a fresh start. Either way,
 * `hydrated` flips once this settles. */
export async function hydrateExtras(): Promise<void> {
  const started = generation;
  try {
    const on = await commands.getExtrasUnlocked();
    if (started === generation) publish(Boolean(on));
  } catch {
    // no IPC here
  } finally {
    if (started === generation) publishHydrated();
  }
}

/** Save through Rust, then tell every screen. Throws - changing nothing -
 * when the save fails. */
export async function setExtrasUnlocked(on: boolean): Promise<void> {
  generation += 1;
  const r = await commands.setExtrasUnlocked(on);
  if (r.status === "error") throw new Error(r.error);
  publish(on);
}

/** Whether Auto Run is shown right now: always in a development build,
 * and in a release build while this machine's extras are unlocked - except
 * in capture mode, which hides it whichever of those made it visible, since
 * the screenshot script must never shoot it. */
export function autoRunVisible(): boolean {
  if (isCaptureMode()) return false;
  return AUTO_RUN_DEV || unlocked;
}

export function useExtrasUnlocked(): boolean {
  return useSyncExternalStore(subscribeExtras, extrasUnlockedSnapshot);
}

export function useExtrasHydrated(): boolean {
  return useSyncExternalStore(subscribeExtrasHydrated, extrasHydratedSnapshot);
}

export function useAutoRunVisible(): boolean {
  const on = useExtrasUnlocked();
  // Capture mode hides Auto Run even in the dev build it always ships in.
  if (isCaptureMode()) return false;
  return AUTO_RUN_DEV || on;
}

/** Whether the App shell's redirect off Auto Run (it stopped being offered
 * while it was the open tab) should fire right now. False until hydration
 * settles, so a release build with a saved Auto Run tab is not bounced to
 * Manual Entry before Rust's answer to `get_extras_unlocked` arrives. */
export function shouldLeaveAutoRun(section: string, autoRunShown: boolean, hydrated: boolean): boolean {
  return hydrated && !autoRunShown && section === "autorun";
}

/** Tests only: back to locked and un-hydrated, without telling anyone. */
export function resetExtrasStore(): void {
  unlocked = false;
  generation = 0;
  hydrated = false;
}
