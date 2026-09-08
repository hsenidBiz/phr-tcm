// Shaping a work item's revision list for reading.
//
// Azure DevOps renders history as a horizontal strip of every single state
// hop, which for a real item means scrolling sideways past twenty
// near-identical arrows to learn nothing. The question people actually
// have is "where has this been stuck, and how many times has it bounced?"
// - so `stateJourney` answers that instead, and the timeline below it
// carries the detail.

import type { WorkRevision } from "../bindings";

export type StateVisit = {
  state: string;
  /** How many separate times the item entered this state. */
  visits: number;
  /** Total milliseconds spent in it across those visits. */
  totalMs: number;
  /** True for the state the item is in now. */
  current: boolean;
};

const ms = (iso: string) => {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? null : t;
};

/**
 * Time spent in, and number of entries into, each state.
 *
 * @param revisions newest first (what the command returns)
 * @param now       injected so tests aren't clock-dependent
 *
 * States are ordered by first entry, which reads as the item's path.
 * A revision with no usable timestamp still counts as a visit; it just
 * cannot contribute duration.
 */
export function stateJourney(revisions: WorkRevision[], now = Date.now()): StateVisit[] {
  const oldestFirst = [...revisions].reverse().filter((r) => r.state_to);
  if (oldestFirst.length === 0) return [];

  const order: string[] = [];
  const visits = new Map<string, number>();
  const total = new Map<string, number>();

  oldestFirst.forEach((r, i) => {
    const state = r.state_to;
    if (!order.includes(state)) order.push(state);
    visits.set(state, (visits.get(state) ?? 0) + 1);

    const from = ms(r.at);
    const next = oldestFirst[i + 1];
    const to = next ? ms(next.at) : now;
    if (from !== null && to !== null && to > from) {
      total.set(state, (total.get(state) ?? 0) + (to - from));
    }
  });

  const currentState = oldestFirst[oldestFirst.length - 1].state_to;
  return order.map((state) => ({
    state,
    visits: visits.get(state) ?? 0,
    totalMs: total.get(state) ?? 0,
    current: state === currentState,
  }));
}

/** "2d 4h", "3h", "12m", "" for nothing worth showing. */
export function humanDuration(totalMs: number): string {
  if (!Number.isFinite(totalMs) || totalMs < 60_000) return "";
  const mins = Math.floor(totalMs / 60_000);
  const hours = Math.floor(mins / 60);
  const days = Math.floor(hours / 24);
  if (days > 0) return hours % 24 > 0 ? `${days}d ${hours % 24}h` : `${days}d`;
  if (hours > 0) return mins % 60 > 0 ? `${hours}h ${mins % 60}m` : `${hours}h`;
  return `${mins}m`;
}

/** "just now", "20 minutes ago", "3 days ago". */
export function relativeTime(iso: string, now = Date.now()): string {
  const t = ms(iso);
  if (t === null) return "";
  const diff = now - t;
  if (diff < 60_000) return "just now";
  const mins = Math.floor(diff / 60_000);
  if (mins < 60) return `${mins} minute${mins === 1 ? "" : "s"} ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days} day${days === 1 ? "" : "s"} ago`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months} month${months === 1 ? "" : "s"} ago`;
  return `${Math.floor(months / 12)} year${months < 24 ? "" : "s"} ago`;
}

/** Day bucket label: Today / Yesterday / "Fri 24 Jul 2026". */
export function dayLabel(iso: string, now = Date.now()): string {
  const t = ms(iso);
  if (t === null) return "Unknown date";
  const d = new Date(t);
  const today = new Date(now);
  const sameDay = (a: Date, b: Date) =>
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate();
  if (sameDay(d, today)) return "Today";
  const yesterday = new Date(now - 86_400_000);
  if (sameDay(d, yesterday)) return "Yesterday";
  return d.toLocaleDateString(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

/** Consecutive revisions sharing a day, in the order given. */
export function groupByDay(
  revisions: WorkRevision[],
  now = Date.now(),
): { label: string; items: WorkRevision[] }[] {
  const out: { label: string; items: WorkRevision[] }[] = [];
  for (const r of revisions) {
    const label = dayLabel(r.at, now);
    const last = out[out.length - 1];
    if (last && last.label === label) last.items.push(r);
    else out.push({ label, items: [r] });
  }
  return out;
}

/** ADO stores dates as ISO strings, and a raw "2026-07-24T09:28:31Z" in a
 * diff is unreadable. Anything that looks like one is shown the way the
 * rest of the app shows dates; everything else passes through untouched. */
export function displayValue(v: string): string {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(v)) return v;
  const t = Date.parse(v);
  if (Number.isNaN(t)) return v;
  const d = new Date(t);
  // Date-only ADO fields (Start Date, Finish Date) are stored as UTC
  // midnight. Rendering those in local time both invents a clock time and,
  // west of UTC, shows the wrong day - so they print as a bare UTC date.
  const dateOnly =
    d.getUTCHours() === 0 && d.getUTCMinutes() === 0 && d.getUTCSeconds() === 0;
  if (dateOnly) {
    return d.toLocaleDateString(undefined, {
      day: "numeric",
      month: "short",
      year: "numeric",
      timeZone: "UTC",
    });
  }
  return d.toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export type HistoryFilter = "all" | "state" | "fields" | "links";

export function matchesFilter(r: WorkRevision, filter: HistoryFilter): boolean {
  switch (filter) {
    case "state":
      return Boolean(r.state_to);
    case "fields":
      // A state change is also a field change; "fields" means the edits
      // that a state filter would otherwise hide.
      return r.fields.some((f) => f.reference_name !== "System.State");
    case "links":
      return r.links_added.length > 0 || r.links_removed.length > 0;
    default:
      return true;
  }
}

/** One line saying what this revision did, for the collapsed row. */
export function summarize(r: WorkRevision): string {
  const parts: string[] = [];
  if (r.state_to) {
    parts.push(r.state_from ? `${r.state_from} → ${r.state_to}` : `set to ${r.state_to}`);
  }
  const others = r.fields.filter((f) => f.reference_name !== "System.State");
  if (others.length > 0) {
    parts.push(
      others.length === 1 ? `edited ${others[0].label}` : `edited ${others.length} fields`,
    );
  }
  for (const l of r.links_added) parts.push(`added ${l}`);
  for (const l of r.links_removed) parts.push(`removed ${l}`);
  if (r.comment_added) parts.push("commented");
  return parts.join(" · ") || "made changes";
}
