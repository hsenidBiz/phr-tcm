import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { cn } from "../../lib/cn";
import { leaveExitGhost, reducedMotion } from "../../lib/exitGhost";

/** --motion-ease-smooth-out in index.css. */
const EASE = "cubic-bezier(0.22, 1, 0.36, 1)";

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

const canAnimate = (el: HTMLElement | null): el is HTMLElement =>
  el != null && typeof el.animate === "function";

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
export function Collapse({
  open,
  children,
  className,
  animateIn = true,
  row,
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
}) {
  return open ? (
    <Panel className={className} animateIn={animateIn} row={row}>
      {children}
    </Panel>
  ) : null;
}

/** Shrink the copy a fold leaves behind, from its visible height to
 * nothing. Returns how long that takes, for the copy's removal. */
function shrinkCopy(ghost: HTMLElement, original: HTMLElement): number | undefined {
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
  box.animate([{ height: `${span}px` }, { height: "0px" }], { duration: ms, easing: EASE, fill: "forwards" });
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
}: {
  children: ReactNode;
  className?: string;
  animateIn: boolean;
  row?: number;
}) {
  // The box that grows and shrinks, and the node whose copy plays the
  // shrink - the same element, unless this is a table row.
  const el = useRef<HTMLDivElement>(null);
  const outer = useRef<HTMLTableRowElement>(null);
  // Overflow is clipped only WHILE the height moves: a settled panel must
  // let a dropdown inside it paint past its box.
  const [entering, setEntering] = useState(() => animateIn && !reducedMotion());

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
    const ms = foldMs(span);
    const grow = node.animate([{ height: "0px" }, { height: `${span}px` }], { duration: ms, easing: EASE });
    const inner = node.firstElementChild as HTMLElement | null;
    const fade = inner?.animate(
      [
        { opacity: 0, filter: "blur(2px)" },
        { opacity: 1, filter: "blur(0px)" },
      ],
      { duration: ms, easing: EASE },
    );
    grow.onfinish = () => setEntering(false);
    return () => {
      grow.cancel();
      fade?.cancel();
    };
    // Once, on mount: this is the opening, not a response to later renders.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The clip also comes off on a timer, because a panel inside a row
  // content-visibility has skipped may never finish its animation. Without
  // this a dropdown in a row that scrolled into view later would be
  // clipped for good.
  useEffect(() => {
    if (!entering) return;
    const t = window.setTimeout(() => setEntering(false), MAX_MS + 100);
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
    return () => {
      if (node) ghost.current = leaveExitGhost(node, foldMs(0), "after", shrinkCopy);
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
