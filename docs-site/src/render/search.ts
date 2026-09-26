// Ctrl+K search over every screen, control, tip and common task, with a
// small fuzzy scorer (no library): exact > prefix > whole-word substring >
// substring > all words present > an in-order subsequence.

import type { SiteContent } from "../types";
import { h, keys, plain } from "./dom";
import { icon, type IconName } from "./icons";
import { controlAnchor } from "./section";

const norm = (s: string) => s.toLowerCase().replace(/\s+/g, " ").trim();

export function score(query: string, text: string): number {
  const q = norm(query);
  const t = norm(text);
  if (!q || !t) return 0;
  const fit = (100 * q.length) / t.length; // shorter targets rank higher
  if (t === q) return 1000;
  if (t.startsWith(q)) return 800 + fit;
  const at = t.indexOf(q);
  if (at >= 0) return (/[^a-z0-9]/.test(t[at - 1]) ? 600 : 450) + fit;
  const words = q.split(" ");
  if (words.length > 1 && words.every((w) => t.includes(w))) return 350 + fit;

  // Characters in order, rewarding runs and an early start.
  let from = 0;
  let run = 0;
  let longest = 0;
  let gaps = 0;
  let first = -1;
  for (const ch of q.replace(/ /g, "")) {
    const found = t.indexOf(ch, from);
    if (found < 0) return 0;
    if (first < 0) first = found;
    run = found === from && from > 0 ? run + 1 : 1;
    gaps += found - from;
    longest = Math.max(longest, run);
    from = found + 1;
  }
  return Math.max(1, Math.min(299, 200 + longest * 8 - gaps * 3 - first));
}

type Kind = "screen" | "control" | "tip" | "task";
type Item = { kind: Kind; title: string; context: string; href: string };

const KIND: Record<Kind, { label: string; icon: IconName; rank: number }> = {
  screen: { label: "Screen", icon: "screen", rank: 0 },
  control: { label: "Control", icon: "control", rank: 1 },
  task: { label: "Common task", icon: "task", rank: 2 },
  tip: { label: "Tip", icon: "tip", rank: 3 },
};

export function buildIndex(content: SiteContent): Item[] {
  const items: Item[] = [];
  for (const s of content.screens) {
    items.push({ kind: "screen", title: s.title, context: plain(s.summary), href: s.id });
    for (const c of s.controls) {
      items.push({
        kind: "control",
        title: plain(c.name),
        context: `${s.title} · ${plain(c.does)}`,
        href: controlAnchor(s.id, c.id),
      });
      for (const t of c.tips ?? []) {
        items.push({ kind: "tip", title: plain(t), context: `${s.title} · ${plain(c.name)}`, href: controlAnchor(s.id, c.id) });
      }
    }
    for (const t of s.tips ?? []) items.push({ kind: "tip", title: plain(t), context: s.title, href: s.id });
    for (const how of s.howTo ?? []) {
      items.push({ kind: "task", title: plain(how.title), context: `${s.title} · ${how.steps.map(plain).join(" ")}`, href: s.id });
    }
  }
  for (const r of content.recipes) {
    items.push({ kind: "task", title: r.title, context: r.steps.map((s) => plain(s.text)).join(" · "), href: `common-tasks/${r.id}` });
  }
  return items;
}

/** The context line counts only for a real word match - a loose
 *  subsequence across a long summary matches almost anything. */
function contextScore(q: string, context: string): number {
  if (q.length < 2) return 0;
  const s = score(q, context);
  return s >= 350 ? s * 0.5 : 0;
}

export function search(items: Item[], query: string, limit = 12): Item[] {
  const q = norm(query);
  if (!q) return [];
  return items
    .map((item) => ({ item, s: Math.max(score(q, item.title), contextScore(q, item.context)) }))
    .filter((r) => r.s > 0)
    .sort((a, b) => b.s - a.s || KIND[a.item.kind].rank - KIND[b.item.kind].rank)
    .slice(0, limit)
    .map((r) => r.item);
}

/** Wraps the first case-insensitive occurrence of the query in <mark>. */
function highlight(text: string, query: string): Node[] {
  const q = norm(query);
  const at = q ? text.toLowerCase().indexOf(q) : -1;
  if (at < 0) return [document.createTextNode(text)];
  return [
    document.createTextNode(text.slice(0, at)),
    h("mark", {}, text.slice(at, at + q.length)),
    document.createTextNode(text.slice(at + q.length)),
  ];
}

export type Palette = { el: HTMLElement; open(): void; close(): void; isOpen(): boolean };

