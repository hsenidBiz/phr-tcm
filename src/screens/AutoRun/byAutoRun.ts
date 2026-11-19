// The steps Auto Test performs itself (launching the app, signing in, reaching
// the area): the TypeScript mirror of `autorun::by_auto_run::done_by_auto_run`,
// held to the same fixture (`src-tauri/tests/fixtures/by_auto_run.json`).

import type { CaseScript } from "../../bindings";

/** A step counts when it has `by_auto_run` and sits in the leading run of case
 * steps (each one before it has the mark too). With no mark anywhere (a script
 * saved before the field existed), the leading run from step 1 of case steps
 * with no actions and a whole-step `unchecked` sentence counts instead. The
 * clean-up never counts. */
export function doneByAutoRun(script: CaseScript): Set<number> {
  const out = new Set<number>();
  const caseSteps = script.steps.filter((s) => !s.cleanup);
  if (script.steps.some((s) => s.by_auto_run === true)) {
    for (const step of caseSteps) {
      if (step.by_auto_run !== true) break;
      out.add(step.step_number);
    }
    return out;
  }
  for (const [i, step] of caseSteps.entries()) {
    const wholeStep = typeof step.unchecked === "string" && step.unchecked.trim() !== "";
    const opening = i !== 0 || step.step_number === 1;
    if (step.actions.length > 0 || !wholeStep || !opening) break;
    out.add(step.step_number);
  }
  return out;
}
