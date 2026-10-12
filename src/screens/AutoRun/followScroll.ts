// The Setup panel follows the person down the page, a little behind them,
// as if catching up: it eases toward where it should be each frame rather
// than being pinned. It never leaves its own column (not past its bottom,
// not above its top, so never over the header), and under
// prefers-reduced-motion it does not animate at all: it simply stays in
// place with position: sticky.

import { useEffect, useRef, type RefObject } from "react";

/** How far below the top of the scrolling area the panel rests, in px. */
export const FOLLOW_GAP = 16;

/** The share of the remaining distance covered each frame: small enough to
 * be seen catching up, large enough to arrive within half a second. */
export const FOLLOW_EASE = 0.12;

/** Closer than this, in px, and the panel is where it should be. */
const SETTLED = 0.5;

/** Where things are, in viewport px, for one frame. */
export type FollowGeometry = {
  /** The top of the panel's column, as it is now. */
  columnTop: number;
  /** The column's height: the panel never goes past its bottom. */
  columnHeight: number;
  /** The panel's own height. */
  panelHeight: number;
  /** The top and bottom of the area the page scrolls in. */
  viewTop: number;
  viewBottom: number;
};

/** How far down its column the panel should sit, in px. The panel rests
 * `FOLLOW_GAP` below the top of the scrolling area; one taller than that
 * area keeps its bottom in view instead, so every row can still be read.
 * Always clamped to the column: from 0 (its own place, never above it) to
 * the column's height less the panel's. */
export function followOffset(g: FollowGeometry): number {
  const room = g.viewBottom - g.viewTop - 2 * FOLLOW_GAP;
  const wanted =
    g.panelHeight <= room
      ? g.viewTop + FOLLOW_GAP - g.columnTop
      : g.viewBottom - FOLLOW_GAP - g.panelHeight - g.columnTop;
  return clampOffset(wanted, g.columnHeight, g.panelHeight);
}

/** `offset` kept within the column. */
export function clampOffset(offset: number, columnHeight: number, panelHeight: number): number {
  const most = Math.max(0, columnHeight - panelHeight);
  return Math.min(Math.max(offset, 0), most);
}

/** One frame's step from `current` toward `target`: `FOLLOW_EASE` of the
 * way, or all of it once it is close enough. */
export function stepToward(current: number, target: number): number {
  const left = target - current;
  if (Math.abs(left) < SETTLED) return target;
  return current + left * FOLLOW_EASE;
}

/** Whether the person asked for less motion. */
export function prefersReducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}

/** The nearest ancestor the page scrolls in, or null for the window. */
function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const y = getComputedStyle(p).overflowY;
    if (y === "auto" || y === "scroll" || y === "overlay") return p;
  }
  return null;
}

/** Moves `panel` down `column` as the page scrolls, eased (`stepToward`),
 * by a transform. Does nothing when `reduced`: the caller makes the panel
 * sticky instead. */
export function useFollowScroll(
  column: RefObject<HTMLElement | null>,
  panel: RefObject<HTMLElement | null>,
  reduced: boolean,
): void {
  const at = useRef(0);
  useEffect(() => {
    const col = column.current;
    const box = panel.current;
    if (!col || !box) return;
    if (reduced) {
      at.current = 0;
      box.style.transform = "";
      return;
    }
    const scroller = scrollParent(col);
    let frame = 0;
    const geometry = (): FollowGeometry => {
      const c = col.getBoundingClientRect();
      const view = scroller ? scroller.getBoundingClientRect() : { top: 0, bottom: window.innerHeight };
      return {
        // The column's own top, with the panel's current offset left out.
        columnTop: c.top,
        columnHeight: c.height,
        panelHeight: box.offsetHeight,
        viewTop: view.top,
        viewBottom: view.bottom,
      };
    };
    const tick = () => {
      frame = 0;
      const target = followOffset(geometry());
      const next = stepToward(at.current, target);
      at.current = next;
      box.style.transform = next === 0 ? "" : `translateY(${next}px)`;
      if (next !== target) frame = requestAnimationFrame(tick);
    };
    const wake = () => {
      if (!frame) frame = requestAnimationFrame(tick);
    };
    const target: HTMLElement | Window = scroller ?? window;
    target.addEventListener("scroll", wake, { passive: true });
    window.addEventListener("resize", wake);
    // The panel opening or shutting, or the list growing, moves the limits.
    const sizes = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(wake);
    sizes?.observe(col);
    sizes?.observe(box);
    wake();
    return () => {
      target.removeEventListener("scroll", wake);
      window.removeEventListener("resize", wake);
      sizes?.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }, [column, panel, reduced]);
}
