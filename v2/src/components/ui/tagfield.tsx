import { X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { cn } from "../../lib/cn";

/** Convert the model's semicolon-joined tag string <-> an array. Azure
 * DevOps tags cannot contain commas or semicolons, so ";" is a safe join. */
export function splitTags(value: string): string[] {
  return value
    .split(";")
    .map((t) => t.trim())
    .filter(Boolean);
}
export function joinTags(tags: string[]): string {
  return tags.join("; ");
}

/** Searchable multi-select tag input: current tags show as removable chips,
 * a search box filters project suggestions, and picking one (or typing a
 * new one + Enter) adds a chip. Value in/out is the semicolon-joined
 * string the TestCase model stores. */
export default function TagField({
  value,
  onChange,
  suggestions,
  placeholder = "Add tags…",
  ariaLabel = "Tags",
  className,
}: {
  value: string;
  onChange: (v: string) => void;
  suggestions: string[];
  placeholder?: string;
  ariaLabel?: string;
  className?: string;
}) {
  const tags = splitTags(value);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
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

  const q = query.trim().toLowerCase();
  const has = (t: string) => tags.some((x) => x.toLowerCase() === t.toLowerCase());
  const options = useMemo(
    () =>
      suggestions
        .filter((s) => !has(s))
        .filter((s) => (q ? s.toLowerCase().includes(q) : true)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [suggestions, q, value],
  );
  const showCreate = query.trim() && !suggestions.some((s) => s.toLowerCase() === q) && !has(query.trim());

  const addTag = (t: string) => {
    const clean = t.trim();
    if (!clean || has(clean)) return;
    onChange(joinTags([...tags, clean]));
    setQuery("");
    setActive(0);
  };
  const removeTag = (t: string) => onChange(joinTags(tags.filter((x) => x !== t)));

  return (
    <div ref={ref} className={cn("relative", className)}>
      <div
        className="flex flex-wrap items-center gap-1 rounded-md border border-border bg-surface px-1.5 py-1 text-sm transition-colors hover:border-border-strong focus-within:border-accent"
        onClick={() => {
          setOpen(true);
          inputRef.current?.focus();
        }}
      >
        {tags.map((t) => (
          <span
            key={t}
            className="flex items-center gap-1 rounded bg-accent-soft px-1.5 py-0.5 text-xs text-accent"
          >
            {t}
            <button
              aria-label={`Remove ${t}`}
              className="hover:text-danger"
              onClick={(e) => {
                e.stopPropagation();
                removeTag(t);
              }}
            >
              <X size={11} />
            </button>
          </span>
        ))}
        <input
          ref={inputRef}
          aria-label={ariaLabel}
          className="min-w-24 flex-1 bg-transparent px-1 py-0.5 text-sm text-text outline-none placeholder:text-faint"
          placeholder={tags.length ? "" : placeholder}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setOpen(true);
            setActive(0);
          }}
          // Deliberately NO onFocus opener: tabbing through the form must
          // not detonate a dropdown on the way past (the Module combobox
          // is the reference behavior - its trigger opens on click/keys
          // only). The list opens by clicking, typing, or ArrowDown.
          onKeyDown={(e) => {
            const rows = options.length + (showCreate ? 1 : 0);
            if (e.key === "ArrowDown") {
              e.preventDefault();
              if (!open) setOpen(true);
              else setActive((a) => Math.min(a + 1, rows - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => Math.max(a - 1, 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              if (!open) return;
              if (active < options.length) addTag(options[active]);
              else if (showCreate) addTag(query.trim());
            } else if (e.key === "Backspace" && !query && tags.length) {
              removeTag(tags[tags.length - 1]);
            } else if (e.key === "Escape" && open) {
              // Ours to swallow only while the list is showing: an Escape
              // on a closed field belongs to whatever dialog contains it.
              e.stopPropagation();
              setOpen(false);
            }
          }}
        />
      </div>

      {open && (options.length > 0 || showCreate) && (
        <div className="absolute left-0 top-full z-40 mt-1 w-full min-w-48 rounded-md border border-border bg-surface shadow-xl">
          <ul className="max-h-56 overflow-y-auto p-1">
            {options.map((o, i) => (
              <li key={o}>
                <button
                  className={cn(
                    "w-full rounded px-2 py-1.5 text-left text-sm",
                    i === active ? "bg-accent-soft text-accent" : "text-text hover:bg-surface-2",
                  )}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => addTag(o)}
                >
                  {o}
                </button>
              </li>
            ))}
            {showCreate && (
              <li>
                <button
                  className={cn(
                    "w-full rounded px-2 py-1.5 text-left text-sm",
                    active === options.length
                      ? "bg-accent-soft text-accent"
                      : "text-text hover:bg-surface-2",
                  )}
                  onMouseEnter={() => setActive(options.length)}
                  onClick={() => addTag(query.trim())}
                >
                  Add "{query.trim()}"
                </button>
              </li>
            )}
          </ul>
        </div>
      )}
    </div>
  );
}
