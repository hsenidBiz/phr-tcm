import type { PbiHit } from "../bindings";

/** Handoff from Run Tests (main window) to the runner window. */
export type RunnerSession = {
  org: string;
  project: string;
  planId: number;
  planName: string;
  suiteId: number;
  pbi: PbiHit;
  /** Restrict the runner to these test case ids, IN THIS ORDER;
   * empty/absent = all. Ordered because the Run Tests list and the
   * runner's own fetch (the PBI's Tested-By links) have no shared
   * ordering contract - the runner walks the sequence it is handed. */
  caseIds?: number[];
  /** Order hint for an unrestricted run: the Run Tests list's visible
   * order. Cases missing from it (list filtered, linked since) still
   * run - after these, in fetch order. Ignored when caseIds is set. */
  caseOrder?: number[];
};

const KEY = "tcm-v2-runner-session";

export function saveRunnerSession(s: RunnerSession) {
  localStorage.setItem(KEY, JSON.stringify(s));
}

export function loadRunnerSession(): RunnerSession | null {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as RunnerSession) : null;
  } catch {
    return null;
  }
}

const PIN_KEY = "tcm-v2-runner-pinned";

/** The runner's remembered always-on-top preference (default: pinned).
 * Read at window creation AND by the runner's own toggle state, so a new
 * run opens the way the user last left it. */
export function loadRunnerPinned(): boolean {
  try {
    return localStorage.getItem(PIN_KEY) !== "off";
  } catch {
    return true;
  }
}

export function saveRunnerPinned(pinned: boolean): void {
  try {
    localStorage.setItem(PIN_KEY, pinned ? "on" : "off");
  } catch {
    // session-only
  }
}
