// The custom title bar's height, in one place. The bar itself is drawn this
// tall (components/TitleBar.tsx), and while it is mounted it publishes the
// same value as `--titlebar-h` on <html> - which every full-window overlay
// starts below (`belowTitleBar`), so a dialog never covers the window's
// drag region or its minimise / maximise / close buttons.
//
// A window that renders no title bar (the runner window has its own header;
// a test renders none) never sets the property, and the `0px` fallback
// makes an overlay there cover the whole viewport, as before.

/** How tall the title bar is drawn. */
export const TITLE_BAR_HEIGHT = "2.25rem";

/** The custom property the mounted title bar sets on <html>. */
export const TITLE_BAR_VAR = "--titlebar-h";

/** Inline style for a `fixed inset-0` overlay: start below the title bar.
 * Inline rather than a class so the exit ghost (lib/exitGhost), which
 * clones the node, keeps it too. */
export const belowTitleBar = { top: `var(${TITLE_BAR_VAR}, 0px)` } as const;

/** The mounted title bar's height in pixels, for an overlay that cuts its
 * pieces in pixel coordinates (the tour's click-swallowing sheets). 0 when
 * there is no title bar - and under jsdom, which lays nothing out. */
export function titleBarPx(): number {
  const bar = document.querySelector<HTMLElement>("[data-title-bar]");
  return bar ? bar.getBoundingClientRect().height : 0;
}
