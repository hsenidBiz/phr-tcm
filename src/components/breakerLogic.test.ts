// The brick game's rules (public/vendor/breaker/logic.js). The page loads
// that file as a plain script, so it has no exports: run it here in a bare
// context, exactly as written, and test what it leaves on globalThis.
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";
import { describe, expect, test } from "vitest";

type Brick = { x: number; y: number; w: number; h: number; row: number; hp: number; maxHp: number; points: number };
type Ball = { x: number; y: number; vx: number; vy: number; r: number };
type Kind = "multi" | "aim" | "boom" | "wide" | "slow";
type Drop = { kind: Kind; x: number; y: number; w: number; h: number };
type Game = {
  mode: "serve" | "play" | "paused" | "over";
  level: number;
  score: number;
  lives: number;
  best: number;
  speed: number;
  paddle: { x: number; y: number; w: number; h: number };
  balls: Ball[];
  drops: Drop[];
  effects: { aim: number; wide: number; slow: number };
  boom: number;
  bricks: Brick[];
};
type Ev = { type: string; [k: string]: unknown };
type Input = { left?: boolean; right?: boolean; pointerX?: number | null };
type Logic = {
  W: number;
  H: number;
  TOP: number;
  LIVES: number;
  BEST_CAP: number;
  MAX_BOUNCE: number;
  PADDLE_W: number;
  WIDE_W: number;
  SLOW_SPEED: number;
  DROP_CHANCE: number;
  DROP_SPEED: number;
  KINDS: Kind[];
  EFFECT_TIME: number;
  BOOM_HITS: number;
  MAX_BALLS: number;
  ballSpeed(level: number): number;
  aimPath(g: Game, b: Ball): { x: number; y: number }[];
  activeEffects(g: Game): { kind: Kind; left: number; total: number }[];
  buildLevel(level: number): Brick[];
  circleRectHit(cx: number, cy: number, r: number, rx: number, ry: number, rw: number, rh: number): { nx: number; ny: number; depth: number } | null;
  paddleBounce(ballX: number, paddleX: number, paddleW: number, speed: number): { vx: number; vy: number };
  createGame(best?: unknown): Game;
  validBest(v: unknown): boolean;
  launch(g: Game): void;
  togglePause(g: Game): void;
  update(g: Game, dt: number, input?: Input, rand?: () => number): Ev[];
};

const file = resolve(dirname(fileURLToPath(import.meta.url)), "../../public/vendor/breaker/logic.js");
const source = readFileSync(file, "utf8");

function load(): Logic {
  const ctx: { BreakerLogic?: Logic } = {};
  runInNewContext(source, ctx);
  return ctx.BreakerLogic!;
}
const L = load();

/** Plain JSON copies: the script's objects come from another realm. */
const plain = <T,>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

