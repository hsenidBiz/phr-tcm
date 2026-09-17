/* Draws the Test map from the #map-data JSON block as a graph: areas and
   cases as nodes on a canvas, laid out by the simulation in
   test-map-graph.js (window.testMap). Zoom decides how much each node
   says - zoomed out, the area names; zoomed in, the cases - and clicking
   a case opens the side panel. Plain script, no dependencies: the page is
   a file in the temp directory and must work with nothing else present. */
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
    ['text', 'muted', 'faint', 'border', 'accent', 'surface', 'bg'].forEach(function (k) {
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
  function toGraph(sx, sy) { return { x: (sx - tx) / scale, y: (sy - ty) / scale }; }
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
  function fit() {
    var t = G.fitTransform(nodes, width, height, 40);
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

  // ---- The simulation: runs while warm, cools to a stop.
  var alpha = 0, running = false, settledOnce = false;
  function warm(a) {
    alpha = Math.max(alpha, a);
    if (!running) { running = true; requestAnimationFrame(tick); }
  }
  function tick() {
    if (alpha < 0.005) {
      running = false; alpha = 0;
      if (!settledOnce) { settledOnce = true; fit(); } else draw();
      return;
    }
    G.step(nodes, edges, alpha);
    alpha *= 0.97;
    draw();
    requestAnimationFrame(tick);
  }

  // ---- Drawing.
  var hover = null, activeNode = null;
  var FONT = 'ui-sans-serif, system-ui, "Segoe UI", sans-serif';

  function neighbourhood(n) {
    var keep = {};
    keep[n.i] = true;
    edges.forEach(function (e) {
      if (e.a === n) keep[e.b.i] = true;
      if (e.b === n) keep[e.a.i] = true;
    });
    return keep;
  }
  function screenRadius(n) {
    if (n.kind === 'area') return n.r * scale;
    return scale < 0.6 ? 2 : n.r * scale;
  }

  function draw() {
    if (!ctx) return;
    var dpr = window.devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    var focus = hover ? neighbourhood(hover) : null;
    var idA = G.labelAlpha('case-id', scale, reduced);
    var titleA = G.labelAlpha('case-title', scale, reduced);

    ctx.lineWidth = 1;
    ctx.strokeStyle = colours.border;
    edges.forEach(function (e) {
      if (e.a.hidden || e.b.hidden) return;
      var dim = focus && !(focus[e.a.i] && focus[e.b.i]);
      ctx.globalAlpha = dim ? 0.25 : 1;
      var a = toScreen(e.a.x, e.a.y), b = toScreen(e.b.x, e.b.y);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
    });

    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';
    nodes.forEach(function (n) {
      if (n.hidden) return;
      var p = toScreen(n.x, n.y);
      var r = screenRadius(n);
      var base = focus && !focus[n.i] ? 0.25 : 1;
      ctx.globalAlpha = base;
      ctx.fillStyle = n.kind === 'area' ? colours.accent : colours.muted;
      ctx.beginPath(); ctx.arc(p.x, p.y, r, 0, Math.PI * 2); ctx.fill();
      if (n === activeNode) {
        ctx.strokeStyle = colours.accent; ctx.lineWidth = 2;
        ctx.beginPath(); ctx.arc(p.x, p.y, r + 3, 0, Math.PI * 2); ctx.stroke();
        ctx.lineWidth = 1; ctx.strokeStyle = colours.border;
      }

      ctx.fillStyle = colours.text;
      if (n.kind === 'area') {
        // Area names keep a fixed screen size: they are the map's titles.
        ctx.font = '600 13px ' + FONT;
        ctx.fillText(n.name + (n.folded ? ' (' + n.count + ')' : ''), p.x, p.y + r + 4);
        return;
      }
      // Case labels scale with the graph and cross-fade from id to title.
      // The id-only and id+title strings share the "#id" prefix, so both are
      // drawn left-aligned from the same x - computed from the id-only
      // string's width - and only the title part visibly fades in; centring
      // each string on its own (different) width would make them ghost.
      // Skip the font/measureText work entirely when no case label draws:
      // hover always draws (both id and title), otherwise at least one of
      // the id/title alphas must be positive.
      if (titleA <= 0 && idA <= 0 && n !== hover) return;
      var size = Math.max(9, Math.min(22, 11 * scale));
      ctx.font = size + 'px ' + FONT;
      var idText = G.caseLabel(n, false);
      ctx.textAlign = 'left';
      var x0 = p.x - ctx.measureText(idText).width / 2;
      if (n === hover) {
        ctx.fillText(G.caseLabel(n, true), x0, p.y + r + 3);
        ctx.textAlign = 'center';
        return;
      }
      if (titleA > 0) {
        ctx.globalAlpha = base * titleA;
        ctx.fillText(G.caseLabel(n, true), x0, p.y + r + 3);
      }
      if (idA > 0 && titleA < 1) {
        ctx.globalAlpha = base * idA * (1 - titleA);
        ctx.fillText(idText, x0, p.y + r + 3);
      }
      ctx.textAlign = 'center';
    });
    ctx.globalAlpha = 1;
    zoomLabel.textContent = Math.round(scale * 100) + '%';
  }

  // ---- Hit testing: the nearest visible node within its radius + 4px.
  function nodeAt(sx, sy) {
    var best = null, bestD = Infinity;
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      if (n.hidden) continue;
      var p = toScreen(n.x, n.y);
      var d = Math.sqrt((p.x - sx) * (p.x - sx) + (p.y - sy) * (p.y - sy));
      if (d <= screenRadius(n) + 4 && d < bestD) { best = n; bestD = d; }
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
    detail.appendChild(el('h2', null, (c.id != null ? '#' + c.id + '  ' : '') + c.title));
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
    // The panel changes the viewport's width.
    resize();
  }
  function hideCase() {
    activeNode = null;
    for (var i = 0; i < listButtons.length; i++) listButtons[i].removeAttribute('aria-current');
    detail.hidden = true;
    detail.innerHTML = '';
    resize();
  }
  for (var b = 0; b < listButtons.length; b++) {
    listButtons[b].addEventListener('click', function () {
      var node = caseNodes[Number(this.getAttribute('data-i'))];
      if (node) showCase(node);
    });
  }

  // ---- Fold.
  function setFolded(area, folded) {
    G.fold(graph, area, folded);
    if (hover && hover.hidden) { hover = null; setCursor(); }
    if (activeNode && activeNode.hidden) hideCase();
    warm(0.5);
    draw(); // a background tab gets no animation frames, so paint now too
  }
  function foldAll(folded) {
    areaNodes.forEach(function (a) { G.fold(graph, a, folded); });
    if (hover && hover.hidden) { hover = null; setCursor(); }
    if (activeNode && activeNode.hidden) hideCase();
    warm(0.5);
    draw(); // a background tab gets no animation frames, so paint now too
  }

  // ---- Pointer: hover, click, drag a node, pan the graph.
  var drag = null; // { node } or { panX, panY }
  var moved = false;
  function setCursor() {
    viewport.classList.toggle('dragging', !!(drag && !drag.node));
    viewport.classList.toggle('over-node', !!(hover || (drag && drag.node)));
  }
  viewport.addEventListener('mousemove', function (e) {
    if (drag) return; // tracked on window, so it keeps going once the pointer leaves
    var p = local(e);
    var h = nodeAt(p.x, p.y);
    if (h !== hover) { hover = h; setCursor(); draw(); }
  });
  viewport.addEventListener('mouseleave', function () {
    if (!drag && hover) { hover = null; setCursor(); draw(); }
  });
  // A node drag or a pan can carry the pointer out of the viewport (into the
  // header, the side panel, or past the window edge); this listener is on
  // window so it keeps updating instead of freezing at the last position
  // inside the viewport.
  window.addEventListener('mousemove', function (e) {
    if (!drag) return;
    if (e.buttons === 0) {
      // The button went up somewhere that never delivered a 'mouseup' (an
      // alt-tab, a release over an OS dialog). Treat it as the drag ending,
      // not as a click - the pointer already moved once since the press.
      moved = true;
      endDrag();
      return;
    }
    var p = local(e);
    if (!moved && (Math.abs(e.clientX - drag.startX) > 4 || Math.abs(e.clientY - drag.startY) > 4)) moved = true;
    if (drag.node) {
      var g = toGraph(p.x, p.y);
      drag.node.x = g.x; drag.node.y = g.y;
      drag.node.vx = 0; drag.node.vy = 0;
      warm(0.3);
    } else {
      tx = p.x - drag.panX; ty = p.y - drag.panY;
      draw();
    }
  });
  viewport.addEventListener('mousedown', function (e) {
    if (e.button !== 0) return;
    moved = false; // every press starts clean
    var p = local(e);
    var n = nodeAt(p.x, p.y);
    if (n) {
      n.pinned = true;
      drag = { node: n, startX: e.clientX, startY: e.clientY };
    } else {
      drag = { panX: p.x - tx, panY: p.y - ty, startX: e.clientX, startY: e.clientY };
    }
    setCursor();
    e.preventDefault();
  });
  function endDrag() {
    if (!drag) return;
    var n = drag.node;
    drag = null;
    if (n) {
      n.pinned = false;
      if (moved) warm(0.5);
      else if (n.kind === 'case') showCase(n);
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
    // Plain wheel pans; Ctrl/Cmd + wheel zooms about the cursor.
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
  if (nodes.length) {
    fit();
    warm(1);
  }
})();
