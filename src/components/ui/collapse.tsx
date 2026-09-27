import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { cn } from "../../lib/cn";
import { leaveExitGhost, reducedMotion } from "../../lib/exitGhost";

/** --motion-ease-smooth-out in index.css. */
export const EASE = "cubic-bezier(0.22, 1, 0.36, 1)";

/**
 * The grow for a fold taller than TALL_PX. EASE covers ~96% of the distance
 * in half the time, which suits a detail but left a window-tall group
 * crawling through its whole second half. This one keeps moving to the end.
 */
export const EASE_TALL = "cubic-bezier(0.33, 0, 0.2, 1)";
const TALL_PX = 400;

/** The longest a fold may take. Also how long the clip may stay on if the
 * animation never reports its end. */
const MAX_MS = 600;

/**
 * How long a fold of `px` visible pixels takes. A fixed time made a tall
 * group sweep past the eye too fast to see, so the time grows with the
 * distance: a short detail folds in about 250ms, a screenful in about half
 * a second.
 */
export function foldMs(px: number): number {
  return Math.round(Math.min(MAX_MS, Math.max(200, 200 + px * 0.35)));
}

/**
 * How much of `el` is on screen, measured down from its top edge - the
 * only part of a fold anyone can watch. The rest sits below the window,
 * and snapping it costs nothing anyone sees.
 */
export function visibleSpan(el: HTMLElement): number {
  const r = el.getBoundingClientRect();
  return Math.max(0, Math.min(r.height, window.innerHeight - r.top));
}

/**
 * A fold's motion as it starts: how far its visible edge travels (`span`,
 * plus `margin` - the fold's own outer margin, which a closing copy
 * shrinks away with it), for how long, and on which curve. Reported so a
 * screen can plan other motion around the edge (Settings' cards).
 */
export type FoldMotion = { span: number; margin: number; ms: number; easing: string };

const canAnimate = (el: HTMLElement | null): el is HTMLElement =>
  el != null && typeof el.animate === "function";

/**
 * Mark the long-list rows (`.cv-row`) a grow of `span` pixels can bring on
 * screen, so index.css renders them before the grow starts. A growing fold
 * clips its content, and a clipped `content-visibility: auto` row counts as
 * off screen: a large group rendered its rows a band at a time as the clip
 * edge moved, stalling the grow, then the rest at once when the clip came
 * off. Only rows within the span and one more window below it are marked -
 * the margin covers rows whose real height outgrows the estimate they were
 * measured at. The hundreds further down a big group stay skipped: the grow
 * never shows them, and rendering them all would cost the opening frame.
 */
function revealRows(panel: HTMLElement, span: number): HTMLElement[] {
  const top = panel.getBoundingClientRect().top;
  const reach = span + window.innerHeight;
  const marked: HTMLElement[] = [];
  // Document order is top-to-bottom order here, so the first row past the
  // reach ends the walk and a long group costs no more than a short one.
  for (const row of panel.querySelectorAll<HTMLElement>(".cv-row")) {
    if (row.getBoundingClientRect().top - top >= reach) break;
    marked.push(row);
  }
  // Marked only after every read: a mark changes style, and marking inside
  // the loop would force a fresh layout for each row measured after it.
  for (const row of marked) row.dataset.unfolding = "";
  return marked;
}

/**
 * True once `ready` has been true for a commit - for `animateIn`, so a
 * screen's groups do not all unfold as its data lands, only the one the
 * user opens afterwards. Pass the condition under which the collapsible
 * content first appears (the list has rows, the plans have loaded).
 */
export function useSettled(ready = true): boolean {
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    if (ready) setSettled(true);
  }, [ready]);
  return settled;
}

/**
 * Content that grows open and shrinks shut - transitions.dev's accordion,
 * for the groups, steps, details and subtrees this app folds.
 *
 * The content MOUNTS when open and UNMOUNTS when closed, as every fold in
 * the app already works, so a folded group of five hundred cases costs
 * nothing. The shrink plays on a copy the content leaves behind on the way
 * out (lib/exitGhost), in place, so nothing has to stay mounted for it.
 *
 * Only the part on screen moves, and the time follows its height (foldMs):
 * a list five hundred rows tall used to cross the whole window in the same
 * quarter-second as a three-line detail, which read as no animation at all.
 */
