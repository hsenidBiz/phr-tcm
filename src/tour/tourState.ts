/**
 * Whether the guided tour has been seen, and whether it is running right
 * now. Kept apart from the tour itself so App, Settings and the command
 * palette can ask without pulling the walkthrough - and its sample data -
 * into the chunk the app loads at startup.
 */

const TOUR_DONE_KEY = "tcm-v2-tour-done";

/** Fired by Settings' "Show UI tour" button; App listens and reopens. */
export const START_TOUR_EVENT = "tcm-start-tour";

export function tourDone(): boolean {
  try {
    return localStorage.getItem(TOUR_DONE_KEY) === "yes";
  } catch {
    return true; // no storage -> never auto-run
  }
}

export function markTourDone(): void {
  try {
    localStorage.setItem(TOUR_DONE_KEY, "yes");
  } catch {
    // session-only
  }
}

// Running state lives in memory only: a tour interrupted by a crash must
// not come back locked on the next launch.
let running = false;
const listeners = new Set<() => void>();

export function setTourRunning(value: boolean): void {
  if (running === value) return;
  running = value;
  for (const cb of listeners) cb();
}

/** For useSyncExternalStore, and for plain guards in event handlers. */
export function tourRunningSnapshot(): boolean {
  return running;
}

export function subscribeTour(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}
