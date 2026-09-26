// Where the numbered markers go on a shot, worked out in the shot's own
// pixels (so the same boxes always give the same layout, at any rendered
// size) and free of the DOM, so it is tested as a pure function.
//
// Each marker sits just OUTSIDE a corner of its control - the top-left one
// unless another corner is clearly better (the frame's edge, other
// controls, markers already placed) - and no two markers ever overlap:
// one that cannot have any corner is moved to the nearest free spot around
// it, and gets a thin leader line back to its control's corner.

import { shotSize, type Box, type Control, type ShotPositions, type Size } from "../types";
import type { Stage } from "./plan";

/** A marker's diameter, in shot pixels. The stylesheet draws it at this
 *  size scaled with the shot, never smaller than MARKER_MIN_PX. */
export const MARKER_SIZE = 24;
/** The least room between two markers, in shot pixels. */
export const MARKER_GAP = 4;
/** The smallest a marker is ever drawn (CSS px; styles.css .marker --size). */
export const MARKER_MIN_PX = 12;
/** Below this render scale (CSS px per shot px) the MARKER_MIN_PX floor is
 *  wider than the room layoutMarkers keeps, and markers could touch. The
 *  smallest desktop figure is a whole 1440 shot in a 1024 px window: the
 *  654 px content column is a scale of 0.454; the full-screen view keeps
 *  its stage at least that wide (a narrower list panel on small windows). */
export const MIN_RENDER_SCALE = MARKER_MIN_PX / (MARKER_SIZE + MARKER_GAP);
/** A marker further than this (shot px) from its natural spot is joined to
 *  its control's corner by a leader line. */
export const LEADER_AFTER = MARKER_SIZE / 2;
/** Markers keep this far (shot px) inside the frame, whole. */
const FRAME_MARGIN = 2;

export type Corner = "tl" | "tr" | "bl" | "br";

export type MarkerSpot = {
  /** The marker's centre. */
  x: number;
  y: number;
  /** The control corner it belongs to... */
  corner: Corner;
  /** ...and that corner's point, where a leader line ends. */
  anchor: { x: number; y: number };
  /** Moved away from its natural spot far enough to need a leader line. */
  leader: boolean;
};

type Point = { x: number; y: number };

/** Earlier in this list wins a tie: top-left first, as the eye expects. */
const CORNERS: Corner[] = ["tl", "tr", "bl", "br"];
const CORNER_BIAS: Record<Corner, number> = { tl: 0, tr: 6, bl: 10, br: 14 };
/** What covering its own control costs, and covering another one (per
 *  full marker area), in shot px of displacement. */
const COVER_OWN = 40;
const COVER_OTHER = 12;
/** Clamping a marker back into the frame moves it onto its control. */
const CLAMP_COST = 2;
/** The nearest-free-spot search: rings this far apart, this many spots each. */
const RING_STEP = MARKER_SIZE / 3;
const RING_SPOTS = 24;

function corner(b: Box, c: Corner): Point {
  return { x: c.endsWith("l") ? b.x : b.x + b.w, y: c.startsWith("t") ? b.y : b.y + b.h };
}

/** Area of a marker's square (centre p, radius r) inside box b, as a share of the square. */
function cover(p: Point, r: number, b: Box): number {
  const w = Math.min(p.x + r, b.x + b.w) - Math.max(p.x - r, b.x);
  const h = Math.min(p.y + r, b.y + b.h) - Math.max(p.y - r, b.y);
  return w > 0 && h > 0 ? (w * h) / (4 * r * r) : 0;
}

const contains = (outer: Box, inner: Box) =>
  outer.x <= inner.x && outer.y <= inner.y && outer.x + outer.w >= inner.x + inner.w && outer.y + outer.h >= inner.y + inner.h;

const dist = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);

/**
 * One marker per box, in the order given (the reading order: earlier boxes
 * get first pick). `frame` is the area the markers must stay whole inside
 * (the shot, or the crop of it being shown), in the same pixels as the
 * boxes. `obstacles` are every control box on that frame, the ones being
 * marked included: markers prefer not to cover them.
 */
