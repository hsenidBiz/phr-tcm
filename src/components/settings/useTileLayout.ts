import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { EASE, type FoldMotion } from "../ui/collapse";
import { reducedMotion } from "../../lib/exitGhost";
import { parseEasing } from "../../lib/cubicBezier";
import { useMediaQuery } from "../../lib/useMediaQuery";
import { driftFrames, planClose, planOpen, type TileMove, type TileTiming } from "./tileSchedule";

/**
 * Where the Settings cards sit, and the motion that moves them.
 *
 * On a wide window the screen is two columns. While the changelog shows
 * only its latest version, the cards that are not about the look of the
 * app (the `moving` ones) sit under it in the right column, so the space
 * under a short changelog is not left empty. Opening the full history
 * needs that column, so the cards slide over to the foot of the left
 * column - like tiles in a sliding puzzle - while the history grows into
 * the space they leave. Closing runs the other way: the history folds, and
 * each card slides back in as its spot comes free.
 *
 * It is one motion, planned from measurements (tileSchedule.ts): the
 * fold's edge follows a known curve (Collapse reports it), so each card
 * sets off just early enough that the edge never crosses it - top-down on
 * the way out, with the grow held only as long as Updates needs to clear
 * the column; bottom-up on the way back, as the shrinking fold frees each
 * spot.
 *
 * The slide itself is a FLIP: every card carries `data-settings-card`, its
 * position is read before the change and again after it, and it is played
 * back from the old place to the new one. A card may be a new element
 * after it changes column - the measurements are keyed by the id, not the
 * node. Below the breakpoint there is one column and nothing moves; under
 * reduced motion the layout and the expansion change at once.
 *
 * When the right column has room for two cards across (roomy), the `spare`
 * cards leave the left column for a second stack beside the moving ones,
 * under the panel, so they are not left below the bottom of the window.
 * They then change column with the others when the history opens, and are
 * timed like them. The room is read from the grid's own width, which no
 * card placement changes; crossing it settles at once, like the breakpoint.
 */

/** Tailwind's `lg` - the width the Settings grid turns two columns at. */
export const WIDE_QUERY = "(min-width: 64rem)";

/** The attribute every card carries, naming it for the measurements. */
export const CARD_ATTR = "data-settings-card";

/** How long one card takes to glide to its new place. */
export const TILE_MS = 420;
/** The gap between cards that nothing else times (the edge never reaches
 * them, or their spot is free from the start). */
export const TILE_STAGGER_MS = 70;
/** Slack between a card clearing a spot and the fold's edge arriving. */
export const TILE_SAFETY_MS = 20;
/** Slack on top of a timed wait, for an animation that never reports its
 * end - the sequence must not stall on one. */
const GRACE_MS = 60;

const TIMING: TileTiming = {
  ease: parseEasing(EASE),
  tileMs: TILE_MS,
  staggerMs: TILE_STAGGER_MS,
  safetyMs: TILE_SAFETY_MS,
};

/** Where the moving cards render: under everything in one column, or at
 * the foot of the left or the right column of the wide layout. */
export type TilePlacement = "single" | "left" | "right";

/** The widest a card gets, the gap between the two columns, the gap between
 * two cards side by side and the right track's floor, in rem (Settings.tsx:
 * the grid's tracks, gap-8 and LEFT_TRACK_WIDTH). */
const CARD_REM = 32;
const COLUMN_GAP_REM = 2;
const STACK_GAP_REM = 1;
const RIGHT_FLOOR_REM = 24;

/**
 * Whether the right column of a grid `gridPx` wide holds two cards side by
 * side. Each card is as wide as the left track - min(32rem, the grid less
 * the gap and the right track's floor) - and the two sit 1rem apart.
 */