export function createPalette(content: SiteContent, go: (href: string) => void): Palette {
  const items = buildIndex(content);
  const suggestions: Item[] = items.filter((i) => i.kind === "screen").slice(0, 8);

  const input = h("input", {
    type: "text",
    role: "combobox",
    "aria-expanded": "true",
    "aria-controls": "palette-list",
    "aria-autocomplete": "list",
    "aria-label": "Search the guide",
    placeholder: "Search screens, buttons and tips",
    autocomplete: "off",
    spellcheck: "false",
  });
  const list = h("ul", { id: "palette-list", role: "listbox", "aria-label": "Results" });
  const empty = h("p", { class: "palette-empty" });
  const panel = h(
    "div",
    { class: "palette-panel", role: "dialog", "aria-modal": "true", "aria-label": "Search the guide" },
    h("div", { class: "palette-field" }, icon("search", 20), input, h("kbd", { class: "palette-esc" }, "Esc")),
    list,
    empty,
    h(
      "div",
      { class: "palette-foot", "aria-hidden": "true" },
      h("span", {}, h("kbd", {}, "↑"), h("kbd", {}, "↓"), " to move"),
      h("span", {}, h("kbd", {}, "Enter"), " to open"),
      h("span", {}, h("kbd", {}, "Esc"), " to close"),
    ),
  );
  const backdrop = h("div", { class: "palette-backdrop" });
  const el = h("div", { class: "palette", hidden: true }, backdrop, panel);

  let results: Item[] = [];
  let selected = 0;
  let returnFocus: HTMLElement | null = null;

  function paint() {
    const q = input.value;
    results = q.trim() ? search(items, q) : suggestions;
    selected = Math.min(selected, Math.max(0, results.length - 1));
    list.replaceChildren(
      ...results.map((item, i) => {
        const opt = h(
          "li",
          { role: "option", id: `palette-opt-${i}`, class: "option", "aria-selected": i === selected ? "true" : "false" },
          h("span", { class: `option-icon kind-${item.kind}` }, icon(KIND[item.kind].icon, 16)),
          h(
            "span",
            { class: "option-body" },
            h("span", { class: "option-title" }, ...highlight(item.title, q)),
            h("span", { class: "option-context" }, item.context),
          ),
          h("span", { class: "option-kind" }, KIND[item.kind].label),
          h("span", { class: "option-go", "aria-hidden": "true" }, icon("enter", 14)),
        );
        opt.addEventListener("mousemove", () => {
          if (selected !== i) select(i);
        });
        opt.addEventListener("click", () => choose(i));
        return opt;
      }),
    );
    list.hidden = results.length === 0;
    empty.hidden = results.length > 0;
    empty.textContent = q.trim() ? `Nothing matches “${q.trim()}”. Try fewer letters or another word.` : "Type to search the guide.";
    input.setAttribute("aria-expanded", results.length ? "true" : "false");
    syncActive();
  }

  function syncActive() {
    const opts = list.querySelectorAll<HTMLElement>('[role="option"]');
    opts.forEach((o, i) => o.setAttribute("aria-selected", i === selected ? "true" : "false"));
    const cur = opts[selected];
    if (cur) {
      input.setAttribute("aria-activedescendant", cur.id);
      cur.scrollIntoView?.({ block: "nearest" });
    } else input.removeAttribute("aria-activedescendant");
  }

  function select(i: number) {
    selected = (i + results.length) % Math.max(1, results.length);
    syncActive();
  }

  function choose(i: number) {
    const item = results[i];
    if (!item) return;
    close(false);
    go(item.href);
  }

  function open() {
    if (!el.hidden) return;
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    el.hidden = false;
    document.documentElement.classList.add("palette-open");
    input.value = "";
    selected = 0;
    paint();
    input.focus();
  }

  function close(restore = true) {
    if (el.hidden) return;
    el.hidden = true;
    document.documentElement.classList.remove("palette-open");
    if (restore) returnFocus?.focus?.({ preventScroll: true });
    returnFocus = null;
  }

  input.addEventListener("input", () => {
    selected = 0;
    paint();
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") select(selected + 1);
    else if (e.key === "ArrowUp") select(selected - 1);
    else if (e.key === "Enter") choose(selected);
    else if (e.key === "Escape") close();
    else if (e.key === "Tab") {
      /* the field is the only stop inside the dialog */
    } else return;
    e.preventDefault();
    e.stopPropagation();
  });
  backdrop.addEventListener("click", () => close());

  return { el, open, close: () => close(), isOpen: () => !el.hidden };
}

/** The search button in the top bar / hero, showing the shortcut. */
export function searchButton(className: string, label: string, onOpen: () => void): HTMLButtonElement {
  const b = h(
    "button",
    { type: "button", class: className, "aria-label": `${label} (Ctrl+K)` },
    icon("search", 16),
    h("span", { class: "search-label" }, label),
    keys("Ctrl+K"),
  );
  b.addEventListener("click", onOpen);
  return b;
}
