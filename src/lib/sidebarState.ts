// Whether the sidebar is open or collapsed to its icon rail, published so
// the sticky bottom-left buttons can sit BESIDE it instead of on top of
// its Close button. localStorage holds the value between sessions; the
// listener set exists because a storage write does not notify the same
// window - only the Sidebar ever toggles, everyone else subscribes.

/** The Sidebar's own storage key - shared here so this module and the
 * component read/write the exact same key. */
export const COLLAPSE_KEY = "tcm-v2-sidebar";

/** Tailwind w-52 / w-14, as pixels, for the sticky buttons' left offset. */
export const SIDEBAR_OPEN_PX = 208;
export const SIDEBAR_RAIL_PX = 56;

const listeners = new Set<() => void>();

// The tour keeps the sidebar expanded for its duration, whatever the user
// last chose - in memory only, same shape as `workingDir.ts`'s tour
// override. Consulted by the snapshot; never written to storage, so the
// user's own setting comes straight back once the override clears.
let tourExpanded = false;

export function setTourExpanded(on: boolean): void {
  if (tourExpanded === on) return;
  tourExpanded = on;
  publishSidebarChange();
}

export function clearTourExpanded(): void {
  setTourExpanded(false);
}

export function sidebarCollapsedSnapshot(): boolean {
  if (tourExpanded) return false;
  try {
    return localStorage.getItem(COLLAPSE_KEY) === "collapsed";
  } catch {
    return false;
  }
}

export function subscribeSidebar(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Called by the Sidebar after it writes the new state. */
export function publishSidebarChange(): void {
  for (const l of listeners) l();
}

/** Left offset that clears the sidebar at its current width. */
export function stickyLeftPx(collapsed: boolean): number {
  return (collapsed ? SIDEBAR_RAIL_PX : SIDEBAR_OPEN_PX) + 24;
}
