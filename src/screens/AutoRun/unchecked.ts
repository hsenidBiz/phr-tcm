// What a step's script leaves unchecked, as text. `StepScript.unchecked` is
// one sentence (the step checks nothing, and why) or a list of
// { item, why } entries; the screens show either as one line.

import type { Unchecked } from "../../bindings";

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
