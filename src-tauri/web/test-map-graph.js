/* The Test map's graph: build it from the area tree, lay it out as a tree
   that grows left to right, fold a subtree, decide label opacity from the
   zoom, fit it to a viewport. No DOM in here - the page script
   (test-map.js) draws, and the vitest file under src/lib loads this file
   as-is. Plain ES5 script: the page is a file in the temp directory with
   nothing else present. */
(function (root) {
  var CASE_R = 4;
  // The tree grid: one row per case (or per folded area), one column per
  // level of depth, a gap between the top-level trees.
  var ROW = 22, LEVEL = 220, TREE_GAP = 40;
  // Every case has its own row, so labels never collide: the only reason
  // to hide them is text too small to read. Ids from 50%, titles from 80%.
  var ID_AT = 0.5, TITLE_AT = 0.8, RAMP = 0.1;
  var TITLE_MAX = 70;

  function areaRadius(count) {
    return Math.max(7, Math.min(16, 5 + 1.5 * Math.sqrt(count)));
  }

  function buildGraph(tree) {
    var nodes = [], edges = [], caseCount = 0;
    function add(node) { node.i = nodes.length; nodes.push(node); return node; }
    function addArea(n, parent) {
      var area = add({
        kind: 'area', name: n.name, count: n.count, r: areaRadius(n.count),
        x: 0, y: 0, parent: parent, children: [], cases: [], folded: false, hidden: false
      });
      if (parent) {
        parent.children.push(area);
        edges.push({ a: parent, b: area });
      }
      (n.cases || []).forEach(function (c) {
        var cn = add({ kind: 'case', data: c, ci: caseCount++, r: CASE_R, area: area, x: 0, y: 0, hidden: false });
        area.cases.push(cn);
        edges.push({ a: area, b: cn });
      });
      (n.children || []).forEach(function (ch) { addArea(ch, area); });
    }
    (tree || []).forEach(function (n) { addArea(n, null); });
    return { nodes: nodes, edges: edges };
  }

  // Tidy tree, left to right: depth is the column, every visible case (and
  // every folded area) takes the next row, an area sits at the midpoint of
  // the rows beneath it. Top-level areas stack as separate trees. Hidden
  // nodes are skipped and keep whatever position they had. Returns the
  // extent in graph units.
  function layoutTree(graph) {
    var y = 0, maxDepth = 0;
    function place(area, depth) {
      area.x = depth * LEVEL;
      if (depth > maxDepth) maxDepth = depth;
      var visibleChildren = area.children.filter(function (c) { return !c.hidden; });
      if (area.folded || (area.cases.length === 0 && visibleChildren.length === 0)) {
        area.y = y;
        y += ROW;
        return;
      }
      var first = y;
      var i;
      for (i = 0; i < area.cases.length; i++) {
        var c = area.cases[i];
        if (c.hidden) continue;
        c.x = (depth + 1) * LEVEL;
        c.y = y;
        y += ROW;
        if (depth + 1 > maxDepth) maxDepth = depth + 1;
      }
      for (i = 0; i < visibleChildren.length; i++) place(visibleChildren[i], depth + 1);
      area.y = (first + (y - ROW)) / 2;
    }
    var roots = graph.nodes.filter(function (n) { return n.kind === 'area' && !n.parent && !n.hidden; });
    for (var r = 0; r < roots.length; r++) {
      if (r > 0) y += TREE_GAP;
      place(roots[r], 0);
    }
    return { width: maxDepth * LEVEL, height: Math.max(0, y - ROW) };
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

  function bounds(nodes) {
    var b = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity, any: false };
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      if (n.hidden) continue;
      b.any = true;
      b.minX = Math.min(b.minX, n.x - n.r); b.maxX = Math.max(b.maxX, n.x + n.r);
      b.minY = Math.min(b.minY, n.y - n.r); b.maxY = Math.max(b.maxY, n.y + n.r);
    }
    return b;
  }

  // Fit everything into the viewport, centred. Cap 1.5 so a tiny tree is
  // not blown up; floor 0.2 (the page's zoom floor) so a 0x0 viewport -
  // a page loaded in a hidden tab - never yields a negative scale.
  function fitTransform(nodes, w, h, pad) {
    var b = bounds(nodes);
    if (!b.any) return { scale: 1, tx: w / 2, ty: h / 2 };
    var bw = Math.max(1, b.maxX - b.minX), bh = Math.max(1, b.maxY - b.minY);
    var scale = Math.max(0.2, Math.min(1.5, (w - 2 * pad) / bw, (h - 2 * pad) / bh));
    return {
      scale: scale,
      tx: (w - bw * scale) / 2 - b.minX * scale,
      ty: (h - bh * scale) / 2 - b.minY * scale
    };
  }

  // Fit the tree's width (plus room for the rightmost labels) and start at
  // the top left, like a document - a tall tree is read by scrolling, not
  // shrunk to fit. Never above 100%; same 0.2 floor as fitTransform.
  function fitWidthTransform(nodes, w, pad, labelSpace) {
    var b = bounds(nodes);
    if (!b.any) return { scale: 1, tx: pad, ty: pad };
    var bw = Math.max(1, b.maxX - b.minX) + labelSpace;
    var scale = Math.max(0.2, Math.min(1, (w - 2 * pad) / bw));
    return { scale: scale, tx: pad - b.minX * scale, ty: pad - b.minY * scale };
  }

  function caseLabel(node, withTitle) {
    var id = node.data.id != null ? '#' + node.data.id : 'NEW';
    if (!withTitle) return id;
    var title = node.data.title || '';
    if (title.length > TITLE_MAX) title = title.slice(0, TITLE_MAX - 1) + '…';
    return id + '  ' + title;
  }

  root.testMap = {
    ROW: ROW,
    LEVEL: LEVEL,
    buildGraph: buildGraph,
    layoutTree: layoutTree,
    fold: fold,
    labelAlpha: labelAlpha,
    fitTransform: fitTransform,
    fitWidthTransform: fitWidthTransform,
    caseLabel: caseLabel
  };
})(typeof window !== 'undefined' ? window : this);
