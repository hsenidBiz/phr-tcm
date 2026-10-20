// The brick game's rules: the board, the ball, the bricks and the score.
// No drawing, no DOM, no input devices - game.js owns those and calls in.
// Written for the app; nothing here is third-party.
//
// A plain script, not a module: the page loads it with a <script> tag and
// finds it at globalThis.BreakerLogic. src/components/breakerLogic.test.ts
// runs this same file in a bare context and tests it the same way.
(function (root) {
  "use strict";

  /** The world, in its own units. game.js scales it to fit the canvas. */
  var W = 640;
  var H = 400;
  /** The top bar (score, lives, level) sits above the play area. */
  var TOP = 32;

  var COLS = 10;
  var SIDE = 20;
  var GAP = 4;
  var BRICK_H = 16;
  var BRICKS_TOP = TOP + 30;
  var BRICK_W = (W - 2 * SIDE - (COLS - 1) * GAP) / COLS;

  var PADDLE_W = 84;
  var PADDLE_H = 10;
  var PADDLE_Y = H - 28;
  var PADDLE_KEY_SPEED = 560; // units per second
  var BALL_R = 6;
  var LIVES = 3;
  /** The ball's speed: the level 1 speed, on every level. */
  var BALL_SPEED = 300;
  /** The steepest a paddle bounce leaves, from straight up. */
  var MAX_BOUNCE = (60 * Math.PI) / 180;
  /** A ball never travels flatter than this share of its speed vertically,
   * so it cannot end up sliding sideways for ever. */
  var MIN_VY_SHARE = 0.3;
  /** Highest best score either side accepts, matching the app's cap. */
  var BEST_CAP = 10000000;

  // ---- Pick-ups. A broken brick sometimes lets one fall; the paddle
  // catches it and its effect starts. One that reaches the bottom is gone.
  /** The chance a brick the ball breaks lets a pick-up fall. */
  var DROP_CHANCE = 1 / 8;
  var DROP_W = 38;
  var DROP_H = 14;
  var DROP_SPEED = 140; // units per second, straight down
  /** multi: two more balls. aim: the ball's path shown ahead of it.
   * boom: the next hits also break the bricks touching the one hit.
   * wide: a wider paddle. slow: a slower ball. */
  var KINDS = ["multi", "aim", "boom", "wide", "slow"];
  /** The pick-ups that last a while, and how long (seconds). Catching one
   * that is already on starts its time again. */
  var TIMED = ["aim", "wide", "slow"];
  var EFFECT_TIME = 10;
  /** How many brick hits an explosive ball lasts. */
  var BOOM_HITS = 3;
  var WIDE_W = 126;
  var SLOW_SPEED = 200;
  /** The two extra balls leave this far either side of the ball's way. */
  var SPLIT = (25 * Math.PI) / 180;
  var MAX_BALLS = 9;

  function clamp(v, lo, hi) {
    return v < lo ? lo : v > hi ? hi : v;
  }

  /** The ball's speed on a level: the same on every level. */
  function ballSpeed() {
    return BALL_SPEED;
  }

  /** How many rows a level has. */
  function rowsFor(level) {
    return Math.min(4 + level, 8);
  }

  /** The layouts, taken in turn. Each says whether (row, col) has a brick. */
  var PATTERNS = [
    function full() {
      return true;
    },
    function checker(r, c) {
      return (r + c) % 2 === 0 || r === 0;
    },
    function pyramid(r, c, rows) {
      var inset = rows - 1 - r;
      return c >= Math.floor(inset / 2) && c < COLS - Math.floor(inset / 2);
    },
    function columns(r, c) {
      return c % 3 !== 2;
    },
    function diamond(r, c, rows) {
      var mid = (COLS - 1) / 2;
      var half = (rows - 1) / 2;
      return Math.abs(c - mid) / (mid + 1) + Math.abs(r - half) / (half + 1) <= 1.05;
    },
  ];

  /** A level's bricks. From level 2 the top rows are tougher (two hits). */
  function buildLevel(level) {
    var rows = rowsFor(level);
    var pattern = PATTERNS[(level - 1) % PATTERNS.length];
    var toughRows = level >= 2 ? Math.min(level - 1, 3) : 0;
    var bricks = [];
    for (var r = 0; r < rows; r++) {
      for (var c = 0; c < COLS; c++) {
        if (!pattern(r, c, rows)) continue;
        var hp = r < toughRows ? 2 : 1;
        bricks.push({
          x: SIDE + c * (BRICK_W + GAP),
          y: BRICKS_TOP + r * (BRICK_H + GAP),
          w: BRICK_W,
          h: BRICK_H,
          row: r,
          hp: hp,
          maxHp: hp,
          points: (10 + 5 * (rows - 1 - r)) * hp,
        });
      }
    }
    return bricks;
  }

  /**
   * Where a circle overlaps a rectangle: the push-out direction (a unit
   * normal pointing from the rectangle to the circle) and how far, or null
   * when they do not touch. A centre already inside the rectangle is pushed
   * out through the nearest side.
   */
  function circleRectHit(cx, cy, r, rx, ry, rw, rh) {
    var qx = clamp(cx, rx, rx + rw);
    var qy = clamp(cy, ry, ry + rh);
    var dx = cx - qx;
    var dy = cy - qy;
    var d2 = dx * dx + dy * dy;
    if (d2 > r * r) return null;
    if (d2 > 1e-12) {
      var d = Math.sqrt(d2);
      return { nx: dx / d, ny: dy / d, depth: r - d };
    }
    var left = cx - rx;
    var right = rx + rw - cx;
    var top = cy - ry;
    var bottom = ry + rh - cy;
    var m = Math.min(left, right, top, bottom);
    if (m === top) return { nx: 0, ny: -1, depth: top + r };
    if (m === bottom) return { nx: 0, ny: 1, depth: bottom + r };
    if (m === left) return { nx: -1, ny: 0, depth: left + r };
    return { nx: 1, ny: 0, depth: right + r };
  }

  /** A velocity of the given speed, never flatter than MIN_VY_SHARE. */
  function steady(vx, vy, speed) {
    var len = Math.sqrt(vx * vx + vy * vy) || 1;
    vx = (vx / len) * speed;
    vy = (vy / len) * speed;
    var minVy = speed * MIN_VY_SHARE;
    if (Math.abs(vy) < minVy) {
      vy = vy < 0 ? -minVy : minVy;
      var sx = vx < 0 ? -1 : 1;
      vx = sx * Math.sqrt(speed * speed - vy * vy);
    }
    return { vx: vx, vy: vy };
  }

  /**
   * The ball's velocity leaving the paddle: straight up from the middle,
   * up to MAX_BOUNCE either side at the ends.
   */
  function paddleBounce(ballX, paddleX, paddleW, speed) {
    var offset = clamp((ballX - (paddleX + paddleW / 2)) / (paddleW / 2), -1, 1);
    var a = offset * MAX_BOUNCE;
    return { vx: speed * Math.sin(a), vy: -speed * Math.cos(a) };
  }

  function newPaddle() {
    return { x: (W - PADDLE_W) / 2, y: PADDLE_Y, w: PADDLE_W, h: PADDLE_H };
  }

  function newBall() {
    return { x: 0, y: 0, vx: 0, vy: 0, r: BALL_R };
  }

  /** The (first) ball, at rest on the middle of the paddle. */
  function seatBall(g) {
    var b = g.balls[0];
    b.x = g.paddle.x + g.paddle.w / 2;
    b.y = g.paddle.y - b.r - 0.5;
    b.vx = 0;
    b.vy = 0;
  }

  function noEffects() {
    return { aim: 0, wide: 0, slow: 0 };
  }

  /**
   * A new life or a new level: one ball, the usual paddle and speed, no
   * pick-ups falling and none in effect.
   */
  function freshStart(g) {
    g.speed = ballSpeed(g.level);
    g.paddle = newPaddle();
    g.balls = [newBall()];
    g.drops = [];
    g.effects = noEffects();
    g.boom = 0;
    seatBall(g);
  }

  /** A fresh game at level 1, keeping a best score already known. */
  function createGame(best) {
    var g = {
      mode: "serve", // serve | play | paused | over
      resume: "serve",
      level: 1,
      score: 0,
      lives: LIVES,
      best: validBest(best) ? best : 0,
      speed: ballSpeed(1),
      paddle: newPaddle(),
      balls: [newBall()],
      /** Pick-ups on their way down. */
      drops: [],
      /** Seconds left of each timed pick-up (0: off). */
      effects: noEffects(),
      /** Brick hits left of an explosive ball (0: off). */
      boom: 0,
      bricks: buildLevel(1),
      banner: "Level 1",
    };
    freshStart(g);
    return g;
  }

  function validBest(v) {
    return typeof v === "number" && Number.isInteger(v) && v >= 0 && v < BEST_CAP;
  }

  /** Space or a click: launch a seated ball, or start over after a loss. */
  function launch(g) {
    if (g.mode === "serve") {
      var v = paddleBounce(g.paddle.x + g.paddle.w * (g.level % 2 ? 0.62 : 0.38), g.paddle.x, g.paddle.w, g.speed);
      g.balls[0].vx = v.vx;
      g.balls[0].vy = v.vy;
      g.mode = "play";
      g.banner = "";
    } else if (g.mode === "over") {
      var best = g.best;
      var fresh = createGame(best);
      for (var k in fresh) g[k] = fresh[k];
    }
  }

  /** P: pause a game in progress, or carry on. */
  function togglePause(g) {
    if (g.mode === "paused") {
      g.mode = g.resume;
    } else if (g.mode === "play" || g.mode === "serve") {
      g.resume = g.mode;
      g.mode = "paused";
    }
  }

  /** The paddle, from the keys held or the pointer's last place. */
  function movePaddle(g, dt, input) {
    var p = g.paddle;
    var dir = (input.right ? 1 : 0) - (input.left ? 1 : 0);
    if (dir !== 0) {
      p.x += dir * PADDLE_KEY_SPEED * dt;
    } else if (typeof input.pointerX === "number" && isFinite(input.pointerX)) {
      p.x = input.pointerX - p.w / 2;
    }
    p.x = clamp(p.x, 0, W - p.w);
  }

  /** A velocity turned by angle a (radians). */
  function turn(vx, vy, a) {
    var c = Math.cos(a);
    var s = Math.sin(a);
    return { vx: vx * c - vy * s, vy: vx * s + vy * c };
  }

  /** Every ball in flight at the given speed, each keeping its way. */
  function setSpeed(g, speed) {
    g.speed = speed;
    for (var i = 0; i < g.balls.length; i++) {
      var b = g.balls[i];
      if (b.vx === 0 && b.vy === 0) continue;
      var s = steady(b.vx, b.vy, speed);
      b.vx = s.vx;
      b.vy = s.vy;
    }
  }

  /** The paddle at width w, about the same middle, inside the board. */
  function setPaddleWidth(g, w) {
    var p = g.paddle;
    var mid = p.x + p.w / 2;
    p.w = w;
    p.x = clamp(mid - w / 2, 0, W - w);
  }

  /** Two more balls from where the first ball is, angled apart. */
  function addBalls(g) {
    var from = g.balls[0];
    if (!from) return;
    var turns = [-SPLIT, SPLIT];
    for (var i = 0; i < turns.length && g.balls.length < MAX_BALLS; i++) {
      var v = turn(from.vx, from.vy, turns[i]);
      var s = steady(v.vx, v.vy, g.speed);
      g.balls.push({ x: from.x, y: from.y, vx: s.vx, vy: s.vy, r: from.r });
    }
  }

  /** A caught pick-up takes effect. A timed one already on starts again. */
  function applyPickup(g, kind) {
    if (kind === "multi") {
      addBalls(g);
    } else if (kind === "boom") {
      g.boom = BOOM_HITS;
    } else if (kind === "aim") {
      g.effects.aim = EFFECT_TIME;
    } else if (kind === "wide") {
      g.effects.wide = EFFECT_TIME;
      setPaddleWidth(g, WIDE_W);
    } else if (kind === "slow") {
      g.effects.slow = EFFECT_TIME;
      setSpeed(g, SLOW_SPEED);
    }
  }

  /** The timed pick-ups run down; one that runs out puts things back. */
  function tickEffects(g, dt, events) {
    for (var i = 0; i < TIMED.length; i++) {
      var kind = TIMED[i];
      if (g.effects[kind] <= 0) continue;
      g.effects[kind] = Math.max(0, g.effects[kind] - dt);
      if (g.effects[kind] > 0) continue;
      if (kind === "wide") setPaddleWidth(g, PADDLE_W);
      if (kind === "slow") setSpeed(g, ballSpeed(g.level));
      events.push({ type: "expired", kind: kind });
    }
  }

  /** What is on now, for the indicator: each timed pick-up with the
   * seconds it has left, then an explosive ball with the hits it has left. */
  function activeEffects(g) {
    var out = [];
    for (var i = 0; i < TIMED.length; i++) {
      var kind = TIMED[i];
      if (g.effects[kind] > 0) out.push({ kind: kind, left: g.effects[kind], total: EFFECT_TIME });
    }
    if (g.boom > 0) out.push({ kind: "boom", left: g.boom, total: BOOM_HITS });
    return out;
  }

  /** Sometimes, a pick-up starts to fall from a broken brick. */
  function maybeDrop(g, k, rand, events) {
    if (!(rand() < DROP_CHANCE)) return;
    var kind = KINDS[Math.min(KINDS.length - 1, Math.floor(rand() * KINDS.length))];
    g.drops.push({ kind: kind, x: k.x + k.w / 2 - DROP_W / 2, y: k.y, w: DROP_W, h: DROP_H });
    events.push({ type: "drop", kind: kind });
  }

  /** The falling pick-ups move down; the paddle catches any it meets. */
  function moveDrops(g, dt, events) {
    var p = g.paddle;
    for (var i = g.drops.length - 1; i >= 0; i--) {
      var d = g.drops[i];
      var top0 = d.y;
      d.y += DROP_SPEED * dt;
      var overX = d.x < p.x + p.w && d.x + d.w > p.x;
      // Swept: it met the paddle anywhere along this move.
      var overY = d.y + d.h >= p.y && top0 <= p.y + p.h;
      if (overX && overY) {
        g.drops.splice(i, 1);
        applyPickup(g, d.kind);
        events.push({ type: "pickup", kind: d.kind });
      } else if (d.y > H) {
        g.drops.splice(i, 1);
        events.push({ type: "missed", kind: d.kind });
      }
    }
  }

  /** Two bricks side by side or corner to corner (only the gap between). */
  function touching(a, b) {
    var reach = GAP + 0.5;
    return (
      a !== b &&
      a.x <= b.x + b.w + reach &&
      b.x <= a.x + a.w + reach &&
      a.y <= b.y + b.h + reach &&
      b.y <= a.y + a.h + reach
    );
  }

  /** A brick broken: its points, and maybe a pick-up. */
  function breakBrick(g, k, events, rand, mayDrop) {
    k.hp = 0;
    g.score += k.points;
    events.push({ type: "brick", brick: k });
    if (mayDrop) maybeDrop(g, k, rand, events);
  }

  /** One short move of a ball, with every bounce it makes. */
  function substep(g, b, dt, events, rand) {
    b.x += b.vx * dt;
    b.y += b.vy * dt;

    if (b.x - b.r < 0) {
      b.x = b.r;
      b.vx = Math.abs(b.vx);
    } else if (b.x + b.r > W) {
      b.x = W - b.r;
      b.vx = -Math.abs(b.vx);
    }
    if (b.y - b.r < TOP) {
      b.y = TOP + b.r;
      b.vy = Math.abs(b.vy);
    }

    var p = g.paddle;
    if (b.vy > 0) {
      var ph = circleRectHit(b.x, b.y, b.r, p.x, p.y, p.w, p.h);
      if (ph && b.y < p.y + p.h / 2) {
        b.y = p.y - b.r;
        var v = paddleBounce(b.x, p.x, p.w, g.speed);
        b.vx = v.vx;
        b.vy = v.vy;
        events.push({ type: "paddle" });
      }
    }

    // The brick overlapped most deeply is the one hit; one per substep.
    var hit = null;
    var hitBrick = null;
    for (var i = 0; i < g.bricks.length; i++) {
      var k = g.bricks[i];
      if (k.hp <= 0) continue;
      var h = circleRectHit(b.x, b.y, b.r, k.x, k.y, k.w, k.h);
      if (h && (!hit || h.depth > hit.depth)) {
        hit = h;
        hitBrick = k;
      }
    }
    if (hit) {
      b.x += hit.nx * hit.depth;
      b.y += hit.ny * hit.depth;
      // Already leaving it (a graze on the way out): pushed clear, no hit.
      var dot = b.vx * hit.nx + b.vy * hit.ny;
      if (dot >= 0) return;
      b.vx -= 2 * dot * hit.nx;
      b.vy -= 2 * dot * hit.ny;
      var s = steady(b.vx, b.vy, g.speed);
      b.vx = s.vx;
      b.vy = s.vy;
      if (g.boom > 0) {
        // An explosive ball breaks the brick it hits and every one touching it.
        g.boom -= 1;
        events.push({ type: "blast", brick: hitBrick });
        breakBrick(g, hitBrick, events, rand, true);
        for (var j = 0; j < g.bricks.length; j++) {
          var near = g.bricks[j];
          if (near.hp > 0 && touching(hitBrick, near)) breakBrick(g, near, events, rand, false);
        }
        return;
      }
      hitBrick.hp -= 1;
      if (hitBrick.hp <= 0) {
        breakBrick(g, hitBrick, events, rand, true);
      } else {
        events.push({ type: "crack", brick: hitBrick });
      }
    }
  }

  /** Where a ray from (x, y) along (vx, vy) first meets the rectangle, as a
   * time along it, or null when it misses or is already leaving it. */
  function rayRect(x, y, vx, vy, rx, ry, rw, rh) {
    var tmin = 0;
    var tmax = Infinity;
    var axes = [
      [x, vx, rx, rx + rw],
      [y, vy, ry, ry + rh],
    ];
    for (var i = 0; i < axes.length; i++) {
      var p = axes[i][0];
      var v = axes[i][1];
      var lo = axes[i][2];
      var hi = axes[i][3];
      if (v === 0) {
        if (p < lo || p > hi) return null;
        continue;
      }
      var t1 = (lo - p) / v;
      var t2 = (hi - p) / v;
      tmin = Math.max(tmin, Math.min(t1, t2));
      tmax = Math.min(tmax, Math.max(t1, t2));
    }
    if (tmax <= 1e-6 || tmin > tmax) return null;
    return tmin;
  }

  /**
   * The aim line: where a ball in flight is heading - to the first wall,
   * the bounce there, then on to the next wall or the paddle's line. It
   * stops early at the first brick in the way. The points in order, from
   * the ball; none for a ball at rest.
   */
  function aimPath(g, b) {
    if (b.vx === 0 && b.vy === 0) return [];
    var pts = [{ x: b.x, y: b.y }];
    var x = b.x;
    var y = b.y;
    var vx = b.vx;
    var vy = b.vy;
    var floor = g.paddle.y - b.r;
    for (var leg = 0; leg < 2; leg++) {
      var t = Infinity;
      var wall = null;
      var tt;
      if (vx < 0 && (tt = (b.r - x) / vx) < t) {
        t = tt;
        wall = "side";
      }
      if (vx > 0 && (tt = (W - b.r - x) / vx) < t) {
        t = tt;
        wall = "side";
      }
      if (vy < 0 && (tt = (TOP + b.r - y) / vy) < t) {
        t = tt;
        wall = "top";
      }
      if (vy > 0 && (tt = (floor - y) / vy) < t) {
        t = tt;
        wall = "floor";
      }
      if (wall === null) break;
      t = Math.max(0, t);
      var hitT = null;
      for (var i = 0; i < g.bricks.length; i++) {
        var k = g.bricks[i];
        if (k.hp <= 0) continue;
        var bt = rayRect(x, y, vx, vy, k.x - b.r, k.y - b.r, k.w + 2 * b.r, k.h + 2 * b.r);
        if (bt !== null && bt <= t && (hitT === null || bt < hitT)) hitT = bt;
      }
      if (hitT !== null) {
        pts.push({ x: x + vx * hitT, y: y + vy * hitT });
        return pts;
      }
      x += vx * t;
      y += vy * t;
      pts.push({ x: x, y: y });
      if (wall === "floor") return pts;
      if (wall === "side") vx = -vx;
      else vy = -vy;
    }
    return pts;
  }

  function bricksLeft(g) {
    for (var i = 0; i < g.bricks.length; i++) if (g.bricks[i].hp > 0) return true;
    return false;
  }

  function nextLevel(g) {
    g.level += 1;
    g.bricks = buildLevel(g.level);
    g.mode = "serve";
    g.banner = "Level " + g.level;
    freshStart(g);
  }

  /**
   * Moves the game on by dt seconds. Returns what happened, in order:
   * pick-ups run out, caught or missed; brick, crack and blast hits and
   * pick-ups starting to fall; a lost ball, a cleared level, and "over"
   * with the final score and best. The balls move in substeps no longer
   * than half their radius, so however fast they go they cannot pass
   * through a brick or the paddle (every brick and the paddle are thicker
   * than that).
   *
   * rand (0 <= rand() < 1; Math.random when not given) decides which broken
   * bricks let a pick-up fall, and which one. A ball that falls out of the
   * board is gone; only losing the last one in play costs a life.
   */
  function update(g, dt, input, rand) {
    var events = [];
    input = input || {};
    rand = typeof rand === "function" ? rand : Math.random;
    if (g.mode === "paused" || g.mode === "over") return events;
    if (g.mode === "play") tickEffects(g, dt, events);
    movePaddle(g, dt, input);
    if (g.mode === "serve") {
      seatBall(g);
      return events;
    }
    moveDrops(g, dt, events);

    var fastest = 0;
    for (var j = 0; j < g.balls.length; j++) {
      var bj = g.balls[j];
      fastest = Math.max(fastest, Math.sqrt(bj.vx * bj.vx + bj.vy * bj.vy));
    }
    var n = Math.max(1, Math.ceil((fastest * dt) / (BALL_R * 0.5)));
    var sub = dt / n;
    for (var i = 0; i < n; i++) {
      for (j = 0; j < g.balls.length; j++) substep(g, g.balls[j], sub, events, rand);
      if (!bricksLeft(g)) {
        events.push({ type: "cleared", level: g.level });
        nextLevel(g);
        return events;
      }
      for (j = g.balls.length - 1; j >= 0; j--) {
        if (g.balls[j].y - g.balls[j].r > H) g.balls.splice(j, 1);
      }
      if (g.balls.length === 0) {
        g.lives -= 1;
        events.push({ type: "lost", lives: g.lives });
        g.mode = g.lives <= 0 ? "over" : "serve";
        // The board goes back to one ball at rest either way: a new life,
        // or the last one lost with Space starting over.
        freshStart(g);
        if (g.mode === "over") {
          if (g.score > g.best) g.best = Math.min(g.score, BEST_CAP - 1);
          events.push({ type: "over", score: g.score, best: g.best });
        }
        return events;
      }
    }
    return events;
  }

  root.BreakerLogic = {
    W: W,
    H: H,
    TOP: TOP,
    LIVES: LIVES,
    BALL_R: BALL_R,
    BEST_CAP: BEST_CAP,
    MAX_BOUNCE: MAX_BOUNCE,
    PADDLE_W: PADDLE_W,
    WIDE_W: WIDE_W,
    SLOW_SPEED: SLOW_SPEED,
    DROP_CHANCE: DROP_CHANCE,
    DROP_SPEED: DROP_SPEED,
    KINDS: KINDS,
    EFFECT_TIME: EFFECT_TIME,
    BOOM_HITS: BOOM_HITS,
    MAX_BALLS: MAX_BALLS,
    ballSpeed: ballSpeed,
    aimPath: aimPath,
    activeEffects: activeEffects,
    buildLevel: buildLevel,
    circleRectHit: circleRectHit,
    paddleBounce: paddleBounce,
    createGame: createGame,
    validBest: validBest,
    launch: launch,
    togglePause: togglePause,
    update: update,
  };
})(globalThis);
