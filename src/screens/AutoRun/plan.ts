// What the Auto Run screens do with a plan from `auto_run_plan`: ask for
// one, and word its batches and the pauses between them. The plan carries
// no case titles - the screen has the case list and supplies them.

import { commands, type Pause, type PlanView } from "../../bindings";

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
  if (!Array.isArray(plan.pauses)) return null;
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
export function resetLines(reset: Pick<Pause, "names" | "changed_by">, titleOf: (id: number) => string | undefined): string[] {
  return reset.names.map((name) => {
    const by = reset.changed_by.find(([n]) => n === name)?.[1] ?? [];
    const who = by.map((id) => caseLabel(id, titleOf)).join(", ");
    return who ? `Reset: revert "${name}" (changed by ${who})` : `Reset: revert "${name}"`;
  });
}

/** The lines for the settings a pause sets: before Batch 1, `make sure
 * <name> is <value>`; before a later batch, `Set <name> to <value>`. */
export function setLines(pause: Pick<Pause, "batch" | "set">): string[] {
  return (pause.set ?? []).map(([name, value]) =>
    pause.batch === 1 ? `Make sure ${name} is ${value}` : `Set ${name} to ${value}`,
  );
}

/** Every line a pause shows, settings first, then what to put back. */
export function pauseLines(pause: Pause, titleOf: (id: number) => string | undefined): string[] {
  return [...setLines(pause), ...resetLines(pause, titleOf)];
}

/** The batches of a plan, each with its pause (none before an unpaused
 * Batch 1). Empty when the plan has no pause: one implicit batch shows no
 * heading, so nothing changes for a selection without marks. */
export function planBatches(plan: PlanView | null | undefined): { n: number; ids: number[]; pause: Pause | null }[] {
  if (!plan || !plan.pauses?.length) return [];
  return plan.phases.map((ids, i) => ({
    n: i + 1,
    ids,
    pause: plan.pauses.find((p) => p.before_case_id === ids[0] && p.batch === i + 1) ?? null,
  }));
}

/** A pause's heading: `Before Batch N`. */
export function pauseHeading(batch: number | undefined): string {
  return batch && batch > 0 ? `Before Batch ${batch}` : "Before the next batch";
}

/** `Batch 2 (4 cases)`. */
export function batchLabel(n: number, size: number): string {
  return `Batch ${n} (${size} ${size === 1 ? "case" : "cases"})`;
}

/** The batch a case is in, 1-based, and how many batches there are; null
 * for a plan without pauses or a case it does not hold. */
export function batchOf(plan: PlanView | null | undefined, caseId: number): { n: number; of: number } | null {
  const batches = planBatches(plan);
  const at = batches.findIndex((b) => b.ids.includes(caseId));
  return at < 0 ? null : { n: at + 1, of: batches.length };
}

/** A pause as a run recorded it: its batch (absent on a run saved before
 * batches), what it set, the names reverted and how it ended. */
export type RecordedReset = {
  before_case_id: number;
  batch?: number;
  set?: [string, string][];
  names?: string[];
  outcome?: string;
};

/** The lines a recorded pause is shown with, in the report's words: one
 * per setting (`Before Batch 1: make sure <name> is <value>`, later `Set
 * <name> to <value>`), then one per name (`Reset: revert "<name>"`), each
 * ending ` - continued` (or `- stopped`). */
export function recordedResetLines(reset: RecordedReset): string[] {
  const ended = reset.outcome === "continued" || reset.outcome === "stopped" ? ` - ${reset.outcome}` : "";
  const set = (reset.set ?? []).map(([name, value]) =>
    reset.batch === 1 ? `Before Batch 1: make sure ${name} is ${value}${ended}` : `Set ${name} to ${value}${ended}`,
  );
  return [...set, ...(reset.names ?? []).map((name) => `Reset: revert "${name}"${ended}`)];
}

/** The recorded pause lines due before case `caseId` in a run. */
export function resetLinesBefore(resets: readonly RecordedReset[] | undefined, caseId: number): string[] {
  return (resets ?? []).filter((r) => r.before_case_id === caseId).flatMap(recordedResetLines);
}

/** The batch heading due before case `caseId` in a recorded run, as the
 * report has it: `Batch N` where a recorded pause opened one, and `Batch 1`
 * before the run's first case. None for a run recorded before batches. */
export function batchHeadingBefore(
  run: { resets?: readonly RecordedReset[] | null; cases: readonly { case_id: number }[] },
  caseId: number,
): string | null {
  const resets = run.resets ?? [];
  if (!resets.some((r) => (r.batch ?? 0) > 0)) return null;
  const opened = resets.find((r) => r.before_case_id === caseId && (r.batch ?? 0) > 0);
  if (opened) return `Batch ${opened.batch}`;
  return run.cases[0]?.case_id === caseId ? "Batch 1" : null;
}

/** The sentence every case left is recorded with when a run stops at a
 * pause - the same words as the unattended run's. */
export const STOPPED_AT_RESET = "not run: the run stopped at a reset point";
