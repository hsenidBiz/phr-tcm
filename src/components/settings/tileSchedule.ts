import { timeToReach, type Easing } from "../../lib/cubicBezier";

/**
 * When each Settings card sets off, so the changelog's folding edge and the
 * cards move as one motion without ever crossing (useTileLayout plays it).
 *
 * Pure: everything comes in as measured geometry and known curves, and
 * times are in milliseconds. A card is a box of `width` that travels from
 * `from` to `to` (its left/top) on the tile curve over `tileMs`; the right
 * column starts at x = `colLeft`; the changelog panel sits at the top of
 * that column, and `edge(t)` is its bottom edge at time t.
 *
 * "Never crosses" means: whenever any part of a card is inside the right
 * column, the panel's bottom edge is at or above the card's top. A card's
 * top anywhere along its move is taken as the higher of its two ends, which
 * holds for any path between them that does not overshoot.
 */

export type TileMove = {
  from: { left: number; top: number };
  to: { left: number; top: number };
  width: number;
};

export type TileTiming = {
  /** The cards' own curve and length. */
  ease: Easing;
  tileMs: number;
  /** The least gap between one card setting off and the next. */
  staggerMs: number;
  /** Slack kept between a card clearing a spot and the edge reaching it. */
  safetyMs: number;
};

/** The first t in [lo, hi] at which a monotonic test turns true - `lo` if
 * it already holds there, Infinity if it never does. */
export function firstTime(test: (t: number) => boolean, lo: number, hi: number): number {
  if (test(lo)) return lo;
  if (!test(hi)) return Infinity;
  for (let i = 0; i < 50; i++) {
    const mid = (lo + hi) / 2;
    if (test(mid)) hi = mid;
    else lo = mid;
  }
  return hi;
}

/** The highest the card's top gets on its way (screen y grows downwards). */
const highestTop = (m: TileMove) => Math.min(m.from.top, m.to.top);

/** How long after it sets off a card moving LEFT out of the column is
 * wholly clear of it (its right edge at or left of `colLeft`). */
export function clearTime(m: TileMove, colLeft: number, t: TileTiming): number {
  if (m.from.left + m.width <= colLeft) return 0;
  const dx = m.from.left - m.to.left;
  // How far right of its final place it may still be and be clear.
  const room = colLeft - m.width - m.to.left;
  if (dx <= 0 || room < 0) return Infinity;
  return timeToReach(t.ease, 1 - room / dx) * t.tileMs;
}

/** How long after it sets off a card moving RIGHT into the column first
 * has its right edge inside it. */
export function enterTime(m: TileMove, colLeft: number, t: TileTiming): number {
  if (m.from.left + m.width > colLeft) return 0;
  const dx = m.to.left - m.from.left;
  // How far into the column its right edge ends up.
  const inside = m.to.left + m.width - colLeft;
  if (dx <= 0 || inside <= 0) return Infinity;
  return timeToReach(t.ease, 1 - inside / dx) * t.tileMs;
}

/**
 * Opening: the cards leave the right column top-down and the panel grows
 * into the space they leave. `edge(t)` is the panel's bottom with t
 * measured from the moment its grow starts (t < 0: not grown yet), and it
 * is final from `foldMs` on.
 *
 * Each card sets off as late as it can and still be clear before the edge
 * reaches its top (t_reach - clear - safety), and never before the card
 * above it. The first card sets off at 0; if it would have to leave before
 * the grow starts, the grow waits instead - that wait is `fold`. A card
 * the edge never reaches follows the one above it by the stagger.
 */
