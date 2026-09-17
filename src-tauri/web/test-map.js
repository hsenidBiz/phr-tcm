/* Draws the Test map from the #map-data JSON block as a tree that grows
   left to right: areas and cases as nodes on a canvas, laid out by
   test-map-graph.js (window.testMap), each case on its own row with its
   label beside it. Zoom decides how much each node says - zoomed out, the
   area names; zoomed in, the cases - and clicking a case opens the side
   panel. Plain script, no dependencies: the page is a file in the temp
   directory and must work with nothing else present. */
(function () {
  var dataEl = document.getElementById('map-data');
  if (!dataEl || !window.testMap) return;
  var G = window.testMap;
  var data = JSON.parse(dataEl.textContent || '[]');
  var canvas = document.getElementById('graph');
  var viewport = document.getElementById('viewport');
  var detail = document.getElementById('detail');
  var zoomLabel = document.getElementById('map-zoom');
  var ctx = canvas.getContext ? canvas.getContext('2d') : null;
  var reduced = !!(window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches);

  var graph = G.buildGraph(data);
  var nodes = graph.nodes, edges = graph.edges;
  var caseNodes = [];
  var areaNodes = [];
  nodes.forEach(function (n) { (n.kind === 'case' ? caseNodes : areaNodes).push(n); });
  G.layoutTree(graph);

  // Room left of the tree for the padding, and right of the last column
  // for its labels, when fitting the width: sized from the longest label
  // at roughly 6.2 units per character (11px text at 100%), capped so one
  // very long title cannot shrink the whole tree.
  var PAD = 40;
  var longest = 0;
  caseNodes.forEach(function (n) { longest = Math.max(longest, G.caseLabel(n, true).length); });
  var LABEL_SPACE = Math.min(700, 60 + longest * 6.2);
  // How far right of a case's dot its label reaches, for hit testing:
  // measured when drawn, this estimate until then.
  var LABEL_REACH = LABEL_SPACE;

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  // ---- Colours: the palette variables, re-read when the theme switches.
  var colours = {};
  function readColours() {
    var cs = getComputedStyle(document.documentElement);
    ['text', 'muted', 'border', 'accent'].forEach(function (k) {
      colours[k] = cs.getPropertyValue('--' + k).trim();
    });
  }
  readColours();
  if (window.MutationObserver) {
    new MutationObserver(function () { readColours(); draw(); })
      .observe(document.documentElement, { attributes: true, attributeFilter: ['data-scheme'] });
  }

  // ---- The view: graph units -> screen px.
  var scale = 1, tx = 0, ty = 0, width = 0, height = 0;
  function toScreen(x, y) { return { x: x * scale + tx, y: y * scale + ty }; }
  function setZoom(next, cx, cy) {
    next = Math.min(4, Math.max(0.2, next));
    // Zoom about the pointer (or the viewport's centre), so the spot under
    // the cursor stays put.
    var px = cx == null ? width / 2 : cx, py = cy == null ? height / 2 : cy;
    tx = px - (px - tx) * (next / scale);
    ty = py - (py - ty) * (next / scale);
    scale = next;
    draw();
  }
  // The tree is read like a document: fit its width, start at the top.
  function fit() {
    var t = G.fitWidthTransform(nodes, width, PAD, LABEL_SPACE);
    scale = t.scale; tx = t.tx; ty = t.ty;
    draw();
  }
  function resize() {
    var prevW = width, prevH = height;
    var r = viewport.getBoundingClientRect();
    width = r.width; height = r.height;
    var dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width = width + 'px';
    canvas.style.height = height + 'px';
    // A page loaded in a hidden tab measures 0x0 here; becoming visible
    // later fires no window 'resize', only the ResizeObserver below. Either
    // way, the first time a real size arrives after a 0x0 start, the initial
    // fit() (computed against 0x0) is stale - fit again instead of just
    // drawing.
    if ((prevW === 0 || prevH === 0) && width > 0 && height > 0) fit();
    else draw();
  }
  // Covers becoming visible with no window 'resize' (a hidden tab, or a
  // panel resize that doesn't change the window); the window listener below
  // stays as the fallback for browsers without ResizeObserver.
  if (window.ResizeObserver) { new ResizeObserver(resize).observe(viewport); }

  // ---- Drawing.
  var hover = null, activeNode = null;
  var FONT = 'ui-sans-serif, system-ui, "Segoe UI", sans-serif';

  // A hover lights the whole chain from the top-level area down to the
  // node - the nodes, and the limbs between consecutive ones.
  function chainFocus(n) {
    var chain = G.pathTo(n), keep = {};
    for (var i = 0; i < chain.length; i++) keep[chain[i].i] = true;
    return { keep: keep, chain: chain };
  }

  // ---- The pulse: while a case is hovered, a short bright segment runs
  // along its chain's limbs from the top-level area to the case, one pass
  // per PULSE_MS, until the pointer leaves. Not under reduced motion.
  var PULSE_MS = 900, PULSE_TAIL = 0.18;
  var pulseT = 0, pulseAt = 0, pulsing = false;
  function pulseTick(now) {
    if (!hover || hover.kind !== 'case' || reduced) { pulsing = false; return; }
    if (pulseAt) pulseT = (pulseT + (now - pulseAt) / PULSE_MS) % 1;
    pulseAt = now;
    draw();
    requestAnimationFrame(pulseTick);
  }
  function startPulse() {
    if (pulsing || reduced || !hover || hover.kind !== 'case') return;
    pulsing = true; pulseT = 0; pulseAt = 0;
    requestAnimationFrame(pulseTick);
  }
  // The point at t along the limb from a to b (the same curve limb() draws).
  function limbPoint(a, b, t) {
    var p = toScreen(a.x, a.y), q = toScreen(b.x, b.y);
    var mx = (p.x + q.x) / 2, u = 1 - t;
    return {
      x: u * u * u * p.x + 3 * u * u * t * mx + 3 * u * t * t * mx + t * t * t * q.x,
      y: u * u * u * p.y + 3 * u * u * t * p.y + 3 * u * t * t * q.y + t * t * t * q.y
    };
  }
  function drawPulse(chain) {
    var limbs = chain.length - 1;
    if (limbs < 1) return;
    var pos = pulseT * limbs;
    var idx = Math.min(limbs - 1, Math.floor(pos));
    var head = pos - idx;
    var a = chain[idx], b = chain[idx + 1];
    ctx.save();
    ctx.strokeStyle = colours.accent;
    ctx.fillStyle = colours.accent;
    ctx.lineWidth = 3;
    ctx.lineCap = 'round';
    ctx.globalAlpha = 0.95;
    ctx.beginPath();
    var steps = 8, t0 = Math.max(0, head - PULSE_TAIL);
    for (var i = 0; i <= steps; i++) {
      var pt = limbPoint(a, b, t0 + (head - t0) * (i / steps));
      if (i === 0) ctx.moveTo(pt.x, pt.y); else ctx.lineTo(pt.x, pt.y);
    }
    ctx.stroke();
    var h = limbPoint(a, b, head);
    ctx.beginPath(); ctx.arc(h.x, h.y, 3.5, 0, Math.PI * 2); ctx.fill();
    ctx.restore();
  }
  function screenRadius(n) {
    if (n.kind === 'area') return n.r * scale;
    return scale < 0.5 ? 2 : n.r * scale;
  }

  // A limb from an area to what hangs off it: leaves the right side of the
  // parent, arrives at the left of the child, bending at the halfway column.
  function limb(a, b) {
    var p = toScreen(a.x, a.y), q = toScreen(b.x, b.y);
    var mx = (p.x + q.x) / 2;
    ctx.beginPath();
    ctx.moveTo(p.x, p.y);
    ctx.bezierCurveTo(mx, p.y, mx, q.y, q.x, q.y);
    ctx.stroke();
  }

  function draw() {
    if (!ctx) return;
    var dpr = window.devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    var focus = hover ? chainFocus(hover) : null;
    var idA = G.labelAlpha('case-id', scale, reduced);
    var titleA = G.labelAlpha('case-title', scale, reduced);
    // Only what is on screen is drawn: a 500-row tree is mostly off-screen
    // at any one time.
    var top = -ty / scale - G.ROW, bottom = (height - ty) / scale + G.ROW;
    function onScreen(n) { return n.y >= top && n.y <= bottom; }

    ctx.lineWidth = Math.max(1, 1.2 * scale);
    ctx.strokeStyle = colours.border;
    edges.forEach(function (e) {
      if (e.a.hidden || e.b.hidden) return;
      if (!onScreen(e.a) && !onScreen(e.b) && (e.a.y - top) * (e.b.y - top) > 0 && (e.a.y - bottom) * (e.b.y - bottom) > 0) return;
      var onChain = focus && focus.keep[e.a.i] && focus.keep[e.b.i];
      ctx.globalAlpha = focus && !onChain ? 0.25 : 1;
      ctx.strokeStyle = onChain ? colours.accent : colours.border;
      limb(e.a, e.b);
    });
    ctx.strokeStyle = colours.border;
    if (focus && hover.kind === 'case' && !reduced) drawPulse(focus.chain);

    ctx.textAlign = 'left';
    nodes.forEach(function (n) {
      if (n.hidden || !onScreen(n)) return;
      var p = toScreen(n.x, n.y);
      var r = screenRadius(n);
      var base = focus && !focus.keep[n.i] ? 0.25 : 1;
      ctx.globalAlpha = base;
      ctx.fillStyle = n.kind === 'area' ? colours.accent : colours.muted;
      ctx.beginPath(); ctx.arc(p.x, p.y, r, 0, Math.PI * 2); ctx.fill();
      if (n === activeNode) {
        ctx.strokeStyle = colours.accent; ctx.lineWidth = 2;
        ctx.beginPath(); ctx.arc(p.x, p.y, r + 3, 0, Math.PI * 2); ctx.stroke();
        ctx.lineWidth = Math.max(1, 1.2 * scale); ctx.strokeStyle = colours.border;
      }

      ctx.fillStyle = colours.text;
      if (n.kind === 'area') {
        // Area names keep a fixed screen size: they are the map's titles.
        // Drawn above the node, starting at its left edge, so they sit in
        // the gap the limbs leave and never on a case row.
        ctx.font = '600 13px ' + FONT;
        ctx.textBaseline = 'bottom';
        ctx.fillText(n.name + (n.folded ? ' (' + n.count + ')' : ''), p.x - r, p.y - r - 3);
        return;
      }
      // Case labels sit to the right of the dot, on the case's own row, and
      // scale with the graph; the id-only and id+title strings share the
      // "#id" prefix, so drawn from the same x only the title fades in.
      if (titleA <= 0 && idA <= 0 && n !== hover) return;
      var size = Math.max(9, Math.min(22, 11 * scale));
      ctx.font = size + 'px ' + FONT;
      ctx.textBaseline = 'middle';
      var x0 = p.x + r + 6;
      var full = G.caseLabel(n, true);
      n.labelW = ctx.measureText(full).width;
      if (n === hover) {
        ctx.fillText(full, x0, p.y);
        return;
      }
      if (titleA > 0) {
        ctx.globalAlpha = base * titleA;
        ctx.fillText(full, x0, p.y);
      }
      if (idA > 0 && titleA < 1) {
        ctx.globalAlpha = base * idA * (1 - titleA);
        ctx.fillText(G.caseLabel(n, false), x0, p.y);
      }
    });
    ctx.globalAlpha = 1;
    zoomLabel.textContent = Math.round(scale * 100) + '%';
  }

  // ---- Hit testing: an area by its circle (+4px); a case by its row, from
  // the dot to the end of where its label can reach.
  function nodeAt(sx, sy) {
    var best = null, bestD = Infinity;
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      if (n.hidden) continue;
      var p = toScreen(n.x, n.y);
      var r = screenRadius(n);
      var d;
      if (n.kind === 'area') {
        d = Math.sqrt((p.x - sx) * (p.x - sx) + (p.y - sy) * (p.y - sy));
        if (d > r + 4) continue;
      } else {
        var half = Math.max(r + 4, (G.ROW / 2) * scale);
        if (Math.abs(sy - p.y) > half) continue;
        var reach = n.labelW != null ? r + 6 + n.labelW : LABEL_REACH * scale;
        if (sx < p.x - r - 4 || sx > p.x + reach) continue;
        d = Math.abs(sy - p.y) + (sx < p.x ? p.x - sx : 0);
      }
      if (d < bestD) { best = n; bestD = d; }
    }
    return best;
  }
  function local(e) {
    var r = viewport.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  // ---- The side panel.
  var listButtons = document.querySelectorAll('#map-list button.case');
  function showCase(node) {
    var c = node.data;
    activeNode = node;
    for (var i = 0; i < listButtons.length; i++) {
      if (Number(listButtons[i].getAttribute('data-i')) === node.ci) listButtons[i].setAttribute('aria-current', 'true');
      else listButtons[i].removeAttribute('aria-current');
    }
    detail.innerHTML = '';
    var close = el('button', 'close', 'Close');
    close.type = 'button';
    close.setAttribute('aria-label', 'Close the test case');
    close.addEventListener('click', hideCase);
    detail.appendChild(close);
    // The title leads with what an upload would do: create it (New) or
    // update the work item it names.
    var h2 = el('h2');
    h2.appendChild(el('span', 'chip ' + (c.id != null ? 'update' : 'new'), c.id != null ? 'Update' : 'New'));
    h2.appendChild(document.createTextNode((c.id != null ? '#' + c.id + '  ' : '') + c.title));
    detail.appendChild(h2);
    var meta = [];
    if (c.tags) meta.push('Tags: ' + c.tags);
    if (c.automation_status) meta.push(c.automation_status);
    detail.appendChild(el('p', 'meta', meta.join(' · ')));
    if (c.preconditions) {
      detail.appendChild(el('h3', null, 'Preconditions'));
      detail.appendChild(el('p', 'pre', c.preconditions));
    }
    if (!c.steps || c.steps.length === 0) {
      detail.appendChild(el('p', 'meta', 'No steps.'));
    } else {
      var table = el('table');
      var head = el('tr');
      ['#', 'Action', 'Expected'].forEach(function (h) { head.appendChild(el('th', null, h)); });
      table.appendChild(head);
      c.steps.forEach(function (s, i) {
        var tr = el('tr');
        tr.appendChild(el('td', 'n', String(i + 1)));
        tr.appendChild(el('td', null, s.action));
        tr.appendChild(el('td', 'exp', s.expected));
        table.appendChild(tr);
      });
      detail.appendChild(table);
    }
    detail.hidden = false;
    grip.hidden = false;
    // The panel changes the viewport's width.
    resize();
  }
  function hideCase() {
    activeNode = null;
    for (var i = 0; i < listButtons.length; i++) listButtons[i].removeAttribute('aria-current');
    detail.hidden = true;
    grip.hidden = true;
    detail.innerHTML = '';
    resize();
  }

  // ---- The panel's width: dragged by the grip on its left edge, kept for
  // next time when storage allows (a file:// page may refuse it), reset by
  // a double-click on the grip.
  var DETAIL_KEY = 'tcm-map-detail-w', DETAIL_DEFAULT = 400, DETAIL_MIN = 280;
  var grip = el('div', 'detail-grip');
  grip.setAttribute('role', 'separator');
  grip.setAttribute('aria-orientation', 'vertical');
  grip.setAttribute('aria-label', 'Resize the test case panel');
  grip.title = 'Drag to resize; double-click to reset';
  grip.hidden = true;
  detail.parentNode.insertBefore(grip, detail);
  function setDetailWidth(w) {
    var max = Math.max(DETAIL_MIN, window.innerWidth * 0.8);
    w = Math.round(Math.min(max, Math.max(DETAIL_MIN, w)));
    document.documentElement.style.setProperty('--detail-w', w + 'px');
    return w;
  }
  try {
    var kept = parseInt(localStorage.getItem(DETAIL_KEY), 10);
    if (kept > 0) setDetailWidth(kept);
  } catch (e) { /* file:// storage refused */ }
  var gripDrag = null;
  grip.addEventListener('mousedown', function (e) {
    if (e.button !== 0) return;
    gripDrag = { startX: e.clientX, startW: detail.getBoundingClientRect().width };
    grip.classList.add('dragging');
    e.preventDefault();
  });
  window.addEventListener('mousemove', function (e) {
    if (!gripDrag) return;
    if (e.buttons === 0) { endGrip(); return; }
    // The panel is on the right, so moving left widens it.
    setDetailWidth(gripDrag.startW + (gripDrag.startX - e.clientX));
    resize();
  });
  function endGrip() {
    if (!gripDrag) return;
    gripDrag = null;
    grip.classList.remove('dragging');
    try { localStorage.setItem(DETAIL_KEY, String(Math.round(detail.getBoundingClientRect().width))); } catch (e) { /* file:// */ }
  }
  window.addEventListener('mouseup', endGrip);
  window.addEventListener('blur', endGrip);
  grip.addEventListener('dblclick', function () {
    setDetailWidth(DETAIL_DEFAULT);
    try { localStorage.removeItem(DETAIL_KEY); } catch (e) { /* file:// */ }
    resize();
  });
  for (var b = 0; b < listButtons.length; b++) {
    listButtons[b].addEventListener('click', function () {
      var node = caseNodes[Number(this.getAttribute('data-i'))];
      if (node) showCase(node);
    });
  }

  // ---- Fold: the tree re-lays out around the change.
  function afterFold() {
    G.layoutTree(graph);
    if (hover && hover.hidden) { hover = null; setCursor(); }
    if (activeNode && activeNode.hidden) hideCase();
    draw();
  }
  function setFolded(area, folded) {
    G.fold(graph, area, folded);
    afterFold();
  }
  function foldAll(folded) {
    areaNodes.forEach(function (a) { G.fold(graph, a, folded); });
    afterFold();
  }

  // ---- Pointer: hover, click, pan. A press that moves is a pan wherever it
  // started; a press that does not move on a node is a click on it.
  var drag = null; // { panX, panY, startX, startY, node }
  var moved = false;
  function setCursor() {
    viewport.classList.toggle('dragging', !!(drag && moved));
    viewport.classList.toggle('over-node', !!hover && !(drag && moved));
  }
  viewport.addEventListener('mousemove', function (e) {
    if (drag) return; // tracked on window, so it keeps going once the pointer leaves
    var p = local(e);
    var h = nodeAt(p.x, p.y);
    if (h !== hover) { hover = h; setCursor(); draw(); startPulse(); }
  });
  viewport.addEventListener('mouseleave', function () {
    if (!drag && hover) { hover = null; setCursor(); draw(); }
  });
  // A pan can carry the pointer out of the viewport (into the header, the
  // side panel, or past the window edge); this listener is on window so it
  // keeps updating instead of freezing at the last position inside.
  window.addEventListener('mousemove', function (e) {
    if (!drag) return;
    if (e.buttons === 0) {
      // The button went up somewhere that never delivered a 'mouseup' (an
      // alt-tab, a release over an OS dialog). Treat it as the pan ending,
      // not as a click - the pointer already moved once since the press.
      moved = true;
      endDrag();
      return;
    }
    var p = local(e);
    if (!moved && (Math.abs(e.clientX - drag.startX) > 4 || Math.abs(e.clientY - drag.startY) > 4)) {
      moved = true;
      setCursor();
    }
    if (moved) {
      tx = p.x - drag.panX; ty = p.y - drag.panY;
      draw();
    }
  });
  viewport.addEventListener('mousedown', function (e) {
    if (e.button !== 0) return;
    moved = false; // every press starts clean
    var p = local(e);
    drag = { panX: p.x - tx, panY: p.y - ty, startX: e.clientX, startY: e.clientY, node: nodeAt(p.x, p.y) };
    setCursor();
    e.preventDefault();
  });
  function endDrag() {
    if (!drag) return;
    var n = drag.node;
    drag = null;
    if (n && !moved) {
      if (n.kind === 'case') showCase(n);
      else setFolded(n, !n.folded);
    }
    setCursor();
    draw();
  }
  window.addEventListener('mouseup', endDrag);
  window.addEventListener('blur', function () {
    if (drag) { moved = true; endDrag(); }
  });
  viewport.addEventListener('wheel', function (e) {
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      var p = local(e);
      setZoom(scale * (e.deltaY < 0 ? 1.1 : 1 / 1.1), p.x, p.y);
      return;
    }
    // Plain wheel scrolls the tree; Ctrl/Cmd + wheel zooms about the cursor.
    tx -= e.deltaX; ty -= e.deltaY;
    draw();
  }, { passive: false });

  // ---- Chrome.
  document.getElementById('map-in').addEventListener('click', function () { setZoom(scale * 1.2); });
  document.getElementById('map-out').addEventListener('click', function () { setZoom(scale / 1.2); });
  document.getElementById('map-reset').addEventListener('click', fit);
  document.getElementById('map-expand').addEventListener('click', function () { foldAll(false); });
  document.getElementById('map-collapse').addEventListener('click', function () { foldAll(true); });
  window.addEventListener('resize', resize);

  // ---- Go.
  resize();
  if (nodes.length) fit();
})();
