
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

  Array.prototype.forEach.call(document.querySelectorAll('[data-ado]'), function (box) {
    wire(box, document.getElementById(box.dataset.status), function (text) {
      return { token: NOTE_TOKEN, kind: 'ado', org: NOTE_ORG, case_id: Number(box.dataset.ado), text: text };
    });
  });

  Array.prototype.forEach.call(document.querySelectorAll('[data-case]'), function (box) {
    var t = DRAFT_CASES[Number(box.dataset.case)];
    wire(box, document.getElementById(box.dataset.status), function (text) {
      return { token: NOTE_TOKEN, kind: 'case', path: t.path, id: t.id, title: t.title, text: text };
    });
  });

  Array.prototype.forEach.call(document.querySelectorAll('[data-file]'), function (box) {
    var f = DRAFT_FILES[Number(box.dataset.file)];
    wire(box, document.getElementById(box.dataset.status), function (text) {
      return { token: NOTE_TOKEN, kind: 'general', path: f.path, text: text };
    });
  });
})();
