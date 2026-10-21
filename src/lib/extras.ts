// This machine's hidden Extras switch, as the webview sees it. Rust owns it
// (src-tauri/src/extras.rs: saved in the app data dir); this mirrors it for
// the screens. Turned on by a key sequence on the Settings screen
// (lib/extrasSequence.ts), off by that section's Reset to default. It shows
// the Extras card and what is held back until a machine unlocks it; a change
// makes the module store ask Rust again (lib/modules.ts).
import { useSyncExternalStore } from "react";
import { commands } from "../bindings";
import { hydrateModules } from "./modules";

let unlocked = false;
/** Bumped by every save, so a read that started before it cannot put the
 * old value back when it answers late. */
let generation = 0;
const listeners = new Set<() => void>();

function notify(): void {
  for (const l of [...listeners]) l();
}

function publish(next: boolean): void {
  if (next === unlocked) return;
  unlocked = next;
  notify();
  // What is offered follows the switch (Rust answers the modules with it).
  void hydrateModules();
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

/** Ask Rust for the flag. Outside the app (a test without IPC, a browser
 * preview) the answer is simply "as it was" - off, from a fresh start. */
export async function hydrateExtras(): Promise<void> {
  const started = generation;
  try {
    const on = await commands.getExtrasUnlocked();
    if (started === generation) publish(Boolean(on));
  } catch {
    // no IPC here
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

/** The hidden extras switch - for what belongs to it (the Extras card). */
export function useExtrasUnlocked(): boolean {
  return useSyncExternalStore(subscribeExtras, extrasUnlockedSnapshot);
}

/** Tests only: set the flag without Rust and without the modules store
 * asking again. */
export function setExtrasForTest(next: boolean): void {
  unlocked = next;
  notify();
}

/** Tests only: back to locked, without telling anyone. */
export function resetExtrasStore(): void {
  unlocked = false;
  generation = 0;
}
