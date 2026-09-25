
(function () {
  // --- Search filter. Re-runnable: after a live swap the cards are new
  // nodes, so everything node-shaped is (re)collected inside wireSearch
  // and the listener carries a data-wired guard.
  function wireSearch() {
    var input = document.getElementById('tc-search');
    var count = document.getElementById('tc-count');
    var noMatch = document.getElementById('tc-no-match');
    var field = document.getElementById('tc-field');
    if (!input || !count) return function () {};
    var cards = Array.prototype.slice.call(document.querySelectorAll('.case'));
    // Per card, one lower-cased haystack per field, so a search can be
    // narrowed to just the title, ID, prerequisites, steps, tags or module
    // instead of the whole card's text.
    function fieldsOf(card) {
      var text = function (sel) {
        return Array.prototype.map.call(card.querySelectorAll(sel), function (e) { return e.textContent; }).join(' ');
      };
      var pre = card.querySelector('.pre');
      return {
        all: card.textContent.toLowerCase(),
        title: text('.title').toLowerCase(),
        // Both the bare key and the #-prefixed work item id, so "157957"
        // and "#157957" both match.
        id: ((card.getAttribute('data-key') || '') + ' ' + text('.wid')).toLowerCase(),
        pre: (pre ? pre.textContent.replace(/^\s*Prerequisites:\s*/, '') : '').toLowerCase(),
        steps: text('.action, .expected').toLowerCase(),
        tags: text('.chip.tag').toLowerCase(),
        module: text('.chip.module').toLowerCase()
      };
    }
    var fields = cards.map(fieldsOf);
    var total = cards.length;

    function apply() {
      // A missing select (an older cached page, or a page without one)
      // searches every field, same as before this field selector existed.
      var key = field && fields.length && field.value in fields[0] ? field.value : 'all';
      var words = input.value.toLowerCase().split(/\s+/).filter(Boolean);
      var shown = 0;
      fields.forEach(function (f, i) {
        var hit = words.every(function (w) { return f[key].indexOf(w) !== -1; });
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
    if (field && !field.dataset.wired) {
      field.dataset.wired = '1';
      field.addEventListener('change', apply);
    }
    apply();
    return apply;
  }

  // --- Reviewer notes / findings on/off. One class on <body>; the CSS does
  // the rest. Both blocks work the same way, so they share this.
  function wireBlockToggle(opts) {
    var btn = document.getElementById(opts.buttonId);
    if (!btn) return;
    // The page is opened from a temp file, and a file:// origin can refuse
    // storage outright - so the preference is best-effort and the button
    // still works without it.
    function remember(off) {
      try { localStorage.setItem(opts.key, off ? '1' : '0'); } catch (e) {}
    }
    function recall() {
      try { return localStorage.getItem(opts.key) === '1'; } catch (e) { return false; }
    }
    function paint(off) {
      document.body.classList.toggle(opts.bodyClass, off);
      btn.setAttribute('aria-pressed', off ? 'true' : 'false');
      btn.textContent = off ? opts.showLabel : opts.hideLabel;
    }
    // After a live swap the button is new but <body> is not: keep what the
    // body already says, so the label matches even where storage is refused.
    paint(document.body.classList.contains(opts.bodyClass) || recall());
    if (!btn.dataset.wired) {
      btn.dataset.wired = '1';
      btn.addEventListener('click', function () {
        var off = !document.body.classList.contains(opts.bodyClass);
        paint(off);
        remember(off);
        // Show brings EVERY block back, including ones closed one-by-one
        // with their own x - one button that undoes everything, rather
        // than the reader having to remember which x they clicked where.
        if (!off) {
          var closed = document.querySelectorAll(opts.closedSelector);
          for (var i = 0; i < closed.length; i++) closed[i].classList.remove(opts.closedClass);
        }
      });
    }
  }

  function wireNotesToggle() {
    wireBlockToggle({
      buttonId: 'tc-notes',
      key: 'tcm-report-notes-off',
      bodyClass: 'notes-off',
      closedSelector: '.rev-wrap.rev-closed',
      closedClass: 'rev-closed',
      hideLabel: 'Hide reviewer notes',
      showLabel: 'Show reviewer notes',
    });
  }

  function wireFindingsToggle() {
    wireBlockToggle({
      buttonId: 'tc-findings',
      key: 'tcm-report-findings-off',
      bodyClass: 'findings-off',
      closedSelector: '.find-wrap.find-closed',
      closedClass: 'find-closed',
      hideLabel: 'Hide findings',
      showLabel: 'Show findings',
    });
  }

  // --- The Options menu. A native disclosure, so it opens on its own; what
  // is added here is what one does not do by itself. Both listeners sit on
  // the document and look the menu up when they fire, so they survive every
  // content swap untouched.
  document.addEventListener('keydown', function (e) {
    if (e.key !== 'Escape') return;
    var open = document.querySelector('.tc-menu[open]');
    if (!open) return;
    open.removeAttribute('open');
    var summary = open.querySelector('summary');
    if (summary && summary.focus) summary.focus();
  });
  document.addEventListener('click', function (e) {
    var open = document.querySelector('.tc-menu[open]');
    if (!open) return;
    var within = e.target && e.target.closest ? e.target.closest('.tc-menu') : null;
    if (within !== open) open.removeAttribute('open');
  });

  // --- The bookmark: where the review stopped. One per page, and it never
  // leaves the browser - neither the app nor the file is told about it.
  //
  // Which review this is comes from the renderer as data-scope on <body>:
  // these pages are all opened over file://, where every document shares
  // one storage area, so the scope is what keeps two reviews apart.
  var sessionMark = null;
  function markStore() {
    return 'tcm-report-mark:' + (document.body.getAttribute('data-scope') || '');
  }
  function rememberMark(key) {
    sessionMark = key;
    try {
      if (key) localStorage.setItem(markStore(), key);
      else localStorage.removeItem(markStore());
    } catch (e) {}
  }
  function recallMark() {
    try {
      var stored = localStorage.getItem(markStore());
      // A real answer means storage is working, so it takes over as the
      // write-through cache's value. A null answer is ambiguous - it means
      // either "never marked" or "the write silently failed" (quota,
      // Safari private mode) - so it must NOT clobber an in-memory mark
      // that a failed setItem left behind; that is what let a click appear
      // to do nothing on a page whose reads work but whose writes do not.
      if (stored !== null) sessionMark = stored;
      return sessionMark;
    } catch (e) {}
    return sessionMark;
  }
  function markedCase() {
    var key = recallMark();
    if (!key) return null;
    var cards = document.querySelectorAll('.case');
    for (var i = 0; i < cards.length; i++) {
      if (cards[i].getAttribute('data-key') === key) return cards[i];
    }
    return null;
  }

  // Re-runnable for the same reason wireSearch is: after a live swap the
  // cards and the bar are new nodes that know nothing of the mark.
  function wireMarks() {
    var key = recallMark();
    var cards = document.querySelectorAll('.case');
    var found = false;
    for (var i = 0; i < cards.length; i++) {
      var on = !!key && cards[i].getAttribute('data-key') === key;
      if (on) found = true;
      cards[i].classList.toggle('marked', on);
      var mark = cards[i].querySelector('.tc-mark');
      if (mark) mark.setAttribute('aria-pressed', on ? 'true' : 'false');
    }
    var go = document.getElementById('tc-goto');
    if (!go) return;
    // Nothing marked, nowhere to go: a button that scrolls to nothing is
    // just another thing to read.
    go.classList.toggle('hidden', !found);
    if (go.dataset.wired) return;
    go.dataset.wired = '1';
    go.addEventListener('click', function () {
      var card = markedCase();
      if (card && card.scrollIntoView) card.scrollIntoView({ block: 'center', behavior: 'smooth' });
    });
  }
  window.__tcmWireMarks = wireMarks;
  // Test-only: re-wires the search filter against whatever cards are
  // currently in the document, exactly as a live swap does.
  window.__tcmWireSearch = function () { applyFilter = wireSearch(); };
  // Test-only: the in-memory write-through cache now genuinely outlives a
  // storage read that comes back null (that is the fix), which on a real
  // page is exactly right - it lasts for the page's own lifetime. A test
  // file that loads this script once and reuses it across many tests has
  // no such natural reset between them, so it needs one to ask for.
  window.__tcmForgetMark = function () {
    sessionMark = null;
  };

  // Delegated, like the blocks' own x, so it survives every swap untouched.
  document.addEventListener('click', function (e) {
    var mark = e.target && e.target.closest ? e.target.closest('.tc-mark') : null;
    if (!mark) return;
    var card = mark.closest('.case');
    if (!card) return;
    var key = card.getAttribute('data-key');
    // There is only ever one mark: marking another case moves it, and
    // clicking the marked case again clears it.
    rememberMark(recallMark() === key ? null : key);
    wireMarks();
  });

  var applyFilter = wireSearch();
  wireNotesToggle();
  wireFindingsToggle();
  wireMarks();

  // Each note's/finding's own x: collapses just that case's block, with the
  // same motion as the global toggle. Session-only on purpose - which single
  // blocks were dismissed is scroll-position-grade state, not a preference.
  // Delegated on document, so it survives every content swap untouched.
  document.addEventListener('click', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('.rev-close, .find-close') : null;
    if (!btn) return;
    // The x lives inside a <summary>; without this, hiding the block would
    // also toggle the disclosure underneath it.
    e.preventDefault();
    e.stopPropagation();
    var wrap = btn.closest('.rev-wrap, .find-wrap');
    if (!wrap) return;
    wrap.classList.add(wrap.classList.contains('find-wrap') ? 'find-closed' : 'rev-closed');
  });

  // --- Which sections of which case are open. Keyed by the case TITLE
  // (the h2 without its position number, so a reorder keeps it) plus the
  // section's class - not by the first <summary>, which is "Reviewer
  // notes" in every case that has notes. Both states are restored: a
  // section the reader folded stays folded even though fresh markup opens it.
  function caseTitle(caseEl) {
    var h = caseEl && caseEl.querySelector('h2');
    if (!h) return '';
    var copy = h.cloneNode(true);
    var seq = copy.querySelector('.seq');
    if (seq) seq.parentNode.removeChild(seq);
    return copy.textContent.replace(/\s+/g, ' ').trim();
  }
  function detailsKey(d) {
    return caseTitle(d.closest('.case')) + '\n' + d.className;
  }
  function openState(root) {
    var out = {};
    Array.prototype.forEach.call(root.querySelectorAll('.case details'), function (d) {
      out[detailsKey(d)] = d.hasAttribute('open');
    });
    return out;
  }
  function restoreOpen(root, state) {
    Array.prototype.forEach.call(root.querySelectorAll('.case details'), function (d) {
      var k = detailsKey(d);
      if (!Object.prototype.hasOwnProperty.call(state, k)) return;
      if (state[k]) d.setAttribute('open', ''); else d.removeAttribute('open');
    });
  }
  window.tcmPage = {
    openState: openState,
    restoreOpen: restoreOpen,
    // A property, not a bare call, so the vitest file can see Refresh fire.
    reload: function () { location.reload(); }
  };

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
    // The banner and its Refresh button live inside .page, which swap()
    // replaces - so they are looked up each time, and Refresh is a click
    // delegated on document, never a listener on one button.
    function staleBanner() { return document.getElementById('tc-stale'); }
    // The FALLBACK for anything the swap cannot do safely - fetch failed,
    // structure unrecognisable: offer a manual refresh, never silently
    // leave stale content up.
    function banner() {
      var s = staleBanner();
      if (s) s.classList.add('show');
    }
    document.addEventListener('click', function (e) {
      var go = e.target && e.target.closest ? e.target.closest('#tc-stale-go') : null;
      if (go) window.tcmPage.reload();
    });

    function swap(fresh) {
      var doc = new DOMParser().parseFromString(fresh, 'text/html');
      var next = doc.querySelector('.shell') || doc.querySelector('.page');
      var cur = document.querySelector('.shell') || document.querySelector('.page');
      if (!next || !cur) return false;

      // What the reviewer would lose to a reload, carried across by hand.
      var input = document.getElementById('tc-search');
      var q = input ? input.value : '';
      var sel = document.getElementById('tc-field');
      var f = sel ? sel.value : '';
      var y = window.scrollY;
      var open = openState(document);

      cur.parentNode.replaceChild(document.importNode(next, true), cur);

      // Restore: filter text, open/closed sections, toggles, scroll.
      var input2 = document.getElementById('tc-search');
      if (input2) input2.value = q;
      var sel2 = document.getElementById('tc-field');
      if (sel2 && f) sel2.value = f;
      restoreOpen(document, open);
      applyFilter = wireSearch();
      wireNotesToggle();
      wireFindingsToggle();
      wireMarks();
      if (window.__tcmWireNotes) window.__tcmWireNotes();
      if (window.__tcmWireSpecs) window.__tcmWireSpecs();
      window.scrollTo(0, y);
      return true;
    }

    // How long until the next ask. A closed app answers nothing, and every
    // refused ask is an error line in the browser's console - so each
    // failure doubles the wait, up to a minute, and the first answer brings
    // it back to 4 s.
    var POLL_MS = 4000, POLL_MAX_MS = 60000, wait = POLL_MS;
    function schedule() { setTimeout(poll, wait); }
    function poll() {
      var s = staleBanner();
      if (document.hidden || (s && s.classList.contains('show'))) { schedule(); return; }
      fetch(base + '/version?' + qs)
        .then(function (r) { return r.json(); })
        .then(function (v) {
          wait = POLL_MS;
          // null means the app did not recognise this page; that is not
          // staleness and must not be reported as it.
          if (typeof v.revision !== 'number' || v.revision === rev) { return; }
          // Never swap under a reviewer's typing, or under an edit that
          // has not landed yet: the focused-textarea check alone misses a
          // box clicked away from before its debounce fired, or while its
          // save is still in flight or queued behind another (see
          // window.tcmNotes.busy in cases-notes.js) - any of that would
          // show the box's OLD text under whatever was typed next. The
          // revision stays ahead of ours either way, so the next poll
          // simply tries again.
          var ae = document.activeElement;
          if (ae && ae.tagName === 'TEXTAREA') { return; }
          if (window.tcmNotes && window.tcmNotes.busy() > 0) { return; }
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
        .catch(function () { wait = Math.min(wait * 2, POLL_MAX_MS); })
        .then(schedule);
    }
    schedule();
  }
})();
