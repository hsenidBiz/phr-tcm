import { cn } from "../lib/cn";

/** The state colours a status pill can take: the theme's tokens, never a hex. */
export type PillTone = "success" | "warning" | "danger";

const TONE: Record<PillTone, string> = {
  success: "text-success",
  warning: "text-warning",
  danger: "text-danger",
};

/** A glowing dot and a short state beside a screen's title: the pill the
 * AI Bridge shows ("Running" / "Not running") and the Playwright header
 * shows for its specs. `title` says why on hover; `ariaLabel` says it to a
 * screen reader. */
export default function StatusPill({
  tone,
  label,
  title,
  ariaLabel,
}: {
  tone: PillTone;
  label: string;
  title: string;
  ariaLabel?: string;
}) {
  return (
    <span
      role="status"
      title={title}
      aria-label={ariaLabel}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border border-border px-2 py-0.5 text-[11px] font-medium",
        TONE[tone],
      )}
    >
      <span aria-hidden className="status-glow size-2 rounded-full bg-current" />
      <span className="label-trim">{label}</span>
    </span>
  );
}
