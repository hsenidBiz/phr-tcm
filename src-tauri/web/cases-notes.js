
(function () {
  // One save in flight per box, the newest text next. Each debounced save
  // used to be its own fetch; the server handles each on its own thread,
  // so an older text could land last and the status showed whichever reply
  // came back last. Now a save waits for the one in flight, only the newest
  // waiting text is sent after it, and only the newest save's reply is shown.
  function makeQueue(send, report) {
    var seq = 0, inflight = false, pending = null;
    function run(payload, n) {
      inflight = true;
      send(payload).then(
        function (r) { finish(n, r, null); },
        function (e) { finish(n, null, e || new Error('failed')); }
      );
    }
    function finish(n, r, err) {
      inflight = false;
      if (pending) {
        var p = pending;
        pending = null;
        run(p.payload, p.n);
        return;
      }
      if (n === seq) report(r, err);
    }
    return function save(payload) {
      seq++;
      if (inflight) { pending = { payload: payload, n: seq }; return; }
      run(payload, seq);
    };
  }

  // How many boxes have an edit not yet safely on disk: an armed debounce
  // timer, a save in flight, or a newer save waiting behind one. The live
  // update in cases-page.js checks this before swapping in a fresh copy -
  // a reviewer can click out of a textarea (so it is no longer focused)
  // before the debounce fires, or while the save it armed is still in
  // flight or queued, and a swap during any of that would show the box's
  // OLD text under whatever was typed next. One counter for the whole
  // page: the poll only needs to know "is anything dirty", never which box.
  var busyCount = 0;
  function busyStart() { busyCount++; }
  function busyEnd() { if (busyCount > 0) busyCount--; }
  window.tcmNotes = { makeQueue: makeQueue, busy: function () { return busyCount; } };

  function wire(box, status, build) {
    var timer = null;
    // Whether THIS box currently holds the one busy-count unit it is
    // allowed to hold - typing again while already dirty (armed, in
    // flight, or queued - see makeQueue) must not double-count it.
    var dirty = false;
    function settle() {
      if (dirty) { dirty = false; busyEnd(); }
    }
    var save = makeQueue(function (payload) {
      return fetch('http://127.0.0.1:' + NOTE_PORT + '/note', {
        method: 'POST',
        headers: { 'Content-Type': 'text/plain' },
        body: JSON.stringify(payload)
      }).then(function (r) { return r.json(); });
    }, function (r, err) {
      // The report callback fires only for the newest save once nothing is
      // queued behind it (see makeQueue's `finish`) - exactly when this box
      // stops being dirty, saved or not.
      settle();
      if (err) {
        status.className = 'note-status bad';
        status.textContent = 'Not saved — the app is closed';
      } else if (r && r.ok) {
        status.className = 'note-status';
        status.textContent = 'Saved ✓';
      } else {
        status.className = 'note-status bad';
        status.textContent = 'Not saved — ' + ((r && r.error) || 'the app refused it');
      }
    });
    box.addEventListener('input', function () {
      if (!dirty) { dirty = true; busyStart(); }
      status.className = 'note-status';
      status.textContent = 'Saving…';
      clearTimeout(timer);
      timer = setTimeout(function () { save(build(box.value)); }, 600);
    });
  }

  // The identities live in the #tc-data JSON block INSIDE the swappable
  // content, so a live swap brings fresh boxes and fresh identities along
  // together - a comment box must never address the title a case had when
  // the tab was opened. Read per save, so they are always current.
  function identities() {
    var el = document.getElementById('tc-data');
    if (!el) return { cases: [], files: [] };
    try { return JSON.parse(el.textContent) || { cases: [], files: [] }; }
    catch (e) { return { cases: [], files: [] }; }
  }

  // Re-runnable: the live update calls this again after swapping fresh
  // content in. The data-wired guard makes it idempotent - a box bound
  // twice would send every save twice.
  window.__tcmWireNotes = function () {
    Array.prototype.forEach.call(document.querySelectorAll('[data-ado]'), function (box) {
      if (box.dataset.wired) return;
      box.dataset.wired = '1';
      wire(box, document.getElementById(box.dataset.status), function (text) {
        return { token: NOTE_TOKEN, kind: 'ado', org: NOTE_ORG, case_id: Number(box.dataset.ado), text: text };
      });
    });

    Array.prototype.forEach.call(document.querySelectorAll('[data-case]'), function (box) {
      if (box.dataset.wired) return;
      box.dataset.wired = '1';
      wire(box, document.getElementById(box.dataset.status), function (text) {
        var data = identities();
        var t = data.cases[Number(box.dataset.case)] || {};
        return {
          token: NOTE_TOKEN, kind: 'case', path: t.path, id: t.id, title: t.title,
          key: t.key || '', pbi_id: data.pbi, text: text
        };
      });
    });

    Array.prototype.forEach.call(document.querySelectorAll('[data-file]'), function (box) {
      if (box.dataset.wired) return;
      box.dataset.wired = '1';
      wire(box, document.getElementById(box.dataset.status), function (text) {
        var f = identities().files[Number(box.dataset.file)] || {};
        return { token: NOTE_TOKEN, kind: 'general', path: f.path, text: text };
      });
    });
  };
  window.__tcmWireNotes();
})();
