// Which environment the AI Bridge tab's switches belong to.
//
// Four of the switches are kept per environment in this webview: the tools
// an assistant may use (mcpTools.ts), database writes (dbServer.ts), API
// template writes (apiTemplates.ts) and the network tools (networkTools.ts).
// Each is stored under `<key>:<env id>` and read for the ACTIVE
// environment, which this file remembers - so the stores answer
// synchronously at start, before the list has loaded. The fifth, "Run
// database changes without asking", is Rust's, on the environment record.
//
// Rust is the authority on which environment is active. Every bridge push
// says which environment its switches are for, and Rust allows no write from
// a push for any other one (`push_context_with`), so a stale id here can
// only ever turn writes OFF.

import type { EnvListView } from "../bindings";

/** The base keys of the per-environment switches. Before any environment
 * is known (and in older versions, for the whole app) each is stored under
 * the base key itself; `migrateBridgeSwitches` moves those values to the
 * environments once. */
export const BRIDGE_SWITCH_KEYS = {
  tools: "tcm-v2-mcp-disabled",
  dbWrites: "tcm-v2-db-writes",
  apiWrites: "tcm-v2-api-writes",
  networkTools: "tcm-v2-network-tools",
} as const;

const ACTIVE_KEY = "tcm-v2-active-env";

/** Set once the single values older versions kept have moved to the
 * environments, so they move exactly once. */
const MIGRATED_KEY = "tcm-v2-bridge-switches-per-env";

const listeners = new Set<() => void>();

function notify(): void {
  for (const l of listeners) l();
}

function get(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function set(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // storage unavailable -> nothing to move either
  }
}

function remove(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    // nothing to remove
  }
}

/** The active environment's id, or "" while none is known. Read from
 * storage each time, so it is never a copy that outlives a reset. */
export function activeEnvIdSnapshot(): string {
  return get(ACTIVE_KEY) ?? "";
}

/** Subscription for the stores: a switch of environment changes what each
 * of them reads. */
export function subscribeActiveEnv(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Make `id` the environment the switches are read for. */
export function setActiveEnvId(id: string): void {
  if (id === activeEnvIdSnapshot()) return;
  if (id) set(ACTIVE_KEY, id);
  else remove(ACTIVE_KEY);
  notify();
}

/** Where a per-environment switch is stored for the active environment
 * (`base:<id>`), or `base` itself while none is known. */
export function envScopedKey(base: string): string {
  const id = activeEnvIdSnapshot();
  return id ? `${base}:${id}` : base;
}

/** Move the single values older versions kept to the environments, once:
 * the tools set to EVERY environment there is now (it was everyone's), the
 * two write switches to the ACTIVE one only - every other environment, and
 * every one made later, starts with them off. The old keys go, so they can
 * never apply to another environment. A list with no active environment
 * moves nothing. */
export function migrateBridgeSwitches(view: EnvListView): void {
  if (get(MIGRATED_KEY) === "1") return;
  if (!view.active || !view.environments.some((e) => e.id === view.active)) return;
  const tools = get(BRIDGE_SWITCH_KEYS.tools);
  if (tools !== null) {
    for (const e of view.environments) {
      const key = `${BRIDGE_SWITCH_KEYS.tools}:${e.id}`;
      if (get(key) === null) set(key, tools);
    }
  }
  for (const base of [BRIDGE_SWITCH_KEYS.dbWrites, BRIDGE_SWITCH_KEYS.apiWrites, BRIDGE_SWITCH_KEYS.networkTools]) {
    if (get(base) === "1") set(`${base}:${view.active}`, "1");
  }
  for (const base of Object.values(BRIDGE_SWITCH_KEYS)) remove(base);
  set(MIGRATED_KEY, "1");
  notify();
}

/** What every list of environments from Rust is told to: the migration,
 * then which environment is active. */
export function noteEnvironments(view: EnvListView): void {
  migrateBridgeSwitches(view);
  if (view.active && view.environments.some((e) => e.id === view.active)) setActiveEnvId(view.active);
}
