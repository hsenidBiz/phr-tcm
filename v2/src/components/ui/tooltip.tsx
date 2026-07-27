// The app's tooltip.
//
// Two rules drive the whole design:
//
// 1. **It adds nothing to the DOM around its trigger.** The trigger is
//    cloned, not wrapped, and the bubble is portalled to <body>. A wrapper
//    element - even `display: contents` - becomes a child of whatever
//    container the trigger lives in, and that broke the sidebar: the
//    collapsed icon rail stopped resolving its width and sat at full size.
//    Nothing here can do that, because nothing here is inside the layout.
// 2. **It is positioned from the trigger's rect at open time**, in fixed
//    coordinates, and clamped to the viewport. No layout dependency, no
//    reflow of anything else.
//
// Native `title=` was the alternative; it looks like the OS, not like the
// app, and its timing can't be tuned.

import { cloneElement, isValidElement, useEffect, useId, useRef, useState } from "react";
import type { ReactElement, ReactNode, Ref } from "react";
import { createPortal } from "react-dom";
import { cn } from "../../lib/cn";

export type TooltipSide = "top" | "bottom" | "left" | "right";

/** Long enough not to fire while the pointer is just passing through,
 * short enough to feel like an answer. */
const OPEN_DELAY_MS = 350;
/** Gap between the trigger and the bubble. */
const OFFSET = 8;
/** Keeps the bubble off the very edge of the window. */
const MARGIN = 8;

type Point = { top: number; left: number };

function place(rect: DOMRect, bubble: DOMRect, side: TooltipSide): Point {
  const centerY = rect.top + rect.height / 2 - bubble.height / 2;
  const centerX = rect.left + rect.width / 2 - bubble.width / 2;
  const raw: Record<TooltipSide, Point> = {
    top: { top: rect.top - bubble.height - OFFSET, left: centerX },
    bottom: { top: rect.bottom + OFFSET, left: centerX },
    left: { top: centerY, left: rect.left - bubble.width - OFFSET },
    right: { top: centerY, left: rect.right + OFFSET },
  };
  const p = raw[side];
  // Clamp rather than flip: a tooltip that jumps to the other side of its
  // trigger is more disorienting than one that slides a few pixels.
  return {
    top: Math.min(Math.max(p.top, MARGIN), window.innerHeight - bubble.height - MARGIN),
    left: Math.min(Math.max(p.left, MARGIN), window.innerWidth - bubble.width - MARGIN),
  };
}

export function Tooltip({
  label,
  side = "top",
  disabled = false,
  children,
}: {
  label: ReactNode;
  side?: TooltipSide;
  /** Turns the tooltip off without changing the trigger at all - used for
   * labels that are already visible (an expanded sidebar). */
  disabled?: boolean;
  /** A single element that can take a ref: the trigger. */
  children: ReactElement;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<Point | null>(null);
  const anchor = useRef<HTMLElement | null>(null);
  const bubble = useRef<HTMLDivElement | null>(null);
  const timer = useRef<number | undefined>(undefined);

  const cancel = () => {
    window.clearTimeout(timer.current);
    timer.current = undefined;
  };
  const hide = () => {
    cancel();
    setOpen(false);
    setPos(null);
  };
  const show = () => {
    if (disabled) return;
    cancel();
    timer.current = window.setTimeout(() => setOpen(true), OPEN_DELAY_MS);
  };

  // Measured AFTER the bubble renders, so its real size is used - a
  // guessed width would misplace every tooltip whose text is long.
  useEffect(() => {
    if (!open || !anchor.current || !bubble.current) return;
    setPos(place(anchor.current.getBoundingClientRect(), bubble.current.getBoundingClientRect(), side));
  }, [open, side, label]);

  // Anything that moves the trigger invalidates the position; closing is
  // both cheaper and less startling than chasing it.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") hide();
    };
    window.addEventListener("scroll", hide, true);
    window.addEventListener("resize", hide);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("scroll", hide, true);
      window.removeEventListener("resize", hide);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => cancel, []);
  useEffect(() => {
    if (disabled) hide();
  }, [disabled]);

  if (!isValidElement(children)) return children;

  const props = children.props as Record<string, unknown>;
  const chain =
    (name: string, ours: () => void) =>
    (...args: unknown[]) => {
      (props[name] as ((...a: unknown[]) => void) | undefined)?.(...args);
      ours();
    };

  const trigger = cloneElement(children as ReactElement<Record<string, unknown>>, {
    ref: ((el: HTMLElement | null) => {
      anchor.current = el;
      // Preserve whatever ref the caller already put on the trigger.
      const own = (children as unknown as { ref?: Ref<HTMLElement> }).ref;
      if (typeof own === "function") own(el);
      else if (own && typeof own === "object") (own as { current: HTMLElement | null }).current = el;
    }) as Ref<HTMLElement>,
    onPointerEnter: chain("onPointerEnter", show),
    onPointerLeave: chain("onPointerLeave", hide),
    // Keyboard users get it too; a pointer-only tooltip is a tooltip half
    // the people who need it never see.
    onFocus: chain("onFocus", show),
    onBlur: chain("onBlur", hide),
    // Clicking a tooltipped control means the user is done reading.
    onClick: chain("onClick", hide),
    "aria-describedby": open ? id : props["aria-describedby"],
  });

  return (
    <>
      {trigger}
      {open &&
        createPortal(
          <div
            id={id}
            role="tooltip"
            ref={bubble}
            style={{
              position: "fixed",
              top: pos?.top ?? -9999,
              left: pos?.left ?? -9999,
              // Hidden until measured, so it never flashes at 0,0.
              visibility: pos ? "visible" : "hidden",
            }}
            className={cn(
              "pointer-events-none z-[200] max-w-64 rounded-md border border-border bg-surface-2",
              "px-2 py-1 text-xs text-text shadow-lg",
              "motion-safe:animate-[tooltip-in_120ms_ease-out]",
            )}
          >
            {label}
          </div>,
          document.body,
        )}
    </>
  );
}
