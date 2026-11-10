/**
 * A `<ul>` that renders only the rows near the visible part of the page
 * once it is long (`useRowWindow`, backlog A1.5). Short lists render every
 * row, exactly as a plain `<ul>` did.
 *
 * `renderRow` gets the row's index and a `measure` ref; the row's own <li>
 * takes `ref={measure}` and `data-index={index}`.
 */
import { Fragment, type ReactNode } from "react";
import { useRowWindow } from "./useRowWindow";

export function WindowedList({
  count,
  estimate,
  gap = 0,
  keyOf,
  pinned,
  className,
  renderRow,
}: {
  count: number;
  /** A collapsed row's height in pixels; real heights are measured. */
  estimate: number;
  /** The list's `space-y-*` gap in pixels, so offsets add up. */
  gap?: number;
  keyOf: (index: number) => number | string;
  /** Rows always rendered, wherever the page is scrolled (open editors). */
  pinned?: readonly number[];
  className?: string;
  renderRow: (index: number, measure: (el: Element | null) => void) => ReactNode;
}) {
  const win = useRowWindow(count, { estimate, keyOf, gap, pinned });
  // A spacer is a list item too, so it takes the list's gap as a margin:
  // its own height is the skipped space less that gap.
  return (
    <ul ref={win.listRef} className={className}>
      {win.items.map(({ index, key, space }) => (
        <Fragment key={key}>
          {space > 0 && <li aria-hidden style={{ height: Math.max(0, space - gap) }} />}
          {renderRow(index, win.measure)}
        </Fragment>
      ))}
      {win.after > 0 && <li aria-hidden style={{ height: Math.max(0, win.after - gap) }} />}
    </ul>
  );
}
