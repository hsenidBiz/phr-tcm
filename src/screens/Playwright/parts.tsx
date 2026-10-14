// Small pieces the Playwright tab's areas share: a status badge in one of
// the theme's tones, a card with a header bar, a number tile, and the
// screen's own tab list (the Auto Run tab pattern: only the chosen tab is in
// the Tab order, and the arrow keys move along).

import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { Badge } from "../../components/ui/badge";
import { cn } from "../../lib/cn";

export type Tone = "success" | "warning" | "danger" | "info" | "muted" | "neutral";

const TONE: Record<Tone, string> = {
  success: "bg-success/15 text-success",
  warning: "bg-warning/15 text-warning",
  danger: "bg-danger/15 text-danger",
  info: "bg-info/15 text-info",
  muted: "bg-surface-2 text-muted",
  neutral: "border border-border bg-transparent text-muted",
};

/** A one-line state, coloured by what it means. */
export function StatusBadge({ tone, title, children }: { tone: Tone; title?: string; children: ReactNode }) {
  return (
    <Badge className={cn("whitespace-nowrap text-[11px] font-medium", TONE[tone])} title={title}>
      {children}
    </Badge>
  );
}

/** A bordered panel with a title bar; `actions` sit at the bar's right. */
export function Card({
  title,
  actions,
  className,
  bodyClassName,
  children,
  ...rest
}: {
  title?: ReactNode;
  actions?: ReactNode;
  className?: string;
  bodyClassName?: string;
  children: ReactNode;
  "aria-label"?: string;
  role?: string;
}) {
  return (
    <section className={cn("min-w-0 overflow-hidden rounded-lg border border-border bg-surface", className)} {...rest}>
      {(title || actions) && (
        <header className="flex min-h-10 flex-wrap items-center gap-2 border-b border-border px-3 py-1.5">
          {title && <h3 className="min-w-0 flex-1 text-sm font-semibold text-text">{title}</h3>}
          {actions}
        </header>
      )}
      <div className={bodyClassName}>{children}</div>
    </section>
  );
}

/** A count with its name under it. */
export function Stat({ value, label }: { value: number; label: string }) {
  return (
    <div className="rounded-lg border border-border bg-surface px-4 py-3">
      <div className="text-2xl font-semibold tabular-nums text-text">{value}</div>
      <div className="text-xs text-muted">{label}</div>
    </div>
  );
}

/** Short helper text under a heading or a toolbar. */
export function Hint({ children, className }: { children: ReactNode; className?: string }) {
  return <p className={cn("text-xs text-muted", className)}>{children}</p>;
}

export type TabSpec<T extends string> = { id: T; label: string; count?: number | null };

/** The tab list and the panels under it. Every panel stays mounted (a run's
 * output and an open form survive a look at another tab); only the chosen
 * one is shown. */
export function SubTabs<T extends string>({
  label,
  tabs,
  shown,
  onShow,
  panels,
}: {
  label: string;
  tabs: TabSpec<T>[];
  shown: T;
  onShow: (id: T) => void;
  panels: Record<T, ReactNode>;
}) {
  const ids = useId();
  const refs = useRef<Partial<Record<T, HTMLButtonElement | null>>>({});
  const onKey = (e: KeyboardEvent<HTMLButtonElement>, i: number) => {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    const last = tabs.length - 1;
    const next =
      e.key === "ArrowRight"
        ? (i === last ? 0 : i + 1)
        : e.key === "ArrowLeft"
          ? (i === 0 ? last : i - 1)
          : e.key === "Home"
            ? 0
            : e.key === "End"
              ? last
              : null;
    if (next === null) return;
    e.preventDefault();
    const id = tabs[next].id;
    onShow(id);
    refs.current[id]?.focus();
  };
  return (
    <div className="space-y-4">
      <div role="tablist" aria-label={label} className="flex gap-1 border-b border-border">
        {tabs.map(({ id, label: name, count }, i) => {
          const on = shown === id;
          return (
            <button
              key={id}
              ref={(el) => {
                refs.current[id] = el;
              }}
              id={`${ids}-${id}-tab`}
              type="button"
              role="tab"
              aria-selected={on}
              aria-controls={`${ids}-${id}-panel`}
              tabIndex={on ? 0 : -1}
              className={cn(
                "-mb-px border-b-2 px-3 py-1.5 text-sm font-medium transition-colors",
                on ? "border-accent text-text" : "border-transparent text-muted hover:text-accent",
              )}
              onClick={() => onShow(id)}
              onKeyDown={(e) => onKey(e, i)}
            >
              <span className="label-trim">
                {name}
                {count != null && (
                  <>
                    {" "}
                    <span className="text-xs text-faint">{count}</span>
                  </>
                )}
              </span>
            </button>
          );
        })}
      </div>
      {tabs.map(({ id }) => (
        <div
          key={id}
          role="tabpanel"
          id={`${ids}-${id}-panel`}
          aria-labelledby={`${ids}-${id}-tab`}
          hidden={shown !== id}
          className="min-w-0"
        >
          {panels[id]}
        </div>
      ))}
    </div>
  );
}
