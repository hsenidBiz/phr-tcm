// The app's tooltip, applied everywhere by delegation.
//
// `TooltipLayer` is mounted once per window and watches for hover over
// ANY element carrying a `title`. It parks the text on a data attribute
// while the pointer is there - which is what suppresses the OS bubble -
// and draws our own instead. So every `title=` already in the app becomes
// a styled tooltip with no call-site change, and anything written later
// gets one for free.
//
// Two rules drive the drawing:
//
//  1. **Nothing is added to the DOM around the trigger.** The bubble is
//     portalled to <body> and positioned in fixed coordinates from the
//     trigger's rect. An earlier attempt wrapped triggers instead, and a
//     wrapper inside the sidebar's flex rail stopped the collapsed rail
//     resolving its width. Nothing here sits in anyone's layout.
//  2. **Position is measured, not guessed.** The bubble renders hidden,
//     is measured, then placed - so a long label never lands off-screen.
//
// Placement defaults to above the trigger; an element can ask for another
// side with `data-tip-side="right"` (or left/bottom).
//
// Placement is taken from the trigger's rect, which is only a good anchor
// while the trigger is roughly pointer-sized. On something big - a board
// card, a work item's description - the centre of the rect can be a long
// way from the mouse, and the bubble reads as belonging to something else
// entirely. So a large trigger anchors to the pointer instead; see
// `anchorBox`.
//
// Known gap: a `disabled` button fires no pointer events at all, so this
// never sees it and the browser keeps showing its native tooltip. That is
// the right fallback - the text still reaches the user - and it is why
// "why is this disabled" titles are left as plain `title`.

import { cloneElement, isValidElement, useEffect, useRef, useState } from "react";
import type { ReactElement, ReactNode } from "react";
import { createPortal } from "react-dom";

export type TooltipSide = "top" | "bottom" | "left" | "right";

/** Long enough not to fire while the pointer is passing through, short
 * enough to feel like an answer. */
const OPEN_DELAY_MS = 350;
/** Gap between the trigger and the bubble. */
const OFFSET = 8;
/** Keeps the bubble off the very edge of the window. */
const MARGIN = 8;
/** Where the text lives while we have taken it off `title`. */
const PARK = "data-tip-text";
/** Past this, a trigger stops being a useful anchor on that axis: the
 * bubble placed from its centre can land nowhere near the pointer. Sized
 * so that ordinary controls - buttons, rail icons, toolbar rows, table
 * cells - stay rect-anchored and keep the placement they have today,
 * while cards and prose blocks fall to the pointer. */
const BIG_TRIGGER = 200;

type Point = { top: number; left: number };
/** Viewport coordinates of the mouse. */
export type Pointer = { x: number; y: number };
/** The parts of a DOMRect this file uses - so tests can pass literals. */
type Rect = {
  top: number;
  left: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
};

/**
 * The rect the bubble is placed against: normally the trigger itself, but
 * on an axis where the trigger is large it collapses onto the pointer.
 *
 * Only the oversized axis collapses. A wide, short card keeps its top
 * edge, so the bubble still sits clear above the card - just above the
 * part of it being pointed at, rather than above its middle. A block
 * that is big both ways has nothing worth clearing and anchors fully to
 * the mouse.
 */
export function anchorBox(rect: Rect, pointer?: Pointer | null): Rect {
  if (!pointer) return rect;
  const wide = rect.width > BIG_TRIGGER;
  const tall = rect.height > BIG_TRIGGER;
  if (!wide && !tall) return rect;
  const left = wide ? pointer.x : rect.left;
  const top = tall ? pointer.y : rect.top;
  const width = wide ? 0 : rect.width;
  const height = tall ? 0 : rect.height;
  return { top, left, width, height, right: left + width, bottom: top + height };
}