export function roomyFor(gridPx: number, remPx: number): boolean {
  const card = Math.min(CARD_REM * remPx, gridPx - (COLUMN_GAP_REM + RIGHT_FLOOR_REM) * remPx);
  if (card <= 0) return false;
  const right = gridPx - card - COLUMN_GAP_REM * remPx;
  return right >= 2 * card + STACK_GAP_REM * remPx;
}

/** The root font size, which the rem widths above are in. */
function remPx(): number {
  const size = parseFloat(getComputedStyle(document.documentElement).fontSize);
  return size > 0 ? size : 16;
}

const wait = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));

const cardSelector = (id: string) => `[${CARD_ATTR}="${id}"]`;

/** `ids` top to bottom by where `rects` has them, left to right within a
 * row: the order the schedule plans in. Two stacks side by side interleave. */
function topDown(ids: readonly string[], rects: Map<string, DOMRect>): string[] {
  return [...ids].sort((a, b) => {
    const ra = rects.get(a);
    const rb = rects.get(b);
    if (!ra || !rb) return 0;
    return ra.top - rb.top || ra.left - rb.left;
  });
}

/** Every card's place on screen, by id. */
function measure(root: HTMLElement): Map<string, DOMRect> {
  const rects = new Map<string, DOMRect>();
  for (const el of root.querySelectorAll<HTMLElement>(`[${CARD_ATTR}]`)) {
    rects.set(el.getAttribute(CARD_ATTR)!, el.getBoundingClientRect());
  }
  return rects;
}

/** The moving cards' journeys, `from` the snapshot `to` where they are laid
 * out now (less `drop`: how far a shrinking fold above still pushes them
 * down). Null if any card is missing. */
function journeys(
  root: HTMLElement,
  before: Map<string, DOMRect>,
  moving: readonly string[],
  drop = 0,
): TileMove[] | null {
  const moves: TileMove[] = [];
  for (const id of moving) {
    const from = before.get(id);
    const el = root.querySelector<HTMLElement>(cardSelector(id));
    if (!from || !el) return null;
    const to = el.getBoundingClientRect();
    moves.push({ from: { left: from.left, top: from.top }, to: { left: to.left, top: to.top - drop }, width: to.width });
  }
  return moves;
}

/** Each card's current slide - see `done` in glide. */
const playing = new WeakMap<HTMLElement, Animation>();

/** How the close runs: when each moving card sets off, and how far (and
 * for how long) the fold's copy still pushes the returning cards down. */
type ClosePlan = { starts: number[]; drift: (t: number) => number; driftMs: number };

/**
 * Play every card that moved from where it was (`before`) to where it is
 * now. A moving card sets off at its start; with a `close` plan it is drawn
 * along keyframes that also cancel the fold copy's push (driftFrames). A
 * card that only shifts to make room goes at once when it moves down, and
 * with the last moving card when it moves up, so it never slides over one
 * that has not left yet.
 */
