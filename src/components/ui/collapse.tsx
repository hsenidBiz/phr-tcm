import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { cn } from "../../lib/cn";
import { leaveExitGhost, reducedMotion } from "../../lib/exitGhost";

/** A --motion-* duration from index.css, with a fallback for jsdom. */
function motionMs(name: string, fallback: number): number {
  const v = parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name));
  return Number.isFinite(v) && v > 0 ? v : fallback;
}

/**
 * True once `ready` has been true for a commit - for `animateIn`, so a
 * screen's groups do not all unfold as its data lands, only the one the
 * user opens afterwards. Pass the condition under which the collapsible
 * content first appears (the list has rows, the plans have loaded).
 */
export function useSettled(ready = true): boolean {
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    if (ready) setSettled(true);
  }, [ready]);
  return settled;
}

/**
 * Content that grows open and shrinks shut - transitions.dev's accordion,
 * for the groups, steps, details and subtrees this app folds.
 *
 * The content MOUNTS when open and UNMOUNTS when closed, as every fold in
 * the app already works, so a folded group of five hundred cases costs
 * nothing. The shrink plays on a copy the content leaves behind on the way
 * out (lib/exitGhost), in place, so nothing has to stay mounted for it.
 * The grid-rows trick does the height: 0fr to 1fr, no measuring.
 */
export function Collapse({
  open,
  children,
  className,
  animateIn = true,
}: {
  open: boolean;
  children: ReactNode;
  className?: string;
  /** Play the open animation on mount. Off while a screen is still
   * settling, so content that appears with the data does not unfold. */
  animateIn?: boolean;
}) {
  return open ? (
    <Panel className={className} animateIn={animateIn}>
      {children}
    </Panel>
  ) : null;
}

function Panel({ children, className, animateIn }: { children: ReactNode; className?: string; animateIn: boolean }) {
  const el = useRef<HTMLDivElement>(null);
  // Overflow is clipped only WHILE the height moves: a settled panel must
  // let a dropdown inside it paint past its box.
  const [entering, setEntering] = useState(() => animateIn && !reducedMotion());
  // The animation's end clears the clip - and so does a timer, because a
  // panel inside a row content-visibility has skipped never plays its
  // animation, and its end never comes. Without this a dropdown in a row
  // that scrolled into view later would be clipped for good.
  useEffect(() => {
    if (!entering) return;
    const t = window.setTimeout(() => setEntering(false), motionMs("--motion-fast", 250) + 50);
    return () => window.clearTimeout(t);
  }, [entering]);

  // Same shape as Modal's: a layout effect, so the cleanup still has the
  // node in the page to copy, and StrictMode's rehearsal unmount cancels
  // its copy before it can paint.
  const ghost = useRef<(() => void) | null>(null);
  useLayoutEffect(() => {
    ghost.current?.();
    ghost.current = null;
    const node = el.current;
    return () => {
      if (node) ghost.current = leaveExitGhost(node, motionMs("--motion-fast", 250), "after");
    };
  }, []);

  return (
    <div
      ref={el}
      className={cn("t-collapse", entering && "is-entering", className)}
      onAnimationEnd={(e) => {
        if (e.target === el.current) setEntering(false);
      }}
    >
      <div className="t-collapse-inner">{children}</div>
    </div>
  );
}