export function place(
  trigger: Rect,
  bubble: Rect,
  side: TooltipSide,
  pointer?: Pointer | null,
): Point {
  const rect = anchorBox(trigger, pointer);
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

function sideOf(el: Element): TooltipSide {
  const raw = el.getAttribute("data-tip-side");
  return raw === "right" || raw === "left" || raw === "bottom" || raw === "top" ? raw : "top";
}

/**
 * Mount once per window. Turns every `title` in the tree into the app's
 * own tooltip.
 */
export function TooltipLayer() {
  const [open, setOpen] = useState<{ text: string; side: TooltipSide } | null>(null);
  const [pos, setPos] = useState<Point | null>(null);
  const anchor = useRef<HTMLElement | null>(null);
  const bubble = useRef<HTMLDivElement | null>(null);
  const timer = useRef<number | undefined>(undefined);
  // Latest mouse position, for the large-trigger case in `anchorBox`. Kept
  // current by pointermove rather than read once on entry: the pointer
  // wanders for the length of the open delay, and where it comes to REST
  // is where the bubble belongs.
  const pointer = useRef<Pointer | null>(null);

  useEffect(() => {
    /** Give the text back, so the element is unchanged once we let go. */
    const restore = () => {
      const el = anchor.current;
      if (el?.isConnected) {
        const parked = el.getAttribute(PARK);
        if (parked !== null) {
          el.setAttribute("title", parked);
          el.removeAttribute(PARK);
        }
      }
      anchor.current = null;
    };
    const hide = () => {
      window.clearTimeout(timer.current);
      timer.current = undefined;
      restore();
      setOpen(null);
      setPos(null);
    };

    const onMove = (e: PointerEvent) => {
      pointer.current = { x: e.clientX, y: e.clientY };
    };

    const onOver = (e: PointerEvent) => {
      onMove(e);
      const target = e.target as Element | null;
      const el = target?.closest?.("[title]") as HTMLElement | null;
      if (!el) {
        // Left the tooltipped element for something that has none.
        if (anchor.current) hide();
        return;
      }
      if (el === anchor.current) return;
      const text = (el.getAttribute("title") ?? "").trim();
      if (!text) return;

      hide();
      anchor.current = el;
      // Taking `title` off is what stops the OS bubble appearing on top
      // of ours; it goes back the moment the pointer leaves.
      el.setAttribute(PARK, text);
      el.removeAttribute("title");
      timer.current = window.setTimeout(
        () => setOpen({ text, side: sideOf(el) }),
        OPEN_DELAY_MS,
      );
    };

    const onOut = (e: PointerEvent) => {
      const to = e.relatedTarget as Node | null;
      // Moving between children of the same trigger is not leaving it.
      if (anchor.current && to && anchor.current.contains(to)) return;
      if (anchor.current) hide();
    };

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") hide();
    };

    document.addEventListener("pointerover", onOver, true);
    document.addEventListener("pointermove", onMove, { capture: true, passive: true });
    document.addEventListener("pointerout", onOut, true);
    // Anything that moves the trigger invalidates the position; closing
    // is cheaper and less startling than chasing it.
    document.addEventListener("pointerdown", hide, true);
    window.addEventListener("scroll", hide, true);
    window.addEventListener("resize", hide);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", hide);
    return () => {
      document.removeEventListener("pointerover", onOver, true);
      document.removeEventListener("pointermove", onMove, true);
      document.removeEventListener("pointerout", onOut, true);
      document.removeEventListener("pointerdown", hide, true);
      window.removeEventListener("scroll", hide, true);
      window.removeEventListener("resize", hide);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", hide);
      hide();
    };
  }, []);

  // Measured AFTER the bubble renders, so its real size is used - a
  // guessed width would misplace every long label.
  useEffect(() => {
    if (!open || !anchor.current || !bubble.current) return;
    setPos(
      place(
        anchor.current.getBoundingClientRect(),
        bubble.current.getBoundingClientRect(),
        open.side,
        pointer.current,
      ),
    );
  }, [open]);

  if (!open) return null;
  return createPortal(
    <div
      role="tooltip"
      ref={bubble}
      style={{
        position: "fixed",
        top: pos?.top ?? -9999,
        left: pos?.left ?? -9999,
        // Hidden until measured, so it never flashes at 0,0.
        visibility: pos ? "visible" : "hidden",
      }}
      className={
        "pointer-events-none z-[200] max-w-64 rounded-md border border-border bg-surface-2 " +
        "px-2 py-1 text-xs text-text shadow-lg motion-safe:animate-[tooltip-in_120ms_ease-out]"
      }
    >
      {open.text}
    </div>,
    document.body,
  );
}

/**
 * Explicit form, for when the label is computed rather than sitting on
 * the element already. It only sets `title` (and the side hint) - the
 * layer above does the drawing, so there is exactly one tooltip
 * implementation in the app.
 */
export function Tooltip({
  label,
  side = "top",
  disabled = false,
  children,
}: {
  label: string;
  side?: TooltipSide;
  /** Turns it off without changing the trigger - used for labels that
   * are already visible, like an expanded sidebar. */
  disabled?: boolean;
  children: ReactElement;
}) {
  if (!isValidElement(children)) return children as ReactNode;
  return cloneElement(children as ReactElement<Record<string, unknown>>, {
    title: disabled ? undefined : label,
    "data-tip-side": side,
  });
}
