/*
 * The flow page (api_templates/flow_page.rs): the stages sit in columns
 * the browser lays out, so a long title wraps and its box grows. Only the
 * browser knows where each box ended up, so the connectors are drawn here:
 * one curve per `g.edge`, from the right edge of the stage it leaves to the
 * left edge of the stage it reaches, each at mid-height, and each edge's
 * colour blend laid along it. Drawn again when the window changes size,
 * since wrapping can move every box.
 *
 * ES5 on purpose: the page opens in whatever browser the person has.
 */
(function () {
  /** A curve from box `a` to box `b` (rects relative to the canvas) whose
   *  handles run level for half the gap, so it leaves and arrives flat. */
  function wire(a, b) {
    var x1 = a.right, y1 = (a.top + a.bottom) / 2;
    var x2 = b.left, y2 = (b.top + b.bottom) / 2;
    var mid = (x2 - x1) / 2;
    return 'M' + x1 + ' ' + y1 + ' C' + (x1 + mid) + ' ' + y1 + ' ' + (x2 - mid) + ' ' + y2 + ' ' + x2 + ' ' + y2;
  }

  function relative(el, origin) {
    var r = el.getBoundingClientRect();
    return { left: r.left - origin.left, right: r.right - origin.left, top: r.top - origin.top, bottom: r.bottom - origin.top };
  }

  function draw() {
    var canvas = document.getElementById('canvas');
    if (!canvas) return;
    var svg = canvas.querySelector('svg');
    if (!svg) return;
    var origin = canvas.getBoundingClientRect();
    var boxes = {};
    var stages = canvas.querySelectorAll('.stage');
    var i;
    for (i = 0; i < stages.length; i++) {
      boxes[stages[i].getAttribute('data-stage')] = relative(stages[i], origin);
    }
    svg.setAttribute('width', String(canvas.scrollWidth));
    svg.setAttribute('height', String(canvas.scrollHeight));
    var edges = svg.querySelectorAll('g.edge');
    for (i = 0; i < edges.length; i++) {
      var g = edges[i];
      var a = boxes[g.getAttribute('data-from')];
      var b = boxes[g.getAttribute('data-to')];
      if (!a || !b) continue;
      var d = wire(a, b);
      var paths = g.querySelectorAll('path');
      for (var p = 0; p < paths.length; p++) paths[p].setAttribute('d', d);
      var grad = document.getElementById(g.getAttribute('data-grad'));
      if (grad) {
        grad.setAttribute('x1', String(a.right));
        grad.setAttribute('x2', String(b.left));
      }
    }
  }

  var pending = false;
  function redraw() {
    if (pending) return;
    pending = true;
    (window.requestAnimationFrame || function (f) { setTimeout(f, 16); })(function () {
      pending = false;
      draw();
    });
  }

  window.FlowPage = { wire: wire, draw: draw };
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', draw);
  } else {
    draw();
  }
  // Fonts arriving late can re-wrap a title; so can a resize.
  window.addEventListener('load', draw);
  window.addEventListener('resize', redraw);
})();
