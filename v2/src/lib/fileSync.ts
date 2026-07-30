// Live-sync for an imported JSON file: an assistant edits the file on
// disk, the app folds the edit into the queue and says what moved.
//
// The rule this module exists to enforce: the file only owns the cases it
// put there. A case typed in Manual Entry is never removed because the
// file doesn't mention it, and a case the file dropped is only removed if
// the file is where it came from. Everything is compared against the last
// snapshot parsed from that same file.

import type { TestCase } from "../bindings";
import type { StepDiff } from "./caseDiff";

/** Identity across re-parses. A kept work-item id is exact; without one
 * the title is the only stable handle we have, so renaming an id-less case
 * reads as a remove plus an add. That is honest - the app cannot tell a
 * rename from a swap - and the report shows both halves. */
export function caseKey(c: TestCase): string {
  return c.update_id != null ? `id:${c.update_id}` : `t:${c.title.trim().toLowerCase()}`;
}

/** Identity WITHIN one list, so repeats of the same title stay distinct.
 *
 * `caseKey` alone is not unique: two new cases in a file can legitimately
 * share a title, and every map and set built from it then held one entry for
 * both. The second case never reached the queue, and on a later edit a
 * single file case was written over two different queue rows - two distinct
 * test cases became the same object, so creating them wrote the same case to
 * Azure DevOps twice.
 *
 * The suffix counts occurrences in order, so the Nth "Login works" in the
 * file lines up with the Nth in the queue. That is a heuristic, not a fact -
 * the app cannot tell two same-titled cases apart - but it is stable, and it
 * never loses one. */
export function keysFor(list: TestCase[]): string[] {
  const seen = new Map<string, number>();
  return list.map((c) => {
    const base = caseKey(c);
    const n = (seen.get(base) ?? 0) + 1;
    seen.set(base, n);
    return n === 1 ? base : `${base}#${n}`;
  });
}

const stepsSig = (c: TestCase) =>
  c.steps.map((s) => `${s.action}\u0000${s.expected}`).join("");

/** One field that differs, with both sides - the report renders the actual
 * words that changed, not just the field's name. Knowing "Title changed"
 * still means opening the file to find out what it changed to. */
export type FieldChange = { name: string; old: string; new: string };

/** What differs between two versions of the same case, field by field.
 * Steps are reported separately by `changedSteps` - they are a list, and
 * "Steps (4)" was the least useful line in the whole report. */
export function changedFields(before: TestCase, after: TestCase): FieldChange[] {
  const out: FieldChange[] = [];
  const push = (name: string, o: string, n: string) => {
    if (o.trim() !== n.trim()) out.push({ name, old: o, new: n });
  };
  push("Title", before.title, after.title);
  push("Tags", before.tags, after.tags);
  push("Automation status", before.automation_status, after.automation_status);
  push("Module", before.module_value, after.module_value);
  push("Preconditions", before.preconditions, after.preconditions);
  // Both app-only notes round-trip through the JSON, so an assistant can
  // edit either even though neither reaches Azure DevOps. Reviewer notes
  // especially: an assistant filling them in IS the change worth seeing.
  push("Comment", before.comment ?? "", after.comment ?? "");
  push("Reviewer notes", before.reviewer_notes ?? "", after.reviewer_notes ?? "");
  if ((before.update_id ?? null) !== (after.update_id ?? null)) {
    out.push({
      name: "Work item id",
      old: before.update_id == null ? "" : String(before.update_id),
      new: after.update_id == null ? "" : String(after.update_id),
    });
  }
  return out;
}

/** Step-by-step difference, in the shape the review gate already renders
 * (`StepDiffLines`) - so a file edit and a pending update are read the
 * same way instead of in two invented formats. */
export function changedSteps(before: TestCase, after: TestCase): StepDiff[] {
  if (stepsSig(before) === stepsSig(after)) return [];
  const out: StepDiff[] = [];
  const max = Math.max(before.steps.length, after.steps.length);
  for (let i = 0; i < max; i++) {
    const o = before.steps[i];
    const n = after.steps[i];
    if (n && !o) out.push({ index: i, kind: "added", new: n });
    else if (!n && o) out.push({ index: i, kind: "removed", old: o });
    else if (o && n && (o.action !== n.action || o.expected !== n.expected)) {
      out.push({ index: i, kind: "changed", old: o, new: n });
    }
  }
  return out;
}

