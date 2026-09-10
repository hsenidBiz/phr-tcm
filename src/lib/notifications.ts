// The notification bell's store: what happened while you were not
// looking, kept until you dismiss it.
//
// Toasts and OS notifications are moments - this is the record. One list
// per organisation in localStorage, newest first, published through
// useSyncExternalStore so the bell's badge and panel repaint the instant
// something is raised from anywhere in the app.
//
// Dismissed is not forgotten: every id ever raised goes into a second,
// bounded set, so a PR whose conflict you dismissed does not come back on
// the next five-minute poll still carrying the same conflict. A source is
// therefore free to re-report the whole current state every time; only
// what is genuinely new reaches the list.

import { useSyncExternalStore } from "react";
import type { AssignedItem, PullRequest } from "../bindings";

export type NotificationKind = "assigned" | "pr-conflict" | "pr-review";

export type AppNotification = {
  /** Stable per event, e.g. `pr-conflict:Web:412` - the dedupe key. */
  id: string;
  kind: NotificationKind;
  title: string;
  body: string;
  /** ISO time the app first saw it. */
  at: string;
  read: boolean;
  /** Where "Open" goes, when there is somewhere to go. */
  href?: string;
};

/** Newest `LIST_CAP` kept in the list; `KNOWN_CAP` ids remembered as seen. */
export const LIST_CAP = 50;
const KNOWN_CAP = 500;

const listKey = (org: string) => `tcm-v2-notifications:${org}`;
const knownKey = (org: string) => `tcm-v2-notifications-known:${org}`;

const lists = new Map<string, AppNotification[]>();
const listeners = new Set<() => void>();
const EMPTY: AppNotification[] = [];

function load(org: string): AppNotification[] {
  const cached = lists.get(org);
  if (cached) return cached;
  let parsed: AppNotification[] = EMPTY;
  try {
    const raw = localStorage.getItem(listKey(org));
    if (raw) parsed = JSON.parse(raw) as AppNotification[];
  } catch {
    parsed = EMPTY;
  }
  lists.set(org, parsed);
  return parsed;
}

function save(org: string, next: AppNotification[]): void {
  lists.set(org, next);
  try {
    localStorage.setItem(listKey(org), JSON.stringify(next));
  } catch {
    // session-only
  }
  for (const l of listeners) l();
}

function known(org: string): string[] {
  try {
    const raw = localStorage.getItem(knownKey(org));
    return raw ? (JSON.parse(raw) as string[]) : [];
  } catch {
    return [];
  }
}

function remember(org: string, ids: string[]): void {
  const next = [...ids, ...known(org)].slice(0, KNOWN_CAP);
  try {
    localStorage.setItem(knownKey(org), JSON.stringify(next));
  } catch {
    // session-only
  }
}

/** The organisation's list, newest first; repaints on every change. */
export function useNotifications(org: string): AppNotification[] {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => (org ? load(org) : EMPTY),
    () => EMPTY,
  );
}

export function unreadCount(list: AppNotification[]): number {
  return list.filter((n) => !n.read).length;
}

/** Add what is new. Ids already raised - listed or since dismissed - are
 * skipped, so callers can report the whole current state every time. */
export function raise(
  org: string,
  items: Array<Omit<AppNotification, "at" | "read">>,
): AppNotification[] {
  if (!org || items.length === 0) return [];
  const seen = new Set([...known(org), ...load(org).map((n) => n.id)]);
  const fresh = items.filter((i) => !seen.has(i.id));
  if (fresh.length === 0) return [];
  const at = new Date().toISOString();
  const added = fresh.map((i) => ({ ...i, at, read: false }));
  remember(
    org,
    added.map((n) => n.id),
  );
  save(org, [...added, ...load(org)].slice(0, LIST_CAP));
  return added;
}

/** Opening the bell: the badge goes, the items stay. */
export function markAllRead(org: string): void {
  const cur = load(org);
  if (cur.every((n) => n.read)) return;
  save(
    org,
    cur.map((n) => (n.read ? n : { ...n, read: true })),
  );
}

export function dismiss(org: string, id: string): void {
  save(
    org,
    load(org).filter((n) => n.id !== id),
  );
}

export function clearAll(org: string): void {
  save(org, []);
}

// --- sources ---------------------------------------------------------------

/** Work items the background poll just found assigned to you. */
export function noteAssigned(org: string, project: string, items: AssignedItem[]): void {
  raise(
    org,
    items.map((i) => ({
      id: `assigned:${i.id}`,
      kind: "assigned" as const,
      title: `${i.work_item_type} #${i.id} assigned to you`,
      body: i.title,
      href: `https://dev.azure.com/${encodeURIComponent(org)}/${encodeURIComponent(project)}/_workitems/edit/${i.id}`,
    })),
  );
}

/** The PR overview as it stands: your PRs that have grown conflicts, and
 * PRs newly waiting on your review. Called on every refresh - the dedupe
 * above is what turns a state into an event. */
export function notePrOverview(
  org: string,
  project: string,
  overview: { mine: PullRequest[]; awaiting: PullRequest[] },
): void {
  const prUrl = (pr: PullRequest) =>
    `https://dev.azure.com/${encodeURIComponent(org)}/${encodeURIComponent(project)}/_git/${encodeURIComponent(pr.repo)}/pullrequest/${pr.id}`;
  const items: Array<Omit<AppNotification, "at" | "read">> = [];
  for (const pr of overview.mine) {
    if (pr.has_conflicts && pr.status === "active") {
      items.push({
        id: `pr-conflict:${pr.repo}:${pr.id}`,
        kind: "pr-conflict",
        title: `PR #${pr.id} has merge conflicts`,
        body: `${pr.title} (${pr.repo})`,
        href: prUrl(pr),
      });
    }
  }
  for (const pr of overview.awaiting) {
    if (pr.status === "active") {
      items.push({
        id: `pr-review:${pr.repo}:${pr.id}`,
        kind: "pr-review",
        title: `PR #${pr.id} is waiting for your review`,
        body: `${pr.title} (${pr.repo}) - by ${pr.author}`,
        href: prUrl(pr),
      });
    }
  }
  raise(org, items);
}

/** Test seam: forget the in-memory copies (storage is cleared by the test). */
export function resetForTests(): void {
  lists.clear();
}
