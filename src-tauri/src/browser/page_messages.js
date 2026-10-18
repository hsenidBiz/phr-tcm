(() => {
  const MAX_FRAME_DEPTH = 5;
  const MAX_FOUND = 50;
  const MAX_CHARS = 1000;
  const MAX_STEP_CHARS = 200;
  const MESSAGE_CLASS = /toast|alert|notif|snackbar|swal|bootbox|invalid|error|feedback|validation|message/i;
  const STEP_CLASS = /step|wizard/i;
  const SKIP_TAGS = { SCRIPT: 1, STYLE: 1, NOSCRIPT: 1, TEMPLATE: 1, INPUT: 1, SELECT: 1, TEXTAREA: 1, OPTION: 1 };

  const collapse = (s) => String(s || '').replace(/\s+/g, ' ').trim();
  const classOf = (el) => (typeof el.className === 'string' ? el.className : el.getAttribute('class') || '');

  // Hidden by what the page itself says, so it reads the same in a browser
  // and in a document with no layout: the hidden attribute, display none, or
  // visibility hidden on the element or any element around it. A fixed
  // element has no offsetParent and is still on screen, which is why
  // offsetParent is not asked.
  function shown(el) {
    const view = el.ownerDocument.defaultView;
    for (let n = el; n && n.nodeType === 1; n = n.parentElement) {
      if (n.hidden || n.getAttribute('hidden') !== null) return false;
      const cs = view.getComputedStyle(n);
      if (cs.display === 'none') return false;
      if (n === el && (cs.visibility === 'hidden' || cs.visibility === 'collapse')) return false;
    }
    return true;
  }

  function textOf(el) {
    let out = '';
    (function walk(n) {
      for (let c = n.firstChild; c; c = c.nextSibling) {
        if (c.nodeType === 3) out += c.nodeValue;
        else if (c.nodeType === 1 && !SKIP_TAGS[c.tagName] && shown(c)) {
          out += ' ';
          walk(c);
          out += ' ';
        }
      }
    })(el);
    return collapse(out).slice(0, MAX_CHARS);
  }

  function describedBy(doc) {
    const ids = new Set();
    doc.querySelectorAll('[aria-invalid="true"]').forEach((f) => {
      (f.getAttribute('aria-describedby') || '').split(/\s+/).forEach((id) => {
        if (id) ids.add(id);
      });
    });
    return ids;
  }

  function isMessage(el, ids) {
    if (SKIP_TAGS[el.tagName] || el === el.ownerDocument.body || el === el.ownerDocument.documentElement) return false;
    const live = el.getAttribute('aria-live');
    return (
      el.matches('[role="alert"],[role="status"]') ||
      (live !== null && live !== 'off') ||
      MESSAGE_CLASS.test(classOf(el)) ||
      (el.id !== '' && ids.has(el.id))
    );
  }

  function frameLabel(frame) {
    const name = frame.getAttribute('title') || frame.getAttribute('name') || frame.id;
    return name ? 'frame ' + collapse(name) : 'frame';
  }

  function documents(doc, label, depth, out) {
    out.push({ doc, label });
    if (depth >= MAX_FRAME_DEPTH) return;
    doc.querySelectorAll('iframe,frame').forEach((f) => {
      let inner = null;
      try {
        inner = f.contentDocument;
      } catch (e) {
        inner = null;
      }
      if (inner && inner.documentElement && shown(f)) documents(inner, frameLabel(f), depth + 1, out);
    });
  }

  const docs = [];
  documents(document, 'page', 0, docs);

  const messages = [];
  const seen = new Set();
  docs.forEach(({ doc, label }) => {
    const ids = describedBy(doc);
    const found = [];
    doc.querySelectorAll('*').forEach((el) => {
      if (isMessage(el, ids)) found.push(el);
    });
    // The outermost of nested matches speaks for them all.
    found
      .filter((el) => !found.some((o) => o !== el && o.contains(el)))
      .forEach((el) => {
        if (messages.length >= MAX_FOUND || !shown(el)) return;
        const text = textOf(el);
        const key = label + '\n' + text;
        if (text === '' || seen.has(key)) return;
        seen.add(key);
        messages.push({ text, where: label });
      });
  });

  function stepIn(doc) {
    const current = doc.querySelector('[aria-current="step"]');
    if (current && shown(current)) return textOf(current).slice(0, MAX_STEP_CHARS);
    const all = doc.querySelectorAll('*');
    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      if (!(el.classList.contains('active') || el.classList.contains('current'))) continue;
      let inside = false;
      for (let p = el.parentElement; p; p = p.parentElement) {
        if (STEP_CLASS.test(classOf(p))) {
          inside = true;
          break;
        }
      }
      if (!inside || !shown(el)) continue;
      const text = textOf(el);
      if (text !== '') return text.slice(0, MAX_STEP_CHARS);
    }
    return null;
  }

  let step = null;
  for (let i = 0; i < docs.length && step === null; i++) step = stepIn(docs[i].doc);
  return { messages, step };
})()
