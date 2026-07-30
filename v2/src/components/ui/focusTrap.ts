import { useEffect, useRef, type RefObject } from "react";

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
 * Keep Tab inside `panel` while it is mounted, and hand focus back to
 * whatever opened it on the way out.
 *
 * `aria-modal="true"` is a promise that the rest of the page is inert. A
 * dialog that sets it without trapping focus is telling assistive tech
 * something untrue: tabbing off the last control lands on the title bar and
 * then the sidebar behind the dialog, so a keyboard user ends up driving a
 * screen they cannot see with the backdrop still swallowing their clicks.
 * On a dialog holding an unsaved draft they can also unmount it from back
 * there and lose the edit.
 *
 * This lives apart from `Modal` because two dialogs need the trap and
 * deliberately do NOT want the rest of `Modal` - the comment dialog has no
 * backdrop-click close on purpose, and the work-item drawer is a side
 * panel, not a centred sheet.
 */
export function useFocusTrap(panel: RefObject<HTMLElement | null>) {
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
    // Once, on mount. The ref is stable and re-running would re-steal
    // focus every render.
  }, [panel]);
}
