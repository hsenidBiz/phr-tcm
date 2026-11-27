// Where a run is in its batches: a "Batch" label with "2 of 3" beside it,
// and a tab per batch with its size, the current one tinted. Shown on the supervised pane and the
// unattended run window. A plan without pauses has one implicit batch and
// shows nothing.

import type { PlanView } from "../../bindings";
import { cn } from "../../lib/cn";
import { batchLabel, batchOf, planBatches } from "./plan";

export default function BatchList({
  plan,
  currentId,
}: {
  plan: PlanView | null | undefined;
  /** The case running now, or the one the run is paused before. */
  currentId: number | null | undefined;
}) {
  const batches = planBatches(plan);
  if (batches.length === 0) return null;
  const at = currentId == null ? null : batchOf(plan, currentId);
  return (
    <div className="space-y-1.5 text-xs">
      <div className="flex items-baseline justify-between gap-2">
        <span className="font-medium text-muted">Batch</span>
        {at && (
          <span role="status" className="text-muted">
            {at.n} of {at.of}
          </span>
        )}
      </div>
      <ol aria-label="Batches" className="flex flex-wrap gap-1">
        {batches.map((b) => {
          const current = at?.n === b.n;
          return (
            <li
              key={b.n}
              aria-current={current ? "step" : undefined}
              aria-label={batchLabel(b.n, b.ids.length)}
              className={cn(
                "rounded-md px-2 py-1",
                current ? "bg-accent/15 font-medium text-accent" : "text-muted",
              )}
            >
              <span className="label-trim">
                {b.n} · {b.ids.length} {b.ids.length === 1 ? "case" : "cases"}
              </span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
