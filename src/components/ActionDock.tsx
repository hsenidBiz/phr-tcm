/**
 * One place for "actions on the thing" - the owner's rule is bottom-right
 * for actions, bottom-left for view controls (Collapse all). This is the
 * one approved way to build a bottom-right action row: render it in place,
 * and once that row is off screen, show the same actions again as a
 * floating copy pinned to the window - exactly the pattern Import File's
 * queue review button used, now shared by Suite Management and Update
 * Test Cases too. `src/ui-consistency.test.ts` enforces that nothing else
 * builds this pattern by hand.
 *
 * The floating copy is portalled to `document.body`. Every screen that
 * needs one renders inside `AnimatedContent`, whose GSAP transform becomes
 * the containing block for a `fixed` descendant - so `right-6 bottom-*`
 * would pin to the SCROLL REGION instead of the window without the portal.
 *
 * `children` is a function so the floating copy can render a simplified
 * or differently-wired version of the same actions (Import File's floating
 * button arms a confirmation instead of submitting outright) while still
 * sharing one row of markup for the common case.
 */
import { type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "../lib/cn";
import { useOnScreen } from "../hooks/useOnScreen";

export default function ActionDock({
  children,
  label,
  active,
  stack,
  className,
  rowProps,
  rowRef,
}: {
  /** Buttons rendered in place (the keyboard path) and, when that row is off
   * screen, again as a floating copy bottom-right. A function so the copy can
   * be rendered with tabIndex -1. */
  children: (floating: boolean) => ReactNode;
  /** Accessible name of the floating region, e.g. "Order actions for Suite A". */
  label: string;
  /** Show the floating copy only while this is true (e.g. a selection exists). */
  active?: boolean;
  /** Stack several docks on one screen: 0 = lowest. */
  stack?: number;
  /** Extra classes for the in-place row. */
  className?: string;
  /** Forwarded to the in-place row (tour anchors like data-tour). */
  rowProps?: React.HTMLAttributes<HTMLDivElement> & Record<`data-${string}`, string>;
  /** Called with the in-place row element, for callers that scroll to it. */
  rowRef?: (el: HTMLDivElement | null) => void;
}) {
  // Same -24px margin as the row this was extracted from: the real row has
  // to be properly in view, not just peeking over the edge, before the
  // floating copy stands down.
  const [dockRef, onScreen] = useOnScreen("0px 0px -24px 0px");
  // Hidden while the real row is reachable, or while the caller says there
  // is nothing worth floating (no selection, no unsaved order).
  const hidden = onScreen || active === false;

  return (
    <>
      <div
        {...rowProps}
        ref={(el) => {
          dockRef(el);
          rowRef?.(el);
        }}
        className={cn("flex flex-wrap items-center justify-end gap-2", className)}
      >
        {children(false)}
      </div>
      {createPortal(
        <div
          role="region"
          aria-label={label}
          data-sticky-action
          aria-hidden={hidden ? "true" : undefined}
          // Matches aria-hidden: a hidden copy must not be reachable by Tab
          // either, even though its buttons already carry tabIndex={-1}.
          inert={hidden ? true : undefined}
          className={cn(
            "fixed right-6 z-40 flex items-center gap-2 transition-all duration-200",
            hidden
              ? "pointer-events-none translate-y-3 opacity-0"
              : "translate-y-0 opacity-100",
          )}
          style={{ bottom: `${1.5 + (stack ?? 0) * 3.5}rem` }}
        >
          {children(true)}
        </div>,
        document.body,
      )}
    </>
  );
}
