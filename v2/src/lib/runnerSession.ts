import type { PbiHit } from "../bindings";

/** Handoff from Run Tests (main window) to the runner window. */
export type RunnerSession = {
  org: string;
  project: string;
  planId: number;
  planName: string;
  suiteId: number;
  pbi: PbiHit;
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
