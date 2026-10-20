// What an assistant's repairs did to a script's preconditions, as the
// script editor shows it and puts it back.

import type { Precondition, PreconditionRepair } from "../../bindings";

/** A precondition as the editor lists it: flow / stage: value. */
export function preconditionText(p: Precondition): string {
  return `${p.flow} / ${p.stage}: ${typeof p.value === "string" ? p.value : JSON.stringify(p.value ?? null)}`;
}

/** The same precondition: flow, stage, value and why all agree. */
function same(a: Precondition, b: Precondition): boolean {
  return (
    a.flow === b.flow &&
    a.stage === b.stage &&
    JSON.stringify(a.value ?? null) === JSON.stringify(b.value ?? null) &&
    (a.why ?? null) === (b.why ?? null)
  );
}

/** One repair in a line, without its reason. */
export function repairText(r: PreconditionRepair): string {
  const before = r.before ? preconditionText(r.before) : "";
  const after = r.after ? preconditionText(r.after) : "";
  if (r.op === "add") return `Added ${after}`;
  if (r.op === "remove") return `Removed ${before}`;
  return `Changed ${before} to ${after}`;
}

/**
 * Whether putting `r` back would change `preconditions`: its result is
 * still there (an added or changed precondition is present, a removed one
 * is absent). When it is not, a later change has changed it again.
 */
export function canPutBack(preconditions: Precondition[], r: PreconditionRepair): boolean {
  const { before, after } = r;
  if (r.op === "remove") return !!before && !preconditions.some((p) => same(p, before));
  return !!after && (r.op === "add" || !!before) && preconditions.some((p) => same(p, after));
}

/**
 * `preconditions` with the repairs at `putBack` (places in `repairs`)
 * undone, newest first: a removed precondition comes back at the end, an
 * added one goes, a changed one goes back to what it was. A repair whose
 * result is no longer there (a person or a later repair changed it again)
 * is left as it is.
 */
export function putBackPreconditions(
  preconditions: Precondition[],
  repairs: PreconditionRepair[],
  putBack: number[],
): Precondition[] {
  let out = [...preconditions];
  for (const i of [...putBack].sort((a, b) => b - a)) {
    const r = repairs[i];
    if (!r) continue;
    const { before, after } = r;
    const at = after ? out.findIndex((p) => same(p, after)) : -1;
    if (r.op === "remove" && before) {
      if (!out.some((p) => same(p, before))) out = [...out, before];
    } else if (r.op === "add" && at >= 0) {
      out = out.filter((_, j) => j !== at);
    } else if (r.op === "change" && before && at >= 0) {
      out = out.map((p, j) => (j === at ? before : p));
    }
  }
  return out;
}
