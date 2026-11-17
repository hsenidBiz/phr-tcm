(function () {
  'use strict';
  var D = {};
  try {
    var blob = document.getElementById('report-data');
    D = JSON.parse(blob ? blob.textContent : '{}') || {};
  } catch (e) { D = {}; }

  var $ = function (sel, root) { return (root || document).querySelector(sel); };
  var $$ = function (sel, root) { return Array.prototype.slice.call((root || document).querySelectorAll(sel)); };
  var NAME = /^(shot|video)-[0-9]+-[0-9]+\.(jpg|mp4)$/;

  // Views ----------------------------------------------------------------
  var tabs = $$('.tabs button');
  var current = 'overview';
  function show(view) {
    if (!$('#view-' + view)) return;
    current = view;
    $$('.view').forEach(function (v) { v.classList.toggle('on', v.id === 'view-' + view); });
    tabs.forEach(function (b) { b.setAttribute('aria-selected', b.getAttribute('data-view') === view ? 'true' : 'false'); });
    var tb = $('.toolbar');
    if (tb) tb.hidden = !(view === 'failures' || view === 'all');
    if (view === 'gallery') renderGallery(false);
    clearCurrent();
  }
  tabs.forEach(function (b) {
    b.addEventListener('click', function () { show(b.getAttribute('data-view')); });
  });

  function go(id) {
    var el = document.getElementById(id);
    if (!el) return;
    var view = el.closest('.view');
    if (view) show(view.id.replace('view-', ''));
    var det = el.tagName === 'DETAILS' ? el : $('details', el);
    if (det) det.open = true;
    el.scrollIntoView({ block: 'start' });
    mark(el);
  }
  document.addEventListener('click', function (e) {
    var a = e.target.closest ? e.target.closest('a[data-goto]') : null;
    if (a) {
      e.preventDefault();
      go(a.getAttribute('data-goto'));
    }
  });

  // Filter ---------------------------------------------------------------
  var filter = $('#filter');
  var countEl = $('#filter-count');

  function tokens(text) {
    var out = [];
    var re = /(?:([a-z]+):)?(?:"([^"]*)"|(\S+))/gi;
    var m;
    while ((m = re.exec(text)) !== null) {
      var key = m[1] ? m[1].toLowerCase() : '';
      var val = (m[2] !== undefined ? m[2] : m[3]).toLowerCase();
      if (key && ['status', 'area', 'module', 'grade', 'cause'].indexOf(key) < 0) {
        // Not one of ours: it is plain text with a colon in it.
        val = (m[1] + ':' + val).toLowerCase();
        key = '';
      }
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
        case 'status':
          ok = t.val === 'flaky' ? d.flaky === '1' : (d.status || '') === t.val.replace(/[ _-]/g, '');
          break;
        case 'area': ok = (d.area || '').toLowerCase().indexOf(t.val) >= 0; break;
        case 'module': ok = (d.module || '').toLowerCase().indexOf(t.val) >= 0; break;
        case 'grade': ok = (d.grade || '-').toLowerCase() === t.val; break;
        case 'cause':
          if (/^c[0-9]+$/.test(t.val)) ok = (d.cause || '').toLowerCase().split(' ')[0] === t.val;
          else ok = (d.cause || '').toLowerCase().indexOf(t.val) >= 0;
          break;
        default:
          ok = (d.text || '').toLowerCase().indexOf(t.val) >= 0;
          // Free words must all be there, so they are not OR-ed.
          if (!ok) return false;
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
    var items = $$('.f-item');
    var shown = 0, total = 0;
    items.forEach(function (it) {
      var ok = toks.length === 0 || matches(it, toks);
      it.hidden = !ok;
      if (it.classList.contains('case-wrap')) { total++; if (ok) shown++; }
    });
    $$('.cluster').forEach(function (c) {
      c.hidden = $$('.fcase', c).every(function (f) { return f.hidden; });
    });
    if (countEl) countEl.textContent = toks.length ? shown + ' of ' + total + ' cases shown' : '';
    clearCurrent();
  }
  if (filter) filter.addEventListener('input', applyFilter);

  // Keyboard navigation ---------------------------------------------------
  var cursor = null;
  function clearCurrent() {
    if (cursor) cursor.classList.remove('cur');
    cursor = null;
  }
  function mark(el) {
    clearCurrent();
    var item = el.classList.contains('f-item') ? el : el.closest('.f-item');
    if (!item) return;
    cursor = item;
    item.classList.add('cur');
    item.setAttribute('tabindex', '-1');
    item.focus({ preventScroll: true });
  }
  function navItems() {
    var sel = current === 'failures' ? '#view-failures .fcase' : current === 'all' ? '#view-all .case-wrap' : '';
    if (!sel) return [];
    return $$(sel).filter(function (el) { return !el.hidden && !(el.closest('.cluster') && el.closest('.cluster').hidden); });
  }
  function step(delta) {
    var items = navItems();
    if (!items.length) return;
    var i = cursor ? items.indexOf(cursor) : -1;
    var next = i < 0 ? (delta > 0 ? 0 : items.length - 1) : Math.max(0, Math.min(items.length - 1, i + delta));
    mark(items[next]);
    items[next].scrollIntoView({ block: 'center' });
  }

  var help = $('#help');
  var lb = $('#lb');
  function typing(t) { return t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT'); }

  document.addEventListener('keydown', function (e) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (lb && !lb.hidden) {
      if (e.key === 'Escape') { closeLightbox(); e.preventDefault(); }
      else if (e.key === 'ArrowRight') { openLightbox(lbIndex + 1); e.preventDefault(); }
      else if (e.key === 'ArrowLeft') { openLightbox(lbIndex - 1); e.preventDefault(); }
      return;
    }
    if (e.key === 'Escape') {
      if (help && !help.hidden) { help.hidden = true; e.preventDefault(); return; }
      if (typing(e.target)) {
        if (filter && e.target === filter && filter.value) { filter.value = ''; applyFilter(); } else e.target.blur();
        e.preventDefault();
      }
      return;
    }
    if (typing(e.target)) return;
    if (e.key === '/') {
      if (filter) {
        if (current !== 'failures' && current !== 'all') show('failures');
        filter.focus();
        e.preventDefault();
      }
    } else if (e.key === '?') {
      if (help) help.hidden = !help.hidden;
      e.preventDefault();
    } else if (e.key === 'j') { step(1); }
    else if (e.key === 'k') { step(-1); }
    else if (e.key === 'Enter' && cursor) {
      var det = $('details', cursor);
      if (det) det.open = !det.open;
    }
  });
  if (help) help.addEventListener('click', function (e) { if (e.target === help) help.hidden = true; });

  // Copy ------------------------------------------------------------------
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
    var label = b.getAttribute('data-label') || b.textContent;
    b.setAttribute('data-label', label);
    copyText(text, function (ok) {
      b.textContent = ok ? 'Copied' : 'Could not copy';
      setTimeout(function () { b.textContent = label; }, 1500);
    });
  });

  // Gallery and lightbox ---------------------------------------------------
  var gallery = (D.gallery || []).filter(function (g) { return NAME.test(g.n); });
  var galShown = 0;
  var PAGE = 48;
  function caption(g) {
    return '#' + g.c + (g.s ? ' ' + g.s : '') + ' ' + g.n;
  }
  function renderGallery(more) {
    var grid = $('#gal-grid');
    if (!grid) return;
    if (!more && grid.children.length) return;
    var end = Math.min(gallery.length, galShown + PAGE);
    for (var i = galShown; i < end; i++) {
      (function (i) {
        var g = gallery[i];
        var b = document.createElement('button');
        b.type = 'button';
        b.setAttribute('aria-label', 'Open ' + caption(g));
        var thumb;
        if (g.k) {
          thumb = document.createElement('div');
          thumb.className = 'vid';
          thumb.textContent = 'Video';
        } else {
          thumb = document.createElement('img');
          thumb.loading = 'lazy';
          thumb.alt = caption(g);
          thumb.src = '../shots/' + g.n;
        }
        var cap = document.createElement('span');
        cap.textContent = caption(g);
        b.appendChild(thumb);
        b.appendChild(cap);
        b.addEventListener('click', function () { openLightbox(i); });
        grid.appendChild(b);
      })(i);
    }
    galShown = end;
    var btn = $('#gal-more');
    if (btn) btn.hidden = galShown >= gallery.length;
  }
  var moreBtn = $('#gal-more');
  if (moreBtn) moreBtn.addEventListener('click', function () { renderGallery(true); });

  var lbIndex = 0;
  function openLightbox(i) {
    if (!lb || !gallery.length) return;
    lbIndex = (i + gallery.length) % gallery.length;
    var g = gallery[lbIndex];
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
    $('.lb-cap', lb).textContent = caption(g) + '  (' + (lbIndex + 1) + ' of ' + gallery.length + ')';
    lb.hidden = false;
  }
  function closeLightbox() {
    if (!lb) return;
    var v = $('video', lb);
    if (v) v.pause();
    lb.hidden = true;
  }
  if (lb) {
    $('.lb-x', lb).addEventListener('click', closeLightbox);
    $('.lb-prev', lb).addEventListener('click', function () { openLightbox(lbIndex - 1); });
    $('.lb-next', lb).addEventListener('click', function () { openLightbox(lbIndex + 1); });
    lb.addEventListener('click', function (e) { if (e.target === lb) closeLightbox(); });
  }

  show('overview');
})();
