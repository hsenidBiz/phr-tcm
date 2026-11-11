/**
 * A request to open Auto Run with one thing in view: the accounts, the
 * sign-in, a divergence waiting for a ruling, or a case.
 *
 * Whoever asks (a bell notification, the Waiting for you list) calls
 * requestAutoRunFocus and goes to the Auto Run section; the screen reads it
 * with useAutoRunFocus and calls clearAutoRunFocus once it has shown the
 * thing. Module scope, like submitRun, so the request outlives the screen
 * that was not mounted yet when it was made.
 */

import { useSyncExternalStore } from "react";
import type { WaitingTarget } from "../bindings";

export type AutoRunFocus =
  | { kind: "accounts" }
  | { kind: "sign-in" }
  | { kind: "divergence"; id: string; caseId: number; pbi: number | null }
  | { kind: "case"; caseId: number };

/** The focus a waiting item opens: what the bell notice and the Open button
 * of the Waiting for you list both ask for. */
export function focusOf(t: WaitingTarget): AutoRunFocus {
  switch (t.kind) {
    case "accounts":
      return { kind: "accounts" };
    case "sign_in":
      return { kind: "sign-in" };
    case "divergence":
      return { kind: "divergence", id: t.id, caseId: t.case_id, pbi: t.pbi };
  }
}

let focus: AutoRunFocus | null = null;
const listeners = new Set<() => void>();

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function requestAutoRunFocus(f: AutoRunFocus): void {
  focus = f;
  for (const l of listeners) l();
}

export function clearAutoRunFocus(): void {
  if (focus === null) return;
  focus = null;
  for (const l of listeners) l();
}

export function useAutoRunFocus(): AutoRunFocus | null {
  return useSyncExternalStore(subscribe, () => focus, () => null);
}
