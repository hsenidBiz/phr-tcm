import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { IconResetView, IconZoomIn, IconZoomOut } from "../lib/actionIcons";
import { cn } from "../lib/cn";
import { Button } from "./ui/button";

/**
 * A pan and zoom viewport over content of a known size - the Test map's
 * mechanism (src-tauri/web/test-map.js), for the API Templates flow maps:
 *
 * - drag with the left button to pan (a press on a button, a link or
 *   anything marked `data-no-pan` stays a click);
 * - a plain wheel scrolls the content, Ctrl/Cmd + wheel zooms about the
 *   cursor by 1.1 a notch;
 * - Zoom out / Zoom in step by 1.2 about the centre, Reset view fits the
 *   whole content again, and a live percentage says where it stands;
 * - with the viewport focused, the arrows pan and + / - / 0 zoom.
 *
 * The content is never measured: its size comes in from the caller (the
 * flow map's layout), so the maths below are the same in a test, where
 * nothing has a size, as on screen. Only the viewport is measured.
 *
 * Use it as a hook plus two parts, so the controls can sit in the caller's
 * own header row: `const pz = usePanZoom(size)`, then
 * `<PanZoomControls pz={pz} />` and `<PanZoomViewport pz={pz} ...>`.
 */

/** Content units to screen px: `translate(tx, ty) scale(s)` from the top-left. */
export type View = { s: number; tx: number; ty: number };
export type Size = { w: number; h: number };

export const MIN_SCALE = 0.2;
export const MAX_SCALE = 4;
/** How much of the content (px) a pan always leaves in view. */
export const KEEP = 40;
/** Room left around the content when it is fitted, and below it in the box. */
export const PAD = 16;
const MIN_HEIGHT = 240;
const BUTTON_STEP = 1.2;
const WHEEL_STEP = 1.1;
const KEY_PAN = 40;
/** A press that moves further than this is a pan, not a click. */
const DRAG_SLOP = 3;
/** A press that starts on one of these is its own, never a pan. Only the
 *  interactive roles: a stage box is a role="group", and must still drag. */
const INTERACTIVE = [
  "button",
  "a[href]",
  "input",
  "select",
  "textarea",
  "summary",
  "[contenteditable='true']",
  ...["button", "link", "checkbox", "radio", "switch", "tab", "menuitem", "option", "textbox", "combobox", "slider"].map(
    (r) => `[role='${r}']`,
  ),
  "[data-no-pan]",
].join(",");

const clampScale = (s: number) => Math.min(MAX_SCALE, Math.max(MIN_SCALE, s));
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The whole content in view, centred, never enlarged past 100%. Before the
 *  viewport has a size, 100% at the top-left. */
export function fitView(content: Size, viewport: Size, pad = 0): View {
  const roomW = viewport.w - 2 * pad;
  const roomH = viewport.h - 2 * pad;
  if (roomW <= 0 || roomH <= 0 || content.w <= 0 || content.h <= 0) return { s: 1, tx: 0, ty: 0 };
  const s = clampScale(Math.min(1, roomW / content.w, roomH / content.h));
  return { s, tx: (viewport.w - content.w * s) / 2, ty: (viewport.h - content.h * s) / 2 };
}

/** Zoom to `next` (clamped to 20%-400%) keeping the screen point (px, py)
 *  over the same spot of the content. */
export function zoomAbout(v: View, next: number, px: number, py: number): View {
  const s = clampScale(next);
  return { s, tx: px - (px - v.tx) * (s / v.s), ty: py - (py - v.ty) * (s / v.s) };
}

/** Keep at least `keep` px of the content inside the viewport on each axis. */
export function clampPan(v: View, content: Size, viewport: Size, keep = KEEP): View {
  const axis = (t: number, c: number, vp: number) => {
    if (vp <= 0) return t;
    const w = c * v.s;
    const k = Math.min(keep, w);
    return clamp(t, k - w, vp - k);
  };
  return { s: v.s, tx: axis(v.tx, content.w, viewport.w), ty: axis(v.ty, content.h, viewport.h) };
}

