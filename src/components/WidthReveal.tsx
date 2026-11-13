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
 * Fully collapsed it renders nothing, so a parent's `gap-*` leaves no empty
 * slot; while animating, a negative left margin cancels the gap and eases
 * back to zero in step with the width.
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
  children: ReactNode;
  /** The parent's gap, cancelled while the wrapper is narrower than its content. */
  gap?: string;
}) {
  const [mounted, setMounted] = useState(show);
  const [open, setOpen] = useState(show);
  const last = useRef<ReactNode>(children);
  if (show) last.current = children;

  useEffect(() => {
    if (show) {
      setMounted(true);
      if (reducedMotion()) {
        setOpen(true);
        return;
      }
      // Let the collapsed frame paint first so the width transition runs.
      const id = requestAnimationFrame(() => requestAnimationFrame(() => setOpen(true)));
      return () => cancelAnimationFrame(id);
    }
    setOpen(false);
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
        if (e.target === e.currentTarget && !show) setMounted(false);
      }}
    >
      <div className="min-w-0 overflow-hidden" style={{ paddingLeft: gap }}>
        {last.current}
      </div>
    </div>
  );
}
