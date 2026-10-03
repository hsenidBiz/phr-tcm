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
    // Column 2 is the tallest here, so centring leaves it at the top.
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

describe("layoutFlow centres each column", () => {
  // The owner: the first template "always starts on top" - centred instead,
  // the arrows fan out up and down and stay distinguishable.
  test("a one-box column beside a taller column sits halfway down", () => {
    const { boxes, height } = layoutFlow(CYCLE, { competencies: 3, participants: 2 });
    const b = byId(boxes);
    expect(b.setup.y).toBe((height - b.setup.h) / 2);
    expect(b.rules.y).toBe((height - b.rules.h) / 2);
    expect(b.publish.y).toBe((height - b.publish.h) / 2);
    expect(b.setup.y).toBeGreaterThan(0);
  });

  test("the tallest column still starts at 0, and height is still its stack", () => {
    const { boxes, height } = layoutFlow(CYCLE, { competencies: 3, participants: 2 });
    const b = byId(boxes);
    expect(b.competencies.y).toBe(0);
    // ...while a shorter column on the same map is pushed down from the top.
    expect(b.setup.y).toBeGreaterThan(0);
    expect(b.publish.y).toBeGreaterThan(0);
    expect(height).toBe(b.competencies.h + GEOMETRY.gapY + b.participants.h);
    expect(b.participants.y + b.participants.h).toBe(height);
  });

  test("a column of several boxes is centred as one stack, gaps included", () => {
    // Column 1 holds b and c (two short boxes); column 2 holds d alone but tall.
    const f = flow([
      s("a", { creates: true }),
      s("b", { requires: ["a"] }),
      s("c", { requires: ["a"] }),
      s("d", { requires: ["b", "c"] }),
      s("e", { requires: ["b", "c"] }),
      s("g", { requires: ["b", "c"] }),
    ]);
    const { boxes, height } = layoutFlow(f, { d: 6 });
    const x = byId(boxes);
    const stack = x.b.h + GEOMETRY.gapY + x.c.h;
    expect(x.b.y).toBe((height - stack) / 2);
    expect(x.c.y).toBe(x.b.y + x.b.h + GEOMETRY.gapY);
  });

  test("no box goes above 0, and every box stays inside the height", () => {
    const cases: Record<string, number>[] = [{}, { setup: 5 }, { competencies: 4 }, { publish: 9, rules: 2 }];
    for (const counts of cases) {
      const { boxes, height } = layoutFlow(CYCLE, counts);
      for (const b of boxes) {
        expect(b.y).toBeGreaterThanOrEqual(0);
        expect(b.y + b.h).toBeLessThanOrEqual(height);
      }
    }
  });

  test("the edges still run from right-middle to left-middle of the placed boxes", () => {
    const { boxes } = layoutFlow(CYCLE, { competencies: 3, participants: 2 });
    const b = byId(boxes);
    const nums = (edgePath(b.setup, b.rules).match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number);
    expect(nums[0]).toBe(b.setup.x + GEOMETRY.colW);
    expect(nums[1]).toBe(b.setup.y + b.setup.h / 2);
    expect(nums[nums.length - 2]).toBe(b.rules.x);
    expect(nums[nums.length - 1]).toBe(b.rules.y + b.rules.h / 2);
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