export function planOpen(
  moves: TileMove[],
  colLeft: number,
  edge: (t: number) => number,
  foldMs: number,
  timing: TileTiming,
): { fold: number; starts: number[] } {
  const latest = moves.map((m) => {
    const top = highestTop(m);
    const reach = firstTime((t) => edge(t) > top, 0, foldMs + 1);
    const clear = clearTime(m, colLeft, timing);
    if (reach === Infinity) return Infinity;
    // A card that can never clear the column would cross whatever we do;
    // it goes as early as the others allow.
    return reach - (clear === Infinity ? timing.tileMs : clear) - timing.safetyMs;
  });
  // Top-down order: a card no later than the one below it. Moving a start
  // EARLIER is always safe here - it only leaves sooner.
  for (let i = latest.length - 2; i >= 0; i--) {
    latest[i] = Math.min(latest[i], latest[i + 1]);
  }
  if (latest.length === 0) return { fold: 0, starts: [] };
  latest[0] = Math.min(latest[0], 0);
  for (let i = 1; i < latest.length; i++) {
    if (latest[i] === Infinity) latest[i] = latest[i - 1] + timing.staggerMs;
  }
  const fold = latest[0] < 0 ? -latest[0] : 0;
  return { fold, starts: latest.map((s) => s + fold) };
}

/**
 * Closing: the panel shrinks from t = 0 and the cards come back into the
 * right column bottom-up, each as soon as its spot is free. `edge(t)` is
 * the panel's bottom (falling), final from `foldMs` on. A card sets off so
 * that it enters the column no sooner than the edge has risen above its top
 * (plus the safety slack), and never before the card below it - so the
 * lowest spot, freed first, fills first. A card whose spot is free from the
 * start follows the one below it by the stagger.
 */
export function planClose(
  moves: TileMove[],
  colLeft: number,
  edge: (t: number) => number,
  foldMs: number,
  timing: TileTiming,
): number[] {
  // null: free from the start, so nothing holds the card back.
  const earliest = moves.map((m): number | null => {
    const top = highestTop(m);
    const free = firstTime((t) => edge(t) <= top, 0, foldMs + 1);
    const enter = enterTime(m, colLeft, timing);
    if (free === 0 || enter === Infinity) return null;
    return Math.max(0, (free === Infinity ? foldMs : free) - enter + timing.safetyMs);
  });
  // Bottom-up order. Moving a start LATER is always safe here - the edge
  // only keeps rising.
  const starts: number[] = new Array(moves.length);
  for (let i = moves.length - 1; i >= 0; i--) {
    const below = i === moves.length - 1 ? null : starts[i + 1];
    const own = earliest[i];
    if (own === null) starts[i] = below === null ? 0 : below + timing.staggerMs;
    else starts[i] = below === null ? own : Math.max(own, below);
  }
  return starts;
}

/** Where a card is drawn at time t: at `from` until `start`, then along
 * the tile curve to `to`. */
export function tileAt(m: TileMove, start: number, t: number, timing: TileTiming): { left: number; top: number } {
  const p = timing.ease(Math.min(1, Math.max(0, (t - start) / timing.tileMs)));
  return { left: m.from.left + (m.to.left - m.from.left) * p, top: m.from.top + (m.to.top - m.from.top) * p };
}

/**
 * Keyframes (linear between them) that draw a card along `tileAt` while
 * its laid-out box sits `drift(t)` px below `to` - a card returning under a
 * shrinking panel is laid out under the shrinking copy, and rises with it.
 * The transform is always what is left over: where it should be drawn,
 * minus where the layout has it. Ends at no transform.
 */
export function driftFrames(
  m: TileMove,
  start: number,
  total: number,
  drift: (t: number) => number,
  timing: TileTiming,
  stepMs = 1000 / 60,
): Keyframe[] {
  const n = Math.max(1, Math.ceil(total / stepMs));
  const frames: Keyframe[] = [];
  for (let k = 0; k <= n; k++) {
    const t = (total * k) / n;
    const at = tileAt(m, start, t, timing);
    const dx = at.left - m.to.left;
    const dy = at.top - (m.to.top + drift(t));
    frames.push({ offset: k / n, transform: k === n ? "none" : `translate(${dx}px, ${dy}px)` });
  }
  return frames;
}
