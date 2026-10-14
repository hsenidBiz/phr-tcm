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
  /** The steepest a paddle bounce leaves, from straight up. */
  var MAX_BOUNCE = (60 * Math.PI) / 180;
  /** A ball never travels flatter than this share of its speed vertically,
   * so it cannot end up sliding sideways for ever. */
  var MIN_VY_SHARE = 0.3;
  /** Highest best score either side accepts, matching the app's cap. */
  var BEST_CAP = 10000000;

  function clamp(v, lo, hi) {
    return v < lo ? lo : v > hi ? hi : v;
  }

  /** The ball's speed on a level: faster each level, up to a ceiling. */
  function ballSpeed(level) {
    return Math.min(300 + 35 * (level - 1), 620);
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

  function seatBall(g) {
    g.ball.x = g.paddle.x + g.paddle.w / 2;
    g.ball.y = g.paddle.y - g.ball.r - 0.5;
    g.ball.vx = 0;
    g.ball.vy = 0;
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
      ball: { x: 0, y: 0, vx: 0, vy: 0, r: BALL_R },
      bricks: buildLevel(1),
      banner: "Level 1",
    };
    seatBall(g);
    return g;
  }

  function validBest(v) {
    return typeof v === "number" && Number.isInteger(v) && v >= 0 && v < BEST_CAP;
  }

  /** Space or a click: launch a seated ball, or start over after a loss. */
  function launch(g) {
    if (g.mode === "serve") {
      var v = paddleBounce(g.paddle.x + g.paddle.w * (g.level % 2 ? 0.62 : 0.38), g.paddle.x, g.paddle.w, g.speed);
      g.ball.vx = v.vx;
      g.ball.vy = v.vy;
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

  /** One short move of the ball, with every bounce it makes. */
  function substep(g, dt, events) {
    var b = g.ball;
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
      hitBrick.hp -= 1;
      if (hitBrick.hp <= 0) {
        g.score += hitBrick.points;
        events.push({ type: "brick", brick: hitBrick });
      } else {
        events.push({ type: "crack", brick: hitBrick });
      }
    }
  }

  function bricksLeft(g) {
    for (var i = 0; i < g.bricks.length; i++) if (g.bricks[i].hp > 0) return true;
    return false;
  }

  function nextLevel(g) {
    g.level += 1;
    g.speed = ballSpeed(g.level);
    g.bricks = buildLevel(g.level);
    g.paddle = newPaddle();
    g.mode = "serve";
    g.banner = "Level " + g.level;
    seatBall(g);
  }

  /**
   * Moves the game on by dt seconds. Returns what happened, in order: brick
   * and crack hits, a lost ball, a cleared level, and "over" with the final
   * score and best. The ball moves in substeps no longer than half its
   * radius, so however fast it goes it cannot pass through a brick or the
   * paddle (every brick and the paddle are thicker than that).
   */
  function update(g, dt, input) {
    var events = [];
    input = input || {};
    if (g.mode === "paused" || g.mode === "over") return events;
    movePaddle(g, dt, input);
    if (g.mode === "serve") {
      seatBall(g);
      return events;
    }

    var b = g.ball;
    var dist = Math.sqrt(b.vx * b.vx + b.vy * b.vy) * dt;
    var n = Math.max(1, Math.ceil(dist / (b.r * 0.5)));
    var sub = dt / n;
    for (var i = 0; i < n; i++) {
      substep(g, sub, events);
      if (!bricksLeft(g)) {
        events.push({ type: "cleared", level: g.level });
        nextLevel(g);
        return events;
      }
      if (b.y - b.r > H) {
        g.lives -= 1;
        events.push({ type: "lost", lives: g.lives });
        if (g.lives <= 0) {
          g.mode = "over";
          if (g.score > g.best) g.best = Math.min(g.score, BEST_CAP - 1);
          events.push({ type: "over", score: g.score, best: g.best });
        } else {
          g.mode = "serve";
          g.paddle = newPaddle();
          seatBall(g);
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
    ballSpeed: ballSpeed,
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