function glide(
  root: HTMLElement,
  before: Map<string, DOMRect>,
  moving: readonly string[],
  starts: number[],
  close?: ClosePlan,
): Animation[] {
  const lastStart = Math.max(0, ...starts);
  const plays: { el: HTMLElement; frames: Keyframe[]; opts: KeyframeAnimationOptions; tile: boolean }[] = [];
  // Every read before any write, so the measuring costs one layout.
  for (const el of root.querySelectorAll<HTMLElement>(`[${CARD_ATTR}]`)) {
    const id = el.getAttribute(CARD_ATTR)!;
    const from = before.get(id);
    if (!from || typeof el.animate !== "function") continue;
    const to = el.getBoundingClientRect();
    const i = moving.indexOf(id);
    if (i >= 0 && close) {
      const drop = close.drift(0);
      const move: TileMove = { from, to: { left: to.left, top: to.top - drop }, width: to.width };
      if (Math.abs(from.left - to.left) < 1 && Math.abs(from.top - move.to.top) < 1 && drop < 1) continue;
      const total = Math.max(starts[i] + TILE_MS, close.driftMs);
      plays.push({
        el,
        frames: driftFrames(move, starts[i], total, close.drift, TIMING),
        opts: { duration: total, easing: "linear" },
        tile: true,
      });
      continue;
    }
    const dx = from.left - to.left;
    const dy = from.top - to.top;
    if (Math.abs(dx) < 1 && Math.abs(dy) < 1) continue;
    const delay = i >= 0 ? (starts[i] ?? 0) : dy > 0 ? lastStart : 0;
    plays.push({
      el,
      frames: [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }],
      // Held at the old place while it waits its turn.
      opts: { duration: TILE_MS, easing: EASE, delay, fill: "backwards" },
      tile: i >= 0,
    });
  }
  return plays.map(({ el, frames, opts, tile }) => {
    // A moving card passes over the others' edges on its way: it goes on top.
    if (tile) {
      el.style.position = "relative";
      el.style.zIndex = "1";
    }
    const a = el.animate(frames, opts);
    playing.set(el, a);
    // A cancelled slide reports it later, after a newer one on the same card
    // may have set these again: only the card's current slide clears them.
    const done = () => {
      if (playing.get(el) !== a) return;
      playing.delete(el);
      el.style.position = "";
      el.style.zIndex = "";
    };
    a.onfinish = done;
    a.oncancel = done;
    return a;
  });
}

/** Resolves once every animation has ended (or been cancelled), or after
 * the longest of them plus some slack if one never says so. */
function settled(anims: Animation[]): Promise<void> {
  if (anims.length === 0) return Promise.resolve();
  const longest = Math.max(
    ...anims.map((a) => {
      const t = a.effect?.getTiming();
      return Number(t?.duration ?? TILE_MS) + Number(t?.delay ?? 0);
    }),
  );
  const ended = Promise.all(anims.map((a) => a.finished.catch(() => undefined))).then(() => undefined);
  return Promise.race([ended, wait(longest + GRACE_MS)]);
}

/** The plain cascade, for motion nothing else times: 0, 70, 140 top-down
 * (or bottom-up with `reverse`). */
const cascade = (n: number, reverse = false) =>
  Array.from({ length: n }, (_, i) => (reverse ? n - 1 - i : i) * TILE_STAGGER_MS);

/**
 * Show less, planned once the cards are laid out back in the right column
 * - under the fold's shrinking copy, which pushes them down by `span +
 * margin` at first and by nothing at the end. Without a copy (none of the
 * history was on screen) nothing pushes them and no edge holds them back.
 */
function planBack(
  root: HTMLElement,
  panel: HTMLElement,
  before: Map<string, DOMRect>,
  moving: readonly string[],
  shrink: FoldMotion | null,
): ClosePlan | null {
  const push = shrink ? shrink.span + shrink.margin : 0;
  const ms = shrink?.ms ?? 0;
  const ease = shrink ? parseEasing(shrink.easing) : () => 1;
  const drift = (t: number) => (ms > 0 && t < ms ? push * (1 - ease(Math.max(0, t) / ms)) : 0);
  const moves = journeys(root, before, moving, push);
  const column = panel.parentElement;
  if (!moves || !column) return null;
  // The panel's foot once the copy has gone, and while it shrinks.
  const foot = panel.getBoundingClientRect().bottom - push;
  const edge = (t: number) => foot + drift(t);
  const starts = planClose(moves, column.getBoundingClientRect().left, edge, ms, TIMING);
  return { starts, drift, driftMs: ms };
}

const NONE: readonly string[] = [];

type Pending = {
  /** open/close: Show more / Show less. switch: the Changelog/Logs toggle. */
  kind: "open" | "close" | "switch";
  rects: Map<string, DOMRect>;
  placement: TilePlacement;
  /** The cards that change column this time, top to bottom: the moving
   * ones, and the spare ones too while the column is roomy. */
  moving: string[];
  done?: () => void;
};

