// Light / dark. Light until the person picks dark, whatever the system is
// set to - as the app itself, whose default look is Light with violet; the
// pick is remembered (storage can be blocked on file:// or in private
// windows, so every access is guarded). Screenshots swap with the theme.

import { h } from "./dom";
import { icon } from "./icons";

export type Theme = "light" | "dark";
const KEY = "tcm-help-theme";

function stored(): Theme | null {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : null;
  } catch {
    return null;
  }
}

function store(t: Theme) {
  try {
    localStorage.setItem(KEY, t);
  } catch {
    /* not remembered - the toggle still works for this visit */
  }
}

export function initialTheme(): Theme {
  return stored() ?? "light";
}

export function shotSrc(theme: Theme, shot: string): string {
  return `img/${theme}/${shot}.jpg`;
}

/** Sets the page theme and points every screenshot at that theme's image. */
export function applyTheme(theme: Theme, scope: ParentNode = document) {
  document.documentElement.dataset.theme = theme;
  for (const img of scope.querySelectorAll<HTMLImageElement>("img[data-shot]")) {
    img.src = shotSrc(theme, img.dataset.shot ?? "");
  }
}

export function currentTheme(): Theme {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

/** The top bar's toggle. Returns the button and a cleanup (nothing to undo now; kept for the caller). */
export function themeToggle(scope: ParentNode): { button: HTMLButtonElement; dispose: () => void } {
  const button = h("button", { type: "button", class: "icon-btn theme-toggle" });
  const sync = () => {
    const next = currentTheme() === "dark" ? "light" : "dark";
    button.setAttribute("aria-label", `Switch to ${next} theme`);
    button.title = `Switch to ${next} theme`;
    button.replaceChildren(icon(next === "light" ? "sun" : "moon"));
  };
  button.addEventListener("click", () => {
    const next: Theme = currentTheme() === "dark" ? "light" : "dark";
    store(next);
    applyTheme(next, scope);
    sync();
  });

  sync();
  return { button, dispose: () => {} };
}
