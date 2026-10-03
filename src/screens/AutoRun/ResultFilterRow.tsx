// The row of result filters above a list of cases (Run review) or runs
// (Past runs): All, Passed, Failed, Blocked, Not run - one pressed at a
// time, All by default, each with how many it would show. One component,
// so the two lists filter the same way. `ResultToggleRow` is its multi-select
// sibling for the Auto Run case list: no All, any number pressed, none
// pressed meaning no filter.

import { cn } from "../../lib/cn";
import { RESULT_BUCKETS, type ResultBucket, type ResultFilter } from "./verdicts";

export default function ResultFilterRow({
  value,
  onChange,
  total,
  counts,
  titleFor,
}: {
  value: ResultFilter;
  onChange: (f: ResultFilter) => void;
  /** What All shows. */
  total: number;
  /** What each bucket shows. */
  counts: Record<ResultBucket, number>;
  /** A tooltip per button, when the counts are not of what the list shows
   * one-to-one (Past runs counts runs, not cases). */
  titleFor?: (f: ResultFilter) => string;
}) {
  const options: ResultFilter[] = ["All", ...RESULT_BUCKETS];
  return (
    <div role="group" aria-label="Filter by result" className="flex flex-wrap gap-1">
      {options.map((f) => (
        <button
          key={f}
          type="button"
          aria-pressed={value === f}
          title={titleFor?.(f)}
          className={pill(value === f)}
          onClick={() => onChange(f)}
        >
          {f} ({f === "All" ? total : counts[f]})
        </button>
      ))}
    </div>
  );
}

const pill = (on: boolean) =>
  cn(
    "rounded-full px-2 py-0.5 text-[11px] transition-colors",
    on ? "bg-accent-soft text-accent" : "text-faint hover:bg-surface-2 hover:text-text",
  );

/** Several result buckets pressed at once. Each button is a toggle; the row
 * never says "All" - nothing pressed is every case. */
export function ResultToggleRow({
  label,
  pressed,
  onToggle,
  counts,
  disabled = false,
}: {
  /** The group's accessible name. */
  label: string;
  pressed: ReadonlySet<ResultBucket>;
  onToggle: (b: ResultBucket) => void;
  /** Left out while the numbers are not known yet: the buttons name just
   * the bucket. */
  counts?: Record<ResultBucket, number>;
  disabled?: boolean;
}) {
  return (
    <div role="group" aria-label={label} className="flex flex-wrap gap-1">
      {RESULT_BUCKETS.map((b) => (
        <button
          key={b}
          type="button"
          aria-pressed={pressed.has(b)}
          disabled={disabled}
          className={cn(pill(pressed.has(b)), disabled && "pointer-events-none opacity-50")}
          onClick={() => onToggle(b)}
        >
          {counts ? `${b} (${counts[b]})` : b}
        </button>
      ))}
    </div>
  );
}
