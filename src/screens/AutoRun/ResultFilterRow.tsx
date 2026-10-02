// The row of result filters above a list of cases (Run review) or runs
// (Past runs): All, Passed, Failed, Blocked, Not run - one pressed at a
// time, All by default, each with how many it would show. One component,
// so the two lists filter the same way.

import { cn } from "../../lib/cn";
import { RESULT_BUCKETS, type ResultBucket, type ResultFilter } from "./verdicts";

export default function ResultFilterRow({
  value,
  onChange,
  total,
  counts,
}: {
  value: ResultFilter;
  onChange: (f: ResultFilter) => void;
  /** What All shows. */
  total: number;
  /** What each bucket shows. */
  counts: Record<ResultBucket, number>;
}) {
  const options: ResultFilter[] = ["All", ...RESULT_BUCKETS];
  return (
    <div role="group" aria-label="Filter by result" className="flex flex-wrap gap-1">
      {options.map((f) => (
        <button
          key={f}
          type="button"
          aria-pressed={value === f}
          className={cn(
            "rounded-full px-2 py-0.5 text-[11px] transition-colors",
            value === f ? "bg-accent-soft text-accent" : "text-faint hover:bg-surface-2 hover:text-text",
          )}
          onClick={() => onChange(f)}
        >
          {f} ({f === "All" ? total : counts[f]})
        </button>
      ))}
    </div>
  );
}
