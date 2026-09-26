import { describe, expect, test } from "vitest";
import realPositions from "../shots/positions.json";
import { screens } from "./content";
import { LEADER_AFTER, MARKER_MIN_PX, MARKER_SIZE, MIN_RENDER_SCALE, layoutMarkers, stageMarkers, type MarkerSpot } from "./render/markers";
import { CROP_PAD, cropFor, figureMaxHeight, figureWidth, planScreen, stageFlow, type Stage } from "./render/plan";
import { shotSize, type Box, type Positions, type Size } from "./types";

// The marker layout and the crops are pure functions of the boxes in
// positions.json, so they are checked here directly - on made-up crowded
// boxes, and on every shot the site really shows.

const positions = realPositions as Positions;
const MAIN: Size = { w: 1440, h: 900 };
const r = MARKER_SIZE / 2;

function overlapping(spots: MarkerSpot[]): string[] {
  const hits: string[] = [];
  spots.forEach((a, i) =>
    spots.slice(i + 1).forEach((b, j) => {
      if (Math.hypot(a.x - b.x, a.y - b.y) < MARKER_SIZE) hits.push(`${i + 1} and ${i + j + 2}`);
    }),
  );
  return hits;
}

const outside = (spots: MarkerSpot[], frame: Size) =>
  spots.map((s, i) => ({ s, i })).filter(({ s }) => s.x - r < 0 || s.y - r < 0 || s.x + r > frame.w || s.y + r > frame.h);

describe("layoutMarkers", () => {
  // A form's stacked column of fields, a side-by-side pair of cells, tiny
  // icons in a row, and controls in the frame's corners.
  const crowded: Box[] = [
    { x: 100, y: 100, w: 300, h: 16 }, // column
    { x: 100, y: 118, w: 300, h: 16 },
    { x: 100, y: 136, w: 300, h: 16 },
    { x: 100, y: 154, w: 300, h: 16 },
    { x: 600, y: 300, w: 240, h: 29 }, // action | expected
    { x: 842, y: 300, w: 240, h: 29 },
    ...[0, 1, 2, 3, 4, 5].map((i) => ({ x: 1000 + i * 12, y: 500, w: 10, h: 10 })), // icons 12 px apart
    { x: 0, y: 0, w: 40, h: 40 }, // corners
    { x: 1400, y: 0, w: 40, h: 40 },
    { x: 0, y: 860, w: 40, h: 40 },
    { x: 1400, y: 860, w: 40, h: 40 },
    { x: 0, y: 300, w: 48, h: 40 }, // the left rail
    { x: 400, y: 4, w: 160, h: 36 }, // the top bar
  ];

  test("never overlaps two markers, and keeps every one whole inside the frame", () => {
    const spots = layoutMarkers(crowded, MAIN);
    expect(spots).toHaveLength(crowded.length);
    expect(overlapping(spots)).toEqual([]);
    expect(outside(spots, MAIN)).toEqual([]);
  });

  test("a marker with room sits just outside its control's top-left corner, with no leader line", () => {
    const [first] = layoutMarkers([{ x: 600, y: 300, w: 100, h: 30 }], MAIN);
    expect(first.corner).toBe("tl");
    expect(first.anchor).toEqual({ x: 600, y: 300 });
    expect(first.x).toBeLessThan(600);
    expect(first.y).toBeLessThan(300);
    // clear of the control: the circle does not reach into its box
    expect(Math.hypot(first.x - 600, first.y - 300)).toBeGreaterThanOrEqual(r);
    expect(first.leader).toBe(false);
  });

  test("a control on the frame's left or top edge gets another corner, so its marker is never cut", () => {
    const [rail, bar, corner] = layoutMarkers(
      [
        { x: 0, y: 300, w: 48, h: 40 },
        { x: 400, y: 4, w: 160, h: 36 },
        { x: 2, y: 600, w: 40, h: 40 },
      ],
      MAIN,
    );
    expect(rail.corner).toBe("tr");
    expect(bar.corner).toBe("bl");
    expect(corner.corner).not.toBe("tl");
  });

  test("markers with room get no leader line; one pushed off its corner gets one, ending on its control's corner", () => {
    const sparse = layoutMarkers(
      [
        { x: 100, y: 100, w: 200, h: 30 },
        { x: 100, y: 300, w: 200, h: 30 },
        { x: 600, y: 300, w: 200, h: 30 },
      ],
      MAIN,
    );
    expect(sparse.map((s) => s.leader)).toEqual([false, false, false]);

    // Six 10 px icons 12 px apart: there are not enough corners to go round.
    const icons = [0, 1, 2, 3, 4, 5].map((i) => ({ x: 1000 + i * 12, y: 500, w: 10, h: 10 }));
    const spots = layoutMarkers(icons, MAIN);
    expect(spots[0].leader).toBe(false); // first in reading order gets its corner
    const led = spots.map((s, i) => ({ s, b: icons[i] })).filter(({ s }) => s.leader);
    expect(led.length).toBeGreaterThan(0);
    for (const { s, b } of led) {
      const corners = [
        { x: b.x, y: b.y },
        { x: b.x + b.w, y: b.y },
        { x: b.x, y: b.y + b.h },
        { x: b.x + b.w, y: b.y + b.h },
      ];
      expect(corners).toContainEqual(s.anchor);
      // further from that corner than a marker sitting on it would be
      expect(Math.hypot(s.x - s.anchor.x, s.y - s.anchor.y)).toBeGreaterThan(0.75 * r * Math.SQRT2 + LEADER_AFTER / 2);
    }
  });

  test("is deterministic: the same boxes always give the same layout", () => {
    expect(layoutMarkers(crowded, MAIN)).toEqual(layoutMarkers(crowded.map((b) => ({ ...b })), MAIN));
  });
});

