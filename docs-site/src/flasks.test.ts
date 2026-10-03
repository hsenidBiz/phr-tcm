// The hero's bouncing flasks: the physics (a pure step), then the DOM driver
// that runs it. The physics needs no DOM, so most of it is plain numbers.

import { afterEach, describe, expect, test, vi } from "vitest";
import {
  GRAVITY,
  MAX_DT,
  RESTITUTION,
  createState,
  restState,
  resizeState,
  step,
  type FlaskState,
} from "./render/flasks";

const SIZES = [22, 26, 20, 28, 24, 18];

/** One flask a hair above the ground, falling. */
function falling(over: Partial<FlaskState["flasks"][number]> = {}, width = 600, height = 96): FlaskState {
  const base = restState(width, height, [24]);
  return { ...base, flasks: [{ ...base.flasks[0], x: 100, y: 1, vy: -300, ...over }] };
}

/** A small seeded generator, so a run is repeatable. */
function seeded(seed: number): () => number {
  let s = seed;
  return () => {
    s = (s * 1664525 + 1013904223) % 4294967296;
    return s / 4294967296;
  };
}

describe("step", () => {
  test("a flask falling onto the ground bounces back upward", () => {
    const next = step(falling(), 0.016, () => 0.5);
    const f = next.flasks[0];
    expect(f.y).toBe(0);
    expect(f.vy).toBeGreaterThan(0);
  });

  test("it loses energy on the bounce: it comes back slower than it hit, kick aside", () => {
    const hit = 400;
    const next = step(falling({ y: 0.1, vy: -hit }), 0.016, () => 0);
    expect(next.flasks[0].vy).toBeLessThan(hit);
    expect(next.flasks[0].vy).toBeGreaterThan(0);
    expect(RESTITUTION).toBeGreaterThanOrEqual(0.55);
    expect(RESTITUTION).toBeLessThanOrEqual(0.7);
  });

  test("a landing with a different random draw bounces to a different height", () => {
    const low = step(falling({ vy: -300 }), 0.016, () => 0.05).flasks[0].vy;
    const high = step(falling({ vy: -300 }), 0.016, () => 0.95).flasks[0].vy;
    expect(high).toBeGreaterThan(low);
  });

  test("repeated landings do not settle into one rhythm", () => {
    const rand = seeded(7);
    let state = createState(600, 96, [24], rand);
    const apexes: number[] = [];
    let apex = 0;
    let wasAirborne = false;
    for (let i = 0; i < 4000 && apexes.length < 12; i++) {
      state = step(state, 0.016, rand);
      const y = state.flasks[0].y;
      if (y > 0) {
        wasAirborne = true;
        apex = Math.max(apex, y);
      } else if (wasAirborne) {
        apexes.push(Math.round(apex));
        apex = 0;
        wasAirborne = false;
      }
    }
    expect(apexes.length).toBeGreaterThanOrEqual(8);
    expect(new Set(apexes).size).toBeGreaterThan(4);
  });

  test("a landing tilts the flask and nudges it sideways", () => {
    const before = falling({ vx: 0, vr: 0, rot: 0 });
    const left = step(before, 0.016, () => 0).flasks[0];
    const right = step(before, 0.016, () => 1).flasks[0];
    expect(left.vx).toBeLessThan(0);
    expect(right.vx).toBeGreaterThan(0);
    expect(left.vr).not.toBe(right.vr);
  });

  test("the tilt settles back upright while the flask is in the air", () => {
    let state = falling({ y: 40, vy: 0, vr: 0, rot: 20 });
    for (let i = 0; i < 20; i++) state = step({ ...state, flasks: [{ ...state.flasks[0], y: 40, vy: 0 }] }, 0.016, () => 0.5);
    expect(Math.abs(state.flasks[0].rot)).toBeLessThan(20);
  });

  test("flasks never go below the ground or outside the side edges, over a long run", () => {
    const rand = seeded(42);
    let state = createState(500, 96, SIZES, rand);
    for (let i = 0; i < 6000; i++) {
      // a mix of ordinary frames and stalls
      state = step(state, i % 97 === 0 ? 5 : 0.016, rand);
      for (const f of state.flasks) {
        expect(f.y).toBeGreaterThanOrEqual(0);
        expect(f.y).toBeLessThanOrEqual(96 - f.h + 1e-6);
        expect(f.x).toBeGreaterThanOrEqual(0);
        expect(f.x).toBeLessThanOrEqual(500 - f.w + 1e-6);
      }
    }
  });

  test("a flask bounces off the side edges instead of leaving", () => {
    const left = step(falling({ x: 0.2, y: 30, vy: 0, vx: -80 }), 0.05, () => 0.5).flasks[0];
    expect(left.x).toBe(0);
    expect(left.vx).toBeGreaterThan(0);
    const w = restState(600, 96, [24]).flasks[0].w;
    const right = step(falling({ x: 600 - w - 0.2, y: 30, vy: 0, vx: 80 }), 0.05, () => 0.5).flasks[0];
    expect(right.x).toBe(600 - w);
    expect(right.vx).toBeLessThan(0);
  });

  test("a huge dt is clamped, so a stalled tab cannot tunnel through the ground", () => {
    const state = falling({ y: 30, vy: 0 });
    const huge = step(state, 60, () => 0.5);
    const clamped = step(state, MAX_DT, () => 0.5);
    expect(huge).toEqual(clamped);
    expect(huge.flasks[0].y).toBeGreaterThanOrEqual(0);
    // and the fall is no faster than MAX_DT of gravity allows
    expect(huge.flasks[0].vy).toBeCloseTo(-GRAVITY * MAX_DT, 5);
  });

  test("a negative dt does nothing", () => {
    const state = falling({ y: 30, vy: 0 });
    expect(step(state, -1, () => 0.5)).toEqual(step(state, 0, () => 0.5));
  });

  test("it does not change the state it is given", () => {
    const state = falling();
    const copy = structuredClone(state);
    step(state, 0.016, () => 0.5);
    expect(state).toEqual(copy);
  });

  test("a bounce never reaches the top of the strip", () => {
    let state = falling({ vy: -5000 }, 600, 96);
    state = step(state, 0.016, () => 1);
    for (let i = 0; i < 400; i++) {
      state = step(state, 0.016, () => 1);
      expect(state.flasks[0].y).toBeLessThanOrEqual(96 - state.flasks[0].h + 1e-6);
    }
  });
});

