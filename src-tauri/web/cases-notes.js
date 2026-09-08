
(function () {
  function wire(box, status, build) {
    var timer = null;
    box.addEventListener('input', function () {
      status.className = 'note-status';
      status.textContent = 'Saving…';
      clearTimeout(timer);
      timer = setTimeout(function () {
        fetch('http://127.0.0.1:' + NOTE_PORT + '/note', {
          method: 'POST',
          headers: { 'Content-Type': 'text/plain' },
          body: JSON.stringify(build(box.value))
        }).then(function (r) { return r.json(); }).then(function (r) {
          if (r && r.ok) {
            status.className = 'note-status';
            status.textContent = 'Saved ✓';
          } else {
            status.className = 'note-status bad';
            status.textContent = 'Not saved — ' + ((r && r.error) || 'the app refused it');
          }
        }).catch(function () {
          status.className = 'note-status bad';
          status.textContent = 'Not saved — the app is closed';
        });
      }, 600);
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
        var t = identities().cases[Number(box.dataset.case)] || {};
        return { token: NOTE_TOKEN, kind: 'case', path: t.path, id: t.id, title: t.title, text: text };
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
