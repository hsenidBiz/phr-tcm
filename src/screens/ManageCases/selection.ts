import type { SuiteCase } from "../../lib/suiteOrder";

/** The cases picked for a bulk action. One plan at a time: a copy or a
 * new suite happens inside a plan, so picking a case in another plan
 * starts over rather than building a selection nothing could act on. */
export type Selection = { planId: number; cases: Map<number, SuiteCase> };

export function toggleSelection(
  sel: Selection | null,
  planId: number,
  cases: SuiteCase[],
  on: boolean,
): Selection | null {
  if (on) {
    const base = sel && sel.planId === planId ? sel.cases : new Map<number, SuiteCase>();
    const next = new Map(base);
    for (const c of cases) next.set(c.id, c);
    return { planId, cases: next };
  }
  if (!sel || sel.planId !== planId) return sel;
  const next = new Map(sel.cases);
  for (const c of cases) next.delete(c.id);
  return next.size === 0 ? null : { planId, cases: next };
}

export function selectedIdsIn(sel: Selection | null, planId: number): Set<number> {
  return sel && sel.planId === planId ? new Set(sel.cases.keys()) : new Set();
}
