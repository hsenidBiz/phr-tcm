// This machine's hidden Extras switch, as the webview sees it. Rust owns it
// (src-tauri/src/extras.rs: saved in the app data dir); this mirrors it for
// the screens. Turned on by a key sequence on the Settings screen
// (lib/extrasSequence.ts), off by that section's Reset to default. It shows
// the Extras card (games) and nothing else: which features are shown is the
// modules' decision (lib/modules.ts).
import { useSyncExternalStore } from "react";
import { commands } from "../bindings";

let unlocked = false;
/** Bumped by every save, so a read that started before it cannot put the
 * old value back when it answers late. */
let generation = 0;
const listeners = new Set<() => void>();

/** True once `hydrateExtras` has settled at least once, success or not.
 * `unlocked` starts false the same way a locked machine would read, so
 * nothing that only checks its value can tell "locked" from "still
 * waiting on Rust's answer" - this is that third state. */
let hydrated = false;

function notify(): void {
  for (const l of [...listeners]) l();
}

function publish(next: boolean): void {
  if (next === unlocked) return;
  unlocked = next;
  notify();
}

function publishHydrated(): void {
  if (hydrated) return;
  hydrated = true;
}

export function subscribeExtras(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function extrasUnlockedSnapshot(): boolean {
  return unlocked;
}

export function extrasHydratedSnapshot(): boolean {
  return hydrated;
}

/** Ask Rust for the flag. Outside the app (a test without IPC, a browser
 * preview) the answer is simply "as it was" - off, from a fresh start.
 * Either way, `hydrated` flips once this settles. */
export async function hydrateExtras(): Promise<void> {
  const started = generation;
  try {
    const on = await commands.getExtrasUnlocked();
    if (started === generation) publish(Boolean(on));
  } catch {
    // no IPC here
  }
  if (started === generation) publishHydrated();
}

/** Save through Rust, then tell every screen. Throws - changing nothing -
 * when the save fails. */
export async function setExtrasUnlocked(on: boolean): Promise<void> {
  generation += 1;
  const r = await commands.setExtrasUnlocked(on);
  if (r.status === "error") throw new Error(r.error);
  publish(on);
}

/** The hidden extras switch - for what belongs to it (the Extras card). */
export function useExtrasUnlocked(): boolean {
  return useSyncExternalStore(subscribeExtras, extrasUnlockedSnapshot);
}

/** Tests only: back to locked and un-hydrated, without telling anyone. */
export function resetExtrasStore(): void {
  unlocked = false;
  generation = 0;
  hydrated = false;
}
