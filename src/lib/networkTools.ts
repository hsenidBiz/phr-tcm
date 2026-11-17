// The AI Bridge tab's Network tools card: whether an assistant gets the
// tools that look under the Auto Run browser's page - its captured
// requests, HAR files, test headers and cookies.
//
// Rust lists them only on this switch's yes and refuses their routes
// without it (`ai_tools::NETWORK_TOOLS`, `ai_bridge::network_guard`); they
// are also Auto Run's tools, offered only while that module is on. The
// store mirrors apiTemplates.ts's write switch.

import { BRIDGE_SWITCH_KEYS, envKeyFor, envScopedKey, subscribeActiveEnv } from "./activeEnv";

/** The switch, the active environment's own (each environment keeps its
 * own - activeEnv.ts). "1" only when it is on, and absent otherwise - so a
 * fresh profile, a cleared one and a new environment all read off. */
const networkToolsKey = (): string => envScopedKey(BRIDGE_SWITCH_KEYS.networkTools);

/** Set to "1" once a person has chosen the switch for an environment, either
 * way. Without it the switch's state is only a default, which turning Auto
 * Run on for the first time may change. */
const NETWORK_TOOLS_SET_BASE = "tcm-v2-network-tools-set";

const listeners = new Set<() => void>();

function notify(): void {
  for (const l of listeners) l();
}

/** Subscription so App can re-push the bridge context the moment the
 * switch changes. A switch of environment changes whose switch it is. */
export function subscribeNetworkTools(cb: () => void): () => void {
  listeners.add(cb);
  const offEnv = subscribeActiveEnv(cb);
  return () => {
    listeners.delete(cb);
    offEnv();
  };
}

/** Whether an assistant gets the network tools. Off unless it was
 * explicitly switched on. */
export function loadNetworkTools(): boolean {
  try {
    return localStorage.getItem(networkToolsKey()) === "1";
  } catch {
    return false;
  }
}

export function saveNetworkTools(on: boolean): void {
  try {
    localStorage.setItem(envScopedKey(NETWORK_TOOLS_SET_BASE), "1");
    if (on) localStorage.setItem(networkToolsKey(), "1");
    else localStorage.removeItem(networkToolsKey());
  } catch {
    // storage unavailable -> the choice lasts for this session only
  }
  notify();
}

/** A primitive, so `useSyncExternalStore` is happy to re-read it. */
export function networkToolsSnapshot(): boolean {
  return loadNetworkTools();
}

/** Switch the network tools on for every environment whose switch nobody has
 * chosen yet, and return how many that changed. Writes no marker: this is a
 * default, not a person's choice, so a later choice (or a switch-off) still
 * counts as theirs. */
export function turnOnNetworkToolsWhereUnset(envIds: string[]): number {
  let changed = 0;
  try {
    for (const id of envIds) {
      if (localStorage.getItem(envKeyFor(NETWORK_TOOLS_SET_BASE, id)) === "1") continue;
      const key = envKeyFor(BRIDGE_SWITCH_KEYS.networkTools, id);
      if (localStorage.getItem(key) === "1") continue;
      localStorage.setItem(key, "1");
      changed += 1;
    }
  } catch {
    // storage unavailable -> whatever was written stays
  }
  if (changed > 0) notify();
  return changed;
}
