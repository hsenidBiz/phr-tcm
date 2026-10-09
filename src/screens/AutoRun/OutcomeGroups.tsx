// A step's action lines, with the lines a component ran gathered under one
// line naming it and indented beneath it. Each screen draws its own lines
// (`line`); this only groups them. An outcome says which component it came
// from (`ActionOutcome.component`); older run files say none.

import { Fragment, type ReactNode } from "react";
import type { ActionOutcome } from "../../bindings";

export type OutcomeGroup = {
  /** The component these lines came from; `null` for the step's own. */
  component: string | null;
  /** Each outcome with its place in the step, so lines keep their numbers. */
  items: { outcome: ActionOutcome; index: number }[];
};

/** Consecutive outcomes from the same component, as one group. The same
 * component used twice with a line of the step's own between them is two
 * groups, as it ran. */
export function groupOutcomes(outcomes: ActionOutcome[]): OutcomeGroup[] {
  const groups: OutcomeGroup[] = [];
  outcomes.forEach((outcome, index) => {
    const component = outcome.component ?? null;
    const last = groups[groups.length - 1];
    if (last && component !== null && last.component === component) last.items.push({ outcome, index });
    else groups.push({ component, items: [{ outcome, index }] });
  });
  return groups;
}

export default function OutcomeGroups({
  outcomes,
  line,
}: {
  outcomes: ActionOutcome[];
  /** One outcome's line; `index` is its place in the step. */
  line: (outcome: ActionOutcome, index: number) => ReactNode;
}) {
  return (
    <>
      {groupOutcomes(outcomes).map((g) =>
        g.component === null ? (
          <Fragment key={g.items[0].index}>{g.items.map(({ outcome, index }) => line(outcome, index))}</Fragment>
        ) : (
          <div key={g.items[0].index} role="group" aria-label={`Component ${g.component}`} className="mt-1">
            <p className="text-xs font-medium text-text">{g.component}</p>
            <div className="border-l border-border/60 pl-3">
              {g.items.map(({ outcome, index }) => line(outcome, index))}
            </div>
          </div>
        ),
      )}
    </>
  );
}
