// Whether the sidebar is open or collapsed to its icon rail, published so
// the sticky bottom-left buttons can sit BESIDE it instead of on top of
// its Close button. localStorage holds the value between sessions; the
// listener set exists because a storage write does not notify the same
// window - only the Sidebar ever toggles, everyone else subscribes.

const KEY = "tcm-v2-sidebar";

/** Tailwind w-52 / w-14, as pixels, for the sticky buttons' left offset. */
export const SIDEBAR_OPEN_PX = 208;
export const SIDEBAR_RAIL_PX = 56;

const listeners = new Set<() => void>();

export function sidebarCollapsedSnapshot(): boolean {
  try {
    return localStorage.getItem(KEY) === "collapsed";
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
