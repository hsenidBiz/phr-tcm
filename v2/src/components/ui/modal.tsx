import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "../../lib/cn";

/**
 * Backdrop + centered panel, PORTALED to document.body. The portal matters:
 * screens render inside AnimatedContent, whose GSAP transform would
 * otherwise trap a `position: fixed` overlay inside that (scrollable)
 * region instead of the viewport - the backdrop covered only the screen
 * area and the panel centered within it, so a tall list pushed the modal
 * off-screen. Rendering at <body> makes `fixed inset-0` the whole viewport.
 * Escape closes; backdrop click closes; clicks inside the panel don't.
 */
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
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        className={cn("rounded-lg border border-border bg-surface shadow-2xl", className)}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}
