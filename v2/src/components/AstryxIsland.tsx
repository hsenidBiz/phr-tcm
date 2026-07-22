import { Theme } from "@astryxdesign/core/theme";
import { useMemo, type ReactNode } from "react";
import { buildAstryxTheme } from "../lib/astryxTheme";

/**
 * Wraps Astryx components in a Theme scope built from the app's live
 * tokens. Used as an ISLAND around each Astryx usage (dialogs, markdown,
 * lightbox...) rather than around the whole app - the rest of the UI stays
 * on the vendored components, untouched and unthemed by Astryx.
 */
export default function AstryxIsland({ children }: { children: ReactNode }) {
  const theme = useMemo(buildAstryxTheme, []);
  return <Theme theme={theme}>{children}</Theme>;
}
