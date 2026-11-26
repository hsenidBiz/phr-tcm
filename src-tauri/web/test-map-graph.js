/* The Test map's graph: build it from the area tree, lay it out as an
   indented outline (one row per area and per case), fold a subtree, work
   out the guide lines between a parent and what hangs off it, clamp the
   view. No DOM in here - the page script (test-map.js) draws, and the
   vitest file under src/lib loads this file as-is. Plain ES5 script: the
   page is a file in the temp directory with nothing else present. */
(function (root) {
  // The outline's grid, all on the 4px grid: a row is 24 tall, each level
  // steps in by 24, and the guide lines turn with an 8px elbow. A node's
  // dot is 4 in radius whatever it counts - the count is written, not drawn.
  var ROW = 24, INDENT = 24, TREE_GAP = 8, DOT_R = 4, ELBOW_R = 8;
  // The gap between a dot and the end of the guide line that reaches it.
  var DOT_GAP = 4;

  function buildGraph(tree) {
    var nodes = [], edges = [], caseCount = 0;
    function add(node) { node.i = nodes.length; nodes.push(node); return node; }
    function addArea(n, parent) {
      var area = add({
        kind: 'area', name: n.name, count: n.count, r: DOT_R,
        x: 0, y: 0, parent: parent, children: [], cases: [], folded: false, hidden: false
      });
      if (parent) {
        parent.children.push(area);
        edges.push({ a: parent, b: area });
      }
      (n.cases || []).forEach(function (c) {
        var cn = add({ kind: 'case', data: c, ci: caseCount++, r: DOT_R, area: area, x: 0, y: 0, hidden: false });
        area.cases.push(cn);
        edges.push({ a: area, b: cn });
      });
      (n.children || []).forEach(function (ch) { addArea(ch, area); });
    }
    (tree || []).forEach(function (n) { addArea(n, null); });
    return { nodes: nodes, edges: edges };
  }

  // The outline: depth is the indent, and every visible area and case takes
  // the next row, an area's own cases before its child areas. Top-level
  // areas stack with a small gap. Hidden nodes are skipped and keep
  // whatever position they had. A row's dot is centred in its row. Returns
  // the extent in graph units.
  function layoutTree(graph) {
    var y = 0, maxDepth = 0;
    function place(area, depth) {
      area.x = depth * INDENT;
      area.y = y + ROW / 2;
      y += ROW;
      if (depth > maxDepth) maxDepth = depth;
      if (area.folded) return;
      var i;
      for (i = 0; i < area.cases.length; i++) {
        var c = area.cases[i];
        if (c.hidden) continue;
        c.x = (depth + 1) * INDENT;
        c.y = y + ROW / 2;
        y += ROW;
        if (depth + 1 > maxDepth) maxDepth = depth + 1;
      }
      for (i = 0; i < area.children.length; i++) {
        if (!area.children[i].hidden) place(area.children[i], depth + 1);
      }
    }
    var roots = graph.nodes.filter(function (n) { return n.kind === 'area' && !n.parent && !n.hidden; });
    for (var r = 0; r < roots.length; r++) {
      if (r > 0) y += TREE_GAP;
      place(roots[r], 0);
    }
    return { width: maxDepth * INDENT, height: y };
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

  // What hangs off an area, in row order: its cases, then its child areas,
  // the visible ones only.
  function kidsOf(area) {
    var out = [], i;
    for (i = 0; i < area.cases.length; i++) if (!area.cases[i].hidden) out.push(area.cases[i]);
    for (i = 0; i < area.children.length; i++) if (!area.children[i].hidden) out.push(area.children[i]);
    return out;
  }

  // The guide from a parent area down to one kid: leaves the trunk with an
  // elbow (radius ELBOW_R) and runs level to just short of the kid's dot.
  // An SVG path string, which a canvas Path2D reads too.
  function elbowPath(parent, kid) {
    var px = parent.x, ky = kid.y;
    return 'M' + px + ' ' + (ky - ELBOW_R) +
      ' Q' + px + ' ' + ky + ' ' + (px + ELBOW_R) + ' ' + ky +
      ' L' + (kid.x - DOT_R - DOT_GAP) + ' ' + ky;
  }

  // The trunk a parent's guides hang from: straight down from under its dot
  // to where the last kid's elbow begins. '' for an area with no kids.
  function trunkPath(parent) {
    var kids = kidsOf(parent);
    if (!kids.length || parent.folded) return '';
    var last = kids[kids.length - 1];
    return 'M' + parent.x + ' ' + (parent.y + DOT_R + DOT_GAP) + ' L' + parent.x + ' ' + (last.y - ELBOW_R);
  }

  // The same guide, from the parent's dot to one kid: the part of the trunk
  // above that kid, then its elbow. What a hover lights up for each link in
  // the chain.
  function linkPath(parent, kid) {
    return 'M' + parent.x + ' ' + (parent.y + DOT_R + DOT_GAP) + ' L' + parent.x + ' ' + (kid.y - ELBOW_R) +
      ' Q' + parent.x + ' ' + kid.y + ' ' + (parent.x + ELBOW_R) + ' ' + kid.y +
      ' L' + (kid.x - DOT_R - DOT_GAP) + ' ' + kid.y;
  }

  // Keep the view over the content: the outline can be panned only until
  // its far edge reaches the viewport's, and never past its top-left pad.
  // Content smaller than the viewport sits at the pad.
  function clampView(tx, ty, contentW, contentH, w, h, pad) {
    var minTx = Math.min(pad, w - contentW - pad), minTy = Math.min(pad, h - contentH - pad);
    return {
      tx: Math.max(minTx, Math.min(pad, tx)),
      ty: Math.max(minTy, Math.min(pad, ty))
    };
  }

  // The chain from the top-level area down to `node`, inclusive, in that
  // order: what a hover lights up.
  function pathTo(node) {
    var chain = [node];
    var p = node.kind === 'case' ? node.area : node.parent;
    while (p) { chain.unshift(p); p = p.parent; }
    return chain;
  }

  // "#81034 Title" for a case that exists in Azure DevOps; a new case shows
  // its title alone. The whole title - the page cuts it to the room it has.
  function caseLabel(node) {
    var id = node.data.id != null ? '#' + node.data.id : '';
    var title = node.data.title || '';
    return id ? id + ' ' + title : title;
  }

  // What an upload would do with the case, written on its row: a case with
  // a work item id updates that item, one without creates a new one.
  function caseTag(node) {
    return node.data.id != null ? 'UPDATE' : 'NEW';
  }

  // An area's row: its name and how many cases it holds (those beneath it
  // included), always written - never left to a dot's size.
  function areaLabel(node) {
    return node.name + ' · ' + node.count;
  }

  // `text` cut to fit `room` pixels by `measure`, with an ellipsis; '' when
  // not even one character and the ellipsis fit.
  function clipLabel(text, room, measure) {
    text = String(text || '');
    if (room <= 0) return '';
    if (measure(text) <= room) return text;
    var ell = '…', lo = 0, hi = text.length;
    while (lo < hi) {
      var mid = Math.ceil((lo + hi) / 2);
      if (measure(text.slice(0, mid) + ell) <= room) lo = mid; else hi = mid - 1;
    }
    return lo === 0 ? '' : text.slice(0, lo).replace(/\s+$/, '') + ell;
  }

  root.testMap = {
    ROW: ROW,
    INDENT: INDENT,
    DOT_R: DOT_R,
    ELBOW_R: ELBOW_R,
    buildGraph: buildGraph,
    layoutTree: layoutTree,
    fold: fold,
    kidsOf: kidsOf,
    elbowPath: elbowPath,
    trunkPath: trunkPath,
    linkPath: linkPath,
    clampView: clampView,
    pathTo: pathTo,
    caseLabel: caseLabel,
    caseTag: caseTag,
    areaLabel: areaLabel,
    clipLabel: clipLabel
  };
})(typeof window !== 'undefined' ? window : this);
