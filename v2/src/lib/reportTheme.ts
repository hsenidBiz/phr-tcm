// The palette handed to reports that open in a real browser, so they
// arrive in the theme the user is already looking at.
//
// Read from the LIVE computed CSS variables rather than from the THEMES
// table: the accent presets compose on top of a theme at runtime, and a
// hardcoded copy would drift the moment either changes. This way a new
// theme needs no change here, and none in Rust.

import type { ReportPalette } from "../bindings";

/** Tokens are named `--color-x` in index.css; the report wants plain
 * values, resolved to whatever is actually in effect right now. */
function read(style: CSSStyleDeclaration, token: string): string {
  return style.getPropertyValue(`--color-${token}`).trim();
}

/**
 * The current palette. Any token that can't be read comes back empty and
 * Rust substitutes its light default, so a partial read degrades to the
 * report's original styling instead of an unreadable page.
 */
export function reportPalette(): ReportPalette {
  const empty: ReportPalette = {
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
  if (typeof document === "undefined") return empty;

  const root = document.documentElement;
  const style = getComputedStyle(root);
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
    // The app marks dark themes with this class; it drives `color-scheme`
    // in the report so the browser's own scrollbars and form controls
    // follow rather than staying stubbornly light.
    dark: root.classList.contains("dark"),
  };
}
