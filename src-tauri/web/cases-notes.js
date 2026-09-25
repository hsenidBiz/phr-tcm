
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
  // A save the app never answers - it hung, or its listener stalled - must
  // still end: until it does the box counts as busy, and the page's live
  // update (cases-page.js) waits for busy() to reach 0 before it swaps.
  var NOTE_TIMEOUT_MS = 10000;
  function postNote(payload) {
    return new Promise(function (ok, fail) {
      var done = false;
      var ctl = typeof AbortController === 'function' ? new AbortController() : null;
      var timer = setTimeout(function () {
        if (done) return;
        done = true;
        if (ctl) ctl.abort();
        var e = new Error('the app did not answer');
        e.name = 'TimeoutError';
        fail(e);
      }, NOTE_TIMEOUT_MS);
      fetch('http://127.0.0.1:' + NOTE_PORT + '/note', {
        method: 'POST',
        headers: { 'Content-Type': 'text/plain' },
        body: JSON.stringify(payload),
        signal: ctl ? ctl.signal : undefined
      }).then(function (r) { return r.json(); }).then(
        function (v) { if (done) return; done = true; clearTimeout(timer); ok(v); },
        function (e) { if (done) return; done = true; clearTimeout(timer); fail(e); }
      );
    });
  }
  // The identities live in the #tc-data JSON block INSIDE the swappable
  // content, so a live swap brings fresh boxes and fresh identities along
  // together - a comment box must never address the title a case had when
  // the tab was opened. Read per call, so they are always current: before a
  // swap for boxKey at failure time, after one for boxKey at restore time.
  function identities() {
    var el = document.getElementById('tc-data');
    if (!el) return { cases: [], files: [] };
    try { return JSON.parse(el.textContent) || { cases: [], files: [] }; }
    catch (e) { return { cases: [], files: [] }; }
  }

  // A save that did not succeed - timed out, the app was closed, or the app
  // refused it - leaves the box's text and status here, keyed by identity,
  // and cleared the moment a save from that box succeeds. A live swap
  // (cases-page.js) calls restoreUnsaved right after it adopts fresh markup,
  // so the fresh copy's OLDER text - and the fact that nothing told the
  // reviewer their edit did not land - can never silently replace what is
  // still sitting unsaved in the box. Freeing the box from busy() as soon as
  // a save ends (rather than only once it succeeds) is what made this
  // reachable: before, a save that never finished held busy() up forever,
  // so a swap could not happen at all.
  //
  // The key must be stable across a swap, not positional: `data-case`/
  // `data-file` are this RENDER's slot/index, and a case inserted, removed
  // or reordered gives the same slot to a different case. Keying by index
  // alone once misfiled a failed comment onto whatever case now sits in
  // that slot - and a retry from there saved it into the wrong case's JSON.
  // `data-ado` (a work item id) is already stable and needs no lookup.
  var failedByKey = {};
  function boxKey(box) {
    if (box.dataset.ado != null) return 'ado:' + box.dataset.ado;
    if (box.dataset.case != null) {
      var c = identities().cases[Number(box.dataset.case)];
      // path + the occurrence key the app already tracks (e.g. "t:login#2")
      // - the identity a comment is actually saved against (see `wire`'s
      // case payload below), and the one thing that does not move.
      return c ? 'case:' + (c.path || '') + '\n' + (c.key || '') : null;
    }
    if (box.dataset.file != null) {
      var f = identities().files[Number(box.dataset.file)];
      return f ? 'file:' + (f.path || '') : null;
    }
    return null;
  }
  // Restores any carried failure onto the box that still has its identity,
  // and drops any entry whose case or file is no longer in the fresh copy -
  // one that matches nothing must never be shown under a different box.
  function restoreUnsaved(root) {
    var present = {};
    Array.prototype.forEach.call((root || document).querySelectorAll('[data-ado],[data-case],[data-file]'), function (box) {
      var key = boxKey(box);
      if (!key) return;
      present[key] = true;
      var f = failedByKey[key];
      if (!f) return;
      box.value = f.value;
      var status = document.getElementById(box.dataset.status);
      if (status) { status.className = f.className; status.textContent = f.text; }
    });
    for (var key2 in failedByKey) {
      if (Object.prototype.hasOwnProperty.call(failedByKey, key2) && !present[key2]) delete failedByKey[key2];
    }
  }
  window.tcmNotes = {
    makeQueue: makeQueue,
    busy: function () { return busyCount; },
    timeoutMs: NOTE_TIMEOUT_MS,
    restoreUnsaved: restoreUnsaved
  };

  function wire(box, status, build) {
    var timer = null;
    var key = boxKey(box);
    // Whether THIS box currently holds the one busy-count unit it is
    // allowed to hold - typing again while already dirty (armed, in
    // flight, or queued - see makeQueue) must not double-count it.
    var dirty = false;
    function settle() {
      if (dirty) { dirty = false; busyEnd(); }
    }
    var save = makeQueue(postNote, function (r, err) {
      // The report callback fires only for the newest save once nothing is
      // queued behind it (see makeQueue's `finish`) - exactly when this box
      // stops being dirty, saved or not.
      settle();
      if (err) {
        status.className = 'note-status bad';
        status.textContent = err.name === 'TimeoutError'
          ? 'Not saved - the app did not answer'
          : 'Not saved — the app is closed';
      } else if (r && r.ok) {
        status.className = 'note-status';
        status.textContent = 'Saved ✓';
      } else {
        status.className = 'note-status bad';
        status.textContent = 'Not saved — ' + ((r && r.error) || 'the app refused it');
      }
      if (key) {
        if (r && r.ok) delete failedByKey[key];
        else failedByKey[key] = { value: box.value, className: status.className, text: status.textContent };
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
