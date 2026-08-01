
(function () {
  var input = document.getElementById('tc-search');
  var count = document.getElementById('tc-count');
  var noMatch = document.getElementById('tc-no-match');
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
    noMatch.classList.toggle('hidden', shown !== 0);
  }

  input.addEventListener('input', apply);
  input.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') { input.value = ''; apply(); }
  });
  apply();

  // Reviewer notes on/off. One class on <body>; the CSS does the rest, so
  // nothing has to be walked and it costs the same on a 600-case page as
  // on a 3-case one.
  // Is what you are looking at still current? The page is a file on disk,
  // so nothing pushes to it - it asks. Only when the app's note listener is
  // there to ask (REPORT_REV is defined on report pages, not on a plain
  // export), and only while the tab is visible, so a forgotten tab is not
  // polling all afternoon.
  if (typeof REPORT_REV === 'number' && typeof NOTE_PORT !== 'undefined') {
    var stale = document.getElementById('tc-stale');
    var go = document.getElementById('tc-stale-go');
    if (stale && go) {
      go.addEventListener('click', function () { location.reload(); });
      setInterval(function () {
        if (document.hidden || stale.classList.contains('show')) { return; }
        fetch('http://127.0.0.1:' + NOTE_PORT + '/version?token=' + encodeURIComponent(NOTE_TOKEN)
              + '&kind=' + encodeURIComponent(REPORT_KIND))
          .then(function (r) { return r.json(); })
          .then(function (v) {
            // null means the app did not recognise this page; that is not
            // staleness and must not be reported as it.
            if (typeof v.revision === 'number' && v.revision !== REPORT_REV) {
              stale.classList.add('show');
            }
          })
          .catch(function () { /* app closed, or no listener - stay quiet */ });
      }, 4000);
    }
  }

  var notesBtn = document.getElementById('tc-notes');
  if (notesBtn) {
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
    notesBtn.addEventListener('click', function () {
      var off = !document.body.classList.contains('notes-off');
      paint(off);
      remember(off);
    });
  }
})();
