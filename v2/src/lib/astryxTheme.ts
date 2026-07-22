import { defineTheme } from "@astryxdesign/core/theme";

/**
 * Astryx theme built at runtime from the app's LIVE CSS tokens, so Astryx
 * components render in the app's identity (any theme, any accent preset)
 * instead of Meta's neutral look. Values are read when an island mounts;
 * dialogs are transient, so a freshly opened one always reflects the
 * current theme. Fallbacks cover jsdom, where computed styles are empty.
 */
export function buildAstryxTheme() {
  const css =
    typeof window !== "undefined" ? getComputedStyle(document.documentElement) : null;
  const v = (name: string, fallback: string) => {
    const val = css?.getPropertyValue(name).trim();
    return val || fallback;
  };
  return defineTheme({
    name: "tcm",
    tokens: {
      "--color-accent": v("--color-accent", "#22c55e"),
      "--color-accent-muted": v("--color-accent-soft", "rgba(34, 197, 94, 0.15)"),
      "--color-on-accent": v("--color-on-accent", "#052e16"),
      "--color-background-body": v("--color-bg", "#0f172a"),
      "--color-background-surface": v("--color-surface", "#1e293b"),
      "--color-background-card": v("--color-surface", "#1e293b"),
      "--color-background-popover": v("--color-surface", "#1e293b"),
      "--color-background-muted": v("--color-surface-2", "#334155"),
      "--color-text-primary": v("--color-text", "#e2e8f0"),
      "--color-text-secondary": v("--color-muted", "#94a3b8"),
      "--color-text-disabled": v("--color-faint", "#64748b"),
      "--color-icon-primary": v("--color-text", "#e2e8f0"),
      "--color-icon-secondary": v("--color-muted", "#94a3b8"),
      "--color-border": v("--color-border", "#334155"),
      "--color-border-emphasized": v("--color-border-strong", "#475569"),
      "--color-error": v("--color-danger", "#ef4444"),
      "--color-success": v("--color-success", "#22c55e"),
      "--color-warning": v("--color-warning", "#f59e0b"),
      "--font-family-body": v("--font-sans", "system-ui, sans-serif"),
      "--font-family-heading": v("--font-sans", "system-ui, sans-serif"),
      "--font-family-code": v("--font-mono", "monospace"),
      // Match the app's rounded-md / rounded-lg feel.
      "--radius-container": "8px",
      "--radius-element": "6px",
    },
  });
}
