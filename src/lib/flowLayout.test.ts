// The flow map's geometry: where each stage box sits and how the arrows
// between them run. jsdom does no layout, so the map is drawn entirely from
// these numbers - which is also what makes them testable.

import { describe, expect, test } from "vitest";
import type { Flow, Stage } from "../bindings";
import { GEOMETRY, edgePath, layoutFlow, type Placed } from "./flowLayout";

function flow(stages: Stage[]): Flow {
  return {
    id: "f",
    title: "F",
    module: "M",
    subject: { name: "cycleId", type: "number" },
    sources: [],
    stages,
  };
}

const s = (id: string, over: Partial<Stage> = {}): Stage => ({ id, title: id, check: "SELECT 1", ...over });

/** The spec's performance cycle flow (§3). */
const CYCLE = flow([
  s("setup", { creates: true }),
  s("rules", { requires: ["setup"] }),
  s("competencies", { requires: ["rules"], optional: true }),
  s("participants", { requires: ["rules"] }),
  s("publish", { requires: ["participants"] }),
]);

function byId(boxes: Placed[]): Record<string, Placed> {
  return Object.fromEntries(boxes.map((b) => [b.id, b]));
}

describe("layoutFlow", () => {
  test("columns_follow_the_longest_path", () => {
    const { boxes } = layoutFlow(CYCLE, {});
    const b = byId(boxes);
    expect(b.setup.col).toBe(0);
    expect(b.rules.col).toBe(1);
    expect(b.competencies.col).toBe(2);
    expect(b.participants.col).toBe(2);
    expect(b.publish.col).toBe(3);
    // Within a column, declaration order.
    expect(b.competencies.row).toBe(0);
    expect(b.participants.row).toBe(1);
    // x follows the column.
    expect(b.publish.x).toBe(3 * (GEOMETRY.colW + GEOMETRY.gapX));
  });

  test("the_diamond_sits_after_both", () => {
    const diamond = flow([
      s("a", { creates: true }),
      s("b", { requires: ["a"] }),
      s("c", { requires: ["a"] }),
      s("d", { requires: ["b", "c"] }),
    ]);
    const { boxes, edges } = layoutFlow(diamond, {});
    const b = byId(boxes);
    expect(b.a.col).toBe(0);
    expect(b.b.col).toBe(1);
    expect(b.c.col).toBe(1);
    expect(b.d.col).toBe(2);
    expect(edges).toEqual([
      { from: "a", to: "b" },
      { from: "a", to: "c" },
      { from: "b", to: "d" },
      { from: "c", to: "d" },
    ]);
  });

  test("a stage sits after the longest of its paths, not the shortest", () => {
    // d requires a directly and c (a -> b -> c): the long path wins.
    const f = flow([
      s("a", { creates: true }),
      s("d", { requires: ["a", "c"] }),
      s("b", { requires: ["a"] }),
      s("c", { requires: ["b"] }),
    ]);
    expect(byId(layoutFlow(f, {}).boxes).d.col).toBe(3);
  });

  test("boxes_in_a_column_do_not_overlap", () => {
    const { boxes } = layoutFlow(CYCLE, { competencies: 3, participants: 2 });
    const b = byId(boxes);
    expect(b.competencies.y + b.competencies.h + GEOMETRY.gapY).toBeLessThanOrEqual(b.participants.y);
    expect(b.competencies.y).toBe(0);
  });

  test("a_box_grows_with_its_templates", () => {
    const { boxes } = layoutFlow(CYCLE, { rules: 3 });
    const b = byId(boxes);
    const { head, perTemplate, pad } = GEOMETRY;
    // No template still leaves room for the "No template yet" line.
    expect(b.setup.h).toBe(head + perTemplate + pad);
    expect(b.rules.h).toBe(head + perTemplate * 3 + pad);
  });

  test("width and height bound every box", () => {
    const { boxes, width, height } = layoutFlow(CYCLE, { competencies: 4 });
    for (const b of boxes) {
      expect(b.x + GEOMETRY.colW).toBeLessThanOrEqual(width);
      expect(b.y + b.h).toBeLessThanOrEqual(height);
    }
    expect(width).toBe(4 * GEOMETRY.colW + 3 * GEOMETRY.gapX);
  });
});

describe("edgePath", () => {
  test("edgePath_starts_right_and_ends_left", () => {
    const from: Placed = { id: "a", col: 0, row: 0, x: 0, y: 10, h: 76 };
    const to: Placed = { id: "b", col: 1, row: 0, x: 264, y: 100, h: 100 };
    const d = edgePath(from, to);
    const nums = (d.match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number);
    // Right-middle of `from`...
    expect(d.startsWith("M")).toBe(true);
    expect(nums[0]).toBe(from.x + GEOMETRY.colW);
    expect(nums[1]).toBe(from.y + from.h / 2);
    // ...to left-middle of `to`, as a cubic.
    expect(d).toContain("C");
    expect(nums[nums.length - 2]).toBe(to.x);
    expect(nums[nums.length - 1]).toBe(to.y + to.h / 2);
  });
});
