import type { Dispatch, SetStateAction } from "react";
import type { TestCase } from "../../bindings";
import { Button } from "../ui/button";
import { Switch } from "../ui/switch";

/** The Order buttons and the Group by area switch above the queue. */
export default function QueueOrderBar({
  queue,
  setQueue,
  areaGrouped,
  onGroupedChange,
}: {
  queue: TestCase[];
  setQueue: Dispatch<SetStateAction<TestCase[]>>;
  areaGrouped: boolean;
  onGroupedChange: (on: boolean) => void;
}) {
  if (!(queue.length > 1 || (queue.length > 0 && areaGrouped))) return null;
  // The two readings of an optimized file: grouped so a tester changes
  // environment as little as possible, or walking down the spec so a
  // reviewer scrolls the document and the queue together. Only offered
  // when EVERY case carries the field - a partial sort would interleave
  // ordered cases with ones that have no opinion, which is neither
  // reading. Reordering is real, not a view: the queue's order is the
  // order the cases are created in.
  //
  // Shown as soon as ANY case carries an order - a bar that only
  // appears when everything is already stamped would hide the reason
  // the buttons are disabled, which is the one thing a mixed queue
  // needs explained.
  //
  // Group by area sits on the same line, at its far end: both change
  // how the list reads, but only the Order buttons change the queue.
  // A single case has nothing to group, so the switch waits for a
  // second - unless grouping is already on, when it stays to be
  // switched off.
  return (
    <div className="flex flex-wrap items-center gap-2 text-xs text-muted">
      {queue.length > 1 &&
        (["tester_order", "spec_order"] as const).some((k) => queue.some((tc) => tc[k] != null)) && (
      <>
        <span>Order:</span>
        {(
          [
            ["tester_order", "For testing", "Cases sharing a setup run together"],
            ["spec_order", "Down the spec", "Cases follow the specification document"],
          ] as const
        ).map(([key, label, hint]) => {
          const available = queue.every((tc) => tc[key] != null);
          const active =
            available && queue.every((tc, i) => i === 0 || (queue[i - 1][key] ?? 0) <= (tc[key] ?? 0));
          return (
            <Button
              key={key}
              variant="outline"
              size="sm"
              disabled={!available}
              aria-pressed={active}
              title={available ? hint : "Not every queued case has this order yet - it's added when an AI assistant optimizes the draft"}
              className={active ? "border-accent text-accent" : undefined}
              onClick={() =>
                setQueue((q) => [...q].sort((a, b) => (a[key] ?? 0) - (b[key] ?? 0)))
              }
            >
              {label}
            </Button>
          );
        })}
      </>
      )}
      <label className="ml-auto flex items-center gap-1.5">
        <Switch
          checked={areaGrouped}
          ariaLabel="Group by area"
          onCheckedChange={onGroupedChange}
        />
        Group by area
      </label>
    </div>
  );
}
