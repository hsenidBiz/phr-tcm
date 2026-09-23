import { useEffect, useRef, useState } from "react";
import { IconPickDate } from "../../lib/actionIcons";
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

/**
 * Date input backed by XiodUI's calendar instead of the native browser date
 * control. Value is "" or "YYYY-MM-DD" (what ADO fields use).
 *
 * The panel is inline (absolutely positioned under the field), not portalled:
 * the only DateField lives in the work-item drawer, whose focus trap would
 * pull focus back out of anything rendered at <body>.
 */
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
  const trigger = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  // The calendar takes focus as it opens, so closing it hands focus back to
  // the field - otherwise it would fall to <body> with the day that held it.
  const close = () => {
    setOpen(false);
    trigger.current?.focus();
  };
  const settle = (v: string) => {
    onChange(v);
    close();
  };

  const date = toDate(value);

  return (
    <div ref={ref} className={cn("relative", className)}>
      <button
        ref={trigger}
        type="button"
        aria-label={ariaLabel}
        aria-expanded={open}
        className={cn(
          "flex w-full items-center justify-between gap-2 rounded-md border border-border bg-surface px-2 py-1.5 text-left text-sm transition-colors hover:border-border-strong focus:border-accent focus:outline-none",
          // While open, focus lives in the calendar - the field keeps the
          // accent explicitly so every dropdown shows the same lit border
          // as a focused Input.
          open && "border-accent",
        )}
        onClick={() => setOpen((o) => !o)}
      >
        <span className={date ? "text-text" : "text-faint"}>
          {date
            ? date.toLocaleDateString(undefined, { day: "2-digit", month: "short", year: "numeric" })
            : "Pick a date"}
        </span>
        <IconPickDate aria-hidden className="size-3.5 shrink-0 text-muted" />
      </button>

      {open && (
        <div
          className="absolute left-0 top-full z-40 mt-1 rounded-2xl border border-border bg-surface p-1 shadow-xl"
          onKeyDown={(e) => {
            // Close the calendar only. The drawer around it closes on a
            // window-level Escape, and would take its unsaved draft along.
            if (e.key !== "Escape") return;
            e.stopPropagation();
            close();
          }}
        >
          <Calendar selected={date} onSelect={(d) => settle(d ? toIso(d) : "")} />
          <div className="flex justify-end border-t border-border px-3 py-1.5 text-xs">
            <button type="button" className="text-muted hover:text-danger" onClick={() => settle("")}>
              Clear
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
