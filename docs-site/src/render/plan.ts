// What a screen's section shows, worked out from the content and
// positions.json alone (no DOM, so it is tested directly): which figures,
// the part of each shot a figure shows (the whole shot, or a crop around a
// group's controls), the controls marked on it in reading order, and - on
// a grouped screen - the overview regions that link to each subsection.

import { SHOT_WIDTH, shotSize, type Box, type Control, type ControlGroup, type Positions, type Screen, type Shot, type Size } from "../types";

/** One figure: `view` is the part of the shot it shows, in shot pixels. */
export type Stage = {
  shot: Shot;
  view: Box;
  cropped: boolean;
  controls: { control: Control; n: number }[];
};

/** A group's area on an overview shot, in shot pixels. */
export type Region = { group: ControlGroup; box: Box };

export type ScreenPlan =
  | { grouped: false; stages: Stage[] }
  | {
      grouped: true;
      overviews: { shot: Shot; regions: Region[] }[];
      groups: { group: ControlGroup; stages: Stage[] }[];
    };

/** Controls on a shot in reading order: placed ones by visual row (centres
 *  within ROW_TOLERANCE shot px share a row), each row left to right; then
 *  any control without a position yet, in the order the content lists them. */
const ROW_TOLERANCE = 16;
export function readingOrder(controls: Control[], boxes: Record<string, Box> | undefined): Control[] {
  const placed = controls
    .filter((c) => boxes?.[c.id])
    .map((c) => ({ c, b: boxes![c.id] }))
    .sort((a, z) => a.b.y + a.b.h / 2 - (z.b.y + z.b.h / 2));
  const rows: (typeof placed)[] = [];
  for (const item of placed) {
    const row = rows[rows.length - 1];
    const cy = item.b.y + item.b.h / 2;
    if (row && cy - (row[0].b.y + row[0].b.h / 2) <= ROW_TOLERANCE) row.push(item);
    else rows.push([item]);
  }
  const ordered = rows.flatMap((row) => row.sort((a, z) => a.b.x - z.b.x)).map((i) => i.c);
  return [...ordered, ...controls.filter((c) => !boxes?.[c.id])];
}

/** The smallest box around all of `boxes`. */
export function union(boxes: Box[]): Box {
  const x = Math.min(...boxes.map((b) => b.x));
  const y = Math.min(...boxes.map((b) => b.y));
  const r = Math.max(...boxes.map((b) => b.x + b.w));
  const btm = Math.max(...boxes.map((b) => b.y + b.h));
  return { x, y, w: r - x, h: btm - y };
}

/** Room left around a group's controls in its crop (shot px). */
export const CROP_PAD = 48;
/** A group wider than tall gets a crop at least this share of the shot's
 *  width, and never a thin sliver (height : width). A tall group keeps a
 *  tall, narrow crop: the height cap (figureWidth) keeps it on screen, and
 *  its list fits beside it. */
const CROP_MIN_W = 0.4;
const CROP_MIN_TALL = 0.25;
/** A crop this close to the whole shot (share of each side) shows the whole shot. */
const CROP_WHOLE = 0.9;

/** Grows [start, start + len) to `want`, centred, then shifts it inside [0, max). */
function span(start: number, len: number, want: number, max: number): [number, number] {
  const size = Math.min(Math.max(len, want), max);
  const from = Math.min(Math.max(start - (size - len) / 2, 0), max - size);
  return [Math.round(from), Math.round(size)];
}

/**
 * The part of a shot of size `shot` a group's figure shows: its controls'
 * boxes with CROP_PAD around them, kept inside the shot. A group wider than
 * tall is widened to CROP_MIN_W of the shot and made at least CROP_MIN_TALL
 * as tall as wide; a tall one stays as narrow as its controls. Null when
 * that is (nearly) the whole shot anyway.
 */
export function cropFor(boxes: Box[], shot: Size): Box | null {
  if (!boxes.length) return null;
  const u = union(boxes);
  let w = u.w + 2 * CROP_PAD;
  let h = u.h + 2 * CROP_PAD;
  if (w >= h) {
    w = Math.max(w, shot.w * CROP_MIN_W);
    h = Math.max(h, w * CROP_MIN_TALL);
  }
  const [x, cw] = span(u.x - CROP_PAD, u.w + 2 * CROP_PAD, w, shot.w);
  const [y, ch] = span(u.y - CROP_PAD, u.h + 2 * CROP_PAD, h, shot.h);
  if (cw >= shot.w * CROP_WHOLE && ch >= shot.h * CROP_WHOLE) return null;
  return { x, y, w: cw, h: ch };
}

