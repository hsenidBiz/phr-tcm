import { Check, Minus } from "lucide-react";
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
        "flex h-4 w-4 shrink-0 items-center justify-center rounded-sm border transition-colors",
        checked
          ? "border-accent bg-accent text-on-accent"
          : mixed
            ? "border-accent bg-accent-soft text-accent"
            : "border-border-strong bg-surface hover:border-accent",
        className,
      )}
      onClick={() => onCheckedChange(!checked)}
    >
      {checked && <Check size={12} strokeWidth={3} />}
      {mixed && <Minus size={12} strokeWidth={3} />}
    </button>
  );
}
