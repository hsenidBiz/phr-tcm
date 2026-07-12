export type Theme = "light" | "dark" | "system";

const KEY = "tcm-v2-theme";

export function getTheme(): Theme {
  const t = localStorage.getItem(KEY);
  return t === "light" || t === "dark" ? t : "system";
}

function resolve(theme: Theme): "light" | "dark" {
  if (theme !== "system") return theme;
  return window.matchMedia?.("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

export function applyTheme(theme: Theme) {
  document.documentElement.classList.toggle("dark", resolve(theme) === "dark");
}

export function setTheme(theme: Theme) {
  if (theme === "system") localStorage.removeItem(KEY);
  else localStorage.setItem(KEY, theme);
  applyTheme(theme);
}

export type Accent = "green" | "blue" | "violet" | "amber" | "rose";
export const ACCENTS: Accent[] = ["green", "blue", "violet", "amber", "rose"];

const ACCENT_KEY = "tcm-v2-accent";

export function getAccent(): Accent {
  const a = localStorage.getItem(ACCENT_KEY);
  return ACCENTS.includes(a as Accent) ? (a as Accent) : "green";
}

export function setAccent(accent: Accent) {
  if (accent === "green") localStorage.removeItem(ACCENT_KEY);
  else localStorage.setItem(ACCENT_KEY, accent);
  applyAccent(accent);
}

function applyAccent(accent: Accent) {
  if (accent === "green") document.documentElement.removeAttribute("data-accent");
  else document.documentElement.setAttribute("data-accent", accent);
}

/** Call once on startup. */
export function initTheme() {
  applyTheme(getTheme());
  applyAccent(getAccent());
}
