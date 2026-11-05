// The Bug dialog's starting point for a case that failed in an Auto Run:
// the steps up to and including the failing one, then what the page said.

import type { ActionOutcome, Step } from "../../bindings";
import { reproStep } from "../../components/BugDialog";
import { failingStep } from "./verdicts";

type RunCase = {
  case_id: number;
  title: string;
  account?: string | null;
  steps: readonly {
    step_number: number;
    screenshot?: string | null;
    outcomes: readonly Pick<ActionOutcome, "ok" | "detail" | "screenshot">[];
  }[];
};

const isFailure = (o: { ok: boolean; detail: string }) => !o.ok && !o.detail.startsWith("not run:");

/** The repro text. `sharedLine` words a Shared Steps row (it has no text of
 * its own). Plain text: no paths, no URLs. */
export function autoRunRepro(
  c: RunCase,
  steps: readonly Step[],
  run: { environment?: string | null },
  sharedLine: (s: Step, n: number) => string,
): string {
  const failing = failingStep(c);
  const upTo = failing === null ? steps : steps.slice(0, failing);
  const lines = [
    `Test case #${c.case_id}: ${c.title}`,
    "",
    "Steps:",
    ...upTo.map((s, i) => (s.shared != null ? sharedLine(s, i + 1) : reproStep(i + 1, s.action, s.expected))),
  ];
  const failed = c.steps.find((s) => s.step_number === failing);
  if (failed) {
    lines.push("", `Failed at step ${failing}:`);
    for (const o of failed.outcomes.filter(isFailure)) lines.push(`- ${o.detail}`);
  }
  const where = [run.environment ? `Environment: ${run.environment}` : "", c.account ? `Account: ${c.account}` : ""]
    .filter(Boolean);
  if (where.length) lines.push("", ...where);
  return lines.join("\n");
}

/** Names of the failure screenshots: the failing step's own, then its failing
 * actions'. Empty when the case has no failing step. */
export function failureShotNames(c: RunCase): string[] {
  const failing = failingStep(c);
  const step = c.steps.find((s) => s.step_number === failing);
  if (!step) return [];
  const names = [step.screenshot, ...step.outcomes.filter(isFailure).map((o) => o.screenshot)];
  return [...new Set(names.filter((n): n is string => Boolean(n)))];
}
