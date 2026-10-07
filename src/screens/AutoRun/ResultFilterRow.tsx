// The row of result filters above a list of cases (Run review) or runs
// (Past runs): All, Passed, Failed, Blocked, Not run - one pressed at a
// time, All by default, each with how many it would show. One component,
// so the two lists filter the same way. `ResultToggleRow` is its multi-select
// sibling for the Auto Run case list: no All, any number pressed, none
// pressed meaning no filter.
//
// Every button is a pill at body text size, outlined in its result's colour
// with its count in a badge, and filled with that colour while pressed. The
// accessible name stays "Failed (2)": the badge is a look, not new words.

import { cn } from "../../lib/cn";
import { RESULT_BUCKETS, type ResultBucket, type ResultFilter } from "./verdicts";

/** Each filter's colours: outlined at rest, filled while pressed, and the
 * count badge's tint at rest. Tokens only, the ones `bucketTone` uses. */
const PILL_TONE: Record<ResultFilter, { rest: string; on: string; badge: string }> = {
  All: { rest: "border-accent/60 text-accent", on: "border-accent bg-accent text-on-accent", badge: "bg-accent/15" },
  Passed: { rest: "border-success/60 text-success", on: "border-success bg-success text-on-status", badge: "bg-success/15" },
  Failed: { rest: "border-danger/60 text-danger", on: "border-danger bg-danger text-on-status", badge: "bg-danger/15" },
  Blocked: { rest: "border-warning/60 text-warning", on: "border-warning bg-warning text-on-status", badge: "bg-warning/15" },
  "Not run": { rest: "border-border-strong text-muted", on: "border-muted bg-muted text-on-status", badge: "bg-surface-2" },
};

/** One pill: the filter's name, then its count in a badge when known. */
function Pill({
  filter,
  on,
  count,
  disabled = false,
  title,
  onClick,
}: {
  filter: ResultFilter;
  on: boolean;
  count?: number;
  disabled?: boolean;
  title?: string;
  onClick: () => void;
}) {
  const tone = PILL_TONE[filter];
  return (
    <button
      type="button"
      aria-pressed={on}
      aria-label={count === undefined ? filter : `${filter} (${count})`}
      title={title}
      disabled={disabled}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-sm font-medium transition-colors",
        on ? tone.on : cn(tone.rest, "bg-surface hover:bg-surface-2"),
        disabled && "pointer-events-none opacity-50",
      )}
      onClick={onClick}
    >
      <span className="label-trim">{filter}</span>
      {count !== undefined && (
        <span
          aria-hidden
          className={cn(
            "min-w-5 rounded-full px-1.5 text-center text-xs font-semibold",
            on ? "bg-on-status/20" : tone.badge,
          )}
        >
          <span className="label-trim">{count}</span>
        </span>
      )}
    </button>
  );
}

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
    <div role="group" aria-label="Filter by result" className="flex flex-wrap gap-1.5">
      {options.map((f) => (
        <Pill
          key={f}
          filter={f}
          on={value === f}
          count={f === "All" ? total : counts[f]}
          title={titleFor?.(f)}
          onClick={() => onChange(f)}
        />
      ))}
    </div>
  );
}

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
    <div role="group" aria-label={label} className="flex flex-wrap gap-1.5">
      {RESULT_BUCKETS.map((b) => (
        <Pill
          key={b}
          filter={b}
          on={pressed.has(b)}
          count={counts?.[b]}
          disabled={disabled}
          onClick={() => onToggle(b)}
        />
      ))}
    </div>
  );
}
