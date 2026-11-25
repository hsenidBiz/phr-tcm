// What Auto Test (and the top bar) is working on: one PBI, or one test
// suite. Never both - choosing one drops the other (App's setPbi / setSuite).

import type { AutoRunSuiteRef, PbiHit, RunScope } from "../bindings";

/** A test suite as the top bar holds and remembers it. `project` is the
 * suite's plan's own project; left out it is the project the bar is on. */
export type SuiteScope = {
  planId: number;
  suiteId: number;
  title: string;
  planName: string;
  project?: string;
};

export type Scope = { kind: "pbi"; pbi: PbiHit } | { kind: "suite"; suite: SuiteScope };

/** The scope a bar holds: its PBI, else its suite, else none. */
export function scopeOf(pbi: PbiHit | null, suite: SuiteScope | null): Scope | null {
  if (pbi) return { kind: "pbi", pbi };
  if (suite) return { kind: "suite", suite };
  return null;
}

const pbiKey = (pbiId: number) => `pbi:${pbiId}`;
const suiteKey = (planId: number, suiteId: number) => `suite:${planId}:${suiteId}`;

/** Two scopes are the same work exactly when their keys are equal. */
export function scopeKey(s: Scope): string {
  return s.kind === "pbi" ? pbiKey(s.pbi.id) : suiteKey(s.suite.planId, s.suite.suiteId);
}

/** The same key, from what a screen holds apart from a `Scope`. */
export function currentKey(pbiId: number | null | undefined, suite: AutoRunSuiteRef | null | undefined): string | null {
  if (suite) return suiteKey(suite.plan_id, suite.suite_id);
  return pbiId != null ? pbiKey(pbiId) : null;
}

/** The key of the scope a saved run belongs to (`pbi_id` is 0 for a suite run). */
export function runKey(run: { pbi_id: number; suite?: AutoRunSuiteRef | null }): string {
  return run.suite ? suiteKey(run.suite.plan_id, run.suite.suite_id) : pbiKey(run.pbi_id);
}

/** What the Rust commands take to name the scope. */
export function toRunScope(s: Scope): RunScope {
  return s.kind === "pbi"
    ? { kind: "pbi", pbi_id: s.pbi.id }
    : { kind: "suite", plan_id: s.suite.planId, suite_id: s.suite.suiteId, title: s.suite.title };
}

/** The run scope from the pieces a screen holds (a PBI id, or a suite). */
export function runScopeOf(pbiId: number, suite: AutoRunSuiteRef | null | undefined): RunScope {
  return suite
    ? { kind: "suite", plan_id: suite.plan_id, suite_id: suite.suite_id, title: suite.title ?? "" }
    : { kind: "pbi", pbi_id: pbiId };
}

/** The suite reference a saved run carries. */
export function suiteRefOf(s: SuiteScope): AutoRunSuiteRef {
  return { plan_id: s.planId, suite_id: s.suiteId, title: s.title };
}

/** The title Auto Test shows for the scope: the PBI's or the suite's. */
export function scopeTitle(s: Scope): string {
  return s.kind === "pbi" ? s.pbi.title : s.suite.title;
}
