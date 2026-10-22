// What a step's script leaves unchecked, as text. `StepScript.unchecked` is
// one sentence (the step checks nothing, and why) or a list of
// { item, why } entries; the screens show either as one line.

import type { CaseScript, Unchecked } from "../../bindings";
import { plural } from "../../lib/plural";

/** A sentence as it stands, or the entries as "item - why; item - why";
 * null for none or blank. The value comes from JSON a person may be typing,
 * so anything that is not one of the two shapes counts as none. */
export function uncheckedText(u: Unchecked | null | undefined): string | null {
  if (typeof u === "string") return u.trim() || null;
  if (!Array.isArray(u)) return null;
  const parts = u
    .filter((e) => typeof e === "object" && e !== null && typeof e.item === "string" && e.item.trim() !== "")
    .map((e) => `${e.item.trim()} - ${typeof e.why === "string" ? e.why.trim() : ""}`);
  return parts.length > 0 ? parts.join("; ") : null;
}

/** One expectation a script leaves unchecked: the TypeScript mirror of
 * `autorun::unchecked::UncheckedExpectation`, held to the same fixture. */
export type UncheckedExpectation = { step: number; item: string; why: string; stand_in: string | null };

/** The item of a step whose `unchecked` is one sentence. */
export const WHOLE_STEP_ITEM = "the step's expected result";

function collapsed(s: string): string {
  return s.split(/\s+/).filter(Boolean).join(" ");
}

/** `autorun::unchecked::expectations`: case steps (not `cleanup`) in run
 * order, whitespace collapsed; a sentence is one expectation, a list one per
 * entry, and a blank sentence or entry is left out. */
export function uncheckedExpectations(script: CaseScript): UncheckedExpectation[] {
  const out: UncheckedExpectation[] = [];
  for (const step of script.steps) {
    if (step.cleanup) continue;
    const u = step.unchecked;
    if (typeof u === "string") {
      const why = collapsed(u);
      if (why) out.push({ step: step.step_number, item: WHOLE_STEP_ITEM, why, stand_in: null });
    } else if (Array.isArray(u)) {
      for (const entry of u) {
        if (typeof entry !== "object" || entry === null || typeof entry.item !== "string") continue;
        const item = collapsed(entry.item);
        if (!item) continue;
        const stand_in = typeof entry.stand_in === "string" ? collapsed(entry.stand_in) || null : null;
        const why = typeof entry.why === "string" ? collapsed(entry.why) : "";
        out.push({ step: step.step_number, item, why, stand_in });
      }
    }
  }
  return out;
}

/** "step 3: item - why" + " (stand-in: kind)". */
export function uncheckedLine(e: UncheckedExpectation): string {
  const line = `step ${e.step}: ${e.item} - ${e.why}`;
  return e.stand_in ? `${line} (stand-in: ${e.stand_in})` : line;
}

/** "1 expectation unchecked" / "N expectations unchecked". */
export function uncheckedWords(n: number): string {
  return `${plural(n, "expectation")} unchecked`;
}