describe("starting states", () => {
  test("rest: every flask on the ground, upright, still, inside the strip, in order", () => {
    const s = restState(600, 96, SIZES);
    expect(s.flasks).toHaveLength(SIZES.length);
    for (const f of s.flasks) {
      expect([f.y, f.rot, f.vx, f.vy, f.vr]).toEqual([0, 0, 0, 0, 0]);
      expect(f.x).toBeGreaterThanOrEqual(0);
      expect(f.x + f.w).toBeLessThanOrEqual(600);
    }
    const xs = s.flasks.map((f) => f.x);
    expect([...xs].sort((a, b) => a - b)).toEqual(xs);
  });

  test("rest in a strip with no width yet is still well formed", () => {
    expect(restState(0, 96, SIZES).flasks.every((f) => f.x === 0)).toBe(true);
  });

  test("the start of a run drops the flasks in from different heights", () => {
    const s = createState(600, 96, SIZES, seeded(3));
    expect(new Set(s.flasks.map((f) => Math.round(f.y))).size).toBeGreaterThan(1);
    for (const f of s.flasks) expect(f.y).toBeLessThanOrEqual(96 - f.h);
  });

  test("a resize keeps the flasks inside the new size", () => {
    const s = createState(900, 96, SIZES, seeded(5));
    const small = resizeState(s, 300, 96);
    for (const f of small.flasks) {
      expect(f.x).toBeGreaterThanOrEqual(0);
      expect(f.x + f.w).toBeLessThanOrEqual(300 + 1e-6);
    }
  });

  test("the first real width after a zero-width start spreads them out", () => {
    const zero = restState(0, 96, SIZES);
    const grown = resizeState(zero, 600, 96);
    expect(new Set(grown.flasks.map((f) => f.x)).size).toBe(SIZES.length);
  });
});

afterEach(() => vi.restoreAllMocks());
