/* The spec pane in the review page: tabs, the grip that sets its width,
   the Hide/Show chip, heading anchors, and the citation links in reviewer
   notes that open a tab at its heading. The pure helpers sit on
   window.tcmSpecs so the vitest file under src/lib can load this file as-is.
   Re-runnable: the live update swaps the whole .shell and calls wireSpecs
   again; what the reader had open is saved and restored around the swap. */
(function (root) {
  // ---- Pure helpers ---------------------------------------------------
  var EXT_RE = /\.[A-Za-z][A-Za-z0-9]{0,5}$/;

  // "Spec: <document> <section> [> "quote"] [- no quotable text (why)]"
  // -> { document, section }, or null when the line is not a citation.
  function splitCitation(text) {
    var m = /^\s*spec:\s*(.*?)\s*$/i.exec(text || '');
    if (!m) return null;
    var tail = m[1];
    tail = tail.replace(/\s*>\s*".*$/, '');
    tail = tail.replace(/\s*[-–—]\s*no quotable text\s*\([^)]*\)\s*$/i, '');
    var words = tail.split(/\s+/);
    var doc = [], i = 0;
    for (; i < words.length; i++) {
      doc.push(words[i]);
      if (EXT_RE.test(words[i])) { i++; break; }
    }
    // No token carried an extension: the first word is the document.
    if (!EXT_RE.test(doc[doc.length - 1] || '')) { doc = [words[0] || '']; i = 1; }
    return { document: doc.join(' '), section: words.slice(i).join(' ').trim() };
  }

  // Where a "Spec:" citation starts inside a longer run of text, or -1 when
  // there is none. The token must sit at the very start of the text, or be
  // preceded by whitespace - so "Respect: none" never matches, but a note
  // written as one paragraph ("Checks the flags.\nSpec: Step13.md 5.8")
  // still finds the citation after the markdown renderer turns that line
  // break into a plain space, joining both into one text node.
  function citationStart(text) {
    var m = /(^|\s)spec:\s/i.exec(text || '');
    if (!m) return -1;
    return m.index + m[1].length;
  }

  function norm(s) {
    return String(s || '').toLowerCase().replace(EXT_RE, '').replace(/[-_]+/g, ' ').replace(/\s+/g, ' ').trim();
  }
  function baseName(source) {
    var s = String(source || '').replace(/[?#].*$/, '');
    return s.split(/[\\/]/).pop() || s;
  }

  // norm() with spaces removed too, so a citation built from a CamelCase
  // file name ("Step13-CalculationEngine.md") still lines up with a title
  // written with spaces ("Calculation Engine").
  function squash(s) {
    return norm(s).replace(/ /g, '');
  }

  // The tab for a cited document: exact (case-, extension- and
  // space-insensitive) on title or file name, then containment either way -
  // preferring whichever candidate's title/file name is the LONGEST match,
  // so "CalculationEngine" picks the file tab titled "Calculation Engine"
  // over a wiki tab merely titled "Engine" - else -1.
  function findSpecTab(docs, document) {
    var want = norm(document), wantSquashed = squash(document);
    if (!want) return -1;
    var i;
    for (i = 0; i < docs.length; i++) {
      var t = norm(docs[i].title), b = norm(baseName(docs[i].source));
      if (t === want || b === want || squash(docs[i].title) === wantSquashed || squash(baseName(docs[i].source)) === wantSquashed) return i;
    }
    var best = -1, bestLen = 0;
    for (i = 0; i < docs.length; i++) {
      var ts = squash(docs[i].title), bs = squash(baseName(docs[i].source));
      var candidates = [ts, bs];
      for (var c = 0; c < candidates.length; c++) {
        var cand = candidates[c];
        if (!cand) continue;
        if ((cand.indexOf(wantSquashed) >= 0 || wantSquashed.indexOf(cand) >= 0) && cand.length > bestLen) {
          bestLen = cand.length;
          best = i;
        }
      }
    }
    return best;
  }

  function leadingNumber(s) {
    var m = /^\s*(\d+(?:\.\d+)*)\.?(?=\s|$)/.exec(s || '');
    return m ? m[1] : '';
  }
  function wordSet(s) {
    var out = {}, parts = norm(s).replace(/[^a-z0-9À-￿ ]/g, ' ').split(' ');
    for (var i = 0; i < parts.length; i++) if (parts[i]) out[parts[i]] = true;
    return out;
  }

  // The heading a section cites: the one whose leading number equals the
  // section's ("5.8" matches "5.8 Display Rules", not "5.80"); else the one
  // sharing the most words (two or more, or one when the section is one word).
  function matchHeading(headings, section) {
    var num = leadingNumber(section), i;
    if (num) {
      for (i = 0; i < headings.length; i++) if (leadingNumber(headings[i]) === num) return i;
    }
    var want = wordSet(section), wantCount = 0, k;
    for (k in want) wantCount++;
    if (!wantCount) return -1;
    var best = -1, bestScore = 0;
    for (i = 0; i < headings.length; i++) {
      var have = wordSet(headings[i]), score = 0;
      for (k in want) if (have[k]) score++;
      if (score > bestScore) { bestScore = score; best = i; }
    }
    var need = wantCount === 1 ? 1 : 2;
    return bestScore >= need ? best : -1;
  }

  function slug(text) {
    return String(text || '').toLowerCase().trim().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-+|-+$/g, '');
  }

  root.tcmSpecs = { splitCitation: splitCitation, findSpecTab: findSpecTab, matchHeading: matchHeading, slug: slug, citationStart: citationStart };

  // ---- The page -------------------------------------------------------
  if (typeof document === 'undefined' || !document.getElementById) return;
  var W_KEY = 'tcm-report-spec-w', OFF_KEY = 'tcm-report-spec-off';
  var W_DEFAULT = 520, W_MIN = 320;
  function store(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* file:// */ } }
  function recall(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }

  // What a swap must not lose: the active tab, each doc's scroll, the chip.
  var state = { active: 0, scroll: {}, off: recall(OFF_KEY) === '1' };

  // The grip drag, and whether the three window-level listeners it needs
  // have been registered yet. wireSpecs() re-runs on every live swap (a new
  // .shell with a new grip element each time), but window itself survives
  // the swap - registering these again on every re-run would pile up one
  // more of each with no way to remove the old ones, so they go on once and
  // read the current drag/grip/pane through these module-level variables.
  var windowWired = false;
  var drag = null; // { startX, startW } while a drag is in progress
  var dragGrip = null, dragPane = null; // the grip/pane the current drag owns

  function endDrag() {
    if (!drag) return;
    drag = null;
    if (dragGrip) dragGrip.classList.remove('dragging');
    if (dragPane) store(W_KEY, String(Math.round(dragPane.getBoundingClientRect().width)));
    dragGrip = null;
    dragPane = null;
  }

  function docsMeta() {
    var el = document.getElementById('tc-specs-data');
    if (!el) return [];
    try { return JSON.parse(el.textContent || '[]'); } catch (e) { return []; }
  }
  function tabKey(docs) {
    return 'tcm-report-spec-tab:' + docs.map(function (d) { return d.source; }).join('|');
  }

  function setWidth(w) {
    var max = Math.max(W_MIN, window.innerWidth * 0.6);
    w = Math.round(Math.min(max, Math.max(W_MIN, w)));
    document.documentElement.style.setProperty('--spec-w', w + 'px');
    return w;
  }

  function activate(pane, i, docs) {
    var tabs = pane.querySelectorAll('.spec-tab'), articles = pane.querySelectorAll('.spec-doc'), k;
    if (i < 0 || i >= articles.length) i = 0;
    for (k = 0; k < tabs.length; k++) tabs[k].setAttribute('aria-selected', k === i ? 'true' : 'false');
    for (k = 0; k < articles.length; k++) {
      if (k === i) articles[k].removeAttribute('hidden'); else articles[k].setAttribute('hidden', '');
    }
    state.active = i;
    store(tabKey(docs), String(i));
    if (state.scroll[i] != null && articles[i]) articles[i].scrollTop = state.scroll[i];
  }

  function paintChip(chip) {
    document.body.classList.toggle('spec-off', state.off);
    if (chip) {
      chip.setAttribute('aria-pressed', state.off ? 'true' : 'false');
      chip.textContent = state.off ? 'Show spec' : 'Hide spec';
    }
  }

  // Heading ids for the citation links: spec-<i>-<slug>, -2, -3 on repeats.
  // Every heading gets a stable id this way, both for jumpTo's own
  // scrollIntoView and for a hand-written link inside a spec document that
  // wants to target one directly.
  function anchorHeadings(pane) {
    var articles = pane.querySelectorAll('.spec-doc');
    for (var i = 0; i < articles.length; i++) {
      var hs = articles[i].querySelectorAll('h1, h2, h3, h4'), seen = {};
      for (var k = 0; k < hs.length; k++) {
        var base = 'spec-' + i + '-' + (slug(hs[k].textContent) || 'h'), id = base, n = 2;
        while (seen[id]) { id = base + '-' + n++; }
        seen[id] = true;
        hs[k].id = id;
      }
    }
  }

  function headingTexts(article) {
    var hs = article.querySelectorAll('h1, h2, h3, h4'), out = [];
    for (var k = 0; k < hs.length; k++) out.push(hs[k].textContent);
    return out;
  }

  function jumpTo(pane, docs, tab, section) {
    if (state.off) { state.off = false; store(OFF_KEY, '0'); paintChip(document.getElementById('tc-spec')); }
    activate(pane, tab, docs);
    var article = pane.querySelectorAll('.spec-doc')[tab];
    if (!article) return;
    var hs = article.querySelectorAll('h1, h2, h3, h4');
    var at = matchHeading(headingTexts(article), section);
    if (at < 0) { article.scrollTop = 0; return; }
    var h = hs[at];
    h.scrollIntoView({ block: 'start' });
    h.classList.remove('spec-flash');
    void h.offsetWidth; // restart the animation
    h.classList.add('spec-flash');
  }

  // Every "Spec:" line in a reviewer note becomes a link to its heading.
  function wireCitations(pane, docs) {
    var notes = document.querySelectorAll('.rev');
    for (var i = 0; i < notes.length; i++) {
      var walker = document.createTreeWalker(notes[i], NodeFilter.SHOW_TEXT), texts = [];
      while (walker.nextNode()) texts.push(walker.currentNode);
      for (var t = 0; t < texts.length; t++) {
        var node = texts[t];
        if (node.parentNode && node.parentNode.classList && node.parentNode.classList.contains('spec-link')) continue;
        var start = citationStart(node.nodeValue);
        if (start < 0) continue;
        // Prose before the citation (often the rest of the paragraph, once
        // a soft line break has been rendered as a space) stays a plain
        // text node; only the "Spec: ..." tail is turned into a link.
        if (start > 0) node = node.splitText(start);
        var cite = splitCitation(node.nodeValue);
        if (!cite) continue;
        var a = document.createElement('a');
        a.className = 'spec-link';
        a.href = '#';
        a.textContent = node.nodeValue;
        var tab = findSpecTab(docs, cite.document);
        if (tab < 0) {
          a.className += ' spec-link-dead';
          a.title = 'No spec tab for ' + cite.document;
          a.addEventListener('click', function (e) { e.preventDefault(); });
        } else {
          a.setAttribute('data-spec', String(tab));
          a.setAttribute('data-section', cite.section);
          a.title = 'Open ' + docs[tab].title + (cite.section ? ' at ' + cite.section : '');
          a.addEventListener('click', function (e) {
            e.preventDefault();
            jumpTo(pane, docs, Number(this.getAttribute('data-spec')), this.getAttribute('data-section') || '');
          });
        }
        node.parentNode.replaceChild(a, node);
      }
    }
  }

  function wireSpecs() {
    var pane = document.getElementById('tc-specs');
    var chip = document.getElementById('tc-spec');
    if (!pane) return;
    var docs = docsMeta();
    var kept = parseInt(recall(W_KEY), 10);
    setWidth(kept > 0 ? kept : W_DEFAULT);
    paintChip(chip);
    if (chip && !chip.dataset.wired) {
      chip.dataset.wired = '1';
      chip.addEventListener('click', function () {
        state.off = !state.off;
        store(OFF_KEY, state.off ? '1' : '0');
        paintChip(chip);
      });
    }
    anchorHeadings(pane);
    var tabs = pane.querySelectorAll('.spec-tab');
    for (var i = 0; i < tabs.length; i++) {
      tabs[i].setAttribute('role', 'tab');
      tabs[i].addEventListener('click', function () { activate(pane, Number(this.getAttribute('data-spec')), docs); });
    }
    var articles = pane.querySelectorAll('.spec-doc');
    for (var k = 0; k < articles.length; k++) {
      articles[k].addEventListener('scroll', (function (idx) {
        return function () { state.scroll[idx] = this.scrollTop; };
      })(k));
    }
    var remembered = parseInt(recall(tabKey(docs)), 10);
    activate(pane, state.active || (remembered >= 0 ? remembered : 0), docs);

    // The grip element is new after every swap, so its own listeners are
    // fine to add again each time; only the window-level ones must not be.
    var grip = pane.querySelector('.spec-grip');
    if (grip) {
      grip.addEventListener('mousedown', function (e) {
        if (e.button !== 0) return;
        drag = { startX: e.clientX, startW: pane.getBoundingClientRect().width };
        dragGrip = grip;
        dragPane = pane;
        grip.classList.add('dragging');
        e.preventDefault();
      });
      grip.addEventListener('dblclick', function () { setWidth(W_DEFAULT); try { localStorage.removeItem(W_KEY); } catch (e) { /* file:// */ } });
    }
    if (!windowWired) {
      windowWired = true;
      window.addEventListener('mousemove', function (e) {
        if (!drag) return;
        if (e.buttons === 0) { endDrag(); return; }
        // The pane is on the right, so moving left widens it.
        setWidth(drag.startW + (drag.startX - e.clientX));
      });
      window.addEventListener('mouseup', endDrag);
      window.addEventListener('blur', endDrag);
    }

    wireCitations(pane, docs);
  }

  root.__tcmWireSpecs = wireSpecs;
  wireSpecs();
})(typeof window !== 'undefined' ? window : this);
