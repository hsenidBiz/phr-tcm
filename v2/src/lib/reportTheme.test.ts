import { afterEach, expect, test } from "vitest";
import { reportPalette } from "./reportTheme";

afterEach(() => {
  document.documentElement.removeAttribute("style");
  document.documentElement.classList.remove("dark");
});

function setTokens(tokens: Record<string, string>) {
  const root = document.documentElement;
  for (const [k, v] of Object.entries(tokens)) root.style.setProperty(`--color-${k}`, v);
}

test("the palette is read from the live CSS variables", () => {
  setTokens({
    bg: "#000000",
    surface: "#0b0b0d",
    "surface-2": "#141418",
    text: "#e5e7eb",
    muted: "#9ca3af",
    faint: "#6b7280",
    border: "#27272a",
    accent: "#22c55e",
    success: "#22c55e",
    danger: "#ef4444",
    warning: "#f59e0b",
  });
  document.documentElement.classList.add("dark");

  const p = reportPalette();
  expect(p.bg).toBe("#000000");
  expect(p.surface_2).toBe("#141418"); // the hyphenated token maps to snake_case
  expect(p.danger).toBe("#ef4444");
  expect(p.dark).toBe(true);
});

test("a light theme reports dark: false", () => {
  setTokens({ bg: "#f8fafc" });
  const p = reportPalette();
  expect(p.dark).toBe(false);
  expect(p.bg).toBe("#f8fafc");
});

/** An unreadable token comes back empty on purpose - Rust substitutes its
 * light default, which is safer than guessing here. */
test("tokens that cannot be read come back empty, not invented", () => {
  const p = reportPalette();
  expect(p.accent).toBe("");
  expect(p.warning).toBe("");
});

test("whatever the theme is, every field the report needs is present", () => {
  setTokens({ bg: "#111111" });
  const p = reportPalette();
  for (const key of [
    "bg",
    "surface",
    "surface_2",
    "text",
    "muted",
    "faint",
    "border",
    "accent",
    "success",
    "danger",
    "warning",
  ] as const) {
    expect(p[key], `missing ${key}`).toBeTypeOf("string");
  }
  expect(p.dark).toBeTypeOf("boolean");
});
