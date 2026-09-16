import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "../../lib/cn";
import { leaveExitGhost } from "../../lib/exitGhost";
import { useFocusTrap } from "./focusTrap";

/**
 * Backdrop + centered panel, PORTALED to document.body. The portal matters:
 * screens render inside AnimatedContent, whose GSAP transform would
 * otherwise trap a `position: fixed` overlay inside that (scrollable)
 * region instead of the viewport - the backdrop covered only the screen
 * area and the panel centered within it, so a tall list pushed the modal
 * off-screen. Rendering at <body> makes `fixed inset-0` the whole viewport.
 * Escape closes; backdrop click closes; clicks inside the panel don't.
 *
 * Focus is TRAPPED here and restored on close, via `useFocusTrap` - see
 * there for why `aria-modal` without one is a claim the app does not
 * honour.
 *
 * It closes with an animation even though every screen unmounts it the
 * instant it is dismissed: on the way out it leaves a fading copy of
 * itself behind (lib/exitGhost), so no screen has to keep it mounted.
 */

/** The close animation's length - --motion-quick in index.css. */
function exitMs(): number {
  const v = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--motion-quick"));
  return Number.isFinite(v) && v > 0 ? v : 150;
}
export function Modal({
  onClose,
  className,
  children,
}: {
  onClose: () => void;
  /** Panel classes (width, max-height, padding, layout). */
  className?: string;
  children: ReactNode;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const backdrop = useRef<HTMLDivElement>(null);
  useFocusTrap(panel);

  // A layout effect, because its cleanup runs while the backdrop is still
  // in the page - a passive cleanup would find it already removed, with
  // nothing to clone and no scroll positions to read. StrictMode rehearses
  // an unmount straight after the first mount: the copy that leaves is
  // cancelled by the re-run before it can paint.
  const ghost = useRef<(() => void) | null>(null);
  useLayoutEffect(() => {
    ghost.current?.();
    ghost.current = null;
    const node = backdrop.current;
    return () => {
      if (node) ghost.current = leaveExitGhost(node, exitMs());
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return createPortal(
    <div
      ref={backdrop}
      className="t-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onClick={onClose}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        className={cn(
          // Scales up from centre as it opens (see "Motion" in index.css).
          "t-modal rounded-lg border border-border bg-surface shadow-2xl focus:outline-none",
          className,
        )}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}