/** A plain wheel scrolls like a scroll box: only along an axis where the
 *  content is larger than the viewport, and only until its edge meets the
 *  viewport's. An axis that already shows everything does not move, so the
 *  wheel goes on to scroll the page. (A content dragged past its edge can
 *  be scrolled back, never further out.) */
export function wheelPan(v: View, dx: number, dy: number, content: Size, viewport: Size): View {
  const axis = (t: number, d: number, c: number, vp: number) => {
    const w = c * v.s;
    if (w <= vp) return t;
    return clamp(t - d, Math.min(vp - w, t), Math.max(0, t));
  };
  return { s: v.s, tx: axis(v.tx, dx, content.w, viewport.w), ty: axis(v.ty, dy, content.h, viewport.h) };
}

export type PanZoomController = {
  view: View;
  content: Size;
  /** A pan is under way: the cursor is grabbing. */
  dragging: boolean;
  /** The last change came from a button or a key: ease into it, where motion is welcome. */
  animate: boolean;
  viewportRef: RefObject<HTMLDivElement | null>;
  zoomIn: () => void;
  zoomOut: () => void;
  reset: () => void;
  onPointerDown: (e: PointerEvent<HTMLDivElement>) => void;
  onPointerMove: (e: PointerEvent<HTMLDivElement>) => void;
  onPointerEnd: (e: PointerEvent<HTMLDivElement>) => void;
  onPointerLeave: () => void;
  onClickCapture: (e: MouseEvent<HTMLDivElement>) => void;
  onKeyDown: (e: KeyboardEvent<HTMLDivElement>) => void;
};

