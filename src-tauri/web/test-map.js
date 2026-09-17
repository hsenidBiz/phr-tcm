/* Draws the Test map from the #map-data JSON block: a collapsible tree of
   areas with their cases, zoom and pan on the canvas, and a side panel for
   the case that was clicked. Plain script, no dependencies - the page is a
   file in the temp directory and must work with nothing else present. */
(function () {
  var data = JSON.parse(document.getElementById('map-data').textContent || '[]');
  var canvas = document.getElementById('canvas');
  var viewport = document.getElementById('viewport');
  var detail = document.getElementById('detail');
  var zoomLabel = document.getElementById('map-zoom');

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  // ---- The tree -----------------------------------------------------
  var active = null;

  function renderNode(node) {
    var li = el('li', 'node');
    var row = el('div', 'row');
    var fold = el('button', 'fold');
    fold.type = 'button';
    row.appendChild(fold);
    row.appendChild(el('span', 'name', node.name));
    row.appendChild(el('span', 'count', String(node.count)));
    li.appendChild(row);

    var body = el('div', 'body');
    if (node.cases.length) {
      var ul = el('ul', 'cases');
      node.cases.forEach(function (c) {
        var item = el('li');
        var btn = el('button', 'case');
        btn.type = 'button';
        btn.appendChild(el('span', 'wid', c.id != null ? '#' + c.id : 'NEW'));
        btn.appendChild(el('span', 'title', c.title));
        btn.addEventListener('click', function (ev) {
          ev.stopPropagation();
          showCase(c, btn);
        });
        item.appendChild(btn);
        ul.appendChild(item);
      });
      body.appendChild(ul);
    }
    if (node.children.length) body.appendChild(renderBranch(node.children));
    li.appendChild(body);

    function setOpen(open) {
      li.setAttribute('data-open', String(open));
      fold.setAttribute('aria-expanded', String(open));
      fold.setAttribute('aria-label', (open ? 'Collapse ' : 'Expand ') + node.name);
      fold.textContent = open ? '▾' : '▸';
    }
    li.setOpen = setOpen;
    setOpen(true);
    row.addEventListener('click', function () {
      if (moved) return; // a drag that started on the row must not also toggle it
      setOpen(li.getAttribute('data-open') !== 'true');
    });
    return li;
  }

  function renderBranch(nodes) {
    var ul = el('ul', 'branch');
    nodes.forEach(function (n) { ul.appendChild(renderNode(n)); });
    return ul;
  }

  function setAll(open) {
    var nodes = canvas.querySelectorAll('li.node');
    for (var i = 0; i < nodes.length; i++) nodes[i].setOpen(open);
  }

  // ---- The side panel -----------------------------------------------
  function showCase(c, btn) {
    if (active) active.classList.remove('active');
    active = btn;
    btn.classList.add('active');
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
    if (c.steps.length === 0) {
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
  }

  function hideCase() {
    if (active) active.classList.remove('active');
    active = null;
    detail.hidden = true;
    detail.innerHTML = '';
  }

  // ---- Zoom and pan -------------------------------------------------
  var scale = 1, tx = 16, ty = 16;
  function apply() {
    canvas.style.transform = 'translate(' + tx + 'px,' + ty + 'px) scale(' + scale + ')';
    zoomLabel.textContent = Math.round(scale * 100) + '%';
  }
  function zoom(factor, cx, cy) {
    var next = Math.min(3, Math.max(0.3, scale * factor));
    // Zoom about the pointer (or the viewport's centre), so the spot under
    // the cursor stays put.
    var rect = viewport.getBoundingClientRect();
    var px = (cx == null ? rect.width / 2 : cx - rect.left);
    var py = (cy == null ? rect.height / 2 : cy - rect.top);
    tx = px - (px - tx) * (next / scale);
    ty = py - (py - ty) * (next / scale);
    scale = next;
    apply();
  }
  document.getElementById('map-in').addEventListener('click', function () { zoom(1.2); });
  document.getElementById('map-out').addEventListener('click', function () { zoom(1 / 1.2); });
  document.getElementById('map-reset').addEventListener('click', function () { scale = 1; tx = 16; ty = 16; apply(); });
  document.getElementById('map-expand').addEventListener('click', function () { setAll(true); });
  document.getElementById('map-collapse').addEventListener('click', function () { setAll(false); });

  viewport.addEventListener('wheel', function (e) {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      zoom(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientX, e.clientY);
      return;
    }
    // Plain wheel pans the tree vertically, so a tall tree can be
    // scrolled with the wheel; Ctrl/Cmd + wheel zooms instead.
    e.preventDefault();
    ty -= e.deltaY;
    apply();
  }, { passive: false });

  var drag = null;
  var moved = false;
  viewport.addEventListener('mousedown', function (e) {
    if (e.button !== 0) return;
    if (e.target.closest('button')) return; // a click, not a drag
    moved = false;
    drag = { x: e.clientX - tx, y: e.clientY - ty, startX: e.clientX, startY: e.clientY };
    viewport.classList.add('dragging');
  });
  window.addEventListener('mousemove', function (e) {
    if (!drag) return;
    if (!moved && (Math.abs(e.clientX - drag.startX) > 4 || Math.abs(e.clientY - drag.startY) > 4)) {
      moved = true;
    }
    tx = e.clientX - drag.x;
    ty = e.clientY - drag.y;
    apply();
  });
  window.addEventListener('mouseup', function () {
    drag = null;
    viewport.classList.remove('dragging');
  });

  // ---- Go ------------------------------------------------------------
  if (data.length === 0) {
    canvas.appendChild(el('p', 'empty', 'No test cases to map.'));
  } else {
    canvas.appendChild(renderBranch(data));
  }
  apply();
})();
