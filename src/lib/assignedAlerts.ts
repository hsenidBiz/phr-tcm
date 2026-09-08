// How a newly-assigned work item reaches the user.
//
// In the app: a toast, because they are looking at it. Not in the app: a
// Windows notification, because a toast behind another window is the same
// as no notification at all. The choice is made here rather than in Rust
// so there is one definition of "is the user looking at this".

import type { AssignedItem } from "../bindings";

/** One line per item, or a count once there are too many to read. */
export function summarize(items: AssignedItem[]): { title: string; body: string } {
  if (items.length === 1) {
    const i = items[0];
    return {
      title: `${i.work_item_type} #${i.id} assigned to you`,
      body: i.title,
    };
  }
  const shown = items.slice(0, 3).map((i) => `#${i.id} ${i.title}`);
  const rest = items.length - shown.length;
  return {
    title: `${items.length} work items assigned to you`,
    body: rest > 0 ? `${shown.join("\n")}\n…and ${rest} more` : shown.join("\n"),
  };
}

/**
 * Whether the user can see the app right now.
 *
 * `document.hasFocus()` is the honest test: it is false when the window is
 * minimized, behind another window, or on another virtual desktop - all
 * the cases where a toast would go unseen. `document.hidden` alone would
 * miss a visible-but-unfocused window.
 */
export function appIsInView(): boolean {
  if (typeof document === "undefined") return false;
  return document.hasFocus() && !document.hidden;
}

/** Fire an OS notification, asking for permission the first time. Resolves
 * false when permission is refused, so the caller can fall back. */
export async function osNotify(title: string, body: string): Promise<boolean> {
  try {
    const { isPermissionGranted, requestPermission, sendNotification } = await import(
      "@tauri-apps/plugin-notification"
    );
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (!granted) return false;
    sendNotification({ title, body });
    return true;
  } catch {
    return false; // plugin unavailable (browser dev / tests)
  }
}
