// Mentions of you, turned into bell notifications. Two sources: work-item
// discussions (the recent_mentions command) and the threads of the PRs you
// are on, which the PR badge already reads - so the PR half makes no
// request of its own.
//
// The first check ever for an organisation must not flood the bell with a
// month of old mentions: anything created more than a day before that
// first check is recorded as seen and never shown.

import type { Mention, PrThread } from "../bindings";
import { announce, summarizeLines } from "./assignedAlerts";
import { markSeen, raise, type AppNotification } from "./notifications";

export type NewNotification = Omit<AppNotification, "at" | "read">;

/** A comment in a PR thread that mentions you. */
export type PrMention = {
  repo: string;
  prId: number;
  threadId: number;
  commentId: number;
  author: string;
  excerpt: string;
  createdDate: string;
};

/** A notification-to-be and when its comment was written. */
export type FoundMention = { notification: NewNotification; created: string };

export const EXCERPT_CHARS = 140;
export const FIRST_RUN_WINDOW_MS = 24 * 60 * 60_000;

const baselineKey = (org: string) => `tcm-v2-mentions-baseline:${org}`;
const enc = encodeURIComponent;

/** Whitespace collapsed, at most 140 characters, the last an ellipsis when cut. */
export function excerpt(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length <= EXCERPT_CHARS ? flat : `${flat.slice(0, EXCERPT_CHARS - 1).trimEnd()}…`;
}

export function workItemMentionId(m: { item_id: number; comment_id: number }): string {
  return `mention:wi:${m.item_id}:${m.comment_id}`;
}

export function prMentionId(m: { repo: string; prId: number; threadId: number; commentId: number }): string {
  return `mention:pr:${m.repo}:${m.prId}:${m.threadId}:${m.commentId}`;
}

export function workItemNotification(org: string, project: string, m: Mention): NewNotification {
  return {
    id: workItemMentionId(m),
    kind: "mention",
    title: `${m.author || "Someone"} mentioned you on ${m.item_type} #${m.item_id}`,
    body: m.excerpt,
    href: `https://dev.azure.com/${enc(org)}/${enc(project)}/_workitems/edit/${m.item_id}`,
    target: { kind: "work-item", id: m.item_id, project },
  };
}

export function prNotification(org: string, project: string, m: PrMention): NewNotification {
  return {
    id: prMentionId(m),
    kind: "mention",
    title: `${m.author || "Someone"} mentioned you on PR #${m.prId}`,
    body: m.excerpt,
    href: `https://dev.azure.com/${enc(org)}/${enc(project)}/_git/${enc(m.repo)}/pullrequest/${m.prId}`,
    target: { kind: "pr", repo: m.repo, id: m.prId, project },
  };
}

/** PR comments name people as `@<identity id>`. In an excerpt that reads
 * as "@you" for you and "@someone" for anyone else. */
function readable(content: string, me: string): string {
  return content.replace(/@<([^<>\s]+)>/g, (_, id: string) => (id.toLowerCase() === me ? "@you" : "@someone"));
}

/** The comments in a PR's threads that mention `me` (`@<id>`, any case)
 * and were not written by `me`. An empty id matches nothing. */
export function prMentions(pr: { repo: string; id: number }, threads: PrThread[], me: string): PrMention[] {
  const id = me.trim().toLowerCase();
  if (!id) return [];
  const token = `@<${id}>`;
  const out: PrMention[] = [];
  for (const t of threads) {
    for (const c of t.comments) {
      if ((c.author_id ?? "").toLowerCase() === id) continue;
      if (!c.content.toLowerCase().includes(token)) continue;
      out.push({
        repo: pr.repo,
        prId: pr.id,
        threadId: t.id,
        commentId: c.id,
        author: c.author,
        excerpt: excerpt(readable(c.content, id)),
        createdDate: c.published,
      });
    }
  }
  return out;
}

/** When this organisation was first checked, in ms. The first call sets it.
 * A stored value that will not parse as a positive number - corruption,
 * never written by this code - counts as no baseline yet, not as "show
 * everything": it is reset to now rather than trusted as epoch. */
function baseline(org: string, now: number): number {
  try {
    const raw = localStorage.getItem(baselineKey(org));
    const at = raw === null ? NaN : Number(raw);
    if (Number.isFinite(at) && at > 0) return at;
    localStorage.setItem(baselineKey(org), String(now));
  } catch {
    // session-only: every check then counts as the first
  }
  return now;
}

/** Raise what is new; return what was raised. A mention written more than
 * 24 hours before this organisation's first check, or with no readable
 * date, is recorded as seen and not shown. */
export function noteMentions(org: string, found: FoundMention[], now = Date.now()): AppNotification[] {
  if (!org) return [];
  const cutoff = baseline(org, now) - FIRST_RUN_WINDOW_MS;
  const old: string[] = [];
  const fresh: NewNotification[] = [];
  for (const { notification, created } of found) {
    const t = Date.parse(created);
    if (Number.isNaN(t) || t < cutoff) old.push(notification.id);
    else fresh.push(notification);
  }
  markSeen(org, old);
  return raise(org, fresh);
}

/** The moment: one announcement per check, however many mentions arrived,
 * through the same toast/OS-notification rule a new assignment uses. */
export function announceMentions(added: AppNotification[]): void {
  if (added.length === 0) return;
  if (added.length === 1) {
    announce(added[0].title, added[0].body);
    return;
  }
  announce(`${added.length} new mentions`, summarizeLines(added.map((n) => n.title)));
}