export type SyncChange = {
  kind: "added" | "changed" | "removed";
  key: string;
  title: string;
  /** Populated for "changed" only. */
  fields: FieldChange[];
  /** Populated for "changed" only. */
  steps: StepDiff[];
};

export type SyncResult = {
  queue: TestCase[];
  changes: SyncChange[];
  /** The snapshot to compare the NEXT edit against. */
  snapshot: TestCase[];
};

/**
 * Fold a fresh parse of the watched file into the queue.
 *
 * @param queue current queue (may contain cases the file knows nothing about)
 * @param prev  the snapshot parsed from this file last time
 * @param next  the fresh parse
 *
 * "changed" is measured queue-vs-file, not prev-vs-next, so the report
 * names what the user will actually see move - including a field they
 * edited in-app that the file has now overwritten.
 */
export function syncFromFile(queue: TestCase[], prev: TestCase[], next: TestCase[]): SyncResult {
  // Occurrence-aware throughout: two cases sharing a title are two cases.
  const nextKeys = keysFor(next);
  const nextBy = new Map(next.map((c, i) => [nextKeys[i], c]));
  const changes: SyncChange[] = [];

  // Drop only what this file previously contributed and has now dropped.
  const prevKeys = keysFor(prev);
  const gone = new Set(prevKeys.filter((k) => !nextBy.has(k)));

  const queueKeys = keysFor(queue);
  const keptKeys: string[] = [];
  const kept = queue.filter((c, i) => {
    const k = queueKeys[i];
    if (!gone.has(k)) {
      keptKeys.push(k);
      return true;
    }
    changes.push({ kind: "removed", key: k, title: c.title, fields: [], steps: [] });
    return false;
  });

  // Replace in place: the queue keeps its order so the list doesn't
  // reshuffle under someone mid-review. Each file case is consumed at most
  // once, so one entry can never be written over two different queue rows.
  const unclaimed = new Map(nextBy);
  const present = new Set<string>();
  const synced = kept.map((c, i) => {
    const k = keptKeys[i];
    present.add(k);
    const fresh = unclaimed.get(k);
    if (!fresh) return c;
    unclaimed.delete(k);
    const fields = changedFields(c, fresh);
    const stepDiffs = changedSteps(c, fresh);
    // BOTH, or a steps-only edit is thrown away. This guard used to read
    // `fields.length === 0` alone, which was correct only while
    // changedFields folded a "Steps (1 -> 2)" entry into the field list;
    // the moment steps moved out into their own diff, an edit that
    // touched nothing but the steps stopped being applied at all.
    if (fields.length === 0 && stepDiffs.length === 0) return c;
    changes.push({ kind: "changed", key: k, title: fresh.title, fields, steps: stepDiffs });
    return fresh;
  });

  // Genuinely new cases land at the end, in file order.
  next.forEach((c, i) => {
    const k = nextKeys[i];
    if (present.has(k)) return;
    present.add(k);
    synced.push(c);
    changes.push({ kind: "added", key: k, title: c.title, fields: [], steps: [] });
  });

  return { queue: synced, changes, snapshot: next };
}

export function countBy(changes: SyncChange[], kind: SyncChange["kind"]): number {
  return changes.filter((c) => c.kind === kind).length;
}

/** What the app remembers about the file it is watching, per PBI. */
export type WatchedFile = {
  path: string;
  /** Fingerprint of the contents last folded in. */
  stamp: string;
  /** The cases that fingerprint parsed to. */
  snapshot: TestCase[];
  /** The file's comment about this set as a whole. Lives in the file's
   * top-level `comments`, so it travels with the file; kept here so the
   * panel can show it without re-reading on every render. Absent in
   * entries written before general comments existed. */
  comment?: string;
};

/** Replace one file's remembered fields, leaving the rest of the list (and
 * the other files) untouched. */
export function patchWatch(
  list: WatchedFile[],
  path: string,
  fields: Partial<WatchedFile>,
): WatchedFile[] {
  return list.map((w) => (w.path === path ? { ...w, ...fields } : w));
}

