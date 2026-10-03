import type { Flow } from "../bindings";

/**
 * Where each stage of a flow sits on the API Templates tab's map (spec §8).
 *
 * A layered layout, by hand: a flow has at most 30 stages and its
 * `requires` never loop (the Rust side refuses a flow that does), so the
 * longest-path depth of each stage is all a column needs, and declaration
 * order is all a row needs. No graph library - the app has none.
 *
 * Every number the map draws with comes from here, never from measuring
 * the DOM, so the map is the same in a test (where nothing has a size) as
 * on screen.
 */

/** A stage box, placed: its column and row, and its top-left and height in px. */
export type Placed = { id: string; col: number; row: number; x: number; y: number; h: number };

/** Box width, the gaps between columns and between boxes in a column, and
 * a box's height parts: its header, one line per template, bottom padding. */
export const GEOMETRY = { colW: 208, gapX: 56, gapY: 16, head: 40, perTemplate: 24, pad: 12 };

/** A stage box's height: a line per template, and one line even with none
 * (it says "No template yet"). */
function boxHeight(templates: number): number {
  return GEOMETRY.head + GEOMETRY.perTemplate * Math.max(1, templates) + GEOMETRY.pad;
}

export function layoutFlow(
  flow: Flow,
  templateCounts: Record<string, number>,
): { boxes: Placed[]; edges: Array<{ from: string; to: string }>; width: number; height: number } {
  const byId = new Map(flow.stages.map((s) => [s.id, s]));

  // Column = the longest `requires` path from the creating stage. Memoised;
  // a stage met again while it is still being worked out would be a loop,
  // which a saved flow cannot have - it counts as depth 0 rather than
  // recursing forever.
  const depth = new Map<string, number>();
  const visiting = new Set<string>();
  const colOf = (id: string): number => {
    const known = depth.get(id);
    if (known !== undefined) return known;
    if (visiting.has(id)) return 0;
    visiting.add(id);
    const reqs = (byId.get(id)?.requires ?? []).filter((r) => byId.has(r));
    const col = reqs.length === 0 ? 0 : 1 + Math.max(...reqs.map(colOf));
    visiting.delete(id);
    depth.set(id, col);
    return col;
  };

  const rowsUsed = new Map<number, number>();
  const nextY = new Map<number, number>();
  const boxes: Placed[] = flow.stages.map((s) => {
    const col = colOf(s.id);
    const row = rowsUsed.get(col) ?? 0;
    const y = nextY.get(col) ?? 0;
    const h = boxHeight(templateCounts[s.id] ?? 0);
    rowsUsed.set(col, row + 1);
    nextY.set(col, y + h + GEOMETRY.gapY);
    return { id: s.id, col, row, x: col * (GEOMETRY.colW + GEOMETRY.gapX), y, h };
  });

  const edges = flow.stages.flatMap((s) =>
    (s.requires ?? []).filter((r) => byId.has(r)).map((r) => ({ from: r, to: s.id })),
  );

  const width = boxes.reduce((w, b) => Math.max(w, b.x + GEOMETRY.colW), 0);
  const height = boxes.reduce((h, b) => Math.max(h, b.y + b.h), 0);

  // Centre every column against the tallest one, so a lone first stage sits
  // halfway down and its arrows fan out up and down rather than all running
  // along the top. A column's stack is where its next box would have gone,
  // less the trailing gap. The tallest column's offset is 0, so `height`
  // stands.
  const centred = boxes.map((b) => {
    const stack = (nextY.get(b.col) ?? 0) - GEOMETRY.gapY;
    return { ...b, y: b.y + (height - stack) / 2 };
  });
  return { boxes: centred, edges, width, height };
}

/** An arrow's line: a cubic from `from`'s right-middle to `to`'s left-middle,
 * leaving and arriving horizontally. */
export function edgePath(from: Placed, to: Placed): string {
  const sx = from.x + GEOMETRY.colW;
  const sy = from.y + from.h / 2;
  const ex = to.x;
  const ey = to.y + to.h / 2;
  const mx = (sx + ex) / 2;
  return `M${sx} ${sy} C${mx} ${sy} ${mx} ${ey} ${ex} ${ey}`;
}
