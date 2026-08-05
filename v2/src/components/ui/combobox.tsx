import { Check, ChevronsUpDown, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { cn } from "../../lib/cn";

/** Searchable single-select dropdown. Click to open a filterable list;
 * type to narrow; Enter/click selects. When `allowCustom`, a value not in
 * the list can still be entered (free-entry fields). */
export default function Combobox({
  value,
  onChange,
  options,
  placeholder = "Select…",
  ariaLabel,
  className,
  allowCustom = false,
  loading = false,
  details,
}: {
  value: string;
  onChange: (v: string) => void;
  options: string[];
  placeholder?: string;
  ariaLabel?: string;
  className?: string;
  allowCustom?: boolean;
  loading?: boolean;
  /** Right-aligned faint annotation per option (e.g. a sprint's date
   * range, like Azure DevOps's iteration dropdown). */
  details?: Record<string, string>;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const ref = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  useEffect(() => {
    if (open) inputRef.current?.focus();
    else setQuery("");
  }, [open]);

  const q = query.trim().toLowerCase();
  const filtered = useMemo(
    () => (q ? options.filter((o) => o.toLowerCase().includes(q)) : options),
    [options, q],
  );
  const showCustom = allowCustom && query.trim() && !options.some((o) => o.toLowerCase() === q);

  const commit = (v: string) => {
    onChange(v);
    setOpen(false);
  };

  return (
    <div ref={ref} className={cn("relative", className)}>
      <button
        type="button"
        aria-label={ariaLabel}
        className="flex w-full items-center justify-between gap-2 rounded-md border border-border bg-surface px-2 py-1.5 text-left text-sm transition-colors hover:border-border-strong"
        onClick={() => setOpen((o) => !o)}
      >
        <span className={cn("truncate", value ? "text-text" : "text-faint")}>
          {value || placeholder}
        </span>
        <span className="flex shrink-0 items-center gap-1">
          {value && (
            <span
              role="button"
              aria-label="Clear"
              className="text-muted hover:text-danger"
              onClick={(e) => {
                e.stopPropagation();
                onChange("");
              }}
            >
              <X size={13} />
            </span>
          )}
          <ChevronsUpDown size={13} className="text-muted" />
        </span>
      </button>

      {open && (
        // w-max, not w-full: the trigger's width fits a SELECTED value,
        // but option lists like iteration paths run much longer - the
        // panel grows to the longest option (viewport-capped, then the
        // rows truncate) instead of squeezing everything to the box.
        <div className="absolute left-0 top-full z-40 mt-1 w-max min-w-full max-w-[min(42rem,calc(100vw-3rem))] rounded-md border border-border bg-surface shadow-xl">
          <div className="border-b border-border p-1.5">
            <input
              ref={inputRef}
              className="w-full rounded bg-surface-2 px-2 py-1 text-sm text-text outline-none placeholder:text-faint"
              placeholder="Search…"
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setActive(0);
              }}
              onKeyDown={(e) => {
                const rows = filtered.length + (showCustom ? 1 : 0);
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setActive((a) => Math.min(a + 1, rows - 1));
                } else if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setActive((a) => Math.max(a - 1, 0));
                } else if (e.key === "Enter") {
                  e.preventDefault();
                  if (active < filtered.length) commit(filtered[active]);
                  else if (showCustom) commit(query.trim());
                } else if (e.key === "Escape") {
                  setOpen(false);
                }
              }}
            />
          </div>
          <ul className="max-h-56 overflow-y-auto p-1">
            {loading && <li className="px-2 py-1.5 text-sm text-muted">Loading…</li>}
            {!loading && filtered.length === 0 && !showCustom && (
              <li className="px-2 py-1.5 text-sm text-muted">No matches</li>
            )}
            {filtered.map((o, i) => (
              <li key={o}>
                <button
                  className={cn(
                    "flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-sm",
                    i === active ? "bg-accent-soft text-accent" : "text-text hover:bg-surface-2",
                  )}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => commit(o)}
                >
                  <span className="truncate">{o}</span>
                  <span className="ml-2 flex shrink-0 items-center gap-1.5">
                    {details?.[o] && (
                      <span className="whitespace-nowrap text-xs text-faint">{details[o]}</span>
                    )}
                    {o === value && <Check size={13} className="shrink-0 text-accent" />}
                  </span>
                </button>
              </li>
            ))}
            {showCustom && (
              <li>
                <button
                  className={cn(
                    "w-full rounded px-2 py-1.5 text-left text-sm",
                    active === filtered.length
                      ? "bg-accent-soft text-accent"
                      : "text-text hover:bg-surface-2",
                  )}
                  onMouseEnter={() => setActive(filtered.length)}
                  onClick={() => commit(query.trim())}
                >
                  Use "{query.trim()}"
                </button>
              </li>
            )}
          </ul>
        </div>
      )}
    </div>
  );
}