const watchKey = (org: string, pbiId: number) => `tcm-v2-watch:${org}/${pbiId}`;

/** Every file being followed for this PBI. A queue is often assembled from
 * several JSON files - one per feature - and each is tracked separately so
 * one can be dropped without disturbing the rest. */
/** An entry this code can actually use, rather than one it was told to
 * assume. The cast that used to stand here trusted whatever was in
 * storage, and this shape has already changed once (see `comment`) - a
 * half-written or older entry got as far as `snapshot.map(...)` and threw
 * mid-render, taking the Import screen with it. */
function isWatchedFile(v: unknown): v is WatchedFile {
  if (!v || typeof v !== "object") return false;
  const w = v as Partial<WatchedFile>;
  return (
    typeof w.path === "string" &&
    w.path.length > 0 &&
    typeof w.stamp === "string" &&
    Array.isArray(w.snapshot) &&
    (w.comment === undefined || typeof w.comment === "string")
  );
}

export function loadWatches(org: string, pbiId: number): WatchedFile[] {
  try {
    const raw = localStorage.getItem(watchKey(org, pbiId));
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    // Tolerate the single-object shape written before multi-file support.
    const list = Array.isArray(parsed) ? parsed : [parsed];
    // Dropping a bad entry costs the user one "watch this file again";
    // keeping it costs them the screen.
    return list.filter(isWatchedFile);
  } catch {
    return [];
  }
}

export function saveWatches(org: string, pbiId: number, list: WatchedFile[]): void {
  try {
    if (list.length > 0) localStorage.setItem(watchKey(org, pbiId), JSON.stringify(list));
    else localStorage.removeItem(watchKey(org, pbiId));
  } catch {
    // storage unavailable -> watching is session-only
  }
}

/** Add or replace one file's entry, preserving the order files were added
 * so the list doesn't reshuffle when a file changes. */
export function upsertWatch(list: WatchedFile[], w: WatchedFile): WatchedFile[] {
  const i = list.findIndex((x) => x.path === w.path);
  if (i === -1) return [...list, w];
  const next = [...list];
  next[i] = w;
  return next;
}

/** The queue with everything a given file contributed taken out. Used when
 * a watch is dropped and the user says yes to removing its cases. Cases
 * that came from elsewhere - typed by hand, or owned by another watched
 * file - are left alone. */
export function withoutFileCases(
  queue: TestCase[],
  snapshot: TestCase[],
  otherSnapshots: TestCase[][],
): TestCase[] {
  const owned = new Set(keysFor(snapshot));
  for (const other of otherSnapshots) {
    for (const k of keysFor(other)) owned.delete(k);
  }
  const keys = keysFor(queue);
  return queue.filter((_, i) => !owned.has(keys[i]));
}

/** The file each queued case came from, aligned with `queue`; empty for a
 * case that was typed by hand and belongs to no file.
 *
 * A comment is written back into the file that put the case there, so this
 * is what decides where it lands. If two files both claim a case - same
 * title in each, which the app cannot tell apart - the one imported first
 * keeps it, matching the order `withoutFileCases` uses to decide ownership
 * when a watch is dropped. */
export function ownerPaths(queue: TestCase[], watches: WatchedFile[]): string[] {
  const owner = new Map<string, string>();
  for (const w of watches) {
    for (const k of keysFor(w.snapshot)) {
      if (!owner.has(k)) owner.set(k, w.path);
    }
  }
  return keysFor(queue).map((k) => owner.get(k) ?? "");
}

/** Trailing path segment, for the "watching X" line. */
export function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** What to say when a watched file lands while the app is behind another
 * window. The counts, not the titles: this is a notification, and the
 * change report inside the app carries the detail. */
export function syncNotification(
  file: string,
  changes: SyncChange[],
): { title: string; body: string } {
  const parts: string[] = [];
  const added = countBy(changes, "added");
  const changed = countBy(changes, "changed");
  const removed = countBy(changes, "removed");
  if (added) parts.push(`${added} added`);
  if (changed) parts.push(`${changed} changed`);
  if (removed) parts.push(`${removed} removed`);
  return {
    title: `${file} was updated`,
    body: parts.length
      ? `${parts.join(", ")} — the queue is up to date.`
      : "The queue is up to date.",
  };
}
