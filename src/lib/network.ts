/**
 * Whether the machine has a network, as one shared signal.
 *
 * React Query already pauses its QUERIES while `navigator.onLine` is
 * false (networkMode "online", the default) - reads quietly wait and
 * resume on their own. What that leaves uncovered is everything
 * deliberate: the submit button, the runner's per-Next records, the share
 * upload. Those used to attempt-and-fail with an error toast each, which
 * on a dead connection turns a run session into a toast storm.
 *
 * This store carries the same browser signal to the UI so those actions
 * can be PAUSED instead of failed: buttons disable with a reason, the
 * runner defers records and flushes them when the connection returns, and
 * a banner says what is happening.
 *
 * `navigator.onLine` is an honest "definitely offline" signal, not a
 * "definitely reachable" one - a captive portal or a dead VPN can report
 * online. That is fine for this job: the gate exists to stop KNOWN-doomed
 * requests; anything subtler still fails per-operation with its own
 * message, exactly as before.
 */

let online = typeof navigator === "undefined" ? true : navigator.onLine;
const listeners = new Set<() => void>();

function set(value: boolean) {
  if (online === value) return;
  online = value;
  for (const l of listeners) l();
}

if (typeof window !== "undefined") {
  window.addEventListener("online", () => set(true));
  window.addEventListener("offline", () => set(false));
}

export function subscribeOnline(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function onlineSnapshot(): boolean {
  return online;
}

/** The title/tooltip for a control disabled by the gate - one wording
 * everywhere, so the pause reads as one condition, not many bugs. */
export const OFFLINE_HINT = "No internet connection - this resumes when it returns.";
