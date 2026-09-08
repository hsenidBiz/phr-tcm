// The palettes handed to pages that open in a real browser - the
// execution report and the test case view - so they arrive in the theme
// the user is already looking at, and can be flipped once they are open.
//
// Read from the LIVE computed CSS variables rather than from the THEMES
// table: the accent presets compose on top of a theme at runtime, and a
// hardcoded copy would drift the moment either changes. This way a new
// theme needs no change here, and none in Rust.

import type { PagePalette, ReportPalette } from "../bindings";
import { THEMES, darkPref } from "./theme";

/** Tokens are named `--color-x` in index.css; the page wants plain
 * values, resolved to whatever is actually in effect right now. */
function read(style: CSSStyleDeclaration, token: string): string {
  return style.getPropertyValue(`--color-${token}`).trim();
}

const EMPTY: ReportPalette = {
  bg: "",
  surface: "",
  surface_2: "",
  text: "",
  muted: "",
  faint: "",
  border: "",
  accent: "",
  success: "",
  danger: "",
  warning: "",
  dark: false,
};

function readRoot(dark: boolean): ReportPalette {
  const style = getComputedStyle(document.documentElement);
  return {
    bg: read(style, "bg"),
    surface: read(style, "surface"),
    surface_2: read(style, "surface-2"),
    text: read(style, "text"),
    muted: read(style, "muted"),
    faint: read(style, "faint"),
    border: read(style, "border"),
    accent: read(style, "accent"),
    success: read(style, "success"),
    danger: read(style, "danger"),
    warning: read(style, "warning"),
    dark,
  };
}

/**
 * The current palette. Any token that can't be read comes back empty and
 * Rust substitutes its own default, so a partial read degrades to the
 * page's original styling instead of an unreadable page.
 */
export function reportPalette(): ReportPalette {
  if (typeof document === "undefined") return EMPTY;
  // The app marks dark themes with this class; it drives `color-scheme`
  // in the page so the browser's own scrollbars and form controls follow
  // rather than staying stubbornly light.
  return readRoot(document.documentElement.classList.contains("dark"));
}

/**
 * Both schemes, for a page that carries its own light/dark switch.
 *
 * The scheme the user is NOT in has to be read the same way as the one
 * they are - the theme variants are `:root[data-theme=...]` rules, which
 * match nothing on a detached element - so the root is flipped, read, and
 * flipped back. It all happens in one synchronous block: `getComputedStyle`
 * forces a style recalculation but the browser paints no frame until the
 * task yields, so nothing flickers.
 *
 * The accent does NOT flip with it. Each theme carries its own default
 * accent (green, indigo, amber, cyan), so reading the other scheme's
 * palette wholesale would change the app's colour halfway through the
 * page. The accent on screen right now is used for both.
 */
export function pagePalette(): PagePalette {
  if (typeof document === "undefined") {
    return { light: EMPTY, dark: { ...EMPTY, dark: true }, dark_first: false };
  }
  const root = document.documentElement;
  const nowDark = root.classList.contains("dark");
  const here = readRoot(nowDark);

  // The counterpart theme: the app's plain Light going one way, and the
  // same dark the app's own light/dark toggle would restore coming back.
  const otherId = nowDark ? "light" : darkPref();
  const other = THEMES.find((t) => t.id === otherId) ?? THEMES[0];

  const wasDark = root.classList.contains("dark");
  const wasTheme = root.getAttribute("data-theme");
  root.classList.toggle("dark", other.dark);
  if (other.id === "light" || other.id === "slate") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", other.id);
  const there = readRoot(other.dark);
  root.classList.toggle("dark", wasDark);
  if (wasTheme === null) root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", wasTheme);

  // Keep the accent the user chose on both sides.
  const withAccent = (p: ReportPalette): ReportPalette => ({ ...p, accent: here.accent || p.accent });

  return {
    light: withAccent(nowDark ? there : here),
    dark: withAccent(nowDark ? here : there),
    dark_first: nowDark,
  };
}
