// What the Auto Run screens do with a plan from `auto_run_plan`: ask for
// one, and word its reset points. The plan carries no case titles - the
// screen has the case list and supplies them.

import { commands, type PlanView, type Reset } from "../../bindings";

/**
 * The plan for these cases, or null when it could not be had. A caller
 * that gets null runs in list order, as it did before plans: a plan that
 * cannot be read must never stop a run. A plan whose order is not exactly
 * the cases asked for is treated the same way.
 */
export async function fetchPlan(
  org: string,
  project: string,
  pbiId: number,
  ids: number[],
): Promise<PlanView | null> {
  try {
    const r = await commands.autoRunPlan(org, project, pbiId, ids);
    if (r?.status !== "ok") return null;
    const plan = r.data;
    if (!plan || !Array.isArray(plan.order) || !Array.isArray(plan.phases)) return null;
    if (plan.order.length !== ids.length) return null;
    const want = new Set(ids);
    if (!plan.order.every((id) => want.delete(id))) return null;
    return plan;
  } catch {
    return null;
  }
}

/** `#<id> <title>`, or `#<id>` alone when the title is not known. */
export function caseLabel(id: number, titleOf: (id: number) => string | undefined): string {
  const t = titleOf(id)?.trim();
  return t ? `#${id} ${t}` : `#${id}`;
}

/** One line per name to revert, in the spec's words. */
export function resetLines(reset: Reset, titleOf: (id: number) => string | undefined): string[] {
  return reset.names.map((name) => {
    const by = reset.changed_by.find(([n]) => n === name)?.[1] ?? [];
    const who = by.map((id) => caseLabel(id, titleOf)).join(", ");
    return who ? `Reset: revert "${name}" (changed by ${who})` : `Reset: revert "${name}"`;
  });
}
