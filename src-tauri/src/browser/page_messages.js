(() => {
  const MAX_FRAME_DEPTH = 5;
  const MAX_RETURNED = 200;
  const MAX_SCANNED = 1000;
  const MAX_CHARS = 1000;
  const MAX_STEP_CHARS = 200;
  const MESSAGE_CLASS = /toast|alert|notif|snackbar|swal|bootbox|invalid|error|feedback|validation|message/i;
  const URGENT_CLASS = /toast|alert|notif|snackbar|swal|bootbox/i;
  const STEP_CLASS = /step|wizard/i;
  const SKIP_TAGS = { SCRIPT: 1, STYLE: 1, NOSCRIPT: 1, TEMPLATE: 1, INPUT: 1, SELECT: 1, TEXTAREA: 1, OPTION: 1, BUTTON: 1 };

  const collapse = (s) => String(s || '').replace(/\s+/g, ' ').trim();
  const classOf = (el) => (typeof el.className === 'string' ? el.className : el.getAttribute('class') || '');

  // Hidden by what the page itself says, so it reads the same in a browser
  // and in a document with no layout: the hidden attribute, display none,
  // opacity 0 (a toast waiting in the markup to fade in is not shown yet),
  // or - on the element itself, since it is inherited - visibility hidden.
  // A fixed element has no offsetParent and is still on screen, which is
  // why offsetParent is not asked.
  function hiddenSelf(el, view, withVisibility) {
    if (el.getAttribute('hidden') !== null) return true;
    const cs = view.getComputedStyle(el);
    if (cs.display === 'none' || parseFloat(cs.opacity) === 0) return true;
    return withVisibility && (cs.visibility === 'hidden' || cs.visibility === 'collapse');
  }

  function shown(el) {
    const view = el.ownerDocument.defaultView;
    for (let n = el; n && n.nodeType === 1; n = n.parentElement) {
      if (hiddenSelf(n, view, n === el)) return false;
    }
    return true;
  }

  // The element's text the way a person reads it: no hidden part, no
  // controls or buttons (an x or an OK is not a message), and with
  // noLabels no labels either. The element is already known to be shown,
  // so only each descendant's own style is asked.
  function textOf(el, noLabels) {
    const view = el.ownerDocument.defaultView;
    let out = '';
    (function walk(n) {
      for (let c = n.firstChild; c; c = c.nextSibling) {
        if (c.nodeType === 3) out += c.nodeValue;
        else if (c.nodeType === 1 && !SKIP_TAGS[c.tagName] && !(noLabels && c.tagName === 'LABEL') && !hiddenSelf(c, view, true)) {
          out += ' ';
          walk(c);
          out += ' ';
        }
      }
    })(el);
    return collapse(out).slice(0, MAX_CHARS);
  }

  // What an invalid field points at as its message: the element its
  // aria-describedby names, and the element right after it.
  function relatedTo(doc) {
    const out = new Set();
    doc.querySelectorAll('[aria-invalid="true"]').forEach((f) => {
      (f.getAttribute('aria-describedby') || '').split(/\s+/).forEach((id) => {
        const t = id ? doc.getElementById(id) : null;
        if (t) out.add(t);
      });
      const next = f.nextElementSibling;
      if (next && !SKIP_TAGS[next.tagName] && next.tagName !== 'LABEL' && collapse(next.textContent) !== '') out.add(next);
    });
    return out;
  }

  function isMessage(el, related) {
    if (SKIP_TAGS[el.tagName] || el === el.ownerDocument.body || el === el.ownerDocument.documentElement) return false;
    const live = el.getAttribute('aria-live');
    return (
      el.matches('[role="alert"],[role="status"]') ||
      (live !== null && live !== 'off') ||
      MESSAGE_CLASS.test(classOf(el)) ||
      related.has(el)
    );
  }

  function isUrgent(el) {
    const live = el.getAttribute('aria-live');
    return el.matches('[role="alert"],[role="status"]') || (live !== null && live !== 'off') || URGENT_CLASS.test(classOf(el));
  }

  // The matches, nested as the page nests them (document order in, forest out).
  function forest(found) {
    const roots = [];
    const stack = [];
    found.forEach((el) => {
      while (stack.length && !stack[stack.length - 1].el.contains(el)) stack.pop();
      const node = { el, kids: [] };
      (stack.length ? stack[stack.length - 1].kids : roots).push(node);
      stack.push(node);
    });
    return roots;
  }

  // A list: two of the matched children are the same tag with the same
  // first class.
  function isList(kids) {
    const keys = new Set();
    return kids.some((k) => {
      const key = k.el.tagName + '|' + (classOf(k.el).split(/\s+/)[0] || '');
      if (keys.has(key)) return true;
      keys.add(key);
      return false;
    });
  }

  function holdsControl(el) {
    return Array.from(el.querySelectorAll('input,select,textarea')).some((c) => c.type !== 'hidden' && shown(c));
  }

  // The outermost match speaks for what is inside it - a toast with a title
  // and a body is one message - unless it is a wrapper: a list of matches
  // (a toast stack) or the holder of a field (a form, a form group). Those
  // hand over to the matches inside them, and a field's holder with none
  // says what it says outside its labels and controls.
  function report(node, label, out) {
    const el = node.el;
    if (out.length >= MAX_SCANNED || !shown(el)) return;
    const holder = holdsControl(el);
    if (node.kids.length > 0 && (holder || isList(node.kids))) {
      node.kids.forEach((k) => report(k, label, out));
      return;
    }
    const text = textOf(el, holder);
    if (text !== '') out.push({ text, where: label, urgent: isUrgent(el) });
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

  const found = [];
  docs.forEach(({ doc, label }) => {
    const related = relatedTo(doc);
    const matched = [];
    doc.querySelectorAll('*').forEach((el) => {
      if (isMessage(el, related)) matched.push(el);
    });
    forest(matched).forEach((root) => report(root, label, found));
  });
  // A page can hold more messages than are read; the ones a page uses for
  // news come first, so a crowd of ordinary rows cannot push a toast out.
  const messages = found
    .filter((m) => m.urgent)
    .concat(found.filter((m) => !m.urgent))
    .slice(0, MAX_RETURNED)
    .map((m) => ({ text: m.text, where: m.where }));

  function stepIn(doc) {
    const current = doc.querySelector('[aria-current="step"]');
    if (current && shown(current)) return textOf(current, false).slice(0, MAX_STEP_CHARS);
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
      const text = textOf(el, false);
      if (text !== '') return text.slice(0, MAX_STEP_CHARS);
    }
    return null;
  }

  let step = null;
  for (let i = 0; i < docs.length && step === null; i++) step = stepIn(docs[i].doc);
  return { messages, step };
})()