/** An overview region: the group's controls with a little room around them. */
const REGION_PAD = 10;
export function regionFor(boxes: Box[], shot: Size): Box {
  const u = union(boxes);
  const x = Math.max(0, u.x - REGION_PAD);
  const y = Math.max(0, u.y - REGION_PAD);
  return { x, y, w: Math.min(shot.w, u.x + u.w + REGION_PAD) - x, h: Math.min(shot.h, u.y + u.h + REGION_PAD) - y };
}

const whole = (shot: Shot): Box => ({ x: 0, y: 0, ...shotSize(shot) });

/** Boxes are used in shot pixels as they are: positions.json is measured at
 *  each shot's own size (validate.ts positionsProblems reports any entry
 *  that is not). */
export function planScreen(screen: Screen, positions: Positions): ScreenPlan {
  const onShot = (shot: Shot, controls: Control[]) => {
    const boxes = positions[shot.id]?.controls;
    return readingOrder(
      controls.filter((c) => c.shot === shot.id),
      boxes,
    ).map((control, i) => ({ control, n: i + 1 }));
  };

  if (!screen.groups?.length) {
    return { grouped: false, stages: screen.shots.map((shot) => ({ shot, view: whole(shot), cropped: false, controls: onShot(shot, screen.controls) })) };
  }

  const boxesOf = (shot: Shot, controls: Control[]) =>
    controls.flatMap((c) => {
      const b = c.shot === shot.id ? positions[shot.id]?.controls?.[c.id] : undefined;
      return b ? [b] : [];
    });

  const groups = screen.groups.map((group) => {
    const mine = screen.controls.filter((c) => c.group === group.id);
    const stages: Stage[] = screen.shots
      .filter((shot) => mine.some((c) => c.shot === shot.id))
      .map((shot) => {
        const crop = cropFor(boxesOf(shot, mine), shotSize(shot));
        return { shot, view: crop ?? whole(shot), cropped: !!crop, controls: onShot(shot, mine) };
      });
    return { group, stages };
  });

  // A shot showing two groups or more is shown whole once, as a map of them.
  const overviews = screen.shots.flatMap((shot) => {
    const regions = screen.groups!.flatMap((group) => {
      const boxes = boxesOf(
        shot,
        screen.controls.filter((c) => c.group === group.id),
      );
      return boxes.length ? [{ group, box: regionFor(boxes, shotSize(shot)) }] : [];
    });
    return regions.length > 1 ? [{ shot, regions }] : [];
  });

  return { grouped: true, overviews, groups };
}

/** How wide (CSS px) a figure may be drawn inline: a whole shot at most at
 *  its own size (shots are captured at 1x); a crop zoomed a little, up to
 *  CROP_ZOOM, but never wider than a whole main-window shot; and never so
 *  wide that it is taller than `maxHeight` (see figureMaxHeight), so the
 *  control being spotlighted is always on screen. */
export const CROP_ZOOM = 1.25;
export function figureWidth(stage: Pick<Stage, "view" | "cropped">, maxHeight = Infinity): number {
  const { w, h } = stage.view;
  const zoomed = stage.cropped ? Math.min(w * CROP_ZOOM, Math.max(w, SHOT_WIDTH)) : w;
  return Math.round(Math.min(zoomed, (w * maxHeight) / h));
}

/** The tallest a figure is drawn inline (CSS px) in a window `viewport` px
 *  tall: its own size at most (900), and the window less the top bar and
 *  some room. The stylesheet's --fig-max-h is the same rule. */
export const FIGURE_MAX_H = 900;
export const FIGURE_V_ROOM = 116;
export const FIGURE_MIN_H = 240;
export const figureMaxHeight = (viewport: number): number =>
  Math.max(FIGURE_MIN_H, Math.min(FIGURE_MAX_H, viewport - FIGURE_V_ROOM));

/** The control list's narrowest comfortable width beside a figure, and the gap. */
export const LIST_MIN = 340;
export const STAGE_GAP = 28;

/** Whether the list fits beside a figure in a stage this wide, or goes below it. */
export const stageFlow = (stageWidth: number, figure: number): "beside" | "below" =>
  figure + STAGE_GAP + LIST_MIN <= stageWidth ? "beside" : "below";
