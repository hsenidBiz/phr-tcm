// The test-plan scan is the app's longest wait, so it gets a real
// progress bar rather than a line of text: determinate once ADO starts
// reporting plan counts, indeterminate before that, and glowing either
// way so it reads as "working" at a glance.
//
// Honours prefers-reduced-motion (the glow and the sweep both stop) -
// same rule as the flask visuals.

import { cn } from "../lib/cn";

export default function ScanProgress({
  label,
  done,
  total,
  className,
}: {
  label: string;
  /** Omit both to get the indeterminate sweep. */
  done?: number;
  total?: number;
  className?: string;
}) {
  const determinate = typeof done === "number" && typeof total === "number" && total > 0;
  const pct = determinate ? Math.min(100, Math.round((done! / total!) * 100)) : 0;

  return (
    <div className={cn("space-y-1.5", className)}>
      <div className="flex items-center justify-between gap-2 text-sm text-muted">
        <span>{label}</span>
        {determinate && (
          <span className="id-mono shrink-0 text-xs text-faint">
            {done} / {total}
          </span>
        )}
      </div>
      <div
        role="progressbar"
        aria-label={label}
        aria-valuemin={determinate ? 0 : undefined}
        aria-valuemax={determinate ? total : undefined}
        aria-valuenow={determinate ? done : undefined}
        className="h-1.5 w-full overflow-hidden rounded-full bg-surface-2"
      >
        <div
          className={cn(
            "h-full rounded-full bg-accent scan-glow",
            determinate ? "transition-[width] duration-300 ease-out" : "scan-sweep",
          )}
          style={determinate ? { width: `${pct}%` } : undefined}
        />
      </div>
    </div>
  );
}
