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
  checkedFirst = false,
}: {
  options: string[];
  selected: string[];
  onChange: (v: string[]) => void;
  allLabel?: string;
  ariaLabel?: string;
  className?: string;
  /** Snapshot the option order (checked first, in `options` order, then
   * the rest) the moment the panel opens, rather than live - otherwise an
   * option a person just ticked would jump out from under the pointer. */
  checkedFirst?: boolean;
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

  // The checked-first order is a snapshot taken when the panel OPENS, not
  // recomputed on every render - toggling a row while open must not move
  // it out from under the pointer. Deliberately depends on `open` alone;
  // `options`/`selected`/`checkedFirst` are read fresh at the moment this
  // runs, from whichever render happened to trigger it.
  const [order, setOrder] = useState(options);
  useEffect(() => {
    if (!open || !checkedFirst) return;
    setOrder([...options.filter((o) => selected.includes(o)), ...options.filter((o) => !selected.includes(o))]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // `options` can itself change while the panel stays open - PrPanel's repo
  // list is still loading when a person opens it, so the snapshot above may
  // have caught only "Your Pull Requests". A later options change must not
  // just replace the snapshot with the raw list (that would drop the
  // checked-first grouping and reflow everything already on screen): rows
  // already shown keep their place, and newly-arrived ones join whichever
  // group - checked or unchecked - they belong in, in `options` order
  // within that group. Keyed on the options' own content, not identity, so
  // a caller passing a fresh array literal every render does not retrigger
  // this on every keystroke elsewhere on the page.
  const optionsKey = options.join("\u0001");
  useEffect(() => {
    if (!open || !checkedFirst) return;
    setOrder((prev) => {
      const stillHere = prev.filter((o) => options.includes(o));
      const arrived = options.filter((o) => !prev.includes(o));
      return [
        ...stillHere.filter((o) => selected.includes(o)),
        ...arrived.filter((o) => selected.includes(o)),
        ...stillHere.filter((o) => !selected.includes(o)),
        ...arrived.filter((o) => !selected.includes(o)),
      ];
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [optionsKey]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  // Only the checked-first order is a snapshot worth holding; without it
  // the rows are simply `options`, read in the same render that brings them
  // - a copy in state would show the old list for a commit first.
  const shown = checkedFirst ? order : options;

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
        <div className="t-dropdown absolute left-0 top-full z-40 mt-1 w-full min-w-44 rounded-md border border-border bg-surface shadow-xl">
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
            {shown.length === 0 && <li className="px-2 py-1.5 text-sm text-muted">No options</li>}
            {shown
              .filter((opt) => !query.trim() || opt.toLowerCase().includes(query.trim().toLowerCase()))
              .map((opt) => (
                <li key={opt}>
                  <label className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm text-text hover:bg-surface-2">
                    <Checkbox checked={selected.includes(opt)} onCheckedChange={() => toggle(opt)} />
                    {opt}
                  </label>
                </li>
              ))}
            {shown.length > 0 &&
              query.trim() &&
              !shown.some((opt) => opt.toLowerCase().includes(query.trim().toLowerCase())) && (
                <li className="px-2 py-1.5 text-sm text-muted">No matches</li>
              )}
          </ul>
        </div>
      )}
    </div>
  );
}
