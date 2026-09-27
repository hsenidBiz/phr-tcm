import { describe, expect, test } from "vitest";
import { cubicBezier, parseEasing, timeToReach } from "./cubicBezier";
import { EASE, EASE_TALL } from "../components/ui/collapse";

/** Brute force: walk the parameter finely and take the point whose x is
 * nearest - slow, but obviously right, to check the solver against. */
function bruteForce(x1: number, y1: number, x2: number, y2: number, x: number): number {
  const at = (a: number, b: number, t: number) => 3 * (1 - t) ** 2 * t * a + 3 * (1 - t) * t * t * b + t ** 3;
  let best = 0;
  let bestErr = Infinity;
  for (let i = 0; i <= 200000; i++) {
    const t = i / 200000;
    const err = Math.abs(at(x1, x2, t) - x);
    if (err < bestErr) {
      bestErr = err;
      best = at(y1, y2, t);
    }
  }
  return best;
}

describe("cubicBezier", () => {
  test("the ends are pinned and outside values clamp", () => {
    const f = cubicBezier(0.22, 1, 0.36, 1);
    expect(f(0)).toBe(0);
    expect(f(1)).toBe(1);
    expect(f(-0.5)).toBe(0);
    expect(f(2)).toBe(1);
  });

  test("the straight-line curve is linear", () => {
    const f = cubicBezier(0, 0, 1, 1);
    for (const x of [0.1, 0.25, 0.5, 0.9]) expect(f(x)).toBeCloseTo(x, 6);
  });

  test("matches a brute-force walk of the curve for the app's easings", () => {
    for (const [x1, y1, x2, y2] of [
      [0.22, 1, 0.36, 1],
      [0.33, 0, 0.2, 1],
      [0.42, 0, 0.58, 1],
      [0, 0, 0.2, 1],
    ]) {
      const f = cubicBezier(x1, y1, x2, y2);
      for (const x of [0.01, 0.1, 0.3, 0.5, 0.7, 0.95]) {
        expect(f(x)).toBeCloseTo(bruteForce(x1, y1, x2, y2, x), 4);
      }
    }
  });

  test("is monotonic for an ease-out", () => {
    const f = cubicBezier(0.22, 1, 0.36, 1);
    let last = 0;
    for (let i = 1; i <= 100; i++) {
      const v = f(i / 100);
      expect(v).toBeGreaterThanOrEqual(last);
      last = v;
    }
  });
});

describe("parseEasing", () => {
  test("reads the motion tokens", () => {
    expect(parseEasing(EASE)(0.5)).toBeCloseTo(cubicBezier(0.22, 1, 0.36, 1)(0.5), 9);
    expect(parseEasing(EASE_TALL)(0.5)).toBeCloseTo(cubicBezier(0.33, 0, 0.2, 1)(0.5), 9);
    expect(parseEasing("linear")(0.3)).toBeCloseTo(0.3, 9);
  });

  test("refuses anything else", () => {
    expect(() => parseEasing("ease-in-out")).toThrow();
  });
});

describe("timeToReach", () => {
  test("inverts the curve", () => {
    const f = cubicBezier(0.22, 1, 0.36, 1);
    for (const y of [0.1, 0.5, 0.9, 0.99]) {
      const x = timeToReach(f, y);
      expect(f(x)).toBeGreaterThanOrEqual(y);
      expect(f(x - 1e-6)).toBeLessThan(y + 1e-9);
    }
  });

  test("0 when already there, Infinity when never", () => {
    const f = cubicBezier(0.22, 1, 0.36, 1);
    expect(timeToReach(f, 0)).toBe(0);
    expect(timeToReach(f, -1)).toBe(0);
    expect(timeToReach(f, 1.5)).toBe(Infinity);
  });
});
