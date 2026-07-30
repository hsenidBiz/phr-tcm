/** Full-UI theme system: each theme is a complete palette (background,
 * surfaces, borders, text, accent) applied via the `.dark` class plus a
 * `data-theme` attribute on <html> (see index.css). "light" and "slate"
 * are the base palettes; the rest override every token. The separate
 * accent presets still compose on top (data-accent wins over the theme's
 * default accent because its CSS comes later). */

export type ThemeId = "light" | "slate" | "midnight" | "graphite" | "ocean" | "oled";
/** What the user picked - "system" follows the OS light/dark preference. */
export type ThemeChoice = ThemeId | "system";

export const THEMES: {
  id: ThemeId;
  label: string;
  dark: boolean;
  /** Swatch colors for the Settings preview cards. */
  preview: { bg: string; surface: string; accent: string };
}[] = [
  { id: "light", label: "Light", dark: false, preview: { bg: "#f8fafc", surface: "#ffffff", accent: "#15803d" } },
  { id: "slate", label: "Slate", dark: true, preview: { bg: "#0f172a", surface: "#1e293b", accent: "#22c55e" } },
  { id: "midnight", label: "Midnight", dark: true, preview: { bg: "#090e1a", surface: "#10172a", accent: "#818cf8" } },
  { id: "graphite", label: "Graphite", dark: true, preview: { bg: "#141416", surface: "#1d1d21", accent: "#f59e0b" } },
  { id: "ocean", label: "Ocean", dark: true, preview: { bg: "#071a1f", surface: "#0d262e", accent: "#22d3ee" } },
  { id: "oled", label: "OLED", dark: true, preview: { bg: "#000000", surface: "#0b0b0d", accent: "#22c55e" } },
];

const ID_KEY = "tcm-v2-theme-id";
const LEGACY_KEY = "tcm-v2-theme"; // pre-theme-system: "light" | "dark"
const DARK_PREF_KEY = "tcm-v2-theme-dark"; // last dark theme, for the sun/moon toggle

export function getThemeChoice(): ThemeChoice {
  const t = localStorage.getItem(ID_KEY);
  if (THEMES.some((x) => x.id === t)) return t as ThemeId;
  const legacy = localStorage.getItem(LEGACY_KEY);
  if (legacy === "light") return "light";
  if (legacy === "dark") return "slate";
  return "system";
}

/** The dark theme the light/dark toggle goes back to - the last dark one
 * the user picked, or Slate. Exported because the browser pages carry a
 * dark scheme too, and it should be the same dark the app would give. */
export function darkPref(): ThemeId {
  const t = localStorage.getItem(DARK_PREF_KEY);
  const theme = THEMES.find((x) => x.id === t);
  return theme?.dark ? theme.id : "slate";
}

export function resolveThemeId(choice: ThemeChoice = getThemeChoice()): ThemeId {
  if (choice !== "system") return choice;
  const light = window.matchMedia?.("(prefers-color-scheme: light)").matches;
  return light ? "light" : darkPref();
}

export function applyThemeChoice(choice: ThemeChoice) {
  const id = resolveThemeId(choice);
  const theme = THEMES.find((x) => x.id === id) ?? THEMES[1];
  const root = document.documentElement;
  root.classList.toggle("dark", theme.dark);
  // light/slate are the base palettes defined by :root / .dark directly.
  if (id === "light" || id === "slate") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", id);
}

export function setThemeChoice(choice: ThemeChoice) {
  if (choice === "system") {
    localStorage.removeItem(ID_KEY);
  } else {
    localStorage.setItem(ID_KEY, choice);
    if (THEMES.find((x) => x.id === choice)?.dark) localStorage.setItem(DARK_PREF_KEY, choice);
  }
  localStorage.removeItem(LEGACY_KEY);
  applyThemeChoice(choice);
}

/* Legacy light/dark helpers - the sun/moon toggle and the Toaster only
 * care about the resolved mode. "dark" restores the last dark theme. */
export type Theme = "light" | "dark" | "system";

export function getTheme(): "light" | "dark" {
  const theme = THEMES.find((x) => x.id === resolveThemeId());
  return theme?.dark ? "dark" : "light";
}

export function setTheme(mode: Theme) {
  if (mode === "system") setThemeChoice("system");
  else setThemeChoice(mode === "light" ? "light" : darkPref());
}

/** "default" = no override: the theme's own accent shows (green for
 * Light/Slate/OLED, indigo for Midnight, amber for Graphite, cyan for
 * Ocean). Every other value forces that accent family on any theme. */
export type Accent = "default" | "green" | "blue" | "violet" | "amber" | "rose";
export const ACCENTS: Accent[] = ["default", "green", "blue", "violet", "amber", "rose"];

const ACCENT_KEY = "tcm-v2-accent";

export function getAccent(): Accent {
  const a = localStorage.getItem(ACCENT_KEY);
  return a && ACCENTS.includes(a as Accent) ? (a as Accent) : "default";
}

export function setAccent(accent: Accent) {
  if (accent === "default") localStorage.removeItem(ACCENT_KEY);
  else localStorage.setItem(ACCENT_KEY, accent);
  applyAccent(accent);
}

function applyAccent(accent: Accent) {
  if (accent === "default") document.documentElement.removeAttribute("data-accent");
  else document.documentElement.setAttribute("data-accent", accent);
}

/** Call once on startup. */
export function initTheme() {
  applyThemeChoice(getThemeChoice());
  applyAccent(getAccent());
}
