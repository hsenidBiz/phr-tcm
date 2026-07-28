import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "../../lib/cn";

/** Everything the browser will stop on, in DOM order. */
const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

/**
 * Backdrop + centered panel, PORTALED to document.body. The portal matters:
 * screens render inside AnimatedContent, whose GSAP transform would
 * otherwise trap a `position: fixed` overlay inside that (scrollable)
 * region instead of the viewport - the backdrop covered only the screen
 * area and the panel centered within it, so a tall list pushed the modal
 * off-screen. Rendering at <body> makes `fixed inset-0` the whole viewport.
 * Escape closes; backdrop click closes; clicks inside the panel don't.
 *
 * Focus is TRAPPED here and restored on close. Without it `aria-modal` is
 * a claim the app does not honour: tabbing off the last control landed on
 * the title bar and then the sidebar behind the dialog, so a keyboard user
 * ended up driving a screen they could not see, with the backdrop still
 * swallowing their clicks.
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
  const panel = useRef<HTMLDivElement>(null);

  // Captured during RENDER, not in the effect below. Children commit - and
  // any `autoFocus` among them fires - before effects run, so by then the
  // active element is already inside the dialog. Restoring to that puts
  // focus on a node that is about to be removed, which is how it ended up
  // nowhere. This runs before the panel exists, so it is the real opener.
  const opener = useRef<HTMLElement | null>(null);
  if (opener.current === null && typeof document !== "undefined") {
    opener.current = document.activeElement as HTMLElement | null;
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    const el = panel.current;
    if (!el) return;
    // Move focus in, unless the content already claimed it (autoFocus).
    if (!el.contains(document.activeElement)) {
      const first = el.querySelector<HTMLElement>(FOCUSABLE);
      (first ?? el).focus();
    }

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const stops = [...el.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
        (n) => n.offsetParent !== null || n === document.activeElement,
      );
      if (stops.length === 0) {
        e.preventDefault();
        return;
      }
      const first = stops[0];
      const last = stops[stops.length - 1];
      // Wrap at both ends. Also catches the case where focus has somehow
      // escaped already (a click on the backdrop, say) and pulls it back.
      if (!el.contains(document.activeElement)) {
        e.preventDefault();
        (e.shiftKey ? last : first).focus();
      } else if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      // Back to whatever opened it, so the next Tab carries on from where
      // the user was rather than restarting at the top of the app. Only if
      // it is still on the page - a dialog that removed its own trigger
      // (a watched file's Stop button) has nothing to go back to.
      const back = opener.current;
      if (back && document.body.contains(back)) back.focus();
    };
  }, []);

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onClick={onClose}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        className={cn(
          "rounded-lg border border-border bg-surface shadow-2xl focus:outline-none",
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
