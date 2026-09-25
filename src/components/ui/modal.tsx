import { createContext, useContext, useEffect, useLayoutEffect, useRef, type ReactNode } from "react";
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

/**
 * Every currently-open `Modal`, with the nesting depth it was opened at. A
 * `Modal` has no idea another one is nested inside it - the screenshot
 * preview inside the run pane's own dialog is a plain `Modal` too - so
 * without this, Escape closes both at once: the preview AND the whole run
 * underneath it. Module-level rather than context alone, because the thing
 * that needs cross-instance visibility (which token answers Escape) has no
 * provider of its own; `ModalDepthContext` below only carries how deep a
 * given `Modal` sits, which is determined by React's own tree structure and
 * so is safe to read during render, unlike mount order (effects for a
 * nested pair fire child-before-parent, which would get this backwards).
 */
let openModals: { token: object; depth: number }[] = [];

const ModalDepthContext = createContext(0);

export function Modal({
  onClose,
  className,
  children,
  labelledBy,
  label,
}: {
  onClose: () => void;
  /** Panel classes (width, max-height, padding, layout). */
  className?: string;
  children: ReactNode;
  /** The id of the element that names this dialog - usually its heading.
   * A dialog with no name is announced as just "dialog". */
  labelledBy?: string;
  /** The dialog's name, when nothing on it says it. */
  label?: string;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const backdrop = useRef<HTMLDivElement>(null);
  useFocusTrap(panel);
  // A stable, unique identity for this instance - not compared by value,
  // just by reference, so `{}` needs no further setup.
  const token = useRef({}).current;
  const depth = useContext(ModalDepthContext) + 1;

  useEffect(() => {
    openModals.push({ token, depth });
    return () => {
      openModals = openModals.filter((m) => m.token !== token);
    };
  }, [token, depth]);

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
      // Only the topmost modal - the one nested deepest - answers Escape.
      // Every instance's listener fires on every press; ties (two modals
      // opened side by side, not nested) go to whichever was mounted last.
      if (e.key !== "Escape" || openModals.length === 0) return;
      const maxDepth = Math.max(...openModals.map((m) => m.depth));
      const atMaxDepth = openModals.filter((m) => m.depth === maxDepth);
      const top = atMaxDepth[atMaxDepth.length - 1];
      if (top?.token === token) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, token]);

  return createPortal(
    <ModalDepthContext.Provider value={depth}>
      <div
        ref={backdrop}
        className="t-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
        onClick={onClose}
      >
        <div
          ref={panel}
          role="dialog"
          aria-modal="true"
          aria-labelledby={labelledBy}
          aria-label={labelledBy ? undefined : label}
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
      </div>
    </ModalDepthContext.Provider>,
    document.body,
  );
}
