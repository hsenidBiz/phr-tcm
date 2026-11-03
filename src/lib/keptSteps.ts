import type { Step } from "../bindings";

/** The steps a new or queued case keeps: one is skipped only when BOTH its
 * action and its expected result are empty (and it is not a Shared Steps
 * reference). One with just an Expected Result stays, so the review gate
 * flags the missing action instead of silently losing what was typed. Manual
 * Entry and the queue editor share this rule; the existing-case editor does
 * not use it, because dropping a stored step there would renumber the
 * steps Azure DevOps already holds. */
export function keptSteps(steps: Step[]): Step[] {
  return steps.filter(
    (s) =>
      s.shared != null ||
      s.action.trim() !== "" ||
      s.expected.trim() !== "" ||
      // A step holding a file stays even with no text: the review gate then
      // names the missing action, and a file is never dropped (or an
      // existing one removed from the work item) without the person's say.
      (s.attachments ?? []).length > 0,
  );
}
