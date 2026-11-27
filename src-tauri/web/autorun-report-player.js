/* The run report's own video player. Plain ES5: the report opens as a file in
   whatever browser the user has. It makes no network call and runs no
   generated code; the video is a file beside the report. The timeline
   (steps, their actions, the failed step) comes from the page's data blob,
   already worked out by the app from recorded start times.

   The maths and the copy are the app player's (VideoPlayer.tsx and
   videoTimeline.ts); keep the two in step. */
(function () {
  'use strict';

  var SKIP_MS = 5000;
  var PREV_GRACE_MS = 1000;
  var SPEEDS = [0.5, 1, 2];
  var NO_MARKERS = 'No step markers: this video was recorded before they were kept.';
  var VIDEO_WONT_PLAY = 'This video cannot be played here - save it instead.';
  var EMPTY = { title: 'Video', length_ms: null, steps: [], marks: [] };

  // The maths ---------------------------------------------------------------------------------
  function isNum(n) {
    return typeof n === 'number' && isFinite(n);
  }
  // The index of the last start at or before ms (a later one wins a tie); -1 before the first.
  function currentAt(starts, ms) {
    var at = -1;
    for (var i = 0; i < starts.length; i++) {
      if (starts[i] <= ms) at = i;
    }
    return at;
  }
  function startsOf(t) {
    var list = t.steps && t.steps.length ? t.steps : t.marks || [];
    return list.map(function (s) { return s.at_ms; });
  }
  // ] goes to the next start after ms; [ to the last start before ms minus a second's grace.
  function stepTarget(t, ms, dir) {
    var starts = startsOf(t);
    var found = null;
    for (var i = 0; i < starts.length; i++) {
      if (dir > 0 && starts[i] > ms && found === null) found = starts[i];
      if (dir < 0 && starts[i] < ms - PREV_GRACE_MS) found = starts[i];
    }
    return found;
  }
  function timelinePercent(ms, lengthMs) {
    if (!isNum(ms) || !isNum(lengthMs) || lengthMs <= 0) return 0;
    return Math.min(100, Math.max(0, ms * 100 / lengthMs));
  }
  function clockText(ms) {
    var s = Math.floor(Math.max(0, isNum(ms) ? ms : 0) / 1000);
    var r = s % 60;
    return Math.floor(s / 60) + ':' + (r < 10 ? '0' : '') + r;
  }
  function failedFrom(t) {
    for (var i = 0; i < (t.steps || []).length; i++) {
      if (t.steps[i].failed) return t.steps[i].at_ms;
    }
    return null;
  }

  // The player --------------------------------------------------------------------------------
  var root = null;
  var vid = null;
  var tl = EMPTY;
  var opener = null;
  var playing = false;
  var els = {};
  var stepButtons = [];
  var markButtons = [];

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined) e.textContent = text;
    return e;
  }
  function button(label, text, cls) {
    var b = el('button', cls || 'btn', text === undefined ? label : text);
    b.type = 'button';
    b.setAttribute('aria-label', label);
    return b;
  }

  // The video's own length once it is known, else the one recorded with the case.
  function lengthMs() {
    var d = vid ? vid.duration : NaN;
    if (isNum(d) && d > 0) return d * 1000;
    return isNum(tl.length_ms) && tl.length_ms > 0 ? tl.length_ms : 0;
  }
  function nowMs() {
    var t = vid ? vid.currentTime : 0;
    return isNum(t) ? t * 1000 : 0;
  }
  function seek(ms) {
    var len = lengthMs();
    var to = Math.max(0, ms);
    if (len > 0) to = Math.min(len, to);
    vid.currentTime = to / 1000;
    refresh();
  }
  function setPlaying(on) {
    playing = on;
    if (els.play) {
      var label = on ? 'Pause' : 'Play';
      els.play.textContent = label;
      els.play.setAttribute('aria-label', label);
    }
  }
  function toggle() {
    if (!vid) return;
    if (playing) {
      vid.pause();
      setPlaying(false);
      return;
    }
    setPlaying(true);
    var p = vid.play();
    if (p && p.catch) p.catch(function () { setPlaying(false); });
  }
  function place(node, ms, len) {
    node.style.left = timelinePercent(ms, len) + '%';
  }

  // Everything that follows the clock or the length: the playhead, the ticks, the stretch.
  function refresh() {
    if (!root) return;
    var len = lengthMs();
    var now = nowMs();
    var starts = startsOf(tl);
    var idx = currentAt(starts, now);
    var named = tl.steps && tl.steps.length && idx >= 0 ? tl.steps[idx].name : '';
    place(els.head, Math.min(now, len || now), len);
    els.slider.setAttribute('aria-valuemax', String(len / 1000));
    els.slider.setAttribute('aria-valuenow', String(now / 1000));
    els.slider.setAttribute('aria-valuetext', clockText(now) + ' of ' + clockText(len) + (named ? ', ' + named : ''));
    els.time.textContent = clockText(now) + ' / ' + clockText(len);
    els.ticks.forEach(function (t) { t.node.hidden = !len; place(t.node, t.at, len); });
    if (els.fail) {
      els.fail.hidden = !len;
      els.fail.style.left = timelinePercent(failedFrom(tl), len) + '%';
    }
    stepButtons.forEach(function (b, i) {
      if (i === idx) b.setAttribute('aria-current', 'step'); else b.removeAttribute('aria-current');
      b.parentNode.parentNode.className = 'vp__step' + (i === idx ? ' vp__step--now' : '');
    });
    var markAt = tl.steps && tl.steps.length ? -1 : idx;
    markButtons.forEach(function (b, i) {
      if (!b.getAttribute('data-flat')) return;
      if (i === markAt) b.setAttribute('aria-current', 'true'); else b.removeAttribute('aria-current');
    });
  }

  function actionButtons(marks, flat) {
    var ol = el('ol', 'vp__marks');
    marks.forEach(function (m) {
      var li = el('li');
      var b = button(m.label + ' at ' + clockText(m.at_ms), undefined, 'vp__jump');
      b.textContent = '';
      b.appendChild(el('span', 'vp__name', m.label));
      b.appendChild(el('span', 'vp__at', clockText(m.at_ms)));
      if (flat) b.setAttribute('data-flat', '1');
      b.addEventListener('click', function () { seek(m.at_ms); });
      li.appendChild(b);
      ol.appendChild(li);
      markButtons.push(b);
    });
    return ol;
  }

  function stepList() {
    var ol = el('ol', 'vp__steps');
    ol.setAttribute('aria-label', 'Steps');
    tl.steps.forEach(function (s) {
      var li = el('li', 'vp__step');
      var row = el('div', 'vp__row');
      var jump = button(s.name + ' at ' + clockText(s.at_ms) + (s.failed ? ', failed' : ''), undefined, 'vp__jump');
      jump.textContent = '';
      jump.appendChild(el('span', 'vp__name', s.name));
      jump.appendChild(el('span', 'vp__at', clockText(s.at_ms)));
      if (s.failed) jump.appendChild(el('span', 'vp__failed', 'Failed'));
      jump.addEventListener('click', function () { seek(s.at_ms); });
      row.appendChild(jump);
      stepButtons.push(jump);
      if (s.actions && s.actions.length) {
        var tog = button('Actions of ' + s.name, '▸', 'vp__toggle');
        var sub = null;
        var setOpen = function (on) {
          tog.setAttribute('aria-expanded', on ? 'true' : 'false');
          tog.textContent = on ? '▾' : '▸';
          if (on && !sub) {
            sub = actionButtons(s.actions, false);
            li.appendChild(sub);
          } else if (!on && sub) {
            li.removeChild(sub);
            sub = null;
          }
        };
        tog.addEventListener('click', function () { setOpen(!sub); });
        row.appendChild(tog);
        li.appendChild(row);
        setOpen(!!s.failed);
        if (s.failed) els.failedItem = li;
      } else {
        li.appendChild(row);
      }
      ol.appendChild(li);
    });
    return ol;
  }

  function build() {
    stepButtons = [];
    markButtons = [];
    els = { ticks: [], fail: null, failedItem: null };
    var hasSteps = !!(tl.steps && tl.steps.length);
    var hasMarks = !!(tl.marks && tl.marks.length);

    var o = el('div', 'overlay vp');
    o.id = 'vp';
    o.hidden = true;
    o.setAttribute('role', 'dialog');
    o.setAttribute('aria-modal', 'true');
    o.setAttribute('aria-label', tl.title);
    var panel = el('div', 'vp__panel');

    var head = el('div', 'vp__head');
    head.appendChild(el('h2', 'vp__title', tl.title));
    var x = button('Close player', 'Close');
    x.addEventListener('click', close);
    head.appendChild(x);
    panel.appendChild(head);

    var body = el('div', 'vp__body');
    var main = el('div', 'vp__main');
    var stage = el('div', 'vp__stage');
    vid = document.createElement('video');
    vid.preload = 'metadata';
    vid.muted = true;
    vid.setAttribute('playsinline', '');
    vid.setAttribute('aria-label', tl.title);
    stage.appendChild(vid);
    els.msg = el('p', 'vp__msg muted');
    els.msg.hidden = true;
    stage.appendChild(els.msg);
    main.appendChild(stage);

    var slider = el('div', 'vp__bar');
    slider.setAttribute('role', 'slider');
    slider.setAttribute('aria-label', 'Position');
    slider.setAttribute('aria-valuemin', '0');
    slider.tabIndex = 0;
    var failAt = failedFrom(tl);
    if (failAt !== null) {
      els.fail = el('span', 'vp__fail');
      els.fail.style.right = '0';
      slider.appendChild(els.fail);
    }
    (hasSteps ? tl.steps : tl.marks || []).forEach(function (m) {
      var tick = el('span', 'vp__tick');
      tick.title = m.name !== undefined ? m.name : m.label;
      slider.appendChild(tick);
      els.ticks.push({ node: tick, at: m.at_ms });
    });
    els.head = el('span', 'vp__head-mark');
    slider.appendChild(els.head);
    slider.addEventListener('click', function (e) {
      var r = slider.getBoundingClientRect();
      var len = lengthMs();
      if (!r.width || !len) return;
      seek((e.clientX - r.left) / r.width * len);
    });
    els.slider = slider;
    main.appendChild(slider);

    var ctl = el('div', 'vp__ctl');
    els.play = button('Play');
    els.play.addEventListener('click', toggle);
    ctl.appendChild(els.play);
    var prev = button('Previous step');
    prev.addEventListener('click', function () { var t = stepTarget(tl, nowMs(), -1); seek(t === null ? 0 : t); });
    ctl.appendChild(prev);
    var next = button('Next step');
    next.addEventListener('click', function () { var t = stepTarget(tl, nowMs(), 1); if (t !== null) seek(t); });
    ctl.appendChild(next);
    els.time = el('span', 'vp__time muted');
    ctl.appendChild(els.time);
    var speeds = el('div', 'vp__speeds');
    speeds.setAttribute('role', 'group');
    speeds.setAttribute('aria-label', 'Speed');
    SPEEDS.forEach(function (r) {
      var b = button(r + 'x');
      b.setAttribute('aria-pressed', r === 1 ? 'true' : 'false');
      b.addEventListener('click', function () {
        vid.playbackRate = r;
        Array.prototype.forEach.call(speeds.children, function (c) { c.setAttribute('aria-pressed', c === b ? 'true' : 'false'); });
      });
      speeds.appendChild(b);
    });
    ctl.appendChild(speeds);
    els.save = el('a', 'btn', 'Save video');
    els.save.setAttribute('download', '');
    ctl.appendChild(els.save);
    main.appendChild(ctl);
    if (!hasSteps && !hasMarks) main.appendChild(el('p', 'vp__none muted', NO_MARKERS));
    body.appendChild(main);

    if (hasSteps || hasMarks) {
      var side = el('div', 'vp__side');
      side.appendChild(el('h3', 'vp__sub', hasSteps ? 'Steps' : 'Actions'));
      if (hasSteps) {
        side.appendChild(stepList());
      } else {
        var flat = actionButtons(tl.marks, true);
        flat.className = 'vp__steps';
        flat.setAttribute('aria-label', 'Actions');
        side.appendChild(flat);
      }
      body.appendChild(side);
    }
    panel.appendChild(body);
    o.appendChild(panel);

    o.addEventListener('click', function (e) { if (e.target === o) close(); });
    ['timeupdate', 'seeked', 'loadedmetadata', 'durationchange'].forEach(function (n) { vid.addEventListener(n, refresh); });
    vid.addEventListener('play', function () { setPlaying(true); });
    vid.addEventListener('pause', function () { setPlaying(false); });
    vid.addEventListener('ended', function () { setPlaying(false); });
    vid.addEventListener('error', function () {
      els.msg.textContent = VIDEO_WONT_PLAY;
      els.msg.hidden = false;
      els.play.disabled = true;
      setPlaying(false);
    });
    return o;
  }

  // Keys and the Tab trap, on the document only while the player is open.
  function onKey(e) {
    var k = e.key;
    if (k === 'Escape') { e.preventDefault(); close(); return; }
    if (k === 'Tab') {
      var list = Array.prototype.slice.call(root.querySelectorAll('button:not([disabled]), a[href], [tabindex="0"]'));
      if (!list.length) return;
      var first = list[0];
      var last = list[list.length - 1];
      var at = document.activeElement;
      var inside = root.contains(at);
      if (e.shiftKey && (at === first || !inside)) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && (at === last || !inside)) { e.preventDefault(); first.focus(); }
      return;
    }
    var t = e.target;
    var onButton = !!t && t.tagName === 'BUTTON';
    var onLink = !!t && t.tagName === 'A';
    if ((k === ' ' || k === 'Enter') && onButton) return;
    if (k === 'Enter' && onLink) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (k === ' ') { e.preventDefault(); toggle(); }
    else if (k === 'ArrowRight') { e.preventDefault(); seek(nowMs() + SKIP_MS); }
    else if (k === 'ArrowLeft') { e.preventDefault(); seek(nowMs() - SKIP_MS); }
    else if (k === ']' || k === '[') {
      e.preventDefault();
      var to = stepTarget(tl, nowMs(), k === ']' ? 1 : -1);
      if (to !== null) seek(to);
    }
  }

  // Entries with no usable start are dropped, the rest sorted by start (ties keep their order),
  // and a missing label becomes "Step N" / "Action N".
  function tidy(list, fallback, key) {
    var kept = [];
    (list || []).forEach(function (m, i) {
      if (!m || typeof m !== 'object' || !isNum(m.at_ms)) return;
      var c = {};
      for (var p in m) { if (Object.prototype.hasOwnProperty.call(m, p)) c[p] = m[p]; }
      if (typeof c[key] !== 'string' || !c[key]) c[key] = fallback + ' ' + (i + 1);
      c.at_ms = Math.max(0, m.at_ms);
      c.i = i;
      kept.push(c);
    });
    kept.sort(function (a, b) { return a.at_ms - b.at_ms || a.i - b.i; });
    return kept;
  }

  function open(timeline, src, from) {
    if (root) close();
    tl = timeline && typeof timeline === 'object' ? timeline : EMPTY;
    var steps = tidy(tl.steps, 'Step', 'name');
    steps.forEach(function (s) { s.actions = tidy(s.actions, 'Action', 'label'); });
    tl = { title: tl.title || 'Video', length_ms: tl.length_ms, steps: steps, marks: tidy(tl.marks, 'Action', 'label') };
    opener = from || null;
    var old = document.getElementById('vp');
    if (old && old.parentNode) old.parentNode.removeChild(old);
    root = build();
    document.body.appendChild(root);
    vid.src = src;
    els.save.setAttribute('href', src);
    setPlaying(false);
    root.hidden = false;
    refresh();
    document.addEventListener('keydown', onKey);
    toggle();
    if (els.failedItem && els.failedItem.scrollIntoView) els.failedItem.scrollIntoView({ block: 'nearest' });
    els.play.focus();
  }

  function close() {
    if (!root) return;
    document.removeEventListener('keydown', onKey);
    if (vid) {
      if (vid.pause) vid.pause();
      vid.removeAttribute('src');
      if (vid.load) vid.load();
    }
    root.hidden = true;
    root = null;
    setPlaying(false);
    var back = opener;
    opener = null;
    if (back && back.focus) back.focus();
  }

  window.ReportPlayer = {
    open: open,
    close: close,
    isOpen: function () { return !!root; },
    at: currentAt,
    target: stepTarget,
    percent: timelinePercent,
    clock: clockText
  };
})();
