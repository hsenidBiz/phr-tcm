// How a newly-assigned work item - or, since Task 5, a mention - reaches
// the user.
//
// In the app: a toast, because they are looking at it. Not in the app: a
// Windows notification, because a toast behind another window is the same
// as no notification at all. The choice is made here, once, rather than
// in Rust or repeated per caller, so there is one definition of "is the
// user looking at this" and one definition of "how does this reach them".

import type { AssignedItem } from "../bindings";
import { toast } from "./toast";

/** The first three lines, then how many more - the shape any list
 * collapses to once there is too much to read at a glance. */
export function summarizeLines(lines: string[]): string {
  const shown = lines.slice(0, 3);
  const rest = lines.length - shown.length;
  return rest > 0 ? `${shown.join("\n")}\n…and ${rest} more` : shown.join("\n");
}

/** One line per item, or a count once there are too many to read. */
export function summarize(items: AssignedItem[]): { title: string; body: string } {
  if (items.length === 1) {
    const i = items[0];
    return {
      title: `${i.work_item_type} #${i.id} assigned to you`,
      body: i.title,
    };
  }
  return {
    title: `${items.length} work items assigned to you`,
    body: summarizeLines(items.map((i) => `#${i.id} ${i.title}`)),
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

/** The moment: a toast when the app is in view, an OS notification when
 * it is not (falling back to a toast if that is refused). Every alert
 * that reaches the user this way - a new assignment, a mention - goes
 * through here, so a change to the duration, the fallback or the in-view
 * rule lands in one place instead of drifting between callers. */
export function announce(title: string, body: string): void {
  if (appIsInView()) {
    toast.info(title, { description: body, duration: 10_000 });
    return;
  }
  // Out of view - go to the OS, and fall back to a toast they will find
  // on return if notifications are refused.
  osNotify(title, body)
    .then((sent) => {
      if (!sent) toast.info(title, { description: body, duration: 10_000 });
    })
    .catch(() => {});
}