describe("cropFor", () => {
  test("pads a wide group's controls, keeps a minimum width and stays inside the shot", () => {
    const c = cropFor([{ x: 600, y: 400, w: 100, h: 30 }], MAIN)!;
    expect(c.w).toBeGreaterThanOrEqual(MAIN.w * 0.4 - 1);
    expect(c.x).toBeLessThanOrEqual(600 - CROP_PAD);
    expect(c.y).toBeLessThanOrEqual(400 - CROP_PAD);
    expect(c.x + c.w).toBeGreaterThanOrEqual(700 + CROP_PAD);
    expect(c.h).toBeGreaterThanOrEqual(c.w * 0.25 - 1);
  });

  test("a tall group stays tall and narrow, shifted (not cut) at the shot's edge", () => {
    const c = cropFor([{ x: 1380, y: 100, w: 40, h: 700 }], MAIN)!;
    expect(c.w).toBe(40 + 2 * CROP_PAD);
    expect(c.h).toBe(700 + 2 * CROP_PAD);
    expect(c.x + c.w).toBe(1440);
  });

  test("a crop that would be (nearly) the whole shot is the whole shot", () => {
    expect(cropFor([{ x: 10, y: 10, w: 1420, h: 880 }], MAIN)).toBeNull();
  });
});

describe("figure size and stage flow", () => {
  test("the list goes beside the figure only when both fit", () => {
    expect(stageFlow(1868, 1440)).toBe("beside");
    expect(stageFlow(1440, 1440)).toBe("below");
    expect(stageFlow(1070, 460)).toBe("beside"); // a narrow window's shot
    expect(stageFlow(700, 460)).toBe("below");
  });

  test("a whole shot is never drawn wider than it was captured; a crop zooms at most 1.25x", () => {
    expect(figureWidth({ view: { x: 0, y: 0, w: 1440, h: 900 }, cropped: false })).toBe(1440);
    expect(figureWidth({ view: { x: 0, y: 0, w: 600, h: 300 }, cropped: true })).toBe(750);
    expect(figureWidth({ view: { x: 0, y: 0, w: 1300, h: 400 }, cropped: true })).toBe(1440);
  });

  test("no figure is drawn taller than the room on screen", () => {
    expect(figureMaxHeight(1440)).toBe(900);
    expect(figureMaxHeight(800)).toBe(684);
    const tall = { view: { x: 0, y: 0, w: 336, h: 670 }, cropped: true };
    expect(figureWidth(tall, 684)).toBe(343); // 684 tall
    expect(figureWidth({ view: { x: 0, y: 0, w: 1440, h: 900 }, cropped: false }, 684)).toBe(1094);
  });
});

