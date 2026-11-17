(function () {
  'use strict';
  var D = {};
  try {
    var blob = document.getElementById('report-data');
    D = JSON.parse(blob ? blob.textContent : '{}') || {};
  } catch (e) { D = {}; }

  var root = document.documentElement;
  var app = document.querySelector('.app');
  var $ = function (sel, from) { return (from || document).querySelector(sel); };
  var $$ = function (sel, from) { return Array.prototype.slice.call((from || document).querySelectorAll(sel)); };
  var NAME = /^(shot|video)-[0-9]+-[0-9]+\.(jpg|mp4)$/;
  var CASE_VIEWS = { failures: 1, flaky: 1, all: 1 };
  var ORDER = ['failures', 'all', 'insights', 'history', 'gallery'];

  var current = '';
  var selected = '';
  var filter = $('#filter');

  // Local times -----------------------------------------------------------
  function local(ms) {
    try {
      return new Date(Number(ms)).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
    } catch (e) { return ''; }
  }
  $$('time[data-ts]').forEach(function (t) {
    var s = local(t.getAttribute('data-ts'));
    if (s) t.textContent = s;
  });
  $$('title[data-ts]').forEach(function (t) {
    var s = local(t.getAttribute('data-ts'));
    if (s) t.textContent = s + ': ' + (t.getAttribute('data-val') || '');
  });

  // Light and dark ----------------------------------------------------------
  var themeBtn = $('#theme-btn');
  if (themeBtn) {
    themeBtn.addEventListener('click', function () {
      var now = root.getAttribute('data-mode');
      if (!now) now = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
      var next = now === 'dark' ? 'light' : 'dark';
      root.setAttribute('data-mode', next);
      try { localStorage.setItem('tcm-report-mode', next); } catch (e) { /* file:// */ }
    });
  }

  // Sidebar -------------------------------------------------------------------
  function rail(open) {
    if (!app) return;
    app.setAttribute('data-rail', open ? 'open' : 'closed');
  }
  function railToggle() {
    rail(!app || app.getAttribute('data-rail') !== 'open');
  }
  if (window.innerWidth < 900) rail(false);
  var railBtn = $('#rail-toggle');
  if (railBtn) railBtn.addEventListener('click', railToggle);

  // Views ----------------------------------------------------------------------
  function rows(kind) {
    return $$('#list-' + kind + ' .row');
  }
  function visibleRows() {
    if (!CASE_VIEWS[current]) return [];
    return rows(current).filter(function (r) {
      return !r.hidden && !(r.closest('.grp') && r.closest('.grp').classList.contains('closed'));
    });
  }
  function countText() {
    var c = $('#list-count');
    var list = $('#list-' + current);
    if (!c || !list) return;
    var total = rows(current).length;
    var shown = rows(current).filter(function (r) { return !r.hidden; }).length;
    c.textContent = shown === total ? String(total) : shown + ' of ' + total;
  }

  function select(id, quiet) {
    selected = String(id || '');
    $$('.dt').forEach(function (d) { d.classList.toggle('on', d.getAttribute('data-id') === selected); });
    $$('.row').forEach(function (r) { r.setAttribute('aria-selected', r.getAttribute('data-id') === selected ? 'true' : 'false'); });
    var empty = $('.detail__empty');
    if (empty) empty.hidden = !!selected;
    var pane = $('#details');
    if (pane) pane.scrollTop = 0;
    if (!quiet) {
      var row = visibleRows().filter(function (r) { return r.getAttribute('data-id') === selected; })[0];
      if (row && row.scrollIntoView) row.scrollIntoView({ block: 'nearest' });
    }
  }

  function nav(name) {
    if (!name) return;
    var isCases = !!CASE_VIEWS[name];
    var target = isCases ? 'cases' : name;
    if (!$('#view-' + target)) return;
    current = name;
    $$('.view').forEach(function (v) { v.classList.toggle('on', v.id === 'view-' + target); });
    $$('.rail__item').forEach(function (a) {
      if (a.getAttribute('data-nav') === name) a.setAttribute('aria-current', 'page');
      else a.removeAttribute('aria-current');
    });
    if (isCases) {
      $$('.list').forEach(function (l) { l.hidden = l.id !== 'list-' + name; });
      var list = $('#list-' + name);
      var title = $('#list-title');
      if (title && list) title.textContent = list.getAttribute('data-title');
      countText();
      var vis = visibleRows();
      var keep = vis.filter(function (r) { return r.getAttribute('data-id') === selected; })[0];
      if (!keep) select(vis.length ? vis[0].getAttribute('data-id') : '', true);
      else select(selected, true);
    }
    if (name === 'gallery') renderGallery(false);
    if (window.innerWidth < 900) rail(false);
  }

  $$('a[data-nav]').forEach(function (a) {
    a.addEventListener('click', function (e) {
      e.preventDefault();
      nav(a.getAttribute('data-nav'));
    });
  });
  $$('button[data-open]').forEach(function (b) {
    b.addEventListener('click', function () {
      if (filter) filter.value = b.getAttribute('data-filter') || '';
      applyFilter();
      nav(b.getAttribute('data-open'));
    });
  });

  function goCase(id) {
    var target = current;
    var here = CASE_VIEWS[current] && visibleRows().some(function (r) { return r.getAttribute('data-id') === String(id); });
    if (!here) {
      target = 'all';
      if (filter && filter.value) { filter.value = ''; applyFilter(); }
    }
    nav(target);
    select(id);
  }
  document.addEventListener('click', function (e) {
    var a = e.target.closest ? e.target.closest('a[data-case]') : null;
    if (a) {
      e.preventDefault();
      goCase(a.getAttribute('data-case'));
      return;
    }
    var row = e.target.closest ? e.target.closest('.row') : null;
    if (row) { select(row.getAttribute('data-id')); return; }
    var grp = e.target.closest ? e.target.closest('.group') : null;
    if (grp) {
      var open = grp.getAttribute('aria-expanded') !== 'true';
      grp.setAttribute('aria-expanded', open ? 'true' : 'false');
      grp.parentNode.classList.toggle('closed', !open);
      return;
    }
    var st = e.target.closest ? e.target.closest('button[data-step]') : null;
    if (st) { step(Number(st.getAttribute('data-step'))); return; }
    var close = e.target.closest ? e.target.closest('[data-close]') : null;
    if (close) {
      var h = $('#help');
      if (h) h.hidden = true;
    }
  });

  function step(delta) {
    var list = visibleRows();
    if (!list.length) return;
    var i = -1;
    list.forEach(function (r, k) { if (r.getAttribute('data-id') === selected) i = k; });
    var next = i < 0 ? (delta > 0 ? 0 : list.length - 1) : Math.max(0, Math.min(list.length - 1, i + delta));
    select(list[next].getAttribute('data-id'));
  }

  // History tabs -------------------------------------------------------------------
  $$('.tab[data-tab]').forEach(function (t) {
    t.addEventListener('click', function () {
      var which = t.getAttribute('data-tab');
      $$('.tab[data-tab]').forEach(function (o) { o.setAttribute('aria-selected', o === t ? 'true' : 'false'); });
      var a = $('#hist-trends'), b = $('#hist-compare');
      if (a) a.hidden = which !== 'trends';
      if (b) b.hidden = which !== 'compare';
    });
  });

  // Filter -----------------------------------------------------------------------------
  var KEYS = ['status', 'is', 'area', 'module', 'grade', 'cause'];
  function tokens(text) {
    var out = [];
    var re = /(?:([a-z]+):)?(?:"([^"]*)"|(\S*))/gi;
    var m;
    while ((m = re.exec(text)) !== null) {
      if (m[0] === '') { re.lastIndex++; continue; }
      var key = m[1] ? m[1].toLowerCase() : '';
      var val = (m[2] !== undefined ? m[2] : m[3] || '').toLowerCase();
      if (key && KEYS.indexOf(key) < 0) { val = (m[1] + ':' + val).toLowerCase(); key = ''; }
      // A known key with nothing after it yet is still being typed.
      if (key && val === '') continue;
      if (val !== '') out.push({ key: key, val: val });
    }
    return out;
  }

  function matches(item, toks) {
    var d = item.dataset;
    var groups = {};
    for (var i = 0; i < toks.length; i++) {
      var t = toks[i];
      var ok;
      switch (t.key) {
        case 'status': ok = (d.status || '') === t.val.replace(/[ _-]/g, ''); break;
        case 'is': ok = t.val === 'flaky' ? d.flaky === '1' : false; break;
        case 'area': ok = (d.area || '').toLowerCase().indexOf(t.val) >= 0; break;
        case 'module': ok = (d.module || '').toLowerCase().indexOf(t.val) >= 0; break;
        case 'grade': ok = (d.grade || '-').toLowerCase() === t.val; break;
        case 'cause':
          if (/^c[0-9]+$/.test(t.val)) ok = (d.cause || '').toLowerCase().split(' ')[0] === t.val;
          else ok = (d.cause || '').toLowerCase().indexOf(t.val) >= 0;
          break;
        default:
          if ((d.text || '').toLowerCase().indexOf(t.val) < 0) return false;
          continue;
      }
      // The same key twice is "either", different keys are "and".
      if (groups[t.key] === undefined) groups[t.key] = false;
      if (ok) groups[t.key] = true;
    }
    for (var k in groups) if (groups[k] === false) return false;
    return true;
  }

  function applyFilter() {
    var toks = tokens(filter ? filter.value : '');
    $$('.row.f-item').forEach(function (it) { it.hidden = toks.length > 0 && !matches(it, toks); });
    $$('.grp').forEach(function (g) {
      g.hidden = $$('.row', g).length > 0 && $$('.row', g).every(function (r) { return r.hidden; });
    });
    countText();
    if (CASE_VIEWS[current]) {
      var vis = visibleRows();
      var keep = vis.some(function (r) { return r.getAttribute('data-id') === selected; });
      if (!keep) select(vis.length ? vis[0].getAttribute('data-id') : '', true);
    }
  }
  if (filter) filter.addEventListener('input', applyFilter);
  var searchBtn = $('#search-btn');
  function focusFilter() {
    if (!CASE_VIEWS[current]) nav('failures');
    if (filter) filter.focus();
  }
  if (searchBtn) searchBtn.addEventListener('click', focusFilter);

  // Copy ----------------------------------------------------------------------------------
  var ASK = 'Explain the likely cause and the next check.';
  function copyText(text, done) {
    var fallback = function () {
      var ta = document.createElement('textarea');
      ta.value = text;
      ta.setAttribute('readonly', '');
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      var ok = false;
      try { ok = document.execCommand('copy'); } catch (e) { ok = false; }
      document.body.removeChild(ta);
      done(ok);
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(function () { done(true); }, fallback);
    } else {
      fallback();
    }
  }
  document.addEventListener('click', function (e) {
    var b = e.target.closest ? e.target.closest('button[data-copy]') : null;
    if (!b) return;
    var s = D.sum && D.sum[b.getAttribute('data-id')];
    if (!s) return;
    var text = b.getAttribute('data-copy') === 'prompt'
      ? 'This automated browser test failed. Here is the failure:\n\n' + s + '\n\n' + ASK
      : s;
    var kbd = $('.kbd', b);
    var label = b.getAttribute('data-label');
    if (!label) { label = b.textContent; b.setAttribute('data-label', label); }
    copyText(text, function (ok) {
      var span = document.createElement('span');
      span.textContent = ok ? 'Copied' : 'Could not copy';
      var saved = b.innerHTML;
      b.textContent = '';
      b.appendChild(span);
      setTimeout(function () { b.innerHTML = saved; }, 1500);
    });
    void kbd;
  });

  // Gallery and lightbox -----------------------------------------------------------------
  var all = (D.gallery || []).filter(function (g) { return NAME.test(g.n); });
  var shown = [];
  var galTab = 'all';
  var galCount = 0;
  var PAGE = 48;
  function caption(g) {
    return '#' + g.c + ' ' + (g.k ? 'Video' : g.s);
  }
  function galList() {
    return all.filter(function (g) { return galTab === 'all' || (galTab === 'video') === !!g.k; });
  }
  function renderGallery(more) {
    var grid = $('#gal-grid');
    if (!grid) return;
    if (!more && grid.children.length) return;
    if (!more) { shown = galList(); galCount = 0; grid.textContent = ''; }
    var end = Math.min(shown.length, galCount + PAGE);
    for (var i = galCount; i < end; i++) {
      (function (i) {
        var g = shown[i];
        var li = document.createElement('li');
        li.className = 'gallery__tile';
        var b = document.createElement('button');
        b.type = 'button';
        b.className = 'gallery__media';
        b.setAttribute('aria-label', 'Open ' + caption(g));
        if (g.k) {
          b.textContent = 'Video';
        } else {
          var img = document.createElement('img');
          img.loading = 'lazy';
          img.alt = caption(g);
          img.src = '../shots/' + g.n;
          b.appendChild(img);
        }
        b.addEventListener('click', function () { openLightbox(i); });
        var cap = document.createElement('span');
        cap.className = 'gallery__caption';
        cap.textContent = caption(g);
        li.appendChild(b);
        li.appendChild(cap);
        grid.appendChild(li);
      })(i);
    }
    galCount = end;
    var btn = $('#gal-more');
    if (btn) btn.hidden = galCount >= shown.length;
  }
  var moreBtn = $('#gal-more');
  if (moreBtn) moreBtn.addEventListener('click', function () { renderGallery(true); });
  $$('.tab[data-gtab]').forEach(function (t) {
    t.addEventListener('click', function () {
      galTab = t.getAttribute('data-gtab');
      $$('.tab[data-gtab]').forEach(function (o) { o.setAttribute('aria-selected', o === t ? 'true' : 'false'); });
      var grid = $('#gal-grid');
      if (grid) grid.textContent = '';
      renderGallery(false);
    });
  });

  var lb = $('#lb');
  var lbIndex = 0;
  function openLightbox(i) {
    if (!lb || !shown.length) return;
    lbIndex = (i + shown.length) % shown.length;
    var g = shown[lbIndex];
    var stage = $('.stage', lb);
    while (stage.firstChild) stage.removeChild(stage.firstChild);
    var el;
    if (g.k) {
      el = document.createElement('video');
      el.controls = true;
      el.src = '../shots/' + g.n;
    } else {
      el = document.createElement('img');
      el.src = '../shots/' + g.n;
      el.alt = caption(g);
    }
    stage.appendChild(el);
    $('.lb-cap', lb).textContent = caption(g) + '  (' + (lbIndex + 1) + ' of ' + shown.length + ')';
    lb.hidden = false;
  }
  function closeLightbox() {
    if (!lb) return;
    var v = $('video', lb);
    if (v && v.pause) v.pause();
    lb.hidden = true;
  }
  if (lb) {
    $('.lb-x', lb).addEventListener('click', closeLightbox);
    $('.lb-prev', lb).addEventListener('click', function () { openLightbox(lbIndex - 1); });
    $('.lb-next', lb).addEventListener('click', function () { openLightbox(lbIndex + 1); });
    lb.addEventListener('click', function (e) { if (e.target === lb) closeLightbox(); });
  }

  // Keys --------------------------------------------------------------------------------------
  var help = $('#help');
  if (help) help.addEventListener('click', function (e) { if (e.target === help) help.hidden = true; });
  var keysBtn = $('#keys-btn');
  if (keysBtn) keysBtn.addEventListener('click', function () { if (help) help.hidden = !help.hidden; });

  function typing(t) {
    return !!t && (t.isContentEditable || t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT');
  }
  function overlayOpen() {
    return (help && !help.hidden) || (lb && !lb.hidden);
  }
  var chord = 0;
  var CHORDS = { f: 'failures', l: 'flaky', i: 'insights', h: 'history' };

  document.addEventListener('keydown', function (e) {
    var mod = e.ctrlKey || e.metaKey;
    var k = e.key;
    if (mod && (k === 'b' || k === 'B')) { e.preventDefault(); railToggle(); return; }
    if (mod && (k === 'k' || k === 'K')) { e.preventDefault(); focusFilter(); return; }
    if (lb && !lb.hidden) {
      if (k === 'Escape') { closeLightbox(); e.preventDefault(); }
      else if (k === 'ArrowRight') { openLightbox(lbIndex + 1); e.preventDefault(); }
      else if (k === 'ArrowLeft') { openLightbox(lbIndex - 1); e.preventDefault(); }
      return;
    }
    if (k === 'Escape') {
      if (help && !help.hidden) { help.hidden = true; e.preventDefault(); return; }
      if (typing(e.target)) {
        if (filter && e.target === filter && filter.value) { filter.value = ''; applyFilter(); } else e.target.blur();
        e.preventDefault();
      }
      return;
    }
    if (mod || e.altKey || typing(e.target) || overlayOpen()) return;

    if (chord && Date.now() - chord < 1200) {
      chord = 0;
      if (CHORDS[k]) { e.preventDefault(); nav(CHORDS[k]); }
      return;
    }
    chord = 0;
    if (k === 'g') { chord = Date.now(); return; }
    if (k === 'j' || k === 'ArrowDown') { if (CASE_VIEWS[current]) { e.preventDefault(); step(1); } }
    else if (k === 'k' || k === 'ArrowUp') { if (CASE_VIEWS[current]) { e.preventDefault(); step(-1); } }
    else if (k === '/') { e.preventDefault(); focusFilter(); }
    else if (k === '?') { e.preventDefault(); if (help) help.hidden = !help.hidden; }
    else if (k === 'c') {
      var b = selected ? $('#case-' + selected + ' [data-copy="summary"]') : null;
      if (b) b.click();
    } else if (k === 'Enter') {
      var pane = $('#details');
      if (pane && selected) { pane.setAttribute('tabindex', '-1'); pane.focus(); }
    } else if (/^[1-5]$/.test(k)) { nav(ORDER[Number(k) - 1]); }
  });

  // Paper: every case that is shown in full has its sections open.
  window.addEventListener('beforeprint', function () {
    $$('.dt[data-print="1"] details').forEach(function (d) { d.open = true; });
  });

  var start = document.body.getAttribute('data-start') || 'insights';
  nav(start);
  if (/^#case-[0-9]+$/.test(location.hash)) goCase(location.hash.slice(6));
})();
