
(function () {
  // --- Search filter. Re-runnable: after a live swap the cards are new
  // nodes, so everything node-shaped is (re)collected inside wireSearch
  // and the listener carries a data-wired guard.
  function wireSearch() {
    var input = document.getElementById('tc-search');
    var count = document.getElementById('tc-count');
    var noMatch = document.getElementById('tc-no-match');
    if (!input || !count) return function () {};
    var cards = Array.prototype.slice.call(document.querySelectorAll('.case'));
    var texts = cards.map(function (c) { return c.textContent.toLowerCase(); });
    var total = cards.length;

    function apply() {
      var words = input.value.toLowerCase().split(/\s+/).filter(Boolean);
      var shown = 0;
      texts.forEach(function (t, i) {
        var hit = words.every(function (w) { return t.indexOf(w) !== -1; });
        cards[i].classList.toggle('hidden', !hit);
        if (hit) shown++;
      });
      count.textContent = words.length
        ? shown + ' of ' + total + ' shown'
        : total + ' test case' + (total !== 1 ? 's' : '');
      if (noMatch) noMatch.classList.toggle('hidden', shown !== 0);
    }

    if (!input.dataset.wired) {
      input.dataset.wired = '1';
      input.addEventListener('input', apply);
      input.addEventListener('keydown', function (e) {
        if (e.key === 'Escape') { input.value = ''; apply(); }
      });
    }
    apply();
    return apply;
  }

  // --- Reviewer notes on/off. One class on <body>; the CSS does the rest.
  function wireNotesToggle() {
    var notesBtn = document.getElementById('tc-notes');
    if (!notesBtn) return;
    var KEY = 'tcm-report-notes-off';
    // The page is opened from a temp file, and a file:// origin can refuse
    // storage outright - so the preference is best-effort and the button
    // still works without it.
    function remember(off) {
      try { localStorage.setItem(KEY, off ? '1' : '0'); } catch (e) {}
    }
    function recall() {
      try { return localStorage.getItem(KEY) === '1'; } catch (e) { return false; }
    }
    function paint(off) {
      document.body.classList.toggle('notes-off', off);
      notesBtn.setAttribute('aria-pressed', off ? 'true' : 'false');
      notesBtn.textContent = off ? 'Show reviewer notes' : 'Hide reviewer notes';
    }
    paint(recall());
    if (!notesBtn.dataset.wired) {
      notesBtn.dataset.wired = '1';
      notesBtn.addEventListener('click', function () {
        var off = !document.body.classList.contains('notes-off');
        paint(off);
        remember(off);
        // Show brings EVERY note back, including ones closed one-by-one
        // with their own x - one button that undoes everything, rather
        // than the reviewer having to remember which x they clicked where.
        if (!off) {
          var closed = document.querySelectorAll('.rev-wrap.rev-closed');
          for (var i = 0; i < closed.length; i++) closed[i].classList.remove('rev-closed');
        }
      });
    }
  }

  var applyFilter = wireSearch();
  wireNotesToggle();

  // Each note's own x: collapses just that case's notes, with the same
  // motion as the global toggle. Session-only on purpose - which single
  // notes were dismissed is scroll-position-grade state, not a preference.
  // Delegated on document, so it survives every content swap untouched.
  document.addEventListener('click', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('.rev-close') : null;
    if (!btn) return;
    // The x lives inside a <summary>; without this, hiding the note would
    // also toggle the disclosure underneath it.
    e.preventDefault();
    e.stopPropagation();
    var wrap = btn.closest('.rev-wrap');
    if (wrap) wrap.classList.add('rev-closed');
  });

  // --- Live update. The page is a file on disk, so nothing pushes to it -
  // it asks. When the app says the content moved on, the page pulls the
  // fresh copy over the same loopback listener the comment boxes use and
  // swaps it IN PLACE: no new tab, no reload, scroll and search intact.
  // Only when the app's listener is there to ask (REPORT_REV is defined on
  // report pages, not on a plain export), and only while the tab is
  // visible, so a forgotten tab is not polling all afternoon.
  if (typeof REPORT_REV === 'number' && typeof NOTE_PORT !== 'undefined') {
    var rev = REPORT_REV;
    var base = 'http://127.0.0.1:' + NOTE_PORT;
    var qs = 'token=' + encodeURIComponent(NOTE_TOKEN) + '&kind=' + encodeURIComponent(REPORT_KIND);
    var stale = document.getElementById('tc-stale');
    var go = document.getElementById('tc-stale-go');
    if (go) go.addEventListener('click', function () { location.reload(); });
    // The banner survives as the FALLBACK: anything the swap cannot do
    // safely - fetch failed, structure unrecognisable - degrades to the
    // old offer of a manual refresh, never to silently stale content.
    function banner() { if (stale) stale.classList.add('show'); }

    function swap(fresh) {
      var doc = new DOMParser().parseFromString(fresh, 'text/html');
      var next = doc.querySelector('.shell') || doc.querySelector('.page');
      var cur = document.querySelector('.shell') || document.querySelector('.page');
      if (!next || !cur) return false;

      // What the reviewer would lose to a reload, carried across by hand.
      var input = document.getElementById('tc-search');
      var q = input ? input.value : '';
      var y = window.scrollY;
      var openTitles = {};
      Array.prototype.forEach.call(document.querySelectorAll('.case details[open]'), function (d) {
        var caseEl = d.closest('.case');
        var t = caseEl && caseEl.querySelector('summary');
        if (t) openTitles[t.textContent] = true;
      });

      cur.parentNode.replaceChild(document.importNode(next, true), cur);

      // Restore: filter text, expanded sections, notes state, scroll.
      var input2 = document.getElementById('tc-search');
      if (input2) input2.value = q;
      Array.prototype.forEach.call(document.querySelectorAll('.case details'), function (d) {
        var caseEl = d.closest('.case');
        var t = caseEl && caseEl.querySelector('summary');
        if (t && openTitles[t.textContent]) d.setAttribute('open', '');
      });
      applyFilter = wireSearch();
      wireNotesToggle();
      if (window.__tcmWireNotes) window.__tcmWireNotes();
      window.scrollTo(0, y);
      return true;
    }

    setInterval(function () {
      if (document.hidden || (stale && stale.classList.contains('show'))) { return; }
      fetch(base + '/version?' + qs)
        .then(function (r) { return r.json(); })
        .then(function (v) {
          // null means the app did not recognise this page; that is not
          // staleness and must not be reported as it.
          if (typeof v.revision !== 'number' || v.revision === rev) { return; }
          // Never swap under a reviewer's typing: a comment still inside
          // its autosave debounce would be clobbered. The revision stays
          // ahead of ours, so the next poll simply tries again.
          var ae = document.activeElement;
          if (ae && ae.tagName === 'TEXTAREA') { return; }
          var target = v.revision;
          fetch(base + '/report?' + qs)
            .then(function (r) {
              if (!r.ok) { throw new Error('no report'); }
              return r.text();
            })
            .then(function (html) {
              if (swap(html)) { rev = target; } else { banner(); }
            })
            .catch(banner);
        })
        .catch(function () { /* app closed, or no listener - stay quiet */ });
    }, 4000);
  }
})();
