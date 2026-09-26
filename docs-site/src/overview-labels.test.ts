// Overview labels sit in a strip outside the shot. These pin how a strip
// is laid out: each label as near its part as it can be, never touching
// another, pushed along a little or else onto a new row.

import { expect, test } from "vitest";
import { LABEL_GAP, MAX_NUDGE, layoutStrip, leaderX } from "./render/overview-labels";

test("labels that fit stay where their parts are, in one row", () => {
  expect(layoutStrip([{ key: "a", x: 0, w: 100 }, { key: "b", x: 300, w: 100 }], 800)).toEqual([
    { key: "a", x: 0, lane: 0 },
    { key: "b", x: 300, lane: 0 },
  ]);
});

test("a label that would touch the one before it is pushed along the row", () => {
  const [, b] = layoutStrip([{ key: "a", x: 0, w: 100 }, { key: "b", x: 60, w: 100 }], 800);
  expect(b).toEqual({ key: "b", x: 100 + LABEL_GAP, lane: 0 });
});

test("a label that would have to move too far takes the next row instead", () => {
  const [, b] = layoutStrip([{ key: "a", x: 0, w: 400 }, { key: "b", x: 10, w: 100 }], 800);
  expect(400 + LABEL_GAP - 10).toBeGreaterThan(MAX_NUDGE);
  expect(b).toEqual({ key: "b", x: 10, lane: 1 });
});

test("a label never runs off the strip's right end", () => {
  const [a] = layoutStrip([{ key: "a", x: 750, w: 100 }], 800);
  expect(a.x).toBe(700);
});

test("a label with no room left in the row goes to the next one", () => {
  const spots = layoutStrip([{ key: "a", x: 500, w: 250 }, { key: "b", x: 620, w: 150 }], 800);
  expect(spots[1].lane).toBe(1);
});

test("results come back in the order given, whatever the left-to-right order", () => {
  expect(layoutStrip([{ key: "r", x: 600, w: 50 }, { key: "l", x: 0, w: 50 }], 800).map((s) => s.key)).toEqual(["r", "l"]);
});

test("a leader meets its part level with the label, kept off the part's ends", () => {
  expect(leaderX({ x: 100, w: 80 }, { x: 0, w: 400 })).toEqual({ from: 140, to: 140 });
  // A label past the part's right end: the leader lands just inside it.
  expect(leaderX({ x: 500, w: 80 }, { x: 0, w: 400 })).toEqual({ from: 510, to: 390 });
});
