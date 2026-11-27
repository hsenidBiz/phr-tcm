// What a step's script leaves unchecked, as text. `StepScript.unchecked` is
// one sentence (the step checks nothing, and why) or a list of
// { item, why } entries; the screens show either as one line.

import type { CaseScript, Unchecked } from "../../bindings";
import { plural } from "../../lib/plural";
import { doneByAutoRun } from "./byAutoRun";

/** A sentence as it stands, or the entries as "item - why; item - why";
 * null for none or blank. The value comes from JSON a person may be typing,
 * so anything that is not one of the two shapes counts as none. */
export function uncheckedText(u: Unchecked | null | undefined): string | null {
  const parts = uncheckedParts(u);
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
 * entry, and a blank sentence or entry is left out. A step Auto Test performs
 * itself (`doneByAutoRun`) has none. */
export function uncheckedExpectations(script: CaseScript): UncheckedExpectation[] {
  const out: UncheckedExpectation[] = [];
  const byAutoRun = doneByAutoRun(script);
  for (const step of script.steps) {
    if (step.cleanup || byAutoRun.has(step.step_number)) continue;
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

/** "item - why" + " (stand-in: kind)": the entry without its step, for a line shown under that step. */
export function uncheckedDetail(e: UncheckedExpectation): string {
  const line = `${e.item} - ${e.why}`;
  return e.stand_in ? `${line} (stand-in: ${e.stand_in})` : line;
}

/** "step 3: item - why" + " (stand-in: kind)". */
export function uncheckedLine(e: UncheckedExpectation): string {
  return `step ${e.step}: ${uncheckedDetail(e)}`;
}

/** "1 expectation unchecked" / "N expectations unchecked". */
export function uncheckedWords(n: number): string {
  return `${plural(n, "expectation")} unchecked`;
}

/** What a step leaves unchecked, one line per part: the sentence as it
 * stands, or each entry as "item - why". Empty for none or blank, and for
 * a shape that is neither (the value comes from JSON a person may be
 * typing). */
export function uncheckedParts(u: Unchecked | null | undefined): string[] {
  if (typeof u === "string") return u.trim() ? [u.trim()] : [];
  if (!Array.isArray(u)) return [];
  return u
    .filter((e) => typeof e === "object" && e !== null && typeof e.item === "string" && e.item.trim() !== "")
    .map((e) => `${e.item.trim()} - ${typeof e.why === "string" ? e.why.trim() : ""}`);
}
