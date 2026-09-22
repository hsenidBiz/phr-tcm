// A client-side mirror of the expected-result floor
// (`src-tauri/src/autorun/floor.rs`'s `check_floor`), so the editor can show
// a case step's checked state before a save is even attempted.
//
// Only rules 1 and 2 are mirrored here - a case step with no matching
// script step, or one whose actions carry no check and no "unchecked"
// reason. Rules 3 and 4, which judge whether an "unchecked" reason was
// honest, stay server-side: they are about the SCRIPT policing itself, not
// about what this screen has to show the person.

import type { StepScript } from "../../bindings";

export type CheckState =
  | { kind: "checked" }
  | { kind: "explained"; reason: string }
  | { kind: "unchecked" };

/** Does this action kind JUDGE the page, the way `Action::is_check` does on
 * the Rust side? Every `check_` and `expect_` kind does; nothing else does. */
function isCheckKind(kind: string): boolean {
  return kind === "check_text" || kind === "check_url" || kind.startsWith("expect_");
}

/** One entry per case step with a non-empty expected result, in that
 * step's order, saying whether the script checks it. */
export function floorOf(
  steps: { action: string; expected: string }[],
  script: StepScript[],
): { step_number: number; state: CheckState }[] {
  return steps
    .map((s, i) => ({ step_number: i + 1, expected: s.expected.trim() }))
    .filter((s) => s.expected !== "")
    .map(({ step_number }) => {
      const scriptStep = script.find((ss) => ss.step_number === step_number);
      if (!scriptStep) return { step_number, state: { kind: "unchecked" as const } };
      const hasCheck = scriptStep.actions.some((a) => isCheckKind(a.kind));
      if (hasCheck) return { step_number, state: { kind: "checked" as const } };
      const reason = scriptStep.unchecked?.trim();
      if (reason) return { step_number, state: { kind: "explained" as const, reason } };
      return { step_number, state: { kind: "unchecked" as const } };
    });
}
