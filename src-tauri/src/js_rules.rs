//! JavaScript rules shared by every script the app puts into a page, written
//! once so the places that read them can never disagree. Each macro expands
//! to one string literal, a function expression, for `concat!` to build a
//! script around - which is why they live here, in a module declared before
//! every other: a `macro_rules!` is in scope only after its definition.

/// One JavaScript function expression, `(e) => box | null`: the box (`left`,
/// `top`, `right`, `bottom`, `width`, `height`, in the viewport) of what the
/// element `e` holds that a person can see, or `null` when nothing is. For an
/// element with no box of its own: a link around a floated or absolutely
/// placed icon, or one that is `display: contents`.
///
/// Counted: each drawn element inside it and each run of words, whose own
/// `visibility` is `visible` (so content in a `visibility: hidden` child is
/// not counted, whatever its box), never inside a `display: none` element.
/// Each piece is cut down by every element around it that clips what
/// overflows it (`overflow` `hidden`, `clip`, `auto` or `scroll`), up to the
/// page: words overflowing a collapsed panel are not seen.
macro_rules! content_box_js {
    () => {
        r#"((e) => {
  const doc = e.ownerDocument, view = doc.defaultView;
  const css = (x) => view.getComputedStyle(x);
  const clipping = /^(hidden|clip|auto|scroll)$/;
  const clips = (s) => clipping.test(s.overflowX) || clipping.test(s.overflowY);
  const cut = (r, n) => {
    let l = r.left, t = r.top, rt = r.right, b = r.bottom;
    for (let a = n.parentElement; a; a = a.parentElement) {
      const s = css(a);
      if (s.display === 'contents' || !clips(s)) continue;
      const c = a.getBoundingClientRect();
      l = Math.max(l, c.left); t = Math.max(t, c.top); rt = Math.min(rt, c.right); b = Math.min(b, c.bottom);
      if (rt <= l || b <= t) return null;
    }
    return rt > l && b > t ? { left: l, top: t, right: rt, bottom: b } : null;
  };
  let box = null;
  const add = (r) => {
    if (!r) return;
    box = !box ? r : {
      left: Math.min(box.left, r.left), top: Math.min(box.top, r.top),
      right: Math.max(box.right, r.right), bottom: Math.max(box.bottom, r.bottom),
    };
  };
  const walker = doc.createTreeWalker(e, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, {
    acceptNode: (n) => n.nodeType === 1 && css(n).display === 'none' ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  });
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (n.nodeType === 3) {
      if (!n.textContent.trim() || !n.parentElement || css(n.parentElement).visibility !== 'visible') continue;
      const range = doc.createRange();
      range.selectNodeContents(n);
      for (const r of range.getClientRects()) if (r.width > 0 && r.height > 0) add(cut(r, n));
    } else if (css(n).visibility === 'visible') {
      const r = n.getBoundingClientRect();
      if (r.width > 0 && r.height > 0 && n.checkVisibility({ visibilityProperty: true })) add(cut(r, n));
    }
  }
  return box && { ...box, width: box.right - box.left, height: box.bottom - box.top };
})"#
    };
}

/// One JavaScript function expression, `(e) => boolean`: can a person see
/// the element `e`? The one rule every locator (role, css and text), a
/// click's own check (`input::PROBE_JS`), the page read's frames and the
/// recorder's text count all read:
///
/// - it is drawn: `checkVisibility` (with `visibility`) and a box of some
///   width and height;
/// - or it is a control sized to nothing that a drawn label stands for
///   (the usual toggle switch);
/// - or it has no box of its own and what it holds is seen
///   (`content_box_js`): its own `visibility` is `visible`, the nearest
///   element around it with a box is shown, and it does not clip its
///   content (a collapsed panel, `height: 0; overflow: hidden`, is hidden
///   whatever overflows it).
macro_rules! seen_js {
    () => {
        concat!(
            r#"((e) => {
  const contentBox = "#,
            content_box_js!(),
            r#";
  const shown = (x) => x.checkVisibility({ visibilityProperty: true });
  const drawn = (x) => { const r = x.getBoundingClientRect(); return shown(x) && r.width > 0 && r.height > 0; };
  if (drawn(e)) return true;
  if (e.labels && shown(e) && Array.from(e.labels).some(drawn)) return true;
  const css = (x) => e.ownerDocument.defaultView.getComputedStyle(x);
  const s = css(e);
  if (s.visibility !== 'visible') return false;
  if (s.display !== 'contents') {
    const r = e.getBoundingClientRect();
    if (r.width > 0 && r.height > 0) return false;
    const clipping = /^(hidden|clip|auto|scroll)$/;
    if (clipping.test(s.overflowX) || clipping.test(s.overflowY)) return false;
  }
  let boxed = e;
  while (boxed && css(boxed).display === 'contents') boxed = boxed.parentElement;
  if (!boxed || !shown(boxed)) return false;
  return !!contentBox(e);
})"#
        )
    };
}
