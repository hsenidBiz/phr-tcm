// Tiny DOM helpers. Content text is only ever set as text - never parsed
// as HTML - so the inline markup below is the whole formatting language.

type AttrValue = string | number | boolean | null | undefined;
export type Child = Node | string | null | undefined | false;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Record<string, AttrValue> = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === false || v === null || v === undefined) continue;
    if (k === "class") el.className = String(v);
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  append(el, children);
  return el;
}

export function append(el: Node, children: Child[]) {
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.appendChild(typeof c === "string" ? document.createTextNode(c) : c);
  }
}

const INLINE = /\[\[(.+?)\]\]|\*\*(.+?)\*\*/g;

/** Renders the content's inline markup: [[Ctrl+K]] as key caps, **Label** as a strong UI label. */
export function rich(text: string): DocumentFragment {
  const frag = document.createDocumentFragment();
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    const at = m.index ?? 0;
    if (at > last) frag.appendChild(document.createTextNode(text.slice(last, at)));
    if (m[1] !== undefined) frag.appendChild(keys(m[1]));
    else frag.appendChild(h("strong", { class: "ui-label" }, m[2]));
    last = at + m[0].length;
  }
  if (last < text.length) frag.appendChild(document.createTextNode(text.slice(last)));
  return frag;
}

/** "Ctrl+Shift+K" as a row of key caps. A trailing "+" is the plus key itself. */
export function keys(combo: string): HTMLElement {
  const parts = combo.endsWith("++")
    ? [...combo.slice(0, -2).split("+"), "+"]
    : combo.split("+");
  const wrap = h("span", { class: "keys" });
  parts
    .filter((p) => p.length > 0)
    .forEach((p, i) => {
      if (i > 0) wrap.appendChild(h("span", { class: "keys-plus", "aria-hidden": "true" }, "+"));
      wrap.appendChild(h("kbd", {}, p.trim()));
    });
  return wrap;
}

/** The same text with the inline markup removed - for search and labels. */
export function plain(text: string): string {
  return text.replace(INLINE, (_m, k: string | undefined, b: string | undefined) => k ?? b ?? "");
}

/** Constant, trusted SVG markup (never content) to a node. */
export function svg(markup: string): Node {
  const t = document.createElement("template");
  t.innerHTML = markup.trim();
  return t.content.firstChild as Node;
}

export function reducedMotion(): boolean {
  try {
    return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
}
