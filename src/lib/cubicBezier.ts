/**
 * CSS `cubic-bezier()` timing functions, evaluated in script - for motion
 * that has to be planned around an animation the browser plays (where a
 * folding box's edge will be at a given moment).
 *
 * The curve runs from (0,0) to (1,1) through control points (x1,y1) and
 * (x2,y2); x is time and y is progress. `x1`/`x2` must lie in [0,1] (as CSS
 * requires), which makes x monotonic, so each time has exactly one point.
 */

export type Easing = (x: number) => number;

const sample = (a: number, b: number, t: number) => {
  // One coordinate of the curve at parameter t: 3(1-t)²t·a + 3(1-t)t²·b + t³.
  const u = 1 - t;
  return 3 * u * u * t * a + 3 * u * t * t * b + t * t * t;
};

const slope = (a: number, b: number, t: number) => {
  const u = 1 - t;
  return 3 * u * u * a + 6 * u * t * (b - a) + 3 * t * t * (1 - b);
};

/** The timing function `cubic-bezier(x1, y1, x2, y2)`: progress at time
 * `x` (both 0..1; clamped outside). */
export function cubicBezier(x1: number, y1: number, x2: number, y2: number): Easing {
  return (x: number) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    // Newton first (fast where the curve is not flat in x), then bisection,
    // which cannot fail because x(t) is monotonic on [0,1].
    let t = x;
    for (let i = 0; i < 8; i++) {
      const err = sample(x1, x2, t) - x;
      if (Math.abs(err) < 1e-7) return sample(y1, y2, t);
      const d = slope(x1, x2, t);
      if (Math.abs(d) < 1e-6) break;
      t -= err / d;
    }
    let lo = 0;
    let hi = 1;
    t = x;
    for (let i = 0; i < 60; i++) {
      const v = sample(x1, x2, t);
      if (Math.abs(v - x) < 1e-7) break;
      if (v < x) lo = t;
      else hi = t;
      t = (lo + hi) / 2;
    }
    return sample(y1, y2, t);
  };
}

/** A `cubic-bezier(...)` string (as the motion tokens are written), or
 * `linear`, as a function. Anything else is a programming error. */
export function parseEasing(css: string): Easing {
  if (css.trim() === "linear") return (x) => Math.min(1, Math.max(0, x));
  const m = css.match(/^\s*cubic-bezier\(\s*([^,]+),\s*([^,]+),\s*([^,]+),\s*([^)]+)\)\s*$/);
  if (!m) throw new Error(`not a cubic-bezier easing: ${css}`);
  const [x1, y1, x2, y2] = m.slice(1).map(Number);
  return cubicBezier(x1, y1, x2, y2);
}

/**
 * The first time (0..1) at which a non-decreasing easing reaches progress
 * `y` - 0 if it starts there, Infinity if it never does. Bisection, so any
 * monotonic curve works, overshoot-free ones included.
 */
export function timeToReach(ease: Easing, y: number): number {
  if (ease(0) >= y) return 0;
  if (ease(1) < y) return Infinity;
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 50; i++) {
    const mid = (lo + hi) / 2;
    if (ease(mid) >= y) hi = mid;
    else lo = mid;
  }
  return hi;
}
