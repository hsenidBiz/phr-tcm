/* The Test map's graph: build it from the area tree, lay it out with a
   force simulation, fold a subtree, decide label opacity from the zoom,
   fit it to a viewport. No DOM in here - the page script (test-map.js)
   draws, and the vitest file under src/lib loads this file as-is. Plain
   ES5 script: the page is a file in the temp directory with nothing else
   present. */
(function (root) {
  var CASE_R = 4;
  var SPRING = 0.08, CASE_LEN = 40, AREA_LEN = 110;
  var REPEL = 900, CELL = 120;
  var CENTRE = 0.01, DAMP = 0.85;
  var RING_TOP = 220, RING_CHILD = 90, JITTER = 30;
  var ID_AT = 0.6, TITLE_AT = 1.2, RAMP = 0.1;
  var TITLE_MAX = 40;

  function areaRadius(count) {
    return Math.max(10, Math.min(28, 6 + 3 * Math.sqrt(count)));
  }

  // A seeded 0..1 hash, so the same tree always starts in the same place.
  function hash(n) {
    var x = Math.sin(n * 12.9898 + 78.233) * 43758.5453;
    return x - Math.floor(x);
  }

  function buildGraph(tree) {
    var nodes = [], edges = [], caseCount = 0;
    function add(node) { node.i = nodes.length; nodes.push(node); return node; }
    function addArea(n, parent, index, siblings) {
      var angle = (index / Math.max(1, siblings)) * Math.PI * 2;
      var ring = parent ? RING_CHILD : RING_TOP;
      var cx = parent ? parent.x : 0, cy = parent ? parent.y : 0;
      var area = add({
        kind: 'area', name: n.name, count: n.count, r: areaRadius(n.count),
        x: cx + Math.cos(angle) * ring, y: cy + Math.sin(angle) * ring, vx: 0, vy: 0,
        parent: parent, children: [], cases: [], folded: false, hidden: false, pinned: false
      });
      if (parent) {
        parent.children.push(area);
        edges.push({ a: parent, b: area, len: AREA_LEN });
      }
      (n.cases || []).forEach(function (c) {
        var seed = area.i * 31 + caseCount;
        var cn = add({
          kind: 'case', data: c, ci: caseCount++, r: CASE_R, area: area,
          x: area.x + (hash(seed) - 0.5) * 2 * JITTER,
          y: area.y + (hash(seed + 0.5) - 0.5) * 2 * JITTER,
          vx: 0, vy: 0, hidden: false, pinned: false
        });
        area.cases.push(cn);
        edges.push({ a: area, b: cn, len: CASE_LEN });
      });
      (n.children || []).forEach(function (ch, i, all) { addArea(ch, area, i, all.length); });
    }
    (tree || []).forEach(function (n, i, all) { addArea(n, null, i, all.length); });
    return { nodes: nodes, edges: edges };
  }

  function step(nodes, edges, alpha) {
    var live = [], i, j, n, m;
    for (i = 0; i < nodes.length; i++) if (!nodes[i].hidden) live.push(nodes[i]);

    // Repulsion, bucketed: only nodes within two cells push each other.
    var grid = {};
    for (i = 0; i < live.length; i++) {
      n = live[i];
      var key = Math.floor(n.x / CELL) + ',' + Math.floor(n.y / CELL);
      (grid[key] || (grid[key] = [])).push(n);
    }
    for (i = 0; i < live.length; i++) {
      n = live[i];
      var gx = Math.floor(n.x / CELL), gy = Math.floor(n.y / CELL);
      for (var dx = -2; dx <= 2; dx++) {
        for (var dy = -2; dy <= 2; dy++) {
          var cell = grid[(gx + dx) + ',' + (gy + dy)];
          if (!cell) continue;
          for (j = 0; j < cell.length; j++) {
            m = cell[j];
            if (m === n) continue;
            var ex = n.x - m.x, ey = n.y - m.y;
            var d2 = ex * ex + ey * ey;
            if (d2 < 1) {
              // Coincident: nudge along a seeded direction, never divide by zero.
              var a = hash(n.i * 7 + m.i) * Math.PI * 2;
              ex = Math.cos(a); ey = Math.sin(a); d2 = 1;
            }
            var d = Math.sqrt(d2);
            var f = (REPEL / d2) * alpha;
            n.vx += (ex / d) * f;
            n.vy += (ey / d) * f;
          }
        }
      }
    }

    // Springs along the edges.
    for (i = 0; i < edges.length; i++) {
      var e = edges[i];
      if (e.a.hidden || e.b.hidden) continue;
      var sx = e.b.x - e.a.x, sy = e.b.y - e.a.y;
      var len = Math.sqrt(sx * sx + sy * sy) || 1;
      var pull = (len - e.len) * SPRING * alpha;
      var fx = (sx / len) * pull, fy = (sy / len) * pull;
      e.a.vx += fx; e.a.vy += fy;
      e.b.vx -= fx; e.b.vy -= fy;
    }

    // Centre pull, damping, move.
    for (i = 0; i < live.length; i++) {
      n = live[i];
      if (n.pinned) { n.vx = 0; n.vy = 0; continue; }
      n.vx -= n.x * CENTRE * alpha;
      n.vy -= n.y * CENTRE * alpha;
      n.vx *= DAMP;
      n.vy *= DAMP;
      n.x += n.vx;
      n.y += n.vy;
    }
  }

  // Hidden follows the ancestors: a node is hidden when any area above it
  // is folded. Recomputed from the top of the changed subtree, so a
  // folded child stays folded when its parent opens again.
  function refresh(area) {
    var off = area.hidden || area.folded;
    var i;
    for (i = 0; i < area.cases.length; i++) area.cases[i].hidden = off;
    for (i = 0; i < area.children.length; i++) {
      area.children[i].hidden = off;
      refresh(area.children[i]);
    }
  }

  function fold(graph, area, folded) {
    area.folded = !!folded;
    refresh(area);
  }

  function labelAlpha(kind, scale, reduced) {
    var at = kind === 'case-title' ? TITLE_AT : ID_AT;
    if (reduced) return scale >= at ? 1 : 0;
    var t = (scale - (at - RAMP)) / (2 * RAMP);
    // Float subtraction (e.g. 0.7 - 0.5) can land a hair under the exact
    // 0/1 edge (0.9999999999999998); round away that noise before clamping
    // so a scale exactly RAMP past the threshold reads as a clean 1.
    t = Math.round(t * 1e9) / 1e9;
    return Math.max(0, Math.min(1, t));
  }

  function fitTransform(nodes, w, h, pad) {
    var minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity, any = false;
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      if (n.hidden) continue;
      any = true;
      minX = Math.min(minX, n.x - n.r); maxX = Math.max(maxX, n.x + n.r);
      minY = Math.min(minY, n.y - n.r); maxY = Math.max(maxY, n.y + n.r);
    }
    if (!any) return { scale: 1, tx: w / 2, ty: h / 2 };
    var bw = Math.max(1, maxX - minX), bh = Math.max(1, maxY - minY);
    var scale = Math.min(1.5, (w - 2 * pad) / bw, (h - 2 * pad) / bh);
    return {
      scale: scale,
      tx: (w - bw * scale) / 2 - minX * scale,
      ty: (h - bh * scale) / 2 - minY * scale
    };
  }

  function caseLabel(node, withTitle) {
    var id = node.data.id != null ? '#' + node.data.id : 'NEW';
    if (!withTitle) return id;
    var title = node.data.title || '';
    if (title.length > TITLE_MAX) title = title.slice(0, TITLE_MAX - 1) + '…';
    return id + '  ' + title;
  }

  root.testMap = {
    buildGraph: buildGraph,
    step: step,
    fold: fold,
    labelAlpha: labelAlpha,
    fitTransform: fitTransform,
    caseLabel: caseLabel
  };
})(typeof window !== 'undefined' ? window : this);
