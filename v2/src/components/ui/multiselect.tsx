import { ChevronsUpDown, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "../../lib/cn";
import { Checkbox } from "./checkbox";

/** Options past this count get a search box - below it, the whole list
 * fits on screen and a filter would just push the rows down. */
const SEARCH_FROM = 8;

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
  const [query, setQuery] = useState("");
  const ref = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Focus the search on open, forget the filter on close - a stale query
  // reopening to "No matches" would look like the options vanished.
  useEffect(() => {
    if (open && options.length > SEARCH_FROM) inputRef.current?.focus();
    if (!open) setQuery("");
  }, [open, options.length]);

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
        className={cn(
          "flex w-full items-center justify-between gap-2 rounded-md border border-border bg-surface px-2 py-1.5 text-left text-sm transition-colors hover:border-border-strong focus:border-accent focus:outline-none",
          // While open, focus lives in the panel's search input - the
          // trigger keeps the accent explicitly so every dropdown shows
          // the same lit border as a focused Input.
          open && "border-accent",
        )}
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
        <div className="absolute left-0 top-full z-40 mt-1 w-full min-w-44 rounded-md border border-border bg-surface shadow-xl">
          {options.length > SEARCH_FROM && (
            <div className="border-b border-border p-1.5">
              <input
                ref={inputRef}
                aria-label={ariaLabel ? `Search ${ariaLabel.toLowerCase()}` : "Search options"}
                className="w-full rounded bg-surface-2 px-2 py-1 text-sm text-text outline-none placeholder:text-faint"
                placeholder="Search…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") setOpen(false);
                }}
              />
            </div>
          )}
          <ul className="max-h-64 overflow-y-auto p-1">
            {options.length === 0 && <li className="px-2 py-1.5 text-sm text-muted">No options</li>}
            {options
              .filter((opt) => !query.trim() || opt.toLowerCase().includes(query.trim().toLowerCase()))
              .map((opt) => (
                <li key={opt}>
                  <label className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm text-text hover:bg-surface-2">
                    <Checkbox checked={selected.includes(opt)} onCheckedChange={() => toggle(opt)} />
                    {opt}
                  </label>
                </li>
              ))}
            {options.length > 0 &&
              query.trim() &&
              !options.some((opt) => opt.toLowerCase().includes(query.trim().toLowerCase())) && (
                <li className="px-2 py-1.5 text-sm text-muted">No matches</li>
              )}
          </ul>
        </div>
      )}
    </div>
  );
}