/**
 * For a list that switches between grouped and flat (Group by title): makes
 * the switch ONE motion. Without it, the groups the switch mounts each
 * played their own unfold after the list had already regrouped - the
 * regroup, then every open group opening, one after the other.
 *
 * `regrouping` is true on the render where `mode` changed: pass
 * `animateIn={settled && !regrouping}` so the groups mount already open.
 * `ref` goes on the element wrapping the groups, which plays the app's
 * panel entrance (t-panel-in) once instead. Reduced motion plays nothing.
 */
export function useRegroupMotion<T extends HTMLElement = HTMLDivElement>(mode: unknown) {
  const ref = useRef<T>(null);
  const shown = useRef(mode);
  const regrouping = shown.current !== mode;
  useLayoutEffect(() => {
    if (shown.current === mode) return;
    shown.current = mode;
    const el = ref.current;
    if (!el || reducedMotion()) return;
    // Restart the entrance even if the last one is still playing.
    el.classList.remove("t-panel-in");
    void el.offsetWidth;
    el.classList.add("t-panel-in");
  }, [mode]);
  return { ref, regrouping };
}

export function Collapse({
  open,
  children,
  className,
  animateIn = true,
  row,
  onGrow,
  onShrink,
}: {
  open: boolean;
  children: ReactNode;
  className?: string;
  /** Play the open animation on mount. Off while a screen is still
   * settling, so content that appears with the data does not unfold. */
  animateIn?: boolean;
  /** Render as a table row spanning this many columns - for content that
   * unfolds beneath a row of a table, where a div cannot go. The folding
   * box lives inside the row's one cell, and the copy that shrinks on close
   * is the whole row. */
  row?: number;
  /** Called as the opening grow starts, with its measure. It may return a
   * time (ms) to hold the fold shut before it grows. */
  onGrow?: (grow: FoldMotion) => number | void;
  /** Called as the closing copy starts to shrink, with its measure. */
  onShrink?: (shrink: FoldMotion) => void;
}) {
  return open ? (
    <Panel className={className} animateIn={animateIn} row={row} onGrow={onGrow} onShrink={onShrink}>
      {children}
    </Panel>
  ) : null;
}

/** A fold's own outer margins, read while it is still alone in its place. */
type Margins = { top: number; bottom: number };

function marginsOf(el: HTMLElement | null): Margins {
  if (!el) return { top: 0, bottom: 0 };
  const cs = getComputedStyle(el);
  return { top: parseFloat(cs.marginTop) || 0, bottom: parseFloat(cs.marginBottom) || 0 };
}

/** Shrink the copy a fold leaves behind, from its visible height to
 * nothing. Returns how long that takes, for the copy's removal. `margins`
 * are the original's, read BEFORE the copy was placed beside it: a spaced
 * list (space-y) gives every child but the last a margin, so once the copy
 * follows it the original would read as spaced even when it was last. */
function shrinkCopy(
  ghost: HTMLElement,
  original: HTMLElement,
  margins: Margins,
  report?: (shrink: FoldMotion) => void,
): number | undefined {
  const pick = (n: HTMLElement) => (n.classList.contains("t-collapse") ? n : n.querySelector<HTMLElement>(".t-collapse"));
  const from = pick(original);
  const box = pick(ghost);
  if (!from || !canAnimate(box)) return undefined;
  const span = visibleSpan(from);
  // Nothing of it was on screen: nothing to watch, so no copy either.
  if (span < 2) return 0;
  const ms = foldMs(span);
  // The part below the window goes at once; what is on screen shrinks.
  box.style.height = `${span}px`;
  // Its outer margin (a spaced list gives it one) shrinks with it: left in
  // place, it vanished with the copy at the end and everything below
  // jumped up by it.
  const { top: mt, bottom: mb } = margins;
  const spaced = mt || mb;
  box.animate(
    [
      { height: `${span}px`, ...(spaced ? { marginTop: `${mt}px`, marginBottom: `${mb}px` } : {}) },
      { height: "0px", ...(spaced ? { marginTop: "0px", marginBottom: "0px" } : {}) },
    ],
    { duration: ms, easing: EASE, fill: "forwards" },
  );
  report?.({ span, margin: mt + mb, ms, easing: EASE });
  const inner = box.firstElementChild as HTMLElement | null;
  inner?.animate(
    [
      { opacity: 1, filter: "blur(0px)" },
      { opacity: 0, filter: "blur(2px)" },
    ],
    { duration: ms, easing: EASE, fill: "forwards" },
  );
  return ms;
}