test("the page itself imports nothing", () => {
  expect(source).not.toMatch(/\bimport\b|\brequire\(|\bexport\b/);
  for (const f of ["game.js", "escape.js"]) {
    const s = readFileSync(resolve(dirname(file), f), "utf8");
    expect(s).not.toMatch(/^\s*(import|export)\b|\brequire\(/m);
  }
});

describe("circleRectHit", () => {
  test("no touch is null", () => {
    expect(L.circleRectHit(0, 0, 5, 10, 10, 20, 20)).toBeNull();
  });

  test("touching from above pushes up", () => {
    const h = plain(L.circleRectHit(15, 7, 5, 10, 10, 20, 20))!;
    expect(h.nx).toBeCloseTo(0);
    expect(h.ny).toBeCloseTo(-1);
    expect(h.depth).toBeCloseTo(2);
  });

  test("a centre inside the rectangle goes out through the nearest side", () => {
    const h = plain(L.circleRectHit(28, 20, 5, 10, 10, 20, 20))!;
    expect(h).toEqual({ nx: 1, ny: 0, depth: 7 });
  });

  test("a corner pushes out diagonally", () => {
    const h = plain(L.circleRectHit(8, 8, 5, 10, 10, 20, 20))!;
    expect(h.nx).toBeCloseTo(-Math.SQRT1_2);
    expect(h.ny).toBeCloseTo(-Math.SQRT1_2);
  });
});

describe("paddleBounce", () => {
  test("the middle sends it straight up, the ends at the steepest angle", () => {
    const mid = plain(L.paddleBounce(50, 0, 100, 300));
    expect(mid.vx).toBeCloseTo(0);
    expect(mid.vy).toBeCloseTo(-300);
    const right = plain(L.paddleBounce(100, 0, 100, 300));
    expect(Math.atan2(right.vx, -right.vy)).toBeCloseTo(L.MAX_BOUNCE);
    const left = plain(L.paddleBounce(-40, 0, 100, 300));
    expect(Math.atan2(left.vx, -left.vy)).toBeCloseTo(-L.MAX_BOUNCE);
    expect(Math.hypot(left.vx, left.vy)).toBeCloseTo(300);
  });
});

describe("levels", () => {
  test("every level has bricks, inside the board, none overlapping", () => {
    for (let level = 1; level <= 12; level++) {
      const bricks = L.buildLevel(level);
      expect(bricks.length).toBeGreaterThan(0);
      for (const b of bricks) {
        expect(b.x).toBeGreaterThanOrEqual(0);
        expect(b.x + b.w).toBeLessThanOrEqual(L.W);
        expect(b.y).toBeGreaterThan(L.TOP);
      }
      for (let i = 0; i < bricks.length; i++) {
        for (let j = i + 1; j < bricks.length; j++) {
          const a = bricks[i];
          const b = bricks[j];
          const overlap = a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
          expect(overlap).toBe(false);
        }
      }
    }
  });

  test("level 1 has no tough bricks; from level 2 some take two hits", () => {
    expect(L.buildLevel(1).every((b) => b.hp === 1)).toBe(true);
    for (let level = 2; level <= 6; level++) {
      expect(L.buildLevel(level).some((b) => b.hp === 2 && b.maxHp === 2)).toBe(true);
    }
  });

  test("each level has a new layout, and the ball keeps the level 1 speed", () => {
    const shape = (level: number) => L.buildLevel(level).map((b) => `${b.x.toFixed(1)},${b.y}`).join(";");
    for (let level = 1; level < 6; level++) {
      expect(shape(level + 1)).not.toBe(shape(level));
    }
    for (let level = 1; level <= 20; level++) expect(L.ballSpeed(level)).toBe(L.ballSpeed(1));
  });
});

/** A rand that never lets a pick-up fall. */
const NO_DROPS = () => 0.99;

/** Runs the game for `seconds` in fixed steps, collecting every event. */
function run(g: Game, seconds: number, input: Input = {}, step = 1 / 120, rand: () => number = NO_DROPS): Ev[] {
  const out: Ev[] = [];
  for (let t = 0; t < seconds; t += step) out.push(...plain(L.update(g, step, input, rand)));
  return out;
}

/** A rand that returns these values in turn, then the last one for ever. */
function seq(...values: number[]): () => number {
  let i = 0;
  return () => values[Math.min(i++, values.length - 1)];
}

/** The rand values that make the next broken brick drop `kind`, then no more. */
function dropOf(kind: Kind): () => number {
  return seq(0, (L.KINDS.indexOf(kind) + 0.5) / L.KINDS.length, 0.99);
}

const brick = (x: number, y: number, extra: Partial<Brick> = {}): Brick => ({ x, y, w: 40, h: 16, row: 0, hp: 1, maxHp: 1, points: 10, ...extra });
/** Out of the way, so a test's hits never clear the board. */
const keeper = () => brick(0, 40, { w: 10, h: 10 });

/** A game in play, the ball going up the middle, a pick-up of `kind` just
 * above the paddle's middle. */
function withDropAbovePaddle(kind: Kind): Game {
  const g = L.createGame();
  g.mode = "play";
  g.bricks = [keeper()];
  Object.assign(g.balls[0], { x: 320, y: 250, vx: 0, vy: -g.speed });
  const p = g.paddle;
  g.drops.push({ kind, x: p.x + p.w / 2 - 19, y: p.y - 20, w: 38, h: 14 });
  return g;
}

/** Holds the paddle where it is (the pointer over its middle). */
const stay = (g: Game): Input => ({ pointerX: g.paddle.x + g.paddle.w / 2 });

describe("a game", () => {
  test("starts with 3 lives at level 1, the ball seated until launched", () => {
    const g = L.createGame(42);
    expect(g.lives).toBe(3);
    expect(g.level).toBe(1);
    expect(g.best).toBe(42);
    expect(g.mode).toBe("serve");
    run(g, 0.5);
    expect(g.mode).toBe("serve");
    expect(g.balls[0].vy).toBe(0);
    L.launch(g);
    expect(g.mode).toBe("play");
    expect(g.balls[0].vy).toBeLessThan(0);
  });

  test("an invalid best to start from is ignored", () => {
    for (const bad of [-1, Number.NaN, 1.5, "7", L.BEST_CAP]) expect(L.createGame(bad).best).toBe(0);
  });

  test("the paddle follows the pointer and the keys, inside the board", () => {
    const g = L.createGame();
    L.update(g, 1 / 120, { pointerX: 100 });
    expect(g.paddle.x + g.paddle.w / 2).toBeCloseTo(100);
    L.update(g, 1 / 120, { pointerX: -500 });
    expect(g.paddle.x).toBe(0);
    run(g, 3, { right: true });
    expect(g.paddle.x + g.paddle.w).toBe(L.W);
  });

  test("a hit brick scores its points; a tough one cracks first", () => {
    const g = L.createGame();
    g.bricks = [{ x: 300, y: 100, w: 40, h: 16, row: 0, hp: 2, maxHp: 2, points: 50 }, { x: 0, y: 40, w: 10, h: 10, row: 0, hp: 1, maxHp: 1, points: 10 }];
    g.mode = "play";
    Object.assign(g.balls[0], { x: 320, y: 140, vx: 0, vy: -300 });
    const first = run(g, 0.2);
    expect(first.map((e) => e.type)).toContain("crack");
    expect(g.bricks[0].hp).toBe(1);
    expect(g.score).toBe(0);
    expect(g.balls[0].vy).toBeGreaterThan(0);
    Object.assign(g.balls[0], { x: 320, y: 140, vx: 0, vy: -300 });
    const second = run(g, 0.2);
    expect(second.map((e) => e.type)).toContain("brick");
    expect(g.bricks[0].hp).toBe(0);
    expect(g.score).toBe(50);
  });

  test("a ball far faster than any level never passes through a brick", () => {
    const g = L.createGame();
    g.bricks = [
      { x: 300, y: 100, w: 40, h: 16, row: 0, hp: 1, maxHp: 1, points: 10 },
      { x: 0, y: 40, w: 10, h: 10, row: 0, hp: 1, maxHp: 1, points: 10 },
    ];
    g.mode = "play";
    g.speed = 50000;
    Object.assign(g.balls[0], { x: 320, y: 300, vx: 0, vy: -50000 });
    // One 1/60 s update: over 800 units, far more than a brick is thick.
    const ev = plain(L.update(g, 1 / 60, {}));
    expect(ev.map((e) => e.type)).toContain("brick");
    expect(g.bricks[0].hp).toBe(0);
  });

  test("the ball bounces off the paddle at an angle set by where it lands", () => {
    const g = L.createGame();
    g.mode = "play";
    const p = g.paddle;
    Object.assign(g.balls[0], { x: p.x + p.w * 0.9, y: p.y - 40, vx: 0, vy: g.speed });
    const ev = run(g, 0.3);
    expect(ev.map((e) => e.type)).toContain("paddle");
    expect(g.balls[0].vx).toBeGreaterThan(0);
  });

  test("losing the ball costs a life; the third loss ends the game with the best kept", () => {
    const g = L.createGame(5);
    g.score = 70;
    for (let life = 3; life >= 1; life--) {
      expect(g.lives).toBe(life);
      L.launch(g);
      Object.assign(g.balls[0], { x: 10, y: L.H - 20, vx: 0, vy: 400 });
      const ev = run(g, 0.5);
      expect(ev.map((e) => e.type)).toContain("lost");
      if (life > 1) expect(g.mode).toBe("serve");
      else expect(ev.find((e) => e.type === "over")).toEqual({ type: "over", score: 70, best: 70 });
    }
    expect(g.mode).toBe("over");
    expect(g.lives).toBe(0);
    run(g, 0.5);
    expect(g.mode).toBe("over");
    L.launch(g);
    expect(g.mode).toBe("serve");
    expect(g.lives).toBe(3);
    expect(g.score).toBe(0);
    expect(g.best).toBe(70);
  });

  test("clearing the board starts the next level at the same ball speed", () => {
    const g = L.createGame();
    g.bricks = [{ x: 300, y: 100, w: 40, h: 16, row: 0, hp: 1, maxHp: 1, points: 10 }];
    g.mode = "play";
    Object.assign(g.balls[0], { x: 320, y: 140, vx: 0, vy: -300 });
    const ev = run(g, 0.3);
    expect(ev.map((e) => e.type)).toContain("cleared");
    expect(g.level).toBe(2);
    expect(g.speed).toBe(L.ballSpeed(1));
    expect(g.mode).toBe("serve");
    expect(g.bricks.length).toBeGreaterThan(1);
    expect(g.score).toBe(10);
    L.launch(g);
    expect(Math.hypot(g.balls[0].vx, g.balls[0].vy)).toBeCloseTo(L.ballSpeed(1));
  });

  test("P pauses and resumes; nothing moves while paused", () => {
    const g = L.createGame();
    L.launch(g);
    L.togglePause(g);
    expect(g.mode).toBe("paused");
    const at = plain(g.balls[0]);
    run(g, 0.5, { right: true });
    expect(plain(g.balls[0])).toEqual(at);
    L.togglePause(g);
    expect(g.mode).toBe("play");
  });

  test("the ball never goes flat after a brick bounce", () => {
    const g = L.createGame();
    g.bricks = [
      { x: 300, y: 100, w: 40, h: 16, row: 0, hp: 1, maxHp: 1, points: 10 },
      { x: 0, y: 40, w: 10, h: 10, row: 0, hp: 1, maxHp: 1, points: 10 },
    ];
    g.mode = "play";
    // Into the brick's side, almost horizontally.
    Object.assign(g.balls[0], { x: 280, y: 108, vx: 300, vy: -1 });
    run(g, 0.1);
    expect(Math.abs(g.balls[0].vy)).toBeGreaterThanOrEqual(g.speed * 0.29);
    expect(Math.hypot(g.balls[0].vx, g.balls[0].vy)).toBeCloseTo(g.speed);
  });
});

/** Exactly n updates of dt each. */
function steps(g: Game, n: number, input: Input = {}, rand: () => number = NO_DROPS, dt = 1 / 120): Ev[] {
  const out: Ev[] = [];
  for (let i = 0; i < n; i++) out.push(...plain(L.update(g, dt, input, rand)));
  return out;
}
const types = (ev: Ev[]) => ev.map((e) => e.type);
const speedOf = (b: Ball) => Math.hypot(b.vx, b.vy);
const effect = (g: Game, kind: Kind) => plain(L.activeEffects(g)).find((e) => e.kind === kind);

describe("pick-ups", () => {
  /** A game in play with one brick the ball is about to break. */
  function aboutToBreak(extra: Partial<Brick> = {}): Game {
    const g = L.createGame();
    g.mode = "play";
    g.bricks = [brick(300, 100, extra), keeper()];
    Object.assign(g.balls[0], { x: 320, y: 140, vx: 0, vy: -g.speed });
    return g;
  }

  test("a broken brick lets one fall only when the chance comes up, and the chance is modest", () => {
    expect(L.DROP_CHANCE).toBeGreaterThan(0);
    expect(L.DROP_CHANCE).toBeLessThanOrEqual(1 / 5);

    const miss = aboutToBreak();
    const missEv = run(miss, 0.2, {}, 1 / 120, () => L.DROP_CHANCE);
    expect(types(missEv)).toContain("brick");
    expect(types(missEv)).not.toContain("drop");
    expect(miss.drops).toHaveLength(0);

    for (const kind of L.KINDS) {
      const g = aboutToBreak();
      const ev = run(g, 0.2, {}, 1 / 120, dropOf(kind));
      expect(ev.find((e) => e.type === "drop")).toEqual({ type: "drop", kind });
      expect(g.drops).toHaveLength(1);
      expect(g.drops[0].kind).toBe(kind);
      // From the brick's middle.
      expect(g.drops[0].x + g.drops[0].w / 2).toBeCloseTo(320);
    }
  });

  test("a brick that only cracks lets nothing fall", () => {
    const g = aboutToBreak({ hp: 2, maxHp: 2 });
    const ev = run(g, 0.2, {}, 1 / 120, () => 0);
    expect(types(ev)).toContain("crack");
    expect(g.drops).toHaveLength(0);
  });

  test("a pick-up falls straight down at a steady speed", () => {
    const g = L.createGame();
    g.mode = "play";
    g.bricks = [keeper()];
    Object.assign(g.balls[0], { x: 320, y: 250, vx: 0, vy: -g.speed });
    g.drops.push({ kind: "aim", x: 500, y: 100, w: 38, h: 14 });
    steps(g, 60);
    expect(g.drops[0].x).toBe(500);
    expect(g.drops[0].y).toBeCloseTo(100 + L.DROP_SPEED * 0.5);
    steps(g, 60);
    expect(g.drops[0].y).toBeCloseTo(100 + L.DROP_SPEED);
  });

  test("one the paddle misses falls away and does nothing", () => {
    const g = withDropAbovePaddle("wide");
    g.drops[0].x = 20;
    g.drops[0].y = L.H - 5;
    const ev = run(g, 0.2, stay(g));
    expect(ev.find((e) => e.type === "missed")).toEqual({ type: "missed", kind: "wide" });
    expect(g.drops).toHaveLength(0);
    expect(g.paddle.w).toBe(L.PADDLE_W);
    expect(plain(L.activeEffects(g))).toEqual([]);
  });

  test("multi-ball: caught, two more balls leave from the ball, angled apart", () => {
    const g = withDropAbovePaddle("multi");
    g.drops[0].y = g.paddle.y - g.drops[0].h - 0.5; // caught on the next update
    const ev = steps(g, 1, stay(g));
    expect(ev.find((e) => e.type === "pickup")).toEqual({ type: "pickup", kind: "multi" });
    expect(g.balls).toHaveLength(3);
    for (const b of g.balls) {
      expect(speedOf(b)).toBeCloseTo(g.speed);
      expect(Math.hypot(b.x - g.balls[0].x, b.y - g.balls[0].y)).toBeLessThan(4);
    }
    const sides = g.balls.slice(1).map((b) => Math.sign(b.vx)).sort();
    expect(sides).toEqual([-1, 1]);
    expect(g.balls[0].vx).toBeCloseTo(0);
  });

  test("with several balls, a lost ball costs a life only when it was the last one", () => {
    const g = L.createGame();
    g.mode = "play";
    g.bricks = [keeper()];
    g.balls = [
      { x: 100, y: L.H - 10, vx: 0, vy: 300, r: 6 },
      { x: 200, y: L.H - 10, vx: 0, vy: 300, r: 6 },
      { x: 500, y: 150, vx: 0, vy: -300, r: 6 },
    ];
    const first = run(g, 0.2);
    expect(types(first)).not.toContain("lost");
    expect(g.lives).toBe(3);
    expect(g.mode).toBe("play");
    expect(g.balls).toHaveLength(1);

    // Pick-ups on and falling when the last ball goes: the new life starts clean.
    g.effects.aim = 5;
    g.effects.wide = 5;
    g.paddle.w = L.WIDE_W;
    g.boom = 2;
    g.drops.push({ kind: "slow", x: 20, y: 100, w: 38, h: 14 });
    Object.assign(g.balls[0], { x: 500, y: L.H - 10, vx: 0, vy: 300 });
    const last = run(g, 0.2);
    expect(last.find((e) => e.type === "lost")).toEqual({ type: "lost", lives: 2 });
    expect(g.lives).toBe(2);
    expect(g.mode).toBe("serve");
    expect(g.balls).toHaveLength(1);
    expect(g.drops).toHaveLength(0);
    expect(g.boom).toBe(0);
    expect(plain(g.effects)).toEqual({ aim: 0, wide: 0, slow: 0 });
    expect(g.paddle.w).toBe(L.PADDLE_W);
    expect(g.speed).toBe(L.ballSpeed(1));
  });

  test("aim line: on for a while once caught, then off", () => {
    const g = withDropAbovePaddle("aim");
    const ev = run(g, 0.2, stay(g));
    expect(types(ev)).toContain("pickup");
    expect(effect(g, "aim")!.left).toBeGreaterThan(L.EFFECT_TIME - 0.3);
    expect(effect(g, "aim")!.total).toBe(L.EFFECT_TIME);
    const later = run(g, L.EFFECT_TIME, stay(g));
    expect(later.find((e) => e.type === "expired")).toEqual({ type: "expired", kind: "aim" });
    expect(effect(g, "aim")).toBeUndefined();
    expect(g.effects.aim).toBe(0);
  });

  test("the aim line follows the ball to a wall, bounces once, and goes on", () => {
    const g = L.createGame();
    g.mode = "play";
    g.bricks = [keeper()];
    const r = g.balls[0].r;
    Object.assign(g.balls[0], { x: 320, y: 300, vx: 200, vy: -200 });
    const pts = plain(L.aimPath(g, g.balls[0]));
    expect(pts).toHaveLength(3);
    expect(pts[0]).toEqual({ x: 320, y: 300 });
    // Up and right at 45 degrees: the top first, then the right side.
    expect(pts[1].y).toBeCloseTo(L.TOP + r);
    expect(pts[1].x).toBeCloseTo(320 + (300 - L.TOP - r));
    expect(pts[2].x).toBeCloseTo(L.W - r);
    expect(pts[2].y - pts[1].y).toBeCloseTo(pts[2].x - pts[1].x);

    // Down toward the paddle: it ends on the paddle's line.
    Object.assign(g.balls[0], { x: 320, y: 200, vx: 0, vy: 300 });
    const down = plain(L.aimPath(g, g.balls[0]));
    expect(down[down.length - 1].y).toBeCloseTo(g.paddle.y - r);

    // A ball at rest has no line.
    Object.assign(g.balls[0], { vx: 0, vy: 0 });
    expect(plain(L.aimPath(g, g.balls[0]))).toEqual([]);
  });

  test("the aim line stops at the first brick in the way", () => {
    const g = L.createGame();
    g.mode = "play";
    const r = g.balls[0].r;
    g.bricks = [brick(440, 150), keeper()];
    Object.assign(g.balls[0], { x: 320, y: 300, vx: 200, vy: -200 });
    const pts = plain(L.aimPath(g, g.balls[0]));
    expect(pts).toHaveLength(2);
    const end = pts[1];
    expect(end.x).toBeGreaterThanOrEqual(440 - r - 0.01);
    expect(end.y).toBeLessThanOrEqual(150 + 16 + r + 0.01);
    expect(end.y).toBeGreaterThan(150 - r);
  });

  test("wider paddle: about the same middle, the time shown, caught again it starts over, then back", () => {
    const g = withDropAbovePaddle("wide");
    const mid = g.paddle.x + g.paddle.w / 2;
    const input = stay(g);
    run(g, 0.2, input);
    expect(g.paddle.w).toBe(L.WIDE_W);
    expect(g.paddle.x + g.paddle.w / 2).toBeCloseTo(mid);

    steps(g, 360, input); // 3 s
    expect(effect(g, "wide")!.left).toBeCloseTo(L.EFFECT_TIME - 0.2 - 3, 0);
    const p = g.paddle;
    g.drops.push({ kind: "wide", x: p.x + p.w / 2 - 19, y: p.y - 20, w: 38, h: 14 });
    run(g, 0.2, input);
    expect(effect(g, "wide")!.left).toBeGreaterThan(L.EFFECT_TIME - 0.3);

    const ev = run(g, L.EFFECT_TIME, input);
    expect(types(ev)).toContain("expired");
    expect(g.paddle.w).toBe(L.PADDLE_W);
    expect(g.paddle.x + g.paddle.w / 2).toBeCloseTo(mid);
  });

  test("slower ball: every ball slows for a while, then goes back to the level 1 speed", () => {
    const g = withDropAbovePaddle("slow");
    const input = stay(g);
    run(g, 0.2, input);
    expect(g.speed).toBe(L.SLOW_SPEED);
    expect(L.SLOW_SPEED).toBeLessThan(L.ballSpeed(1));
    expect(speedOf(g.balls[0])).toBeCloseTo(L.SLOW_SPEED);
    run(g, L.EFFECT_TIME / 2, input);
    expect(speedOf(g.balls[0])).toBeCloseTo(L.SLOW_SPEED);
    const ev = run(g, L.EFFECT_TIME / 2 + 0.2, input);
    expect(ev.find((e) => e.type === "expired")).toEqual({ type: "expired", kind: "slow" });
    expect(g.speed).toBe(L.ballSpeed(1));
    expect(speedOf(g.balls[0])).toBeCloseTo(L.ballSpeed(1));
  });

  test("explosive ball: caught, the next hits also break every brick touching the one hit", () => {
    const caught = withDropAbovePaddle("boom");
    run(caught, 0.2, stay(caught));
    expect(caught.boom).toBe(L.BOOM_HITS);
    expect(effect(caught, "boom")).toEqual({ kind: "boom", left: L.BOOM_HITS, total: L.BOOM_HITS });

    const g = L.createGame();
    g.mode = "play";
    g.boom = 2;
    // A 3 x 3 block laid out like a level (40 wide, 4 apart; 20 between rows),
    // a tough one in it, and one well away.
    const grid: Brick[] = [];
    for (const y of [76, 96, 116]) for (const x of [256, 300, 344]) grid.push(brick(x, y));
    grid[4].hp = grid[4].maxHp = 2; // the middle one
    const away = brick(500, 76);
    g.bricks = [...grid, away, keeper()];
    // Up into the bottom middle brick.
    Object.assign(g.balls[0], { x: 320, y: 160, vx: 0, vy: -g.speed });
    const ev = run(g, 0.12);
    expect(types(ev)).toContain("blast");
    const broken = g.bricks.filter((b) => b.hp <= 0).map((b) => `${b.x},${b.y}`).sort();
    expect(broken).toEqual(["256,116", "256,96", "300,116", "300,96", "344,116", "344,96"].sort());
    expect(away.hp).toBe(1);
    expect(g.score).toBe(60);
    expect(g.boom).toBe(1);
    expect(g.balls[0].vy).toBeGreaterThan(0);

    // The last explosive hit, then an ordinary one: only that brick goes.
    g.bricks = [brick(100, 200), brick(144, 200), brick(400, 200), brick(444, 200), keeper()];
    Object.assign(g.balls[0], { x: 120, y: 250, vx: 0, vy: -g.speed });
    run(g, 0.12);
    expect(g.boom).toBe(0);
    expect(g.bricks[0].hp).toBe(0);
    expect(g.bricks[1].hp).toBe(0);
    Object.assign(g.balls[0], { x: 420, y: 250, vx: 0, vy: -g.speed });
    run(g, 0.12);
    expect(g.bricks[2].hp).toBe(0);
    expect(g.bricks[3].hp).toBe(1);
  });

  test("nothing falls or runs down while paused", () => {
    const g = withDropAbovePaddle("aim");
    g.effects.aim = 4;
    g.drops[0].x = 20;
    const y = g.drops[0].y;
    L.togglePause(g);
    run(g, 1);
    expect(g.effects.aim).toBe(4);
    expect(g.drops[0].y).toBe(y);
    L.togglePause(g);
    run(g, 0.1);
    expect(g.effects.aim).toBeLessThan(4);
    expect(g.drops[0].y).toBeGreaterThan(y);
  });

  test("a cleared level starts clean: one ball, nothing falling, nothing on", () => {
    const g = L.createGame();
    g.mode = "play";
    g.bricks = [brick(300, 100)];
    g.effects.slow = 5;
    g.speed = L.SLOW_SPEED;
    g.effects.wide = 5;
    g.paddle.w = L.WIDE_W;
    g.boom = 0;
    g.drops.push({ kind: "multi", x: 20, y: 100, w: 38, h: 14 });
    Object.assign(g.balls[0], { x: 320, y: 140, vx: 0, vy: -L.SLOW_SPEED });
    g.balls.push({ x: 600, y: 300, vx: 0, vy: -L.SLOW_SPEED, r: 6 });
    const ev = run(g, 0.3);
    expect(types(ev)).toContain("cleared");
    expect(g.level).toBe(2);
    expect(g.balls).toHaveLength(1);
    expect(g.drops).toHaveLength(0);
    expect(plain(g.effects)).toEqual({ aim: 0, wide: 0, slow: 0 });
    expect(g.paddle.w).toBe(L.PADDLE_W);
    expect(g.speed).toBe(L.ballSpeed(1));
  });

  test("multi-ball never goes past the most balls allowed", () => {
    const g = withDropAbovePaddle("multi");
    const input = stay(g);
    for (let i = 0; i < 6; i++) {
      const p = g.paddle;
      g.drops = [{ kind: "multi", x: p.x + p.w / 2 - 19, y: p.y - 15, w: 38, h: 14 }];
      steps(g, 2, input);
    }
    expect(g.balls.length).toBeLessThanOrEqual(L.MAX_BALLS);
    expect(g.balls.length).toBe(L.MAX_BALLS);
  });
});
