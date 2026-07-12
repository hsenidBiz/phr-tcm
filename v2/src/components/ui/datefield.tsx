import { CalendarDays } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "../../lib/cn";
import Calendar from "./calendar";

function toDate(v: string): Date | undefined {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(v)) return undefined;
  const [y, m, d] = v.split("-").map(Number);
  return new Date(y, m - 1, d);
}

function toIso(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** Date input backed by the themed Calendar popover instead of the native
 * browser date control. Value is "" or "YYYY-MM-DD" (what ADO fields use). */
export default function DateField({
  value,
  onChange,
  ariaLabel,
  className,
}: {
  value: string;
  onChange: (v: string) => void;
  ariaLabel: string;
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

  const date = toDate(value);

  return (
    <div ref={ref} className={cn("relative", className)}>
      <button
        type="button"
        aria-label={ariaLabel}
        className="flex w-full items-center justify-between gap-2 rounded-md border border-border bg-surface px-2 py-1.5 text-left text-sm transition-colors hover:border-border-strong"
        onClick={() => setOpen((o) => !o)}
      >
        <span className={date ? "text-text" : "text-faint"}>
          {date
            ? date.toLocaleDateString(undefined, { day: "2-digit", month: "short", year: "numeric" })
            : "Pick a date"}
        </span>
        <CalendarDays size={14} className="shrink-0 text-muted" />
      </button>

      {open && (
        <div className="absolute left-0 top-full z-40 mt-1 rounded-md border border-border bg-surface shadow-xl">
          <Calendar
            selected={date}
            onSelect={(d) => {
              onChange(d ? toIso(d) : "");
              setOpen(false);
            }}
          />
          <div className="flex justify-between border-t border-border px-3 py-1.5 text-xs">
            <button
              className="text-muted hover:text-danger"
              onClick={() => {
                onChange("");
                setOpen(false);
              }}
            >
              Clear
            </button>
            <button
              className="text-accent hover:underline"
              onClick={() => {
                onChange(toIso(new Date()));
                setOpen(false);
              }}
            >
              Today
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
