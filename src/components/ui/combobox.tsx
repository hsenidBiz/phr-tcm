import { Check, ChevronsUpDown, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { cn } from "../../lib/cn";
import { IconPinToTop } from "../../lib/actionIcons";

/** Searchable single-select dropdown. Click to open a filterable list;
 * type to narrow; Enter/click selects. When `allowCustom`, a value not in
 * the list can still be entered (free-entry fields). Takes either plain
 * strings (`options`) or value/label pairs (`items`), for lists whose
 * labels are not their values. */
export default function Combobox({
  value,
  onChange,
  options = [],
  items,
  placeholder = "Select…",
  ariaLabel,
  className,
  triggerClassName,
  allowCustom = false,
  loading = false,
  details,
  pins,
}: {
  value: string;
  onChange: (v: string) => void;
  options?: string[];
  /** Value/label pairs, for lists whose labels are not their values (ids,
   * indented names). When given, `options` is ignored: search matches the
   * label, and `onChange` receives the value. */
  items?: Array<{ value: string; label: string }>;
  placeholder?: string;
  ariaLabel?: string;
  className?: string;
  /** Extra classes for the trigger button (width, padding). */
  triggerClassName?: string;
  allowCustom?: boolean;
  loading?: boolean;
  /** Right-aligned faint annotation per option (e.g. a sprint's date
   * range, like Azure DevOps's iteration dropdown). */
  details?: Record<string, string>;
  /** Rows that can be pinned to the top of the list. A pinnable row shows
   * a pin while it is pointed at or active; a pinned one always does, and
   * pressing it unpins. Pinned rows come first, in `pinned` order, above a
   * divider - the ones someone picks most, out of a long list. */
  pins?: {
    pinned: string[];
    canPin: (value: string) => boolean;
    onToggle: (value: string) => void;
  };
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
  const pinned = pins?.pinned;
  const rows = useMemo(() => {
    const all = items ?? options.map((o) => ({ value: o, label: o }));
    if (!pinned?.length) return all;
    const byValue = new Map(all.map((r) => [r.value, r]));
    const top = pinned.flatMap((v) => byValue.get(v) ?? []);
    const onTop = new Set(top.map((r) => r.value));
    return [...top, ...all.filter((r) => !onTop.has(r.value))];
  }, [items, options, pinned]);
  const isPinned = (v: string) => Boolean(pinned?.includes(v));
  const filtered = useMemo(
    () => (q ? rows.filter((r) => r.label.toLowerCase().includes(q)) : rows),
    [rows, q],
  );
  const showCustom = allowCustom && query.trim() && !rows.some((r) => r.label.toLowerCase() === q);
  const selectedLabel = rows.find((r) => r.value === value)?.label ?? value;

  const commit = (v: string) => {
    onChange(v);
    setOpen(false);
  };

  return (
    <div ref={ref} className={cn("relative", className)}>
      <button
        type="button"
        role="combobox"
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-label={ariaLabel}
        className={cn(
          "flex w-full items-center justify-between gap-2 rounded-md border border-border bg-surface px-2 py-1.5 text-left text-sm transition-colors hover:border-border-strong focus:border-accent focus:outline-none",
          // While open, focus lives in the panel's search input - the
          // trigger keeps the accent explicitly so every dropdown shows
          // the same lit border as a focused Input.
          open && "border-accent",
          triggerClassName,
        )}
        onClick={() => setOpen((o) => !o)}
      >
        <span className={cn("truncate", value ? "text-text" : "text-faint")}>
          {selectedLabel || placeholder}
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
        <div className="t-dropdown absolute left-0 top-full z-40 mt-1 w-max min-w-full max-w-[min(42rem,calc(100vw-3rem))] rounded-md border border-border bg-surface shadow-xl">
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
                  if (active < filtered.length) commit(filtered[active].value);
                  else if (showCustom) commit(query.trim());
                } else if (e.key === "Escape") {
                  setOpen(false);
                }
              }}
            />
          </div>
          <ul role="listbox" aria-label={ariaLabel} className="max-h-56 overflow-y-auto p-1">
            {loading && <li className="px-2 py-1.5 text-sm text-muted">Loading…</li>}
            {!loading && filtered.length === 0 && !showCustom && (
              <li className="px-2 py-1.5 text-sm text-muted">No matches</li>
            )}
            {filtered.map((r, i) => {
              const pinnable = Boolean(pins?.canPin(r.value));
              const pinnedRow = isPinned(r.value);
              // The divider under the pinned rows: before the first row
              // that is not pinned, when a pinned one came before it.
              const divide = i > 0 && !pinnedRow && isPinned(filtered[i - 1].value);
              return [
                divide && <li key={`${r.value}-divider`} role="presentation" className="mx-1 my-1 border-t border-border" />,
                <li key={r.value} className="group relative">
                  <button
                    type="button"
                    role="option"
                    aria-selected={r.value === value}
                    className={cn(
                      "flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-sm",
                      pinnable && "pr-8",
                      i === active ? "bg-accent-soft text-accent" : "text-text hover:bg-surface-2",
                    )}
                    onMouseEnter={() => setActive(i)}
                    onClick={() => commit(r.value)}
                  >
                    <span className="truncate">{r.label}</span>
                    <span className="ml-2 flex shrink-0 items-center gap-1.5">
                      {details?.[r.label] && (
                        <span className="whitespace-nowrap text-xs text-faint">{details[r.label]}</span>
                      )}
                      {r.value === value && <Check size={13} className="shrink-0 text-accent" />}
                    </span>
                  </button>
                  {pinnable && pins && (
                    <button
                      type="button"
                      aria-pressed={pinnedRow}
                      aria-label={`${pinnedRow ? "Unpin" : "Pin"} ${r.label}`}
                      title={pinnedRow ? "Unpin" : "Pin to the top"}
                      className={cn(
                        "absolute right-1.5 top-1/2 flex -translate-y-1/2 items-center rounded p-1 transition-opacity hover:text-accent focus-visible:opacity-100",
                        pinnedRow
                          ? "text-accent opacity-100"
                          : cn("text-muted", i === active ? "opacity-100" : "opacity-0 group-hover:opacity-100"),
                      )}
                      // Keep focus in the search box: pinning is a side
                      // trip, and typing should carry on where it was.
                      onMouseDown={(e) => e.preventDefault()}
                      onMouseEnter={() => setActive(i)}
                      onClick={(e) => {
                        e.stopPropagation();
                        pins.onToggle(r.value);
                      }}
                    >
                      <IconPinToTop aria-hidden className={cn("size-3.5", pinnedRow && "fill-current")} />
                    </button>
                  )}
                </li>,
              ];
            })}
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