export function usePanZoom(content: Size): PanZoomController {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState<Size>({ w: 0, h: 0 });
  const sizeRef = useRef(size);
  const contentRef = useRef(content);
  const [state, setState] = useState<{ view: View; animate: boolean }>({ view: { s: 1, tx: 0, ty: 0 }, animate: false });
  const viewRef = useRef(state.view);
  /** The person has panned or zoomed: stop refitting on a resize. */
  const touched = useRef(false);
  const drag = useRef<{ id: number; x: number; y: number; tx: number; ty: number; moved: boolean } | null>(null);
  const [dragging, setDragging] = useState(false);
  const swallowClick = useRef(false);

  const commit = useCallback((next: View, animate: boolean) => {
    viewRef.current = next;
    setState({ view: next, animate });
  }, []);
  const move = useCallback(
    (next: View, animate: boolean) => {
      touched.current = true;
      commit(next, animate);
    },
    [commit],
  );
  const fit = useCallback(() => fitView(contentRef.current, sizeRef.current, PAD), []);

  // The viewport's size: now, and whenever it changes.
  useLayoutEffect(() => {
    const el = viewportRef.current;
    if (!el) return;
    const measure = () => {
      const next = { w: el.clientWidth, h: el.clientHeight };
      if (next.w === sizeRef.current.w && next.h === sizeRef.current.h) return;
      sizeRef.current = next;
      setSize(next);
    };
    measure();
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    ro?.observe(el);
    window.addEventListener("resize", measure);
    return () => {
      ro?.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, []);

  // Fitted to start with, and kept fitted until the person moves it.
  useLayoutEffect(() => {
    contentRef.current = { w: content.w, h: content.h };
    if (!touched.current) commit(fit(), false);
  }, [content.w, content.h, size.w, size.h, commit, fit]);

  const zoomTo = useCallback(
    (s: number, px: number | null, py: number | null, animate: boolean) => {
      const vp = sizeRef.current;
      const z = zoomAbout(viewRef.current, s, px ?? vp.w / 2, py ?? vp.h / 2);
      move(clampPan(z, contentRef.current, vp), animate);
    },
    [move],
  );
  const zoomIn = useCallback(() => zoomTo(viewRef.current.s * BUTTON_STEP, null, null, true), [zoomTo]);
  const zoomOut = useCallback(() => zoomTo(viewRef.current.s / BUTTON_STEP, null, null, true), [zoomTo]);
  const reset = useCallback(() => {
    touched.current = false;
    commit(fit(), true);
  }, [commit, fit]);
  const panBy = useCallback(
    (dx: number, dy: number) => {
      const v = viewRef.current;
      move(clampPan({ s: v.s, tx: v.tx + dx, ty: v.ty + dy }, contentRef.current, sizeRef.current), false);
    },
    [move],
  );

  // The wheel, by hand: React's onWheel is passive, so it could never keep
  // the page still. preventDefault only when the map itself moved (or for a
  // zoom), so a map with nothing more to show lets the page scroll on.
  useEffect(() => {
    const el = viewportRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      const v = viewRef.current;
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        if (e.deltaY === 0) return;
        const r = el.getBoundingClientRect();
        zoomTo(v.s * (e.deltaY < 0 ? WHEEL_STEP : 1 / WHEEL_STEP), e.clientX - r.left, e.clientY - r.top, false);
        return;
      }
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? sizeRef.current.h : 1;
      const next = wheelPan(v, e.deltaX * unit, e.deltaY * unit, contentRef.current, sizeRef.current);
      if (next.tx === v.tx && next.ty === v.ty) return;
      e.preventDefault();
      move(next, false);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [zoomTo, move]);

  const onPointerDown = useCallback((e: PointerEvent<HTMLDivElement>) => {
    swallowClick.current = false;
    if (e.button !== 0) return;
    const hit = (e.target as Element).closest?.(INTERACTIVE);
    if (hit && hit !== e.currentTarget && e.currentTarget.contains(hit)) return;
    const v = viewRef.current;
    drag.current = { id: e.pointerId, x: e.clientX, y: e.clientY, tx: v.tx, ty: v.ty, moved: false };
  }, []);

  const onPointerMove = useCallback(
    (e: PointerEvent<HTMLDivElement>) => {
      const d = drag.current;
      if (!d || e.pointerId !== d.id) return;
      const dx = e.clientX - d.x;
      const dy = e.clientY - d.y;
      if (!d.moved) {
        if (Math.hypot(dx, dy) <= DRAG_SLOP) return;
        d.moved = true;
        setDragging(true);
        // Captured only once it is a pan, so a press that stays put clicks
        // exactly what it would have; from here the pan follows the pointer
        // out of the box and still ends where the button comes up.
        e.currentTarget.setPointerCapture?.(e.pointerId);
      }
      move(clampPan({ s: viewRef.current.s, tx: d.tx + dx, ty: d.ty + dy }, contentRef.current, sizeRef.current), false);
    },
    [move],
  );

  const onPointerEnd = useCallback((e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d || e.pointerId !== d.id) return;
    drag.current = null;
    if (!d.moved) return;
    setDragging(false);
    // The click that ends a pan is not a click on what lies under it. It
    // arrives straight after this, in the same task; anything later (Enter
    // on a focused button) is a real click again.
    swallowClick.current = true;
    setTimeout(() => {
      swallowClick.current = false;
    }, 0);
  }, []);

  // A press that left the box before it became a pan never will.
  const onPointerLeave = useCallback(() => {
    if (drag.current && !drag.current.moved) drag.current = null;
  }, []);

  const onClickCapture = useCallback((e: MouseEvent<HTMLDivElement>) => {
    if (!swallowClick.current) return;
    swallowClick.current = false;
    e.stopPropagation();
    e.preventDefault();
  }, []);

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLDivElement>) => {
      // The viewport's own keys; a focused button inside keeps its own.
      if (e.target !== e.currentTarget || e.altKey || e.ctrlKey || e.metaKey) return;
      switch (e.key) {
        case "ArrowLeft":
          panBy(KEY_PAN, 0);
          break;
        case "ArrowRight":
          panBy(-KEY_PAN, 0);
          break;
        case "ArrowUp":
          panBy(0, KEY_PAN);
          break;
        case "ArrowDown":
          panBy(0, -KEY_PAN);
          break;
        case "+":
        case "=":
          zoomIn();
          break;
        case "-":
          zoomOut();
          break;
        case "0":
          reset();
          break;
        default:
          return;
      }
      e.preventDefault();
    },
    [panBy, zoomIn, zoomOut, reset],
  );

  return {
    view: state.view,
    animate: state.animate,
    content,
    dragging,
    viewportRef,
    zoomIn,
    zoomOut,
    reset,
    onPointerDown,
    onPointerMove,
    onPointerEnd,
    onPointerLeave,
    onClickCapture,
    onKeyDown,
  };
}

