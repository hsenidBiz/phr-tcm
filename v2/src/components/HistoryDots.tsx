import type { RunOutcome } from "../bindings";
import { cn } from "../lib/cn";
import { outcomeLabel } from "../screens/RunPanel";

const DOT: Record<string, string> = {
  passed: "bg-success",
  failed: "bg-danger",
  paused: "bg-muted",
  blocked: "bg-warning",
  notapplicable: "bg-faint",
};

/** Last-N outcome dots for a test case, newest on the left. Each dot
 * tooltips its outcome + date; unknown outcomes render hollow. */
export default function HistoryDots({
  outcomes,
  size = 8,
}: {
  outcomes: RunOutcome[];
  size?: number;
}) {
  if (outcomes.length === 0) return <span className="text-faint">—</span>;
  return (
    <span className="inline-flex items-center gap-1" aria-label="Recent outcomes">
      {outcomes.map((o, i) => {
        const cls = DOT[o.outcome.toLowerCase()];
        const date = o.completed_date ? o.completed_date.slice(0, 10) : "";
        return (
          <span
            key={i}
            title={`${outcomeLabel(o.outcome)}${date ? ` · ${date}` : ""} (run #${o.run_id})`}
            className={cn(
              "inline-block rounded-full",
              cls ?? "border border-border-strong bg-transparent",
            )}
            style={{ width: size, height: size }}
          />
        );
      })}
    </span>
  );
}
