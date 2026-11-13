/**
 * Reveals and collapses its children's WIDTH smoothly, so a flex-grow
 * sibling (a toolbar's search field) glides to its new width instead of
 * jumping when actions appear or disappear.
 *
 * Children stay mounted through the exit animation and unmount once the
 * collapse ends (transitionend, with a timeout fallback), so collapsed
 * buttons are not tabbable and not in the accessibility tree. While
 * collapsing, the LAST children given while `show` was true are kept, so a
 * label that reads a count never flashes "Run 0". Collapsed-in-motion it is
 * `inert` and `aria-hidden`. With reduced motion it is instant.
 *
 * `children` may be a function receiving `settled`: false while the width is
 * animating (in or out), true only once fully open (at once under reduced
 * motion). An IntersectionObserver treats this wrapper's overflow clip as
 * "off screen", so a floating copy of its content must stay down until then.
 *
 * Fully collapsed it renders nothing, so a parent's `gap-*` leaves no empty
 * slot; while animating, a negative left margin cancels the parent's gap and
 * eases back to zero in step with the width (collapsed footprint is exactly 0,
 * open gap is exactly the parent's).
 */
import { useEffect, useRef, useState, type ReactNode } from "react";
import { cn } from "../lib/cn";

const DURATION_MS = 200;

function reducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

export default function WidthReveal({
  show,
  children,
  gap = "0.5rem",
}: {
  show: boolean;
  children: ReactNode | ((settled: boolean) => ReactNode);
  /** The parent's gap, cancelled while the wrapper is narrower than its content. */
  gap?: string;
}) {
  const [mounted, setMounted] = useState(show);
  const [open, setOpen] = useState(show);
  const [settledOpen, setSettledOpen] = useState(show && reducedMotion());
  const last = useRef(children);
  if (show) last.current = children;

  useEffect(() => {
    if (show) {
      setMounted(true);
      if (reducedMotion()) {
        setOpen(true);
        setSettledOpen(true);
        return;
      }
      setSettledOpen(false);
      // Let the collapsed frame paint first so the width transition runs.
      const id = requestAnimationFrame(() => requestAnimationFrame(() => setOpen(true)));
      // transitionend normally settles it; this covers one that never fires.
      const t = window.setTimeout(() => setSettledOpen(true), DURATION_MS + 150);
      return () => {
        cancelAnimationFrame(id);
        window.clearTimeout(t);
      };
    }
    setOpen(false);
    setSettledOpen(false);
    if (reducedMotion()) {
      setMounted(false);
      return;
    }
    // transitionend normally unmounts; this covers a transition that never fires.
    const t = window.setTimeout(() => setMounted(false), DURATION_MS + 100);
    return () => window.clearTimeout(t);
  }, [show]);

  if (!mounted && !show) return null;

  const expanded = show && open;
  return (
    <div
      data-width-reveal
      aria-hidden={show ? undefined : true}
      inert={show ? undefined : true}
      style={{ marginLeft: expanded ? 0 : `calc(-1 * ${gap})` }}
      className={cn(
        "grid shrink-0 transition-[grid-template-columns,margin-left,opacity] duration-200 ease-out motion-reduce:transition-none",
        expanded ? "grid-cols-[1fr] opacity-100" : "grid-cols-[0fr] opacity-0",
      )}
      onTransitionEnd={(e) => {
        if (e.target !== e.currentTarget) return;
        if (show) setSettledOpen(true);
        else setMounted(false);
      }}
    >
      {/* No padding here: a grid item cannot shrink below its padding, so the
          collapsed track would not be 0. The gap is the parent's own, carried
          by the wrapper's transitioned negative margin. */}
      <div className="min-w-0 overflow-hidden">
        {typeof last.current === "function" ? last.current(show && settledOpen) : last.current}
      </div>
    </div>
  );
}
