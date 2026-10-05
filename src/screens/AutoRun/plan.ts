// What the Auto Run screens do with a plan from `auto_run_plan`: ask for
// one, and word its reset points. The plan carries no case titles - the
// screen has the case list and supplies them.

import { commands, type PlanView, type Reset } from "../../bindings";

/**
 * The plan for these cases. Null when the answer is not a plan for exactly
 * these cases: a caller then runs in list order, as it did before plans.
 * A command that fails outright throws, so a caller can say so rather than
 * run in an order the person did not choose.
 */
export async function fetchPlan(
  org: string,
  project: string,
  pbiId: number,
  ids: number[],
  /** An order not saved yet, planned in place of the saved one. */
  preview: number[] | null = null,
): Promise<PlanView | null> {
  const r = await commands.autoRunPlan(org, project, pbiId, ids, preview);
  if (r?.status === "error") throw new Error(String(r.error));
  if (r?.status !== "ok") return null;
  const plan = r.data;
  if (!plan || !Array.isArray(plan.order) || !Array.isArray(plan.phases)) return null;
  if (plan.order.length !== ids.length) return null;
  const want = new Set(ids);
  if (!plan.order.every((id) => want.delete(id))) return null;
  return plan;
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

/** A reset point as a run recorded it: the names reverted and how it ended. */
export type RecordedReset = { before_case_id: number; names?: string[]; outcome?: string };

/** The lines a recorded reset point is shown with, one per name, in the
 * report's words: `Reset: revert "<name>" - continued` (or `- stopped`). */
export function recordedResetLines(reset: RecordedReset): string[] {
  const ended = reset.outcome === "continued" || reset.outcome === "stopped" ? ` - ${reset.outcome}` : "";
  return (reset.names ?? []).map((name) => `Reset: revert "${name}"${ended}`);
}

/** The recorded reset lines due before case `caseId` in a run. */
export function resetLinesBefore(resets: readonly RecordedReset[] | undefined, caseId: number): string[] {
  return (resets ?? []).filter((r) => r.before_case_id === caseId).flatMap(recordedResetLines);
}

/** The sentence every case left is recorded with when a run stops at a
 * reset point - the same words as the unattended run's. */
export const STOPPED_AT_RESET = "not run: the run stopped at a reset point";
