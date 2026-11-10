/**
 * A list's loading state (backlog A1.8): pulsing rows in the list's shape,
 * so the screen keeps its layout while it loads. `label` is said to screen
 * readers; the rows are decoration. Without a label it is decoration only
 * (under a progress bar that already says what is loading).
 */
import type { CSSProperties } from "react";
import { Skeleton } from "./skeleton";

export function ListSkeleton({
  label,
  rows = 5,
  rowClassName = "h-8",
  className = "space-y-1",
  style,
}: {
  label?: string;
  rows?: number;
  rowClassName?: string;
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <div role={label ? "status" : undefined} className={className} style={style}>
      {label && <span className="sr-only">{label}</span>}
      {Array.from({ length: rows }, (_, i) => (
        <Skeleton key={i} aria-hidden className={rowClassName} />
      ))}
    </div>
  );
}
