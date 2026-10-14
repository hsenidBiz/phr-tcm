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
type Game = {
  mode: "serve" | "play" | "paused" | "over";
  level: number;
  score: number;
  lives: number;
  best: number;
  speed: number;
  paddle: { x: number; y: number; w: number; h: number };
  ball: Ball;
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
  ballSpeed(level: number): number;
  buildLevel(level: number): Brick[];
  circleRectHit(cx: number, cy: number, r: number, rx: number, ry: number, rw: number, rh: number): { nx: number; ny: number; depth: number } | null;
  paddleBounce(ballX: number, paddleX: number, paddleW: number, speed: number): { vx: number; vy: number };
  createGame(best?: unknown): Game;
  validBest(v: unknown): boolean;
  launch(g: Game): void;
  togglePause(g: Game): void;
  update(g: Game, dt: number, input?: Input): Ev[];
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

  test("each level has a new layout and a faster ball", () => {
    const shape = (level: number) => L.buildLevel(level).map((b) => `${b.x.toFixed(1)},${b.y}`).join(";");
    for (let level = 1; level < 6; level++) {
      expect(shape(level + 1)).not.toBe(shape(level));
      expect(L.ballSpeed(level + 1)).toBeGreaterThan(L.ballSpeed(level));
    }
  });
});

/** Runs the game for `seconds` in fixed steps, collecting every event. */
function run(g: Game, seconds: number, input: Input = {}, step = 1 / 120): Ev[] {
  const out: Ev[] = [];
  for (let t = 0; t < seconds; t += step) out.push(...plain(L.update(g, step, input)));
  return out;
}

describe("a game", () => {
  test("starts with 3 lives at level 1, the ball seated until launched", () => {
    const g = L.createGame(42);
    expect(g.lives).toBe(3);
    expect(g.level).toBe(1);
    expect(g.best).toBe(42);
    expect(g.mode).toBe("serve");
    run(g, 0.5);
    expect(g.mode).toBe("serve");
    expect(g.ball.vy).toBe(0);
    L.launch(g);
    expect(g.mode).toBe("play");
    expect(g.ball.vy).toBeLessThan(0);
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
    Object.assign(g.ball, { x: 320, y: 140, vx: 0, vy: -300 });
    const first = run(g, 0.2);
    expect(first.map((e) => e.type)).toContain("crack");
    expect(g.bricks[0].hp).toBe(1);
    expect(g.score).toBe(0);
    expect(g.ball.vy).toBeGreaterThan(0);
    Object.assign(g.ball, { x: 320, y: 140, vx: 0, vy: -300 });
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
    Object.assign(g.ball, { x: 320, y: 300, vx: 0, vy: -50000 });
    // One 1/60 s update: over 800 units, far more than a brick is thick.
    const ev = plain(L.update(g, 1 / 60, {}));
    expect(ev.map((e) => e.type)).toContain("brick");
    expect(g.bricks[0].hp).toBe(0);
  });

  test("the ball bounces off the paddle at an angle set by where it lands", () => {
    const g = L.createGame();
    g.mode = "play";
    const p = g.paddle;
    Object.assign(g.ball, { x: p.x + p.w * 0.9, y: p.y - 40, vx: 0, vy: g.speed });
    const ev = run(g, 0.3);
    expect(ev.map((e) => e.type)).toContain("paddle");
    expect(g.ball.vx).toBeGreaterThan(0);
  });

  test("losing the ball costs a life; the third loss ends the game with the best kept", () => {
    const g = L.createGame(5);
    g.score = 70;
    for (let life = 3; life >= 1; life--) {
      expect(g.lives).toBe(life);
      L.launch(g);
      Object.assign(g.ball, { x: 10, y: L.H - 20, vx: 0, vy: 400 });
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

  test("clearing the board starts the next level with a faster ball", () => {
    const g = L.createGame();
    g.bricks = [{ x: 300, y: 100, w: 40, h: 16, row: 0, hp: 1, maxHp: 1, points: 10 }];
    g.mode = "play";
    Object.assign(g.ball, { x: 320, y: 140, vx: 0, vy: -300 });
    const ev = run(g, 0.3);
    expect(ev.map((e) => e.type)).toContain("cleared");
    expect(g.level).toBe(2);
    expect(g.speed).toBe(L.ballSpeed(2));
    expect(g.speed).toBeGreaterThan(L.ballSpeed(1));
    expect(g.mode).toBe("serve");
    expect(g.bricks.length).toBeGreaterThan(1);
    expect(g.score).toBe(10);
  });

  test("P pauses and resumes; nothing moves while paused", () => {
    const g = L.createGame();
    L.launch(g);
    L.togglePause(g);
    expect(g.mode).toBe("paused");
    const at = plain(g.ball);
    run(g, 0.5, { right: true });
    expect(plain(g.ball)).toEqual(at);
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
    Object.assign(g.ball, { x: 280, y: 108, vx: 300, vy: -1 });
    run(g, 0.1);
    expect(Math.abs(g.ball.vy)).toBeGreaterThanOrEqual(g.speed * 0.29);
    expect(Math.hypot(g.ball.vx, g.ball.vy)).toBeCloseTo(g.speed);
  });
});
