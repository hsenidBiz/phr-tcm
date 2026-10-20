// The brick game's page: input, drawing and the frame loop. The rules live
// in logic.js (globalThis.BreakerLogic). Written for the app; nothing here
// is third-party.
//
// The frame runs sandboxed (sandbox="allow-scripts", no
// allow-same-origin): no storage, no access to the app beyond postMessage.
// It sends one message besides escape.js's - {type: "breaker-score", best}
// when a game ends - and accepts one: {type: "breaker-best", best} from the
// app, with the best score kept so far.
(function () {
  "use strict";

  var L = globalThis.BreakerLogic;
  var canvas = document.getElementById("board");
  var ctx = canvas.getContext("2d");

  var game = L.createGame(0);

  // ---- Theme: the frame cannot read the app's CSS variables, so it reads
  // the colour scheme itself and keeps a palette for each.
  var THEMES = {
    light: {
      bg: "#f6f7f9",
      bar: "#ffffff",
      barLine: "#dfe3e8",
      text: "#1f2933",
      muted: "#616e7c",
      paddle: "#3b5bdb",
      ball: "#1f2933",
      veil: "rgba(246, 247, 249, 0.82)",
      bricks: ["#e8590c", "#f08c00", "#2f9e44", "#1c7ed6", "#7048e8", "#c2255c", "#0c8599", "#5c940d"],
      pickText: "#ffffff",
      pick: { multi: "#1c7ed6", aim: "#0c8599", boom: "#e03131", wide: "#2f9e44", slow: "#7048e8" },
    },
    dark: {
      bg: "#14171c",
      bar: "#1b1f26",
      barLine: "#2b313b",
      text: "#e6e9ee",
      muted: "#9aa5b1",
      paddle: "#748ffc",
      ball: "#f1f3f5",
      veil: "rgba(20, 23, 28, 0.8)",
      bricks: ["#ff922b", "#fcc419", "#51cf66", "#4dabf7", "#9775fa", "#f06595", "#22b8cf", "#94d82d"],
      pickText: "#14171c",
      pick: { multi: "#4dabf7", aim: "#22b8cf", boom: "#ff6b6b", wide: "#51cf66", slow: "#9775fa" },
    },
  };
  var darkQuery = window.matchMedia ? window.matchMedia("(prefers-color-scheme: dark)") : null;
  var theme = darkQuery && darkQuery.matches ? THEMES.dark : THEMES.light;
  if (darkQuery && darkQuery.addEventListener) {
    darkQuery.addEventListener("change", function (e) {
      theme = e.matches ? THEMES.dark : THEMES.light;
    });
  }

  /** A colour mixed toward another, t of the way (both "#rrggbb"). */
  function mix(a, b, t) {
    var pa = parseInt(a.slice(1), 16);
    var pb = parseInt(b.slice(1), 16);
    var out = [16, 8, 0].map(function (s) {
      var ca = (pa >> s) & 255;
      var cb = (pb >> s) & 255;
      return Math.round(ca + (cb - ca) * t);
    });
    return "rgb(" + out.join(",") + ")";
  }

  // ---- Size: the world is scaled whole to fit the canvas, centred, and
  // drawn at the screen's pixel density.
  var view = { cssW: 0, cssH: 0, dpr: 0, scale: 1, ox: 0, oy: 0 };
  function fit() {
    var cssW = canvas.clientWidth;
    var cssH = canvas.clientHeight;
    var dpr = window.devicePixelRatio || 1;
    if (cssW === view.cssW && cssH === view.cssH && dpr === view.dpr) return;
    view.cssW = cssW;
    view.cssH = cssH;
    view.dpr = dpr;
    canvas.width = Math.max(1, Math.round(cssW * dpr));
    canvas.height = Math.max(1, Math.round(cssH * dpr));
    view.scale = Math.min(cssW / L.W, cssH / L.H) || 1;
    view.ox = (cssW - L.W * view.scale) / 2;
    view.oy = (cssH - L.H * view.scale) / 2;
  }

  // ---- Input
  var input = { left: false, right: false, pointerX: null };
  var LEFT = { ArrowLeft: 1, a: 1, A: 1 };
  var RIGHT = { ArrowRight: 1, d: 1, D: 1 };

  addEventListener("keydown", function (e) {
    if (LEFT[e.key]) {
      input.left = true;
      input.pointerX = null;
      e.preventDefault();
    } else if (RIGHT[e.key]) {
      input.right = true;
      input.pointerX = null;
      e.preventDefault();
    } else if (e.key === " " || e.key === "Spacebar") {
      e.preventDefault();
      if (!e.repeat) L.launch(game);
    } else if (e.key === "p" || e.key === "P") {
      if (!e.repeat) L.togglePause(game);
    }
  });
  addEventListener("keyup", function (e) {
    if (LEFT[e.key]) input.left = false;
    if (RIGHT[e.key]) input.right = false;
  });
  // Focus left the frame: let go of the keys, and pause a ball in flight.
  addEventListener("blur", function () {
    input.left = false;
    input.right = false;
    if (game.mode === "play") L.togglePause(game);
  });

  function worldX(e) {
    var r = canvas.getBoundingClientRect();
    return (e.clientX - r.left - view.ox) / view.scale;
  }
  canvas.addEventListener("pointermove", function (e) {
    input.pointerX = worldX(e);
  });
  canvas.addEventListener("pointerdown", function (e) {
    input.pointerX = worldX(e);
    if (game.mode === "paused") L.togglePause(game);
    else L.launch(game);
  });

  // ---- Messages
  addEventListener("message", function (e) {
    if (e.source !== parent) return;
    var d = e.data;
    if (!d || typeof d !== "object" || d.type !== "breaker-best") return;
    if (!L.validBest(d.best)) return;
    if (d.best > game.best) game.best = d.best;
  });

  function onEvents(events) {
    for (var i = 0; i < events.length; i++) {
      if (events[i].type === "over") {
        parent.postMessage({ type: "breaker-score", best: game.best }, "*");
      }
    }
  }

  // ---- Drawing
  function roundRect(x, y, w, h, r) {
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
  }

  var FONT = "system-ui, -apple-system, 'Segoe UI', sans-serif";

  function drawBar() {
    ctx.fillStyle = theme.bar;
    ctx.fillRect(0, 0, L.W, L.TOP);
    ctx.fillStyle = theme.barLine;
    ctx.fillRect(0, L.TOP - 1, L.W, 1);
    ctx.textBaseline = "middle";
    ctx.font = "600 14px " + FONT;
    var mid = L.TOP / 2;

    ctx.textAlign = "left";
    ctx.fillStyle = theme.muted;
    ctx.fillText("Score", 14, mid);
    ctx.fillStyle = theme.text;
    ctx.fillText(String(game.score), 14 + ctx.measureText("Score ").width, mid);

    ctx.textAlign = "center";
    ctx.fillStyle = theme.text;
    ctx.fillText("Level " + game.level, L.W / 2 - 60, mid);
    ctx.fillStyle = theme.muted;
    ctx.fillText("Best " + game.best, L.W / 2 + 60, mid);

    ctx.textAlign = "right";
    ctx.fillStyle = theme.muted;
    var livesLabel = "Lives";
    var dotsW = L.LIVES * 14;
    ctx.fillText(livesLabel, L.W - 18 - dotsW, mid);
    for (var i = 0; i < L.LIVES; i++) {
      ctx.beginPath();
      ctx.arc(L.W - 14 - dotsW + 7 + i * 14, mid, 4.5, 0, Math.PI * 2);
      ctx.fillStyle = i < game.lives ? theme.paddle : theme.barLine;
      ctx.fill();
    }
  }

  function drawBricks() {
    for (var i = 0; i < game.bricks.length; i++) {
      var k = game.bricks[i];
      if (k.hp <= 0) continue;
      var base = theme.bricks[k.row % theme.bricks.length];
      var fill = base;
      if (k.maxHp > 1) {
        // A tough brick starts a deeper shade and pales once it is hit.
        fill = k.hp === k.maxHp ? mix(base, "#000000", 0.3) : mix(base, theme.bg, 0.45);
      }
      ctx.fillStyle = fill;
      roundRect(k.x, k.y, k.w, k.h, 3);
      ctx.fill();
      if (k.maxHp > 1 && k.hp === k.maxHp) {
        ctx.strokeStyle = base;
        ctx.lineWidth = 2;
        roundRect(k.x + 1.5, k.y + 1.5, k.w - 3, k.h - 3, 2);
        ctx.stroke();
      }
    }
  }

  /** What each pick-up says, falling and in the indicator. */
  var PICK_LABEL = { multi: "+2", aim: "Aim", boom: "Blast", wide: "Wide", slow: "Slow" };

  function drawDrops() {
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.font = "700 10px " + FONT;
    for (var i = 0; i < game.drops.length; i++) {
      var d = game.drops[i];
      ctx.fillStyle = theme.pick[d.kind];
      roundRect(d.x, d.y, d.w, d.h, d.h / 2);
      ctx.fill();
      ctx.fillStyle = theme.pickText;
      ctx.fillText(PICK_LABEL[d.kind], d.x + d.w / 2, d.y + d.h / 2 + 0.5);
    }
  }

  /** The aim line: a dotted path ahead of each ball in flight. */
  function drawAim() {
    if (!(game.effects.aim > 0)) return;
    ctx.save();
    ctx.strokeStyle = theme.pick.aim;
    ctx.lineWidth = 2;
    ctx.lineCap = "round";
    ctx.setLineDash([1, 7]);
    for (var i = 0; i < game.balls.length; i++) {
      var pts = L.aimPath(game, game.balls[i]);
      if (pts.length < 2) continue;
      ctx.beginPath();
      ctx.moveTo(pts[0].x, pts[0].y);
      for (var j = 1; j < pts.length; j++) ctx.lineTo(pts[j].x, pts[j].y);
      ctx.stroke();
    }
    ctx.restore();
  }

  /** What is on, just under the top bar: each with a bar of what is left. */
  function drawEffects() {
    var on = L.activeEffects(game);
    var x = 14;
    var y = L.TOP + 5;
    var h = 17;
    ctx.textBaseline = "middle";
    ctx.textAlign = "left";
    ctx.font = "600 10px " + FONT;
    for (var i = 0; i < on.length; i++) {
      var e = on[i];
      var text = PICK_LABEL[e.kind] + " " + (e.kind === "boom" ? "x" + e.left : Math.ceil(e.left) + "s");
      var w = ctx.measureText(text).width + 14;
      ctx.fillStyle = theme.bar;
      roundRect(x, y, w, h, h / 2);
      ctx.fill();
      ctx.strokeStyle = theme.pick[e.kind];
      ctx.lineWidth = 1.5;
      ctx.stroke();
      // How much is left, as a line along the bottom of the pill.
      ctx.fillStyle = theme.pick[e.kind];
      ctx.fillRect(x + h / 2, y + h - 3, (w - h) * Math.max(0, Math.min(1, e.left / e.total)), 1.5);
      ctx.fillStyle = theme.text;
      ctx.fillText(text, x + 7, y + h / 2 - 1.5);
      x += w + 6;
    }
  }

  function drawMessage(title, line) {
    ctx.fillStyle = theme.veil;
    ctx.fillRect(0, L.TOP, L.W, L.H - L.TOP);
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillStyle = theme.text;
    ctx.font = "700 26px " + FONT;
    ctx.fillText(title, L.W / 2, L.H / 2 - 8);
    if (line) {
      ctx.fillStyle = theme.muted;
      ctx.font = "500 15px " + FONT;
      ctx.fillText(line, L.W / 2, L.H / 2 + 24);
    }
  }

  function draw() {
    var s = view.scale * view.dpr;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = theme.bg;
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.setTransform(s, 0, 0, s, view.ox * view.dpr, view.oy * view.dpr);

    ctx.fillStyle = theme.bg;
    ctx.fillRect(0, 0, L.W, L.H);
    drawBricks();
    drawAim();
    drawDrops();

    var p = game.paddle;
    ctx.fillStyle = theme.paddle;
    roundRect(p.x, p.y, p.w, p.h, p.h / 2);
    ctx.fill();

    // An explosive ball wears a ring in the blast colour.
    for (var i = 0; i < game.balls.length; i++) {
      var b = game.balls[i];
      ctx.fillStyle = theme.ball;
      ctx.beginPath();
      ctx.arc(b.x, b.y, b.r, 0, Math.PI * 2);
      ctx.fill();
      if (game.boom > 0) {
        ctx.strokeStyle = theme.pick.boom;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.arc(b.x, b.y, b.r + 2.5, 0, Math.PI * 2);
        ctx.stroke();
      }
    }

    drawBar();
    if (game.mode === "play" || game.mode === "paused") drawEffects();

    if (game.mode === "over") {
      drawMessage("Game over", "Score " + game.score + ". Space or click to play again");
    } else if (game.mode === "paused") {
      drawMessage("Paused", "P to carry on");
    } else if (game.mode === "serve") {
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      if (game.banner) {
        ctx.fillStyle = theme.text;
        ctx.font = "700 22px " + FONT;
        ctx.fillText(game.banner, L.W / 2, L.H / 2 + 40);
      }
      ctx.fillStyle = theme.muted;
      ctx.font = "500 15px " + FONT;
      ctx.fillText("Space or click to launch", L.W / 2, L.H / 2 + 70);
    }
  }

  // ---- Loop: a fixed step, so the speed is the same at any frame rate.
  var STEP = 1 / 120;
  var acc = 0;
  var last = null;
  function frame(t) {
    fit();
    if (last === null) last = t;
    acc += Math.min((t - last) / 1000, 0.25);
    last = t;
    while (acc >= STEP) {
      onEvents(L.update(game, STEP, input));
      acc -= STEP;
    }
    draw();
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
})();
