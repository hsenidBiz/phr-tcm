
(function () {
  // --- Search filter. Re-runnable: after a live swap the cards are new
  // nodes, so everything node-shaped is (re)collected inside wireSearch
  // and each listener carries a data-wired guard.

  // How the query matches: the three switches in the search box. Kept here
  // rather than on the buttons, because a live swap replaces the buttons.
  function recallFlag(key) {
    try { return localStorage.getItem(key) === '1'; } catch (e) { return false; }
  }
  function rememberFlag(key, on) {
    try { localStorage.setItem(key, on ? '1' : '0'); } catch (e) {}
  }
  var OPTS = [
    { id: 'tc-case', name: 'matchCase', key: 'tcm-report-search-case', shortcut: 'c' },
    { id: 'tc-word', name: 'wholeWord', key: 'tcm-report-search-word', shortcut: 'w' },
    { id: 'tc-regex', name: 'regex', key: 'tcm-report-search-regex', shortcut: 'r' }
  ];
  var searchOpts = {};
  OPTS.forEach(function (o) { searchOpts[o.name] = recallFlag(o.key); });

  // Text a search ignores, and so never highlights: the page's own labels
  // and controls, not what the case says.
  var NOT_CONTENT = 'button, script, style, svg, select, input, textarea, summary, label, ' +
    '.seq, .metalabel, .pre > b, .note-status';

  function textNodes(root) {
    var out = [];
    var walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, null);
    while (walker.nextNode()) {
      var n = walker.currentNode;
      var el = n.parentElement;
      if (!n.nodeValue || !el || el.closest(NOT_CONTENT)) continue;
      out.push(n);
    }
    return out;
  }

  // Where each field lives in a card: what it searches and what it marks.
  var FIELD_SEL = {
    title: '.title',
    id: '.wid',
    pre: '.pre',
    steps: '.action, .expected',
    tags: '.chip.tag',
    module: '.chip.module'
  };

  function escapeRe(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }

  // What counts as part of a word for Match whole word: letters and digits,
  // accented and non-Latin ones included, but not the punctuation block
  // (U+2000 on: dashes, curly quotes, ellipses), so "login" is still a
  // whole word inside quotes or beside a dash. A plain class rather than a
  // Unicode property escape, which an older browser cannot even parse.
  var WORD = '0-9A-Za-z_\\u00C0-\\u00D6\\u00D8-\\u00F6\\u00F8-\\u1FFF\\u2C00-\\uD7FF';

  // A pattern, wrapped to match whole words when asked. The edge before the
  // word is CAPTURED rather than looked behind at (older browsers have no
  // lookbehind), so a whole-word match starts one edge early; `lead` says
  // so, and the marker steps past it.
  function compile(src, flags) {
    if (!searchOpts.wholeWord) return new RegExp(src, flags);
    var re = new RegExp('(^|[^' + WORD + '])(?:' + src + ')(?=[^' + WORD + ']|$)', flags);
    re.lead = true;
    return re;
  }

  // The query as patterns. Plain text is words that must ALL appear, with
  // "quoted phrases" kept whole; a regular expression is one pattern.
  // Throws on a regular expression that does not compile.
  function matchersFor(q) {
    var srcs = [];
    if (searchOpts.regex) {
      if (q.trim()) srcs.push(q);
    } else {
      var re = /"([^"]*)"?|(\S+)/g;
      var m;
      while ((m = re.exec(q))) {
        var term = m[1] !== undefined ? m[1].trim() : m[2];
        if (term) srcs.push(escapeRe(term));
      }
    }
    var flags = searchOpts.matchCase ? '' : 'i';
    return {
      tests: srcs.map(function (s) { return compile(s, flags); }),
      mark: srcs.length ? compile(srcs.map(function (s) { return '(?:' + s + ')'; }).join('|'), flags + 'g') : null
    };
  }

  function clearHits() {
    var marks = document.querySelectorAll('mark.tc-hit');
    var parents = [];
    for (var i = 0; i < marks.length; i++) {
      var p = marks[i].parentNode;
      p.replaceChild(document.createTextNode(marks[i].textContent), marks[i]);
      if (parents.indexOf(p) === -1) parents.push(p);
    }
    parents.forEach(function (p) { p.normalize(); });
  }

  function markHits(root, re) {
    textNodes(root).forEach(function (n) {
      var text = n.nodeValue;
      var frag = null;
      var last = 0;
      var m;
      re.lastIndex = 0;
      while ((m = re.exec(text))) {
        var lead = re.lead ? m[1].length : 0;
        var start = m.index + lead;
        var found = m[0].slice(lead);
        // A pattern that can match nothing (a*) would never move on.
        if (!found) { if (re.lastIndex === m.index) re.lastIndex++; continue; }
        frag = frag || document.createDocumentFragment();
        if (start > last) frag.appendChild(document.createTextNode(text.slice(last, start)));
        var hit = document.createElement('mark');
        hit.className = 'tc-hit';
        hit.textContent = found;
        frag.appendChild(hit);
        last = start + found.length;
      }
      if (!frag) return;
      if (last < text.length) frag.appendChild(document.createTextNode(text.slice(last)));
      n.parentNode.replaceChild(frag, n);
    });
  }

  // A page grouped by area (the Queue's Group by area) wraps the cases in
  // nested <details class='tc-group'> sections. A section whose cases the
  // search has all hidden goes too, so the filter does not leave a column
  // of empty headings. On a flat page there are no sections and this does
  // nothing.
  //
  // When the QUERY changes, a folded section holding a match opens, or the
  // count would name cases nobody can see. Only then: not when the filter
  // is re-applied for another reason (a match switch, a live refresh), and
  // never a section the reader folds while the search is on - any section
  // that was open at the last sync and is shut now was shut by them, so it
  // stays shut for the rest of the search, whoever opened it. Clearing the
  // search ends it and folds back what the search opened. Kept outside
  // wireSearch, by section key, so a live refresh (which re-wires the
  // search) keeps it.
  var lastQuery = '';
  var wasOpen = {};
  var searchOpened = {};
  var readerFolded = {};
  function syncGroups(query) {
    var groups = Array.prototype.slice.call(document.querySelectorAll('details.tc-group'));
    var keyOf = function (g) { return g.getAttribute('data-area') || ''; };
    var searching = lastQuery !== '';
    var changed = query !== lastQuery;
    lastQuery = query;
    groups.forEach(function (g) {
      g.classList.toggle('hidden', !g.querySelector('.case:not(.hidden)'));
      var key = keyOf(g);
      if (searching && wasOpen[key] && !g.hasAttribute('open')) {
        delete searchOpened[key];
        readerFolded[key] = true;
      }
    });
    if (!query) {
      if (changed) {
        groups.forEach(function (g) {
          if (searchOpened[keyOf(g)]) g.removeAttribute('open');
        });
      }
      searchOpened = {};
      readerFolded = {};
    } else if (changed) {
      groups.forEach(function (g) {
        var key = keyOf(g);
        if (g.hasAttribute('open') || readerFolded[key] || g.classList.contains('hidden')) return;
        g.setAttribute('open', '');
        searchOpened[key] = true;
      });
    }
    wasOpen = {};
    groups.forEach(function (g) {
      if (g.hasAttribute('open')) wasOpen[keyOf(g)] = true;
    });
  }

  // Go to bookmark can reach into a section the reader has folded: every
  // section around `el` is opened first, or there is nothing to scroll to.
  function openGroupsAround(el) {
    for (var g = el && el.parentElement; g; g = g.parentElement) {
      if (g.matches && g.matches('details.tc-group')) g.setAttribute('open', '');
    }
  }

  function wireSearch() {
    var input = document.getElementById('tc-search');
    var count = document.getElementById('tc-count');
    var noMatch = document.getElementById('tc-no-match');
    var field = document.getElementById('tc-field');
    var box = input ? input.closest('.tc-searchbox') : null;
    if (!input || !count) return function () {};
    var cards = Array.prototype.slice.call(document.querySelectorAll('.case'));
    // Per card, one haystack per field, in its own case - lower-casing is
    // the pattern's job (the i flag), so Match case has the real text.
    function fieldsOf(card) {
      var text = function (sel) {
        return Array.prototype.map.call(card.querySelectorAll(sel), function (e) { return e.textContent; }).join(' ');
      };
      var pre = card.querySelector('.pre');
      // All fields: what the case says, not the page's labels around it -
      // so "Module" finds a case about modules, not every card with a row
      // called Module. Comment boxes count: they are the reviewer's text.
      var all = textNodes(card).map(function (n) { return n.nodeValue; }).join(' ') + ' ' + text('textarea');
      return {
        all: all,
        title: text('.title'),
        // Both the bare key and the #-prefixed work item id, so "157957"
        // and "#157957" both match.
        id: (card.getAttribute('data-key') || '') + ' ' + text('.wid'),
        pre: pre ? pre.textContent.replace(/^\s*Prerequisites:\s*/, '') : '',
        steps: text('.action, .expected'),
        tags: text('.chip.tag'),
        module: text('.chip.module')
      };
    }
    // Read before any highlight goes in, so a mark never splits a haystack.
    clearHits();
    var fields = cards.map(fieldsOf);
    var total = cards.length;

    function paintOpts() {
      OPTS.forEach(function (o) {
        var b = document.getElementById(o.id);
        if (b) b.setAttribute('aria-pressed', searchOpts[o.name] ? 'true' : 'false');
      });
    }

    function invalid(bad) {
      if (box) box.classList.toggle('invalid', bad);
      if (bad) input.setAttribute('aria-invalid', 'true');
      else input.removeAttribute('aria-invalid');
    }

    function apply() {
      // A missing select (an older cached page, or a page without one)
      // searches every field, same as before this field selector existed.
      var key = field && fields.length && field.value in fields[0] ? field.value : 'all';
      var found;
      try {
        found = matchersFor(input.value);
      } catch (e) {
        // A regular expression still being typed: say so, and leave the
        // cards as the last good pattern had them rather than flashing.
        invalid(true);
        count.textContent = 'Invalid regular expression';
        return;
      }
      invalid(false);
      clearHits();
      var shown = 0;
      fields.forEach(function (f, i) {
        var hit = found.tests.every(function (re) { return re.test(f[key]); });
        cards[i].classList.toggle('hidden', !hit);
        if (!hit) return;
        shown++;
        if (!found.mark) return;
        if (key === 'all') markHits(cards[i], found.mark);
        else Array.prototype.forEach.call(cards[i].querySelectorAll(FIELD_SEL[key]), function (el) { markHits(el, found.mark); });
      });
      syncGroups(found.tests.length > 0 ? input.value : '');
      count.textContent = found.tests.length
        ? shown + ' of ' + total + ' shown'
        : total + ' test case' + (total !== 1 ? 's' : '');
      if (noMatch) noMatch.classList.toggle('hidden', shown !== 0);
    }

    function toggle(o) {
      searchOpts[o.name] = !searchOpts[o.name];
      rememberFlag(o.key, searchOpts[o.name]);
      paintOpts();
      apply();
    }

    if (!input.dataset.wired) {
      input.dataset.wired = '1';
      input.addEventListener('input', apply);
      input.addEventListener('keydown', function (e) {
        if (e.key === 'Escape') { input.value = ''; apply(); return; }
        // The editor shortcuts for the three switches, while typing.
        if (!e.altKey || e.ctrlKey || e.metaKey) return;
        var k = (e.key || '').toLowerCase();
        OPTS.forEach(function (o) {
          if (k === o.shortcut) { e.preventDefault(); toggle(o); }
        });
      });
    }
    OPTS.forEach(function (o) {
      var b = document.getElementById(o.id);
      if (!b || b.dataset.wired) return;
      b.dataset.wired = '1';
      b.addEventListener('click', function () { toggle(o); });
    });
    if (field && !field.dataset.wired) {
      field.dataset.wired = '1';
      field.addEventListener('change', apply);
    }
    dressField(field);
    paintOpts();
    apply();
    return apply;
  }

  // --- The field picker: the select, dressed as the page's own list. The
  // select stays the value everything reads (and a page whose script never
  // runs keeps it as a plain select); this is a button and a listbox over
  // it, in the page's theme instead of the system's.
  function dressField(field) {
    if (!field || field.dataset.dressed) return;
    field.dataset.dressed = '1';
    var name = field.getAttribute('aria-label') || 'Search in';
    var wrap = document.createElement('div');
    wrap.className = 'tc-pick';
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'tc-pick-btn';
    btn.setAttribute('aria-haspopup', 'listbox');
    btn.setAttribute('aria-expanded', 'false');
    btn.setAttribute('aria-controls', 'tc-field-list');
    var label = document.createElement('span');
    label.className = 'tc-pick-label';
    btn.appendChild(label);
    var list = document.createElement('ul');
    list.id = 'tc-field-list';
    list.className = 'tc-pick-list';
    list.setAttribute('role', 'listbox');
    list.setAttribute('aria-label', name);
    list.hidden = true;
    var items = Array.prototype.map.call(field.options, function (o) {
      var li = document.createElement('li');
      li.setAttribute('role', 'option');
      li.id = 'tc-field-' + o.value;
      li.tabIndex = -1;
      li.setAttribute('data-value', o.value);
      li.textContent = o.text;
      list.appendChild(li);
      return li;
    });
    wrap.appendChild(btn);
    wrap.appendChild(list);
    field.parentNode.insertBefore(wrap, field);
    field.hidden = true;

    function paint() {
      var o = field.options[field.selectedIndex];
      var text = o ? o.text : '';
      label.textContent = text;
      btn.setAttribute('aria-label', name + ': ' + text);
      items.forEach(function (li) {
        li.setAttribute('aria-selected', li.getAttribute('data-value') === field.value ? 'true' : 'false');
      });
    }
    function open() {
      list.hidden = false;
      wrap.classList.add('open');
      btn.setAttribute('aria-expanded', 'true');
      var on = list.querySelector('[aria-selected="true"]') || items[0];
      if (on) on.focus();
    }
    function close(refocus) {
      if (list.hidden) return;
      list.hidden = true;
      wrap.classList.remove('open');
      btn.setAttribute('aria-expanded', 'false');
      if (refocus) btn.focus();
    }
    function choose(li) {
      var v = li.getAttribute('data-value');
      if (field.value !== v) {
        field.value = v;
        field.dispatchEvent(new Event('change'));
      }
      paint();
      close(false);
      // Back to typing: picking a field is almost always followed by a query.
      var input = document.getElementById('tc-search');
      if (input) input.focus();
      else btn.focus();
    }
    btn.addEventListener('click', function () {
      if (list.hidden) open(); else close(true);
    });
    btn.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); open(); }
    });
    list.addEventListener('click', function (e) {
      var li = e.target && e.target.closest ? e.target.closest('[role="option"]') : null;
      if (li) choose(li);
    });
    list.addEventListener('keydown', function (e) {
      var i = items.indexOf(document.activeElement);
      var to = null;
      if (e.key === 'ArrowDown') to = items[Math.min(items.length - 1, i + 1)];
      else if (e.key === 'ArrowUp') to = items[Math.max(0, i - 1)];
      else if (e.key === 'Home') to = items[0];
      else if (e.key === 'End') to = items[items.length - 1];
      else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); if (i !== -1) choose(items[i]); return; }
      else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(true); return; }
      else if (e.key === 'Tab') { close(false); return; }
      if (to) { e.preventDefault(); to.focus(); }
    });
    // Pointing at an option makes it the one the keyboard continues from.
    list.addEventListener('mousemove', function (e) {
      var li = e.target && e.target.closest ? e.target.closest('[role="option"]') : null;
      if (li && document.activeElement !== li) li.focus();
    });
    // A click anywhere else closes it. Tied to this picker's nodes, which a
    // live swap discards along with the listener's reason to act.
    document.addEventListener('click', function (e) {
      if (!list.hidden && !wrap.contains(e.target)) close(false);
    });
    field.addEventListener('change', paint);
    paint();
  }

  // --- Show on cards: Automation Status, Module and Tags each on/off. A
  // class on <body> hides the rows; a card left with no rows at all loses
  // the block too, so there is no empty gap where it was.
  var SHOW_KINDS = ['status', 'module', 'tags'];
  function wireShowFields() {
    function hidden(k) { return document.body.classList.contains('hide-' + k); }
    function settle() {
      var metas = document.querySelectorAll('.case > .meta');
      for (var i = 0; i < metas.length; i++) {
        var rows = metas[i].querySelectorAll('.metarow');
        var any = false;
        for (var j = 0; j < rows.length && !any; j++) {
          var row = rows[j];
          any = !SHOW_KINDS.some(function (k) { return row.classList.contains('m-' + k) && hidden(k); });
        }
        metas[i].classList.toggle('hidden', !any);
      }
    }
    SHOW_KINDS.forEach(function (k) {
      var key = 'tcm-report-hide-' + k;
      // After a live swap <body> already says; on a fresh page, storage does.
      var off = hidden(k) || recallFlag(key);
      document.body.classList.toggle('hide-' + k, off);
      var box = document.getElementById('tc-show-' + k);
      if (!box) return;
      box.checked = !off;
      if (box.dataset.wired) return;
      box.dataset.wired = '1';
      box.addEventListener('change', function () {
        document.body.classList.toggle('hide-' + k, !box.checked);
        rememberFlag(key, !box.checked);
        settle();
      });
    });
    settle();
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
      openGroupsAround(card);
      if (card && card.scrollIntoView) card.scrollIntoView({ block: 'center', behavior: 'smooth' });
    });
  }
  window.__tcmWireMarks = wireMarks;
  // Test-only: re-wires the search filter against whatever cards are
  // currently in the document, exactly as a live swap does.
  window.__tcmWireSearch = function () { applyFilter = wireSearch(); };
  window.__tcmWireShow = function () { wireShowFields(); };
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
  wireShowFields();
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
  // An area section (a grouped page) is keyed by its folded path, which
  // the renderer puts on it as data-area - so a section the reader folded
  // stays folded through a live refresh, as a case's own sections do.
  function groupKey(d) {
    return 'area:' + (d.getAttribute('data-area') || '');
  }
  function openState(root) {
    var out = {};
    Array.prototype.forEach.call(root.querySelectorAll('.case details'), function (d) {
      out[detailsKey(d)] = d.hasAttribute('open');
    });
    Array.prototype.forEach.call(root.querySelectorAll('details.tc-group'), function (d) {
      out[groupKey(d)] = d.hasAttribute('open');
    });
    return out;
  }
  function restoreOpen(root, state) {
    Array.prototype.forEach.call(root.querySelectorAll('details.tc-group'), function (d) {
      var k = groupKey(d);
      if (!Object.prototype.hasOwnProperty.call(state, k)) return;
      if (state[k]) d.setAttribute('open', ''); else d.removeAttribute('open');
    });
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
      wireShowFields();
      wireFindingsToggle();
      wireMarks();
      if (window.__tcmWireNotes) window.__tcmWireNotes();
      if (window.__tcmWireSpecs) window.__tcmWireSpecs();
      // A save that did not succeed - timed out, the app was closed, or it
      // was refused - must survive this: the fresh copy just adopted carries
      // the file's OLDER text and no status at all, which would otherwise
      // silently overwrite what the reviewer typed and erase what they were
      // told about it.
      if (window.tcmNotes && window.tcmNotes.restoreUnsaved) window.tcmNotes.restoreUnsaved(document);
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
          // Returned into the chain: without it, the next poll was armed
          // while a slow /report was still in flight, and could start a
          // second fetch and swap for the very same revision.
          return fetch(base + '/report?' + qs)
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
