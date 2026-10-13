// Where a run is in its batches: "Batch 2 of 3", and every batch with its
// size, the current one marked. Shown on the supervised pane and the
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
    <div className="space-y-1 text-xs">
      {at && (
        <p role="status" className="font-medium text-text">
          Batch {at.n} of {at.of}
        </p>
      )}
      <ol aria-label="Batches" className="flex flex-wrap gap-x-3 gap-y-0.5">
        {batches.map((b) => (
          <li
            key={b.n}
            aria-current={at?.n === b.n ? "step" : undefined}
            className={cn(at?.n === b.n ? "font-semibold text-text" : "text-muted")}
          >
            {batchLabel(b.n, b.ids.length)}
          </li>
        ))}
      </ol>
    </div>
  );
}
