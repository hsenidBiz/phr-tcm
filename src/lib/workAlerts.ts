/**
 * How many newly-assigned work items the user has not looked at yet, as
 * one shared signal.
 *
 * The toast / OS notification announces an assignment once and is gone;
 * this counter is what remains: a badge on the Board rail item saying
 * "N things arrived while you were elsewhere". Opening the Board clears
 * it - the board itself now shows the items, so the badge has done its
 * job. Session-only on purpose: the persistent record IS the board.
 */

let count = 0;
const listeners = new Set<() => void>();

function set(value: number) {
  if (count === value) return;
  count = value;
  for (const l of listeners) l();
}

export function addWorkAlerts(n: number): void {
  if (n > 0) set(count + n);
}

export function clearWorkAlerts(): void {
  set(0);
}

export function subscribeWorkAlerts(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function workAlertsSnapshot(): number {
  return count;
}
