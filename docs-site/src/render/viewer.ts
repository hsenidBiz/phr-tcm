// The full-screen view of a figure: the shot as large as the window allows
// (keeping its shape), its markers, spotlight and captions working as they
// do inline, and a compact control list in a side panel (a strip below on
// a narrow window) that never covers the shot.
//
// A modal dialog: Escape, the close button or a click outside the shot
// closes it; Tab stays inside it; the page behind is inert and does not
// scroll; focus goes back to the figure's expand button. Its keys are
// handled on the document while it is open, so Escape works wherever focus
// is - even on the page body after a click on the shot.

import { h } from "./dom";
import { icon } from "./icons";

export type ViewerContent = {
  /** The dialog's name: the shot's alt text. */
  title: string;
  /** Where it is: the screen, and the group on a grouped screen. */
  context: string;
  figure: HTMLElement;
  list: HTMLElement | null;
};

export type Viewer = {
  el: HTMLElement;
  open(build: () => ViewerContent, from: HTMLElement): void;
  close(): void;
  isOpen(): boolean;
  /** The page behind it, made inert while it is open. */
  setBackground(els: HTMLElement[]): void;
};

const FOCUSABLE = 'button:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

export function createViewer(): Viewer {
  const titleId = "viewer-title";
  const title = h("h2", { class: "viewer-title", id: titleId });
  const context = h("p", { class: "viewer-context" });
  const closeBtn = h("button", { type: "button", class: "icon-btn viewer-close", "aria-label": "Close full screen" }, icon("close"));
  const stage = h("div", { class: "viewer-stage" });
  const side = h("div", { class: "viewer-side" });
  const body = h("div", { class: "viewer-body" }, stage, side);
  const dialog = h(
    "div",
    // tabindex -1: a click on the shot or the list focuses the dialog itself, never the page.
    { class: "viewer-dialog", role: "dialog", "aria-modal": "true", "aria-labelledby": titleId, tabindex: "-1" },
    h("header", { class: "viewer-head" }, h("div", { class: "viewer-heading" }, context, title), closeBtn),
    body,
  );
  const backdrop = h("div", { class: "viewer-backdrop" });
  const el = h("div", { class: "viewer", hidden: true }, backdrop, dialog);

  let returnFocus: HTMLElement | null = null;
  let background: HTMLElement[] = [];

  const focusables = () =>
    [...dialog.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((e) => !e.closest("[hidden]"));

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === "Tab") {
      const all = focusables();
      if (!all.length) return;
      const first = all[0];
      const last = all[all.length - 1];
      const at = document.activeElement;
      if (e.shiftKey && (at === first || !dialog.contains(at))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (at === last || !dialog.contains(at))) {
        e.preventDefault();
        first.focus();
      }
      e.stopPropagation();
    }
  }

  // Focus that escapes (a click on the page behind, a screen reader jump)
  // is brought back.
  function onFocusIn(e: FocusEvent) {
    if (el.hidden) return;
    const t = e.target as Node | null;
    if (t && !el.contains(t)) closeBtn.focus();
  }

  function open(build: () => ViewerContent, from: HTMLElement) {
    const c = build();
    title.textContent = c.title;
    context.textContent = c.context;
    stage.replaceChildren(c.figure);
    side.replaceChildren(...(c.list ? [c.list] : []));
    body.classList.toggle("has-list", !!c.list);
    returnFocus = from;
    if (el.hidden) {
      el.hidden = false;
      document.documentElement.classList.add("viewer-open");
      for (const b of background) b.setAttribute("inert", "");
      document.addEventListener("focusin", onFocusIn);
      document.addEventListener("keydown", onKey);
    }
    closeBtn.focus();
  }

  function close() {
    if (el.hidden) return;
    el.hidden = true;
    document.documentElement.classList.remove("viewer-open");
    for (const b of background) b.removeAttribute("inert");
    document.removeEventListener("focusin", onFocusIn);
    document.removeEventListener("keydown", onKey);
    stage.replaceChildren();
    side.replaceChildren();
    const back = returnFocus;
    returnFocus = null;
    back?.focus?.({ preventScroll: true });
  }

  closeBtn.addEventListener("click", () => close());
  backdrop.addEventListener("click", () => close());
  // The empty space around the shot is backdrop too.
  stage.addEventListener("click", (e) => {
    if (e.target === stage) close();
  });

  return {
    el,
    open,
    close,
    isOpen: () => !el.hidden,
    setBackground(els) {
      background = els;
    },
  };
}
