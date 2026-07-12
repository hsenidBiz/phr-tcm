import { ChevronsUpDown, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "../../lib/cn";
import { Checkbox } from "./checkbox";

/** Checkbox multi-select dropdown (v1 CheckableComboBox parity): the
 * trigger summarizes the selection ("All", "Bug, Task", "3 selected"),
 * the dropdown stays open while toggling rows. Empty selection = all. */
export default function MultiSelect({
  options,
  selected,
  onChange,
  allLabel = "All",
  ariaLabel,
  className,
}: {
  options: string[];
  selected: string[];
  onChange: (v: string[]) => void;
  allLabel?: string;
  ariaLabel?: string;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  const summary =
    selected.length === 0
      ? allLabel
      : selected.length <= 2
        ? selected.join(", ")
        : `${selected.length} selected`;

  const toggle = (opt: string) =>
    onChange(
      selected.includes(opt) ? selected.filter((s) => s !== opt) : [...selected, opt],
    );

  return (
    <div ref={ref} className={cn("relative", className)}>
      <button
        type="button"
        aria-label={ariaLabel}
        className="flex w-full items-center justify-between gap-2 rounded-md border border-border bg-surface px-2 py-1.5 text-left text-sm transition-colors hover:border-border-strong"
        onClick={() => setOpen((o) => !o)}
      >
        <span className={cn("truncate", selected.length ? "text-text" : "text-faint")}>
          {summary}
        </span>
        <span className="flex shrink-0 items-center gap-1">
          {selected.length > 0 && (
            <span
              role="button"
              aria-label="Clear selection"
              className="text-muted hover:text-danger"
              onClick={(e) => {
                e.stopPropagation();
                onChange([]);
              }}
            >
              <X size={13} />
            </span>
          )}
          <ChevronsUpDown size={13} className="text-muted" />
        </span>
      </button>

      {open && (
        <div className="absolute left-0 top-full z-40 mt-1 w-full min-w-44 rounded-md border border-border bg-surface p-1 shadow-xl">
          {options.length === 0 && <p className="px-2 py-1.5 text-sm text-muted">No options</p>}
          {options.map((opt) => (
            <label
              key={opt}
              className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm text-text hover:bg-surface-2"
            >
              <Checkbox checked={selected.includes(opt)} onCheckedChange={() => toggle(opt)} />
              {opt}
            </label>
          ))}
        </div>
      )}
    </div>
  );
}
