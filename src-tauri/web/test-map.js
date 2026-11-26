/* Draws the Test map from the #map-data JSON block as an indented outline:
   one 24px row per area and per case, laid out by test-map-graph.js
   (window.testMap), guide lines turning with 8px elbows from an area down
   to what hangs off it. An area's row says its name and its case count;
   a case's row says "#id Title" and whether an upload would create it
   (NEW) or update it (UPDATE) - none of it waits for a click or a zoom.
   Clicking an area folds it, clicking a case opens the side panel. Plain
   script, no dependencies: the page is a file in the temp directory and
   must work with nothing else present. */
(function () {
  var dataEl = document.getElementById('map-data');
  if (!dataEl || !window.testMap) return;
  var G = window.testMap;
  var data = JSON.parse(dataEl.textContent || '[]');
  var canvas = document.getElementById('graph');
  var viewport = document.getElementById('viewport');
  var detail = document.getElementById('detail');
  var ctx = canvas.getContext ? canvas.getContext('2d') : null;

  var graph = G.buildGraph(data);
  var nodes = graph.nodes;
  var caseNodes = [];
  var areaNodes = [];
  nodes.forEach(function (n) { (n.kind === 'case' ? caseNodes : areaNodes).push(n); });
  var extent = G.layoutTree(graph);

  // Type: area names are the outline's headings, 13px at weight 600; a case
  // is 13px regular; its tag is 10px. Padding round the outline, the gap
  // from a dot to its label, and the room a case title is never squeezed
  // below (past that, the outline pans sideways instead).
  var FONT = 'ui-sans-serif, system-ui, "Segoe UI", sans-serif';
  var FONT_AREA = '600 13px ' + FONT, FONT_CASE = '13px ' + FONT, FONT_TAG = '600 10px ' + FONT;
  var PAD = 16, LABEL_X = 12, TAG_GAP = 8, TAG_PAD = 5, MIN_TITLE = 160;

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
    ['text', 'muted', 'accent', 'surface-2'].forEach(function (k) {
      colours[k] = cs.getPropertyValue('--' + k).trim();
    });
  }
  readColours();
  if (window.MutationObserver) {
    new MutationObserver(function () { readColours(); draw(); })
      .observe(document.documentElement, { attributes: true, attributeFilter: ['data-scheme'] });
  }

  // ---- The view: graph units -> screen px, never scaled; only moved.
  var tx = PAD, ty = PAD, width = 0, height = 0, contentW = 0, contentH = 0;
  function measure(font, s) {
    if (!ctx) return String(s).length * 7;
    ctx.font = font;
    return ctx.measureText(s).width;
  }
  // How far the outline reaches: the widest area row, and for a case its
  // whole row up to a cap, so a very long title cannot make it pan forever.
  function measureContent() {
    var w = 0;
    nodes.forEach(function (n) {
      if (n.hidden) return;
      var right = n.x + LABEL_X;
      if (n.kind === 'area') {
        right += measure(FONT_AREA, G.areaLabel(n));
      } else {
        right += Math.min(480, measure(FONT_CASE, G.caseLabel(n)) + TAG_GAP + measure(FONT_TAG, G.caseTag(n)) + 2 * TAG_PAD);
      }
      if (right > w) w = right;
    });
    contentW = w;
    contentH = extent.height;
  }
  function clamp() {
    var t = G.clampView(tx, ty, contentW, contentH, width, height, PAD);
    tx = t.tx; ty = t.ty;
  }
  function resize() {
    var r = viewport.getBoundingClientRect();
    width = r.width; height = r.height;
    var dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width = width + 'px';
    canvas.style.height = height + 'px';
    clamp();
    draw();
  }
  // Becoming visible (a hidden tab, a panel resize) fires no window
  // 'resize'; the observer covers it, the window listener below is the
  // fallback for browsers without one.
  if (window.ResizeObserver) { new ResizeObserver(resize).observe(viewport); }

  // ---- Drawing.
  var hover = null, activeNode = null;

  // A hover lights the chain from the top-level area down to the node: the
  // dots, and the guide between each pair.
  function chainFocus(n) {
    var chain = G.pathTo(n), keep = {};
    for (var i = 0; i < chain.length; i++) keep[chain[i].i] = true;
    return { keep: keep, chain: chain };
  }

  function strokePath(d) {
    if (typeof Path2D === 'function') ctx.stroke(new Path2D(d));
  }

  function draw() {
    if (!ctx) return;
    var dpr = window.devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    var focus = hover ? chainFocus(hover) : null;
    // Only the rows on screen are drawn: a 500-row outline is mostly
    // off-screen at any one time.
    var top = -ty - G.ROW, bottom = height - ty + G.ROW;
    function onScreen(n) { return n.y >= top && n.y <= bottom; }

    // Row backgrounds: a faint band under the hovered row, a tinted one
    // under the selected case.
    if (hover && !hover.hidden) {
      ctx.fillStyle = colours['surface-2'];
      ctx.fillRect(0, ty + hover.y - G.ROW / 2, width, G.ROW);
    }
    if (activeNode && !activeNode.hidden) {
      ctx.globalAlpha = 0.14;
      ctx.fillStyle = colours.accent;
      ctx.fillRect(0, ty + activeNode.y - G.ROW / 2, width, G.ROW);
      ctx.globalAlpha = 1;
    }

    // Guides: a trunk down from each area's dot, an elbow off it to each
    // kid. Muted, so a limb reaches 3:1 on the page in either scheme; dimmed
    // while a chain is lit, and the chain itself drawn over them in the
    // accent.
    ctx.save();
    ctx.translate(tx, ty);
    ctx.lineWidth = 1.5;
    ctx.lineCap = 'round';
    ctx.strokeStyle = colours.muted;
    ctx.globalAlpha = focus ? 0.35 : 1;
    areaNodes.forEach(function (a) {
      if (a.hidden) return;
      var kids = G.kidsOf(a);
      if (!kids.length || a.folded) return;
      if (!(onScreen(a) || onScreen(kids[kids.length - 1]) || (a.y < top && kids[kids.length - 1].y > bottom))) return;
      strokePath(G.trunkPath(a));
      kids.forEach(function (k) { if (onScreen(k)) strokePath(G.elbowPath(a, k)); });
    });
    if (focus) {
      ctx.globalAlpha = 1;
      ctx.strokeStyle = colours.accent;
      ctx.lineWidth = 2;
      for (var c = 0; c + 1 < focus.chain.length; c++) strokePath(G.linkPath(focus.chain[c], focus.chain[c + 1]));
    }
    ctx.restore();

    ctx.textAlign = 'left';
    ctx.textBaseline = 'middle';
    nodes.forEach(function (n) {
      if (n.hidden || !onScreen(n)) return;
      var px = tx + n.x, py = ty + n.y;
      var lit = !!(focus && focus.keep[n.i]);
      var selected = n === activeNode;
      // The dot: an area is a ring while open and a filled dot while
      // folded; a case is a filled dot. The accent is for the lit chain and
      // the selected case only.
      ctx.lineWidth = 1.5;
      ctx.strokeStyle = lit || selected ? colours.accent : colours.muted;
      ctx.fillStyle = lit || selected ? colours.accent : colours.muted;
      ctx.beginPath(); ctx.arc(px, py, G.DOT_R, 0, Math.PI * 2);
      if (n.kind === 'area' && !n.folded && !selected) ctx.stroke(); else ctx.fill();
      if (selected) {
        ctx.beginPath(); ctx.arc(px, py, G.DOT_R + 3, 0, Math.PI * 2); ctx.stroke();
      }

      var x0 = px + LABEL_X;
      if (n.kind === 'area') {
        // "Leave · 14": the name and the count, in full - never clipped.
        ctx.font = FONT_AREA;
        ctx.fillStyle = colours.text;
        ctx.fillText(G.areaLabel(n), x0, py);
        return;
      }
      // "#id Title" and its tag. Only the title may be cut, to the room the
      // row has left (never less than MIN_TITLE: past that the outline pans).
      var idStr = n.data.id != null ? '#' + n.data.id + ' ' : '';
      ctx.font = FONT_CASE;
      ctx.fillStyle = colours.muted;
      ctx.fillText(idStr, x0, py);
      var idW = ctx.measureText(idStr).width;
      var tag = G.caseTag(n);
      var tagW = measure(FONT_TAG, tag) + 2 * TAG_PAD;
      var room = Math.max(MIN_TITLE, width - x0 - idW - tagW - TAG_GAP - PAD);
      ctx.font = FONT_CASE;
      var title = G.clipLabel(n.data.title || '', room, function (s) { return ctx.measureText(s).width; });
      ctx.fillStyle = colours.text;
      ctx.fillText(title, x0 + idW, py);
      var tagX = x0 + idW + ctx.measureText(title).width + TAG_GAP;
      // The tag: muted, outlined, its words telling NEW from UPDATE.
      ctx.lineWidth = 1;
      ctx.strokeStyle = colours.muted;
      ctx.beginPath();
      if (ctx.roundRect) ctx.roundRect(tagX + 0.5, py - 8 + 0.5, tagW - 1, 15, 4); else ctx.rect(tagX + 0.5, py - 8 + 0.5, tagW - 1, 15);
      ctx.stroke();
      ctx.font = FONT_TAG;
      ctx.fillStyle = colours.muted;
      ctx.fillText(tag, tagX + TAG_PAD, py + 0.5);
    });
  }

  // ---- Hit testing: a row, from its dot (and a little left of it) to the
  // right edge - the whole row is the target.
  function nodeAt(sx, sy) {
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      if (n.hidden) continue;
      if (Math.abs(sy - (ty + n.y)) > G.ROW / 2) continue;
      if (sx < tx + n.x - G.DOT_R - 4) continue;
      return n;
    }
    return null;
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

  // ---- Fold: the outline re-lays out around the change.
  function afterFold() {
    extent = G.layoutTree(graph);
    measureContent();
    clamp();
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
  // started; a press that does not move on a row is a click on it.
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
    if (h !== hover) { hover = h; setCursor(); draw(); }
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
      clamp();
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
  // The wheel scrolls the outline like a document.
  viewport.addEventListener('wheel', function (e) {
    e.preventDefault();
    tx -= e.deltaX; ty -= e.deltaY;
    clamp();
    draw();
  }, { passive: false });

  // ---- Chrome.
  document.getElementById('map-expand').addEventListener('click', function () { foldAll(false); });
  document.getElementById('map-collapse').addEventListener('click', function () { foldAll(true); });
  window.addEventListener('resize', resize);

  // ---- Go.
  measureContent();
  resize();
})();
