// How a newly-assigned work item, or a mention, reaches the user.
//
// In the app: a toast, because they are looking at it. Not in the app: a
// Windows notification, because a toast behind another window is the same
// as no notification at all. The choice is made here, once, rather than
// in Rust or repeated per caller, so there is one definition of "is the
// user looking at this" and one definition of "how does this reach them".

import { createElement } from "react";
import type { AssignedItem } from "../bindings";
import { AlertListView } from "../components/AlertListView";
import { toast } from "./toast";

/** One entry of an alert list: the work item's id and type when known, and
 * the words (its title, or a mention's headline). */
export interface AlertItem {
  id?: number;
  type?: string;
  text: string;
}

/** What several alerts collapse to: the first few, then how many more. */
export interface AlertList {
  items: AlertItem[];
  more: number;
}

/** How many entries an alert shows before it says "and N more". */
const SHOWN = 3;
/** The longest an OS notification line gets, ellipsis included. */
const OS_LINE_MAX = 70;

/** The first three entries, then how many more - the shape any list
 * collapses to once there is too much to read at a glance. */
export function summarizeLines(items: AlertItem[]): AlertList {
  return { items: items.slice(0, SHOWN), more: Math.max(0, items.length - SHOWN) };
}

/** One entry per item, or a count once there are too many to read. */
export function summarize(items: AssignedItem[]): { title: string; body: AlertList } {
  if (items.length === 1) {
    const i = items[0];
    return {
      title: `${i.work_item_type} #${i.id} assigned to you`,
      body: { items: [{ text: i.title }], more: 0 },
    };
  }
  return {
    title: `${items.length} work items assigned to you`,
    body: summarizeLines(items.map((i) => ({ id: i.id, type: i.work_item_type, text: i.title }))),
  };
}

function clip(line: string): string {
  return line.length <= OS_LINE_MAX ? line : `${line.slice(0, OS_LINE_MAX - 1).trimEnd()}…`;
}

/** The plain-text form an OS notification can carry: one line per item,
 * each cut to a readable length when there are several (a lone item keeps
 * its whole text), then "…and N more". */
export function alertListText(list: AlertList): string {
  const several = list.items.length > 1;
  const lines = list.items.map((it) => {
    const line = it.id !== undefined ? `#${it.id} ${it.text}` : it.text;
    return several ? clip(line) : line;
  });
  if (list.more > 0) lines.push(`…and ${list.more} more`);
  return lines.join("\n");
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
export function announce(title: string, body: string | AlertList): void {
  const inApp = () =>
    toast.info(title, {
      description: typeof body === "string" ? body : createElement(AlertListView, { list: body }),
      duration: 10_000,
    });
  if (appIsInView()) {
    inApp();
    return;
  }
  // Out of view - go to the OS (plain text only), and fall back to a toast
  // they will find on return if notifications are refused.
  osNotify(title, typeof body === "string" ? body : alertListText(body))
    .then((sent) => {
      if (!sent) inApp();
    })
    .catch(() => {});
}
