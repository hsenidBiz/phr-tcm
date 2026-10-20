// The AI Bridge tab's Network tools card: whether an assistant gets the
// tools that look under the Auto Run browser's page - its captured
// requests, HAR files, test headers and cookies.
//
// Rust lists them only on this switch's yes and refuses their routes
// without it (`ai_tools::NETWORK_TOOLS`, `ai_bridge::network_guard`); they
// also ride in the Auto Run scripts row (mcpTools.ts's TOOL_PAIRS). The
// store mirrors apiTemplates.ts's write switch.

import { BRIDGE_SWITCH_KEYS, envScopedKey, subscribeActiveEnv } from "./activeEnv";

/** The switch, the active environment's own (each environment keeps its
 * own - activeEnv.ts). "1" only when it is on, and absent otherwise - so a
 * fresh profile, a cleared one and a new environment all read off. */
const networkToolsKey = (): string => envScopedKey(BRIDGE_SWITCH_KEYS.networkTools);

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
