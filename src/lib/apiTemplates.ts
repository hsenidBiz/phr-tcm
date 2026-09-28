// The AI Bridge tab's API templates card: whether the assistant may PROVE
// and RUN a template, which writes test data through the application's
// own endpoints.
//
// A separate decision from whether the four API template tools are
// reachable at all (the "API templates" row in mcpTools.ts's TOOL_PAIRS) -
// the same shape as the Company database card's create/update/delete
// switch (dbServer.ts), which this file mirrors.

/** The prove/run switch. "1" only when it is on, and absent otherwise - so
 * a fresh profile and a cleared one both read off, which is the only
 * default a switch like this may have. */
const API_WRITES_KEY = "tcm-v2-api-writes";

const listeners = new Set<() => void>();

function notify(): void {
  for (const l of listeners) l();
}

/** Subscription so App can re-push the bridge context the moment the
 * switch changes, instead of the change waiting for the next org/project
 * change. The same pattern dbServer's write switch uses. */
export function subscribeApiWrites(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Whether the assistant may prove and run API templates. Off unless it
 * was explicitly switched on. */
export function loadApiWrites(): boolean {
  try {
    return localStorage.getItem(API_WRITES_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveApiWrites(on: boolean): void {
  try {
    if (on) localStorage.setItem(API_WRITES_KEY, "1");
    else localStorage.removeItem(API_WRITES_KEY);
  } catch {
    // storage unavailable -> the choice lasts for this session only
  }
  notify();
}

/** A primitive, so `useSyncExternalStore` is happy to re-read it. */
export function apiWritesSnapshot(): boolean {
  return loadApiWrites();
}