describe("every figure the site shows", () => {
  const stages: { where: string; stage: Stage }[] = screens.flatMap((screen) => {
    const plan = planScreen(screen, positions);
    return plan.grouped
      ? plan.groups.flatMap((g) => g.stages.map((stage) => ({ where: `${screen.id}/${g.group.id}`, stage })))
      : plan.stages.map((stage) => ({ where: screen.id, stage }));
  });

  test("covers grouped and ungrouped screens", () => {
    expect(stages.some((s) => s.stage.cropped)).toBe(true);
    expect(stages.some((s) => !s.stage.cropped)).toBe(true);
    expect(screens.some((s) => s.groups?.length)).toBe(true);
    expect(screens.some((s) => !s.groups?.length)).toBe(true);
  });

  test("markers never overlap and stay inside the frame, on every shot and every crop", () => {
    const problems: string[] = [];
    for (const { where, stage } of stages) {
      const size = shotSize(stage.shot);
      const v = stage.view;
      const marked = stageMarkers(stage, positions[stage.shot.id]);
      const spots = marked.map((m) => m.spot);
      const frame = { w: v.w, h: v.h };
      const label = `${where} ${stage.shot.id}${stage.cropped ? ` crop ${v.x},${v.y} ${v.w}x${v.h}` : ""}`;
      for (const hit of overlapping(spots)) problems.push(`${label}: markers ${hit} overlap`);
      for (const { i } of outside(spots, frame)) problems.push(`${label}: marker ${i + 1} is cut by the frame`);
      // a crop shows every control of its group whole
      for (const { box: b } of marked) {
        if (b.x < 0 || b.y < 0 || b.x + b.w > v.w || b.y + b.h > v.h) problems.push(`${label}: a control is outside the crop`);
      }
      if (v.x < 0 || v.y < 0 || v.x + v.w > size.w || v.y + v.h > size.h) problems.push(`${label}: the crop leaves the shot`);
    }
    expect(problems).toEqual([]);
  });

  test("markers never touch at the smallest desktop size either, where the stylesheet's 12 px floor is bigger than the scaled marker", () => {
    // A 1024 px window: sidebar 250 + gap 56 + padding 64 leave a 654 px
    // column, so a whole 1440 shot is drawn at 0.454 - the smallest scale
    // any figure gets on a desktop (crops are smaller, so drawn larger).
    const scale = (1024 - 370) / 1440;
    expect(scale).toBeGreaterThanOrEqual(MIN_RENDER_SCALE);
    const drawn = Math.max(MARKER_MIN_PX, MARKER_SIZE * scale);
    const problems: string[] = [];
    for (const { where, stage } of stages) {
      const spots = stageMarkers(stage, positions[stage.shot.id]).map((m) => m.spot);
      spots.forEach((a, i) =>
        spots.slice(i + 1).forEach((b, j) => {
          const apart = Math.hypot(a.x - b.x, a.y - b.y) * scale;
          if (apart < drawn - 0.05) problems.push(`${where} ${stage.shot.id}: markers ${i + 1} and ${i + j + 2} are ${apart.toFixed(1)} px apart`);
        }),
      );
    }
    expect(problems).toEqual([]);
  });
});
