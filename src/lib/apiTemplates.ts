// The AI Bridge tab's API templates card: whether the assistant may PROVE
// and RUN a template, which writes test data through the application's
// own endpoints.
//
// A separate decision from whether the API template tools are reachable at
// all (they are, while the API Templates module is on) - the same shape as the Database Read Access card's create/update/delete
// switch (dbServer.ts), which this file mirrors.

import { BRIDGE_SWITCH_KEYS, envScopedKey, subscribeActiveEnv } from "./activeEnv";

/** Where the API Templates screen remembers which of its tabs was last open. */
export const API_TEMPLATES_VIEW_KEY = "tcm-v2-api-templates-view";

/** Asks for the API Templates screen's Fixtures tab. The screen opens on the
 * tab it remembers, so a caller about to take the person there remembers
 * Fixtures first. */
export function askForFixturesView(): void {
  try {
    localStorage.setItem(API_TEMPLATES_VIEW_KEY, "fixtures");
  } catch {
    // Without storage the screen opens on its usual tab.
  }
}

/** The prove/run switch, the active environment's own (each environment
 * keeps its own - activeEnv.ts). "1" only when it is on, and absent
 * otherwise - so a fresh profile, a cleared one and a new environment all
 * read off, which is the only default a switch like this may have. */
const apiWritesKey = (): string => envScopedKey(BRIDGE_SWITCH_KEYS.apiWrites);

const listeners = new Set<() => void>();

function notify(): void {
  for (const l of listeners) l();
}

/** Subscription so App can re-push the bridge context the moment the
 * switch changes, instead of the change waiting for the next org/project
 * change. The same pattern dbServer's write switch uses. A switch of
 * environment changes whose switch it is. */
export function subscribeApiWrites(cb: () => void): () => void {
  listeners.add(cb);
  const offEnv = subscribeActiveEnv(cb);
  return () => {
    listeners.delete(cb);
    offEnv();
  };
}

/** Whether the assistant may prove and run API templates. Off unless it
 * was explicitly switched on. */
export function loadApiWrites(): boolean {
  try {
    return localStorage.getItem(apiWritesKey()) === "1";
  } catch {
    return false;
  }
}

export function saveApiWrites(on: boolean): void {
  try {
    if (on) localStorage.setItem(apiWritesKey(), "1");
    else localStorage.removeItem(apiWritesKey());
  } catch {
    // storage unavailable -> the choice lasts for this session only
  }
  notify();
}

/** A primitive, so `useSyncExternalStore` is happy to re-read it. */
export function apiWritesSnapshot(): boolean {
  return loadApiWrites();
}
