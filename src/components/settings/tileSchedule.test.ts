import { describe, expect, test } from "vitest";
import { parseEasing } from "../../lib/cubicBezier";
import { EASE, EASE_TALL, foldMs } from "../ui/collapse";
import {
  clearTime,
  driftFrames,
  enterTime,
  planClose,
  planOpen,
  tileAt,
  type TileMove,
  type TileTiming,
} from "./tileSchedule";

const timing: TileTiming = { ease: parseEasing(EASE), tileMs: 420, staggerMs: 70, safetyMs: 20 };

// A typical wide window (about 1440x900 with the sidebar): a 32rem left
// column at x=272, the right column from x=816, three cards 512px wide.
// Right column: the changelog panel ends at y=480, then Updates (130px),
// Backup & transfer (100px) and Help & support (80px), 16px apart. Left
// column: the same three land from y=818 under AI tools.
const COL = 816;
const W = 512;
const rightTops = [496, 642, 758];
const leftTops = [818, 964, 1080];
const out: TileMove[] = rightTops.map((top, i) => ({
  from: { left: COL, top },
  to: { left: 272, top: leftTops[i] },
  width: W,
}));
const back: TileMove[] = out.map((m) => ({ from: m.to, to: m.from, width: m.width }));

// Opening: the history box is 450px (50vh), all of it on screen, so it
// grows on the tall curve; with its 16px margin the panel is 16px taller
// the moment it mounts, and 946px at the end.
const growSpan = 450;
const growMs = foldMs(growSpan);
const tall = parseEasing(EASE_TALL);
const openEdge = (t: number) => (t < 0 ? 496 : t < growMs ? 496 + growSpan * tall(t / growMs) : 946);

// Closing: the copy shrinks 450px of box and its 16px margin on EASE.
const shrinkMs = foldMs(growSpan);
const ease = parseEasing(EASE);
const closeEdge = (t: number) => 480 + (growSpan + 16) * (1 - ease(Math.min(1, Math.max(0, t / shrinkMs))));

/**
 * Frame by frame (1ms): the first moment a card has any part inside the
 * right column while the panel's bottom edge is below its top, or null.
 * `byRule` takes each card's top as the planner does - the higher of its
 * two ends - instead of where the card really is at that moment. The real
 * path proves nothing crosses; the rule shows the plan is not later (or
 * sooner) than the rule needs - the cards also move down as they leave
 * and come from below as they return, which leaves the real path slack.
 */
function firstCrossing(
  moves: TileMove[],
  edgeAt: (t: number) => number,
  starts: number[],
  until: number,
  byRule = false,
) {
  for (let t = 0; t <= until; t++) {
    const e = edgeAt(t);
    for (let i = 0; i < moves.length; i++) {
      const at = tileAt(moves[i], starts[i], t, timing);
      const top = byRule ? Math.min(moves[i].from.top, moves[i].to.top) : at.top;
      if (at.left + moves[i].width > COL + 0.01 && e > top + 0.01) return { t, card: i };
    }
  }
  return null;
}

describe("clear and enter times", () => {
  test("a card leaving is clear once its right edge passes the column's left edge", () => {
    const t = clearTime(out[0], COL, timing);
    const at = tileAt(out[0], 0, t, timing);
    expect(at.left + W).toBeLessThanOrEqual(COL + 0.01);
    expect(tileAt(out[0], 0, t - 2, timing).left + W).toBeGreaterThan(COL);
  });

  test("a card coming back enters the column as soon as its right edge crosses in", () => {
    const t = enterTime(back[0], COL, timing);
    expect(tileAt(back[0], 0, t + 0.5, timing).left + W).toBeGreaterThan(COL);
    expect(tileAt(back[0], 0, Math.max(0, t - 0.5), timing).left + W).toBeLessThanOrEqual(COL + 0.01);
  });
});

