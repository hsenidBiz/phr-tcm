import type { ReactNode } from "react";
import { IconAbout, IconRefresh } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";

/**
 * The one header line of a list screen: what you are looking at (a count or
 * a title) with its refresh button right after it, an optional info icon,
 * small notes, and - pushed to the end - every action that acts on the whole
 * page. `children` is the optional sub line below (context that is not an
 * action). The tooltip comes from the app's `title` delegation, which also
 * shows it when the info button takes keyboard focus.
 *
 * A screen whose refresh needs Shift-click or its own handler passes its own
 * button in `aside` instead of `onRefresh`.
 */
export function PageHeader({
  title,
  titleAs: Heading = "h2",
  onRefresh,
  refreshing = false,
  refreshLabel,
  info,
  aside,
  actions,
  children,
}: {
  title: ReactNode;
  titleAs?: "h2" | "h3";
  onRefresh?: () => void;
  refreshing?: boolean;
  refreshLabel?: string;
  info?: string;
  aside?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}) {
  const refreshName = refreshLabel ?? "Refresh";
  return (
    <div className="space-y-1">
      <div className="flex flex-wrap items-center gap-2">
        <Heading className="text-sm font-semibold text-muted">{title}</Heading>
        {onRefresh && (
          <button
            type="button"
            aria-label={refreshName}
            title={refreshName}
            className="rounded p-1 text-muted hover:text-accent"
            onClick={onRefresh}
          >
            {/* A refetch keeps the old rows on screen while it runs, so the
                spin is the only sign the click did anything. */}
            <IconRefresh aria-hidden className={cn("size-3.5", refreshing && "animate-spin")} />
          </button>
        )}
        {info && (
          <button
            type="button"
            aria-label={info}
            title={info}
            className="rounded p-1 text-muted hover:text-accent"
          >
            <IconAbout aria-hidden className="size-3.5" />
          </button>
        )}
        {aside}
        {actions && <div className="ml-auto flex gap-2">{actions}</div>}
      </div>
      {children}
    </div>
  );
}