/** Zoom out, Zoom in, Reset view and the live percentage, for a header row. */
export function PanZoomControls({ pz, className }: { pz: PanZoomController; className?: string }) {
  return (
    <div className={cn("flex items-center gap-1", className)}>
      <Button size="sm" variant="ghost" className="px-2" aria-label="Zoom out" title="Zoom out" onClick={pz.zoomOut}>
        <IconZoomOut aria-hidden />
      </Button>
      <Button size="sm" variant="ghost" className="px-2" aria-label="Zoom in" title="Zoom in" onClick={pz.zoomIn}>
        <IconZoomIn aria-hidden />
      </Button>
      <Button size="sm" variant="ghost" onClick={pz.reset}>
        <IconResetView aria-hidden />
        Reset view
      </Button>
      <span aria-live="polite" className="w-10 text-right text-xs tabular-nums text-muted">
        {Math.round(pz.view.s * 100)}%
      </span>
    </div>
  );
}

/** The box the content pans and zooms in. Its height follows the content,
 *  up to 60% of the window; the content itself is laid out at 100% inside
 *  the transformed layer, from its top-left. */
export function PanZoomViewport({
  pz,
  label,
  testId,
  children,
}: {
  pz: PanZoomController;
  /** The viewport's accessible name, e.g. "<flow> map". */
  label: string;
  testId?: string;
  children: ReactNode;
}) {
  const { view, content } = pz;
  return (
    <div
      ref={pz.viewportRef}
      role="group"
      aria-label={label}
      tabIndex={0}
      data-testid={testId}
      className={cn(
        "relative select-none overflow-hidden rounded-md border border-border focus-visible:outline-2 focus-visible:outline-accent",
        // A button inside keeps its own pointer: it clicks, it does not drag.
        pz.dragging ? "cursor-grabbing" : "cursor-grab [&_button]:cursor-default",
      )}
      // min(content + padding, 60vh), and never under MIN_HEIGHT: CSS lets
      // min-height win over max-height.
      style={{ height: content.h + 2 * PAD, maxHeight: "60vh", minHeight: MIN_HEIGHT }}
      onPointerDown={pz.onPointerDown}
      onPointerMove={pz.onPointerMove}
      onPointerUp={pz.onPointerEnd}
      onPointerCancel={pz.onPointerEnd}
      onLostPointerCapture={pz.onPointerEnd}
      onPointerLeave={pz.onPointerLeave}
      onClickCapture={pz.onClickCapture}
      onKeyDown={pz.onKeyDown}
    >
      <div
        className={cn(
          "absolute left-0 top-0",
          pz.animate && "motion-safe:transition-transform motion-safe:duration-150 motion-safe:ease-out",
        )}
        style={{
          width: content.w,
          height: content.h,
          transform: `translate(${view.tx}px, ${view.ty}px) scale(${view.s})`,
          transformOrigin: "0 0",
        }}
      >
        {children}
      </div>
    </div>
  );
}