function Panel({
  children,
  className,
  animateIn,
  row,
  onGrow,
  onShrink,
}: {
  children: ReactNode;
  className?: string;
  animateIn: boolean;
  row?: number;
  onGrow?: (grow: FoldMotion) => number | void;
  onShrink?: (shrink: FoldMotion) => void;
}) {
  // The box that grows and shrinks, and the node whose copy plays the
  // shrink - the same element, unless this is a table row.
  const el = useRef<HTMLDivElement>(null);
  const outer = useRef<HTMLTableRowElement>(null);
  // Overflow is clipped only WHILE the height moves: a settled panel must
  // let a dropdown inside it paint past its box.
  const [entering, setEntering] = useState(() => animateIn && !reducedMotion());
  // The long-list rows rendered up front for this grow (see revealRows).
  const revealed = useRef<HTMLElement[]>([]);
  // How long the grow is held shut first (onGrow), which the clip's
  // safety timer waits out too. Asked once per mount: StrictMode runs the
  // grow effect twice, and by the second run the caller's plan for this
  // opening is spent - asking again would drop the hold. The shrink reads
  // the latest onShrink.
  const hold = useRef(0);
  const asked = useRef(false);
  const shrinkReport = useRef(onShrink);
  shrinkReport.current = onShrink;
  const unreveal = () => {
    for (const r of revealed.current) delete r.dataset.unfolding;
    revealed.current = [];
  };

  // The grow: from nothing to the part of the content that is on screen.
  // Measured before paint, so the full height never flashes first.
  useLayoutEffect(() => {
    if (!entering) return;
    const node = el.current;
    if (!canAnimate(node)) return;
    const span = visibleSpan(node);
    if (span < 2) {
      setEntering(false);
      return;
    }
    revealed.current = revealRows(node, span);
    const ms = foldMs(span);
    const easing = span > TALL_PX ? EASE_TALL : EASE;
    if (!asked.current) {
      asked.current = true;
      const m = marginsOf(node);
      hold.current = Math.max(0, onGrow?.({ span, margin: m.top + m.bottom, ms, easing }) || 0);
    }
    // Held shut (at the first frame) while it waits, when it waits at all.
    const held = hold.current > 0 ? { delay: hold.current, fill: "backwards" as const } : {};
    const grow = node.animate([{ height: "0px" }, { height: `${span}px` }], { duration: ms, easing, ...held });
    const inner = node.firstElementChild as HTMLElement | null;
    const fade = inner?.animate(
      [
        { opacity: 0, filter: "blur(2px)" },
        { opacity: 1, filter: "blur(0px)" },
      ],
      { duration: ms, easing, ...held },
    );
    grow.onfinish = () => setEntering(false);
    return () => {
      grow.cancel();
      fade?.cancel();
      unreveal();
    };
    // Once, on mount: this is the opening, not a response to later renders.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Grown, by the animation's end or the timer below: the rows go back to
  // being skipped off screen.
  useEffect(() => {
    if (!entering) unreveal();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entering]);

  // The clip also comes off on a timer, because a panel inside a row
  // content-visibility has skipped may never finish its animation. Without
  // this a dropdown in a row that scrolled into view later would be
  // clipped for good.
  useEffect(() => {
    if (!entering) return;
    const t = window.setTimeout(() => setEntering(false), MAX_MS + 100 + hold.current);
    return () => window.clearTimeout(t);
  }, [entering]);

  // Same shape as Modal's: a layout effect, so the cleanup still has the
  // node in the page to measure and copy, and StrictMode's rehearsal
  // unmount cancels its copy before it can paint.
  const ghost = useRef<(() => void) | null>(null);
  useLayoutEffect(() => {
    ghost.current?.();
    ghost.current = null;
    const node = outer.current ?? el.current;
    const box = el.current;
    return () => {
      if (!node) return;
      // Before the copy goes in beside it (see shrinkCopy).
      const margins = marginsOf(box);
      ghost.current = leaveExitGhost(node, foldMs(0), "after", (copy, original) =>
        shrinkCopy(copy, original, margins, shrinkReport.current),
      );
    };
  }, []);

  const box = (
    <div ref={el} className={cn("t-collapse", entering && "is-entering", className)}>
      <div className="t-collapse-inner">{children}</div>
    </div>
  );
  return row ? (
    <tr ref={outer} className="t-collapse-row">
      <td colSpan={row} className="p-0">
        {box}
      </td>
    </tr>
  ) : (
    box
  );
}