describe("opening", () => {
  test("the cards leave top-down, the first at once, and the panel never crosses them", () => {
    const { fold, starts } = planOpen(out, COL, openEdge, growMs, timing);
    expect(starts[0]).toBe(0);
    expect(starts[1]).toBeGreaterThan(starts[0]);
    expect(starts[2]).toBeGreaterThan(starts[1]);
    expect(firstCrossing(out, (t) => openEdge(t - fold), starts, 2000)).toBeNull();
  });

  test("the panel starts no later than it has to: sooner, and Updates would be crossed", () => {
    const { fold, starts } = planOpen(out, COL, openEdge, growMs, timing);
    // Updates sits right under the panel, so the grow waits for it to clear.
    expect(fold).toBeCloseTo(clearTime(out[0], COL, timing) + timing.safetyMs, 3);
    const early = fold - timing.safetyMs - 10;
    expect(firstCrossing(out, (t) => openEdge(t - fold), starts, 2000, true)).toBeNull();
    expect(firstCrossing(out, (t) => openEdge(t - early), starts, 2000, true)).not.toBeNull();
  });

  test("each lower card goes as late as it can: 10ms past its safety margin later, it would be crossed", () => {
    const { fold, starts } = planOpen(out, COL, openEdge, growMs, timing);
    for (const i of [1, 2]) {
      const late = [...starts];
      late[i] += timing.safetyMs + 10;
      expect(firstCrossing(out, (t) => openEdge(t - fold), late, 2000, true)).not.toBeNull();
    }
  });

  // A card that lands higher than it starts (a short left column under a
  // long latest entry) rises across the panel's own corner on its way,
  // which no start time can prevent - so here only the fold's movement is
  // checked: its grown (or not yet shrunk) part never crosses a card.
  test("cards that land higher than they start are not crossed by the fold's movement", () => {
    const up = out.map((m, i) => ({ ...m, to: { left: 272, top: [300, 446, 562][i] } }));
    const { fold, starts } = planOpen(up, COL, openEdge, growMs, timing);
    const grown = (t: number) => (openEdge(t - fold) > 496 ? openEdge(t - fold) : -Infinity);
    expect(firstCrossing(up, grown, starts, 2000)).toBeNull();
    const backUp = up.map((m) => ({ from: m.to, to: m.from, width: m.width }));
    const shrinking = (t: number) => (closeEdge(t) > 480.5 ? closeEdge(t) : -Infinity);
    expect(firstCrossing(backUp, shrinking, planClose(backUp, COL, closeEdge, shrinkMs, timing), 2000)).toBeNull();
  });

  test("when the panel never reaches the cards, it grows at once and they keep the stagger", () => {
    const { fold, starts } = planOpen(out, COL, () => 480, growMs, timing);
    expect(fold).toBe(0);
    expect(starts).toEqual([0, 70, 140]);
  });
});

describe("closing", () => {
  test("the cards come back bottom-up and the shrinking panel never crosses them", () => {
    const starts = planClose(back, COL, closeEdge, shrinkMs, timing);
    // Help & support's spot frees first, then Backup's, then Updates'.
    expect(starts[2]).toBeLessThan(starts[1]);
    expect(starts[1]).toBeLessThan(starts[0]);
    expect(Math.min(...starts)).toBeGreaterThanOrEqual(0);
    expect(firstCrossing(back, closeEdge, starts, 2000)).toBeNull();
    expect(firstCrossing(back, closeEdge, starts, 2000, true)).toBeNull();
  });

  test("each card comes in as soon as its spot is free: 10ms past its safety margin sooner, it would be crossed", () => {
    const starts = planClose(back, COL, closeEdge, shrinkMs, timing);
    for (const i of [0, 1, 2]) {
      const early = [...starts];
      early[i] -= timing.safetyMs + 10;
      expect(firstCrossing(back, closeEdge, early, 2000, true)).not.toBeNull();
    }
  });

  test("with nothing folding, they come back at once, bottom-up by the stagger", () => {
    expect(planClose(back, COL, () => 480, 0, timing)).toEqual([140, 70, 0]);
  });
});

describe("driftFrames", () => {
  test("draws the card on its path while its box rises with the shrinking copy", () => {
    const drift = (t: number) => (growSpan + 16) * (1 - ease(Math.min(1, t / shrinkMs)));
    const start = 50;
    const total = start + timing.tileMs;
    const frames = driftFrames(back[0], start, total, drift, timing);
    expect(frames[0].offset).toBe(0);
    expect(frames[frames.length - 1]).toEqual({ offset: 1, transform: "none" });
    for (const f of frames.slice(0, -1)) {
      const t = (f.offset as number) * total;
      const [, dx, dy] = /translate\((.+)px, (.+)px\)/.exec(f.transform as string)!.map(Number);
      const at = tileAt(back[0], start, t, timing);
      // Drawn = laid out + transform.
      expect(back[0].to.left + dx).toBeCloseTo(at.left, 6);
      expect(back[0].to.top + drift(t) + dy).toBeCloseTo(at.top, 6);
    }
  });
});