export function layoutMarkers(
  boxes: Box[],
  frame: Size,
  opts: { size?: number; gap?: number; obstacles?: Box[] } = {},
): MarkerSpot[] {
  const size = opts.size ?? MARKER_SIZE;
  const r = size / 2;
  const apart = size + (opts.gap ?? MARKER_GAP);
  const lo = r + FRAME_MARGIN;
  const clampPoint = (p: Point): Point => ({
    x: Math.min(Math.max(p.x, lo), Math.max(lo, frame.w - lo)),
    y: Math.min(Math.max(p.y, lo), Math.max(lo, frame.h - lo)),
  });
  // Just outside the corner: the circle clears both of the control's edges.
  const off = 0.75 * r;
  const natural = (b: Box, c: Corner): Point => {
    const p = corner(b, c);
    return { x: p.x + (c.endsWith("l") ? -off : off), y: p.y + (c.startsWith("t") ? -off : off) };
  };

  const placed: Point[] = [];
  const free = (p: Point) => placed.every((q) => dist(p, q) >= apart - 1e-9);
  const out: MarkerSpot[] = [];

  for (const b of boxes) {
    // Other controls this marker should keep off; a box around this one
    // (a panel holding it) is no obstacle - its marker would always be on it.
    const others = (opts.obstacles ?? boxes).filter((o) => o !== b && !contains(o, b) && !sameBox(o, b));
    const coverCost = (p: Point) => cover(p, r, b) * COVER_OWN + others.reduce((s, o) => s + cover(p, r, o) * COVER_OTHER, 0);

    // 1. The four corners, each pulled inside the frame.
    const options = CORNERS.map((c) => {
      const want = natural(b, c);
      const at = clampPoint(want);
      return { c, want, at, cost: CORNER_BIAS[c] + dist(at, want) * CLAMP_COST + coverCost(at) };
    }).sort((a, z) => a.cost - z.cost || CORNERS.indexOf(a.c) - CORNERS.indexOf(z.c));

    let pick: { c: Corner; at: Point } | undefined = options.find((o) => free(o.at));

    // 2. Every corner is taken: the nearest free spot around the best one,
    //    ring by ring, preferring spots off other controls.
    if (!pick) {
      const base = options[0];
      const maxRing = Math.ceil(Math.hypot(frame.w, frame.h) / RING_STEP);
      for (let k = 1; k <= maxRing && !pick; k++) {
        let best: { at: Point; cost: number } | undefined;
        for (let i = 0; i < RING_SPOTS; i++) {
          const a = (i / RING_SPOTS) * 2 * Math.PI;
          const at = clampPoint({ x: base.want.x + k * RING_STEP * Math.cos(a), y: base.want.y + k * RING_STEP * Math.sin(a) });
          if (!free(at)) continue;
          const cost = dist(at, base.want) + coverCost(at);
          if (!best || cost < best.cost - 1e-9) best = { at, cost };
        }
        if (best) pick = { c: base.c, at: best.at };
      }
      // A frame too small to hold them all: overlap rather than vanish.
      pick ??= { c: base.c, at: base.at };
    }

    const at = { x: round(pick.at.x), y: round(pick.at.y) };
    placed.push(at);
    out.push({
      ...at,
      corner: pick.c,
      anchor: corner(b, pick.c),
      leader: dist(at, natural(b, pick.c)) > LEADER_AFTER,
    });
  }
  return out;
}

const sameBox = (a: Box, b: Box) => a.x === b.x && a.y === b.y && a.w === b.w && a.h === b.h;
const round = (n: number) => Math.round(n * 100) / 100;

/** A stage's markers, as the page draws them: each placed control's box in
 *  the stage's view pixels (the whole shot, or its crop) and its spot. The
 *  other controls on the shot are obstacles. Controls without a box have
 *  no marker (their list row still documents them). */
export function stageMarkers(
  stage: Pick<Stage, "shot" | "view" | "controls">,
  placed: ShotPositions | undefined,
): { control: Control; n: number; box: Box; spot: MarkerSpot }[] {
  const size = shotSize(stage.shot);
  const view = stage.view;
  // Boxes are in the pixels they were measured at: the shot's size, unless
  // positions.json is stale (validate.ts positionsProblems reports that).
  const at = placed?.size ?? size;
  const sx = size.w / at.w;
  const sy = size.h / at.h;
  const toView = (b: Box): Box => ({ x: b.x * sx - view.x, y: b.y * sy - view.y, w: b.w * sx, h: b.h * sy });
  const inView = (b: Box) => b.x < view.w && b.y < view.h && b.x + b.w > 0 && b.y + b.h > 0;
  const obstacles = Object.values(placed?.controls ?? {}).map(toView).filter(inView);
  const marked = stage.controls.flatMap(({ control, n }) => {
    const b = placed?.controls?.[control.id];
    return b ? [{ control, n, box: toView(b) }] : [];
  });
  const spots = layoutMarkers(
    marked.map((m) => m.box),
    { w: view.w, h: view.h },
    { obstacles },
  );
  return marked.map((m, i) => ({ ...m, spot: spots[i] }));
}
