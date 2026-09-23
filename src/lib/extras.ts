// This machine's optional extras switch, as the webview sees it. Rust owns
// it (src-tauri/src/extras.rs: saved in the app data dir, read by the AI
// bridge); this mirrors it for the screens. Turned on by a key sequence on
// the Settings screen (lib/extrasSequence.ts), off by that section's Reset
// to default. While it is on, a release build shows Auto Run - its tab and
// its AI tools - the way a development build always does.
import { useSyncExternalStore } from "react";
import { commands } from "../bindings";

/** True in `tauri dev` and vitest, false in `tauri build`: a development
 * build shows Auto Run whatever the switch says. Read once at module load. */
export const AUTO_RUN_DEV: boolean = import.meta.env.DEV;

let unlocked = false;
/** Bumped by every save, so a read that started before it cannot put the
 * old value back when it answers late. */
let generation = 0;
const listeners = new Set<() => void>();

function publish(next: boolean): void {
  if (next === unlocked) return;
  unlocked = next;
  for (const l of [...listeners]) l();
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

/** Ask Rust. Outside the app (a test without IPC, a browser preview) the
 * answer is simply "as it was" - locked, from a fresh start. */
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

/** Whether Auto Run is shown right now: always in a development build,
 * and in a release build while this machine's extras are unlocked. */
export function autoRunVisible(): boolean {
  return AUTO_RUN_DEV || unlocked;
}

export function useExtrasUnlocked(): boolean {
  return useSyncExternalStore(subscribeExtras, extrasUnlockedSnapshot);
}

export function useAutoRunVisible(): boolean {
  const on = useExtrasUnlocked();
  return AUTO_RUN_DEV || on;
}

/** Tests only: back to locked, without telling anyone. */
export function resetExtrasStore(): void {
  unlocked = false;
  generation = 0;
}
