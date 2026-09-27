import type { HTMLAttributes, ReactNode } from "react";
import { cn } from "../../lib/cn";

/**
 * One group of settings: a bordered surface with a small heading, its rows
 * separated by a subtle divider. Every card on the Settings screen is built
 * from this and `SettingRow`, so each group reads the same way.
 *
 * A `<section>` on purpose: tests and the tour find a setting's group with
 * `closest("section")`, and extra attributes (`data-tour`) pass straight
 * through to it.
 */
export function SettingsCard({
  title,
  children,
  className,
  ...rest
}: { title: string; children: ReactNode } & Omit<HTMLAttributes<HTMLElement>, "title">) {
  return (
    <section {...rest} className={cn("rounded-md border border-border bg-surface", className)}>
      <h2 className="px-4 pt-3 text-sm font-semibold text-text">{title}</h2>
      <div className="divide-y divide-border px-4">{children}</div>
    </section>
  );
}

/**
 * One setting: its name and a one-line explanation on the left, its control
 * on the right. When the row is too narrow for both, the control wraps under
 * the name rather than squeezing it. `children` sit under the whole row, for
 * a control too wide to share it (the swatch grids) or a note about the
 * current choice.
 *
 * `asLabel` makes the row a `<label>`, so clicking the name flips the switch
 * in it, the way the old one-line switch labels did. The switch keeps its own
 * `aria-label`, which is what names it.
 */
export function SettingRow({
  name,
  description,
  control,
  children,
  asLabel = false,
}: {
  name?: ReactNode;
  description?: ReactNode;
  control?: ReactNode;
  children?: ReactNode;
  asLabel?: boolean;
}) {
  const Line = asLabel ? "label" : "div";
  const hasText = name !== undefined || description !== undefined;
  return (
    <div className="py-3">
      <Line className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        {hasText && (
          <span className="block min-w-[12rem] flex-1">
            {name !== undefined && <span className="block text-sm text-text">{name}</span>}
            {description !== undefined && <span className="block text-xs text-muted">{description}</span>}
          </span>
        )}
        {control && <span className="flex flex-wrap items-center gap-2">{control}</span>}
      </Line>
      {children && <div className="mt-2">{children}</div>}
    </div>
  );
}
