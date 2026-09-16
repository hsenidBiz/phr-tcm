import { Minus } from "lucide-react";
import { cn } from "../../lib/cn";

/** shadcn-style checkbox: a square button with an accent fill + check mark
 * when checked. role="checkbox" keeps it label- and AT-friendly (clicking
 * a wrapping <label> activates it like a native input). */
export function Checkbox({
  checked,
  indeterminate = false,
  onCheckedChange,
  ariaLabel,
  className,
}: {
  checked: boolean;
  /** "Some but not all" - a minus mark and aria-checked="mixed". Only
   * meaningful while unchecked; clicking from mixed checks the rest
   * (onCheckedChange still receives true). */
  indeterminate?: boolean;
  onCheckedChange: (checked: boolean) => void;
  ariaLabel?: string;
  className?: string;
}) {
  const mixed = !checked && indeterminate;
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked ? true : mixed ? "mixed" : false}
      aria-label={ariaLabel}
      className={cn(
        "t-check flex h-4 w-4 shrink-0 items-center justify-center rounded-sm border",
        checked
          ? "border-accent bg-accent text-on-accent"
          : mixed
            ? "border-accent bg-accent-soft text-accent"
            : "border-border-strong bg-surface hover:border-accent",
        className,
      )}
      onClick={() => onCheckedChange(!checked)}
    >
      {/* The tick is always there, drawn in and rubbed out by its dash
          offset (see "Motion" in index.css) - a tick that mounted on check
          could only ever appear, never draw. */}
      {mixed ? (
        <Minus size={12} strokeWidth={3} />
      ) : (
        <svg aria-hidden viewBox="0 0 10.17 10.17" className="h-2.5 w-2.5" fill="none">
          <path
            className="t-check-tick"
            d="M1 5.52L3.92 9.17L9.17 1"
            stroke="currentColor"
            strokeWidth={1.8}
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      )}
    </button>
  );
}