export function useTileLayout({
  rootRef,
  changelogRef,
  moving,
  spare = NONE,
  fold,
  changelogShown,
}: {
  /** The element around every card - measurements are taken inside it. */
  rootRef: RefObject<HTMLElement | null>;
  /** The changelog/log panel at the top of the right column. */
  changelogRef: RefObject<HTMLElement | null>;
  /** The ids of the cards that change column, top to bottom. */
  moving: readonly string[];
  /** The ids of the cards that stack beside the moving ones while the right
   * column is roomy, and sit in the left column otherwise. */
  spare?: readonly string[];
  /** The folding box of the history, while it is open. */
  fold: () => HTMLElement | null;
  /** False while the right column shows something other than the
   * changelog (the app log): the cards then stay under it. */
  changelogShown: boolean;
}) {
  const wide = useMediaQuery(WIDE_QUERY);
  // Where the moving cards sit while the changelog shows, and whether its
  // history is open. Show more / Show less change both in one commit.
  const [side, setSide] = useState<"left" | "right">("right");
  const [expanded, setExpanded] = useState(false);
  const placement: TilePlacement = !wide ? "single" : changelogShown ? side : "right";
  // Room for two cards across the right column, read from the grid's width.
  // That width is the page's: where the cards sit cannot change it, so this
  // never feeds back into itself.
  const [roomy, setRoomy] = useState(false);
  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const read = () => setRoomy(roomyFor(root.clientWidth, remPx()));
    read();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(read);
    ro.observe(root);
    return () => ro.disconnect();
  }, [rootRef]);

  const pending = useRef<Pending | null>(null);
  // What the fold reported during the commit that moved the cards: the
  // opening plan made as its grow was measured, or its closing copy.
  const opened = useRef<{ starts: number[]; end: number } | null>(null);
  const closed = useRef<FoldMotion | null>(null);
  const running = useRef<Animation[]>([]);
  const busy = useRef(false);
  const latest = useRef({ placement, wide, roomy });
  useLayoutEffect(() => {
    latest.current = { placement, wide, roomy };
  });

  /** Stop any slide under way: the cards settle where they are laid out. */
  const stop = useCallback(() => {
    for (const a of running.current) a.cancel();
    running.current = [];
  }, []);

  useEffect(() => stop, [stop]);

  // Across the breakpoint the layout the slide was planned against is gone
  // (one column, or two where there was one): settle at once.
  const wasWide = useRef(wide);
  useLayoutEffect(() => {
    if (wasWide.current === wide) return;
    wasWide.current = wide;
    stop();
  }, [wide, stop]);

  // The same across the roomy width: the spare stack comes or goes, and a
  // slide planned against the other layout settles at once.
  const wasRoomy = useRef(roomy);
  useLayoutEffect(() => {
    if (wasRoomy.current === roomy) return;
    wasRoomy.current = roomy;
    stop();
  }, [roomy, stop]);

  /**
   * Collapse's grow is measured (Show more): plan the whole motion now,
   * while the cards are already laid out in the left column and the
   * history at its full height. Returns how long the grow waits for
   * Updates to clear the column.
   */
  const onGrow = useCallback(
    (grow: FoldMotion): number => {
      const p = pending.current;
      const root = rootRef.current;
      const panel = changelogRef.current;
      const column = panel?.parentElement;
      const box = fold();
      if (p?.kind !== "open" || !root || !panel || !column || !box) return 0;
      const moves = journeys(root, p.rects, p.moving);
      if (!moves) return 0;
      const full = panel.getBoundingClientRect().bottom;
      // The panel's foot with the history mounted but not grown yet (its
      // margin is there from the first frame).
      const base = full - box.getBoundingClientRect().height;
      const ease = parseEasing(grow.easing);
      const edge = (t: number) => (t < 0 ? base : t < grow.ms ? base + grow.span * ease(t / grow.ms) : full);
      const plan = planOpen(moves, column.getBoundingClientRect().left, edge, grow.ms, TIMING);
      opened.current = { starts: plan.starts, end: plan.fold + grow.ms };
      return plan.fold;
    },
    [rootRef, changelogRef, fold],
  );

  /** Collapse's closing copy starts to shrink (Show less). */
  const onShrink = useCallback((shrink: FoldMotion) => {
    if (pending.current?.kind === "close") closed.current = shrink;
  }, []);

  // The commit after a change: play back whatever moved. Only a change of
  // placement moves the cards on purpose - anything else (a taller panel)
  // lands as it always has.
  useLayoutEffect(() => {
    const p = pending.current;
    const open = opened.current;
    const shrink = closed.current;
    pending.current = null;
    opened.current = null;
    closed.current = null;
    if (!p) return;
    const root = rootRef.current;
    if (!root || !wide || p.placement === placement || p.placement === "single") {
      // Changelog <-> Logs mid-slide, the cards staying in the right column:
      // the fold (and the copy a returning card was drawn against) went with
      // the changelog, so the planned paths no longer fit - settle at once.
      if (p.kind === "switch") stop();
      p.done?.();
      return;
    }
    // A slide still under way gives way to this one, which starts from
    // wherever the cards were caught (the snapshot included the motion).
    stop();
    let anims: Animation[];
    let until = 0;
    if (p.kind === "open") {
      anims = glide(root, p.rects, p.moving, open?.starts ?? cascade(p.moving.length));
      until = open?.end ?? 0;
    } else if (p.kind === "close") {
      // Planned in the order of the spots the cards return to.
      const moving = topDown(p.moving, measure(root));
      const panel = changelogRef.current;
      const back = panel ? planBack(root, panel, p.rects, moving, shrink) : null;
      anims = back
        ? glide(root, p.rects, moving, back.starts, back)
        : glide(root, p.rects, moving, cascade(moving.length, true));
      until = shrink?.ms ?? 0;
    } else {
      anims = glide(root, p.rects, p.moving, cascade(p.moving.length));
    }
    running.current = anims;
    void Promise.all([settled(anims), wait(until)]).then(() => {
      if (running.current === anims) running.current = [];
      p.done?.();
    });
  });

  const run = useCallback(
    (kind: Pending["kind"], change: () => void, done?: () => void) => {
      const root = rootRef.current;
      if (root && latest.current.wide && !reducedMotion()) {
        const rects = measure(root);
        // A card the screen does not show (Extras, while locked) has no
        // journey, and leaving it in would void the whole plan.
        const ids = (latest.current.roomy ? [...moving, ...spare] : [...moving]).filter((id) => rects.has(id));
        pending.current = { kind, rects, placement: latest.current.placement, moving: topDown(ids, rects), done };
      } else {
        done?.();
      }
      change();
    },
    [rootRef, moving, spare],
  );

  /** Apply `change` (a state update that may move the cards), gliding the
   * cards it moves - for the Changelog/Logs switch. Without the wide layout,
   * or under reduced motion, it is just `change()`. */
  const flip = useCallback((change: () => void) => run("switch", change), [run]);

  /** Show more / Show less. A click while the motion runs is ignored, so
   * the cards and the history cannot end up out of step. */
  const toggle = useCallback(() => {
    if (busy.current) return;
    const opening = !expanded;
    const to = opening ? "left" : "right";
    const apply = () => {
      setSide(to);
      setExpanded(opening);
    };
    if (!latest.current.wide || reducedMotion() || side === to) {
      apply();
      return;
    }
    busy.current = true;
    run(opening ? "open" : "close", apply, () => {
      busy.current = false;
    });
  }, [expanded, side, run]);

  /** True while the spare cards stack beside the moving ones under the
   * panel: the wide layout, the cards on the right, and room for two. */
  const spareStack = placement === "right" && roomy;

  return { placement, spareStack, expanded, toggle, flip, onGrow, onShrink };
}
