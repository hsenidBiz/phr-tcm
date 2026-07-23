import { Theme } from "@astryxdesign/core/theme";
import { useLayoutEffect, useMemo, type ReactNode } from "react";
import { buildAstryxTheme } from "../lib/astryxTheme";
import { applyThemeChoice, getThemeChoice } from "../lib/theme";

/**
 * Wraps Astryx components in a Theme scope built from the app's live
 * tokens. Used as an ISLAND around each Astryx usage (markdown, lightbox,
 * empty states...) rather than around the whole app - the rest of the UI
 * stays on the vendored components, untouched and unthemed by Astryx.
 */
export default function AstryxIsland({ children }: { children: ReactNode }) {
  const theme = useMemo(buildAstryxTheme, []);

  // Astryx's Theme, as a ROOT provider (every island is one - the nesting
  // context is module-private), "syncs" onto <html>: it SETS data-theme to
  // light/dark at mount and REMOVES it at unmount. That's OUR theme
  // attribute - both paths knocked OLED/Midnight/... back to Slate the
  // moment a screen with an island mounted. Re-assert our theme after
  // both: parent layout effects run after the child's on mount, and the
  // microtask runs after their cleanup on unmount.
  useLayoutEffect(() => {
    applyThemeChoice(getThemeChoice());
    return () => {
      queueMicrotask(() => applyThemeChoice(getThemeChoice()));
    };
  }, []);

  return <Theme theme={theme}>{children}</Theme>;
}
