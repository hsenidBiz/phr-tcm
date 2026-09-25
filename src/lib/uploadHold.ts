/**
 * Creates whose upload outcome is unknown, held until someone checks.
 *
 * When a `$batch` call fails, Rust asks Azure DevOps what it created. If
 * that lookup fails too (or the batch timed out and may still be running),
 * the creates come back as "unknown": each may exist, and uploading it again
 * could make a duplicate this app cannot delete. So they are HELD. Their rows
 * are marked, Upload refuses, and the only way out is a Check
 * (`reconcile_upload`) that gets an answer.
 *
 * Persisted per PBI in localStorage, like the draft queue itself: a restart
 * must not quietly lift a hold. Rows are named by title, one entry per row,
 * because titles are what the lookup matches on and they survive the
 * reorders and re-syncs that move a row's key.
 *
 * Deviation from the brief (controller ruling, 2026-09-18): `reconcile_upload`
 * cannot tell "not found" from "ambiguous" - a title with more unclaimed
 * matches in Azure DevOps than rows being checked is reported separately
 * (`ReconcileAnswer.ambiguous`), because guessing which found row is really
 * this upload's risks clearing a hold on a case that may still be a
 * duplicate. `UploadHold.ambiguous` names which of the still-held titles are
 * stuck that way, so the row can say why (`ambiguousRows`) instead of just
 * "unknown".
 *
 * A hold is cleared in exactly two ways: a Check (`reconcile_upload`) that
 * comes back with an answer for a title, or the user confirming Release on
 * an ambiguous hold once they have checked Azure DevOps themselves
 * (QueueSection's hold banner). Nothing else ever clears one - in
 * particular, no effect watching the live queue clears a hold whose rows
 * are no longer present (fix round 2: `useQueue` can deliver a PBI switch's
 * new scope a render before its own reload, so "no matching row yet" is not
 * evidence the hold is stale - it can just be early). A hold with no
 * matching row is simply INERT: `heldRows`/`ambiguousRows` mark nothing, so
 * nothing is refused, but the hold itself is left exactly as it was, and it
 * applies again the moment a same-titled row reappears.
 */
import type { ReconciledCase, SubmitItemResult, TestCase } from "../bindings";

export type UploadHold = {
  /** ISO time the upload started (less a margin): the lookup's lower bound. */
  since: string;
  /** One title per held create row; repeats allowed. */
  titles: string[];
  /** Titles held because Azure DevOps has more than one matching case for
   * them, not merely because the outcome is unknown. Subset of `titles`. */
  ambiguous?: string[];
  /** Ids a Check must never claim for a held row: the PBI's test cases
   * that were already linked before the upload began, plus every id the
   * upload reported as created or updated. Without them a Check could
   * stamp a held row with a same-titled case from an earlier chunk of the
   * same upload, or one that was there all along. Optional on input so a
   * hold stored before this field existed still loads (as an empty list). */
  ids?: number[];
  /** Each held row's content (`holdSignature`), aligned with `titles`. The
   * row that was sent is marked by its content first, so of two drafts
   * sharing a title it is the one that was actually held, wherever the
   * queue has moved it. Absent on a hold stored before this field existed,
   * which then marks by title alone, as it always did. */
  sigs?: string[];
};

const holdKey = (org: string, pbiId: number) => `tcm-v2-upload-hold:${org}/${pbiId}`;

/** Used only when storage throws, so a hold still lasts the session. */
const fallback = new Map<string, string>();
/** Parsed value per key, reused while the raw text is unchanged, so
 * `useSyncExternalStore` sees a stable snapshot. */
const parsed = new Map<string, { raw: string; hold: UploadHold | null }>();
const listeners = new Set<() => void>();

function readRaw(k: string): string | null {
  try {
    return localStorage.getItem(k);
  } catch {
    return fallback.get(k) ?? null;
  }
}

function writeRaw(k: string, v: string | null): void {
  try {
    if (v == null) localStorage.removeItem(k);
    else localStorage.setItem(k, v);
  } catch {
    if (v == null) fallback.delete(k);
    else fallback.set(k, v);
  }
}

function parseHold(raw: string): UploadHold | null {
  try {
    const v = JSON.parse(raw);
    if (
      v &&
      typeof v.since === "string" &&
      Array.isArray(v.titles) &&
      v.titles.length > 0 &&
      v.titles.every((t: unknown) => typeof t === "string") &&
      (v.ambiguous === undefined ||
        (Array.isArray(v.ambiguous) && v.ambiguous.every((t: unknown) => typeof t === "string"))) &&
      (v.ids === undefined || (Array.isArray(v.ids) && v.ids.every((n: unknown) => Number.isInteger(n))))
    ) {
      const ids: number[] = v.ids ?? [];
      // Signatures that do not line up with the titles are dropped, not
      // trusted: the hold then marks by title, which is never worse than
      // before signatures existed.
      const sigsOk =
        Array.isArray(v.sigs) &&
        v.sigs.length === v.titles.length &&
        v.sigs.every((s: unknown) => typeof s === "string");
      const hold: UploadHold =
        v.ambiguous !== undefined
          ? { since: v.since, titles: v.titles, ambiguous: v.ambiguous, ids }
          : { since: v.since, titles: v.titles, ids };
      return sigsOk ? { ...hold, sigs: v.sigs } : hold;
    }
  } catch {
    // not a hold
  }
  return null;
}

export function subscribeHold(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function loadHold(org: string, pbiId: number): UploadHold | null {
  const k = holdKey(org, pbiId);
  const raw = readRaw(k);
  if (raw == null) return null;
  const hit = parsed.get(k);
  if (hit && hit.raw === raw) return hit.hold;
  const hold = parseHold(raw);
  parsed.set(k, { raw, hold });
  return hold;
}

/** Set or lift (null) the hold for one PBI. */
export function saveHold(org: string, pbiId: number, hold: UploadHold | null): void {
  const next = hold && hold.titles.length > 0 ? hold : null;
  writeRaw(holdKey(org, pbiId), next ? JSON.stringify(next) : null);
  for (const l of listeners) l();
}

/** A held row's content, as the hold remembers it: title, steps (Shared
 * Steps references included), preconditions, module and tags. */
export function holdSignature(tc: TestCase): string {
  return JSON.stringify([
    tc.title.trim(),
    tc.steps.map((s) => [s.action, s.expected, s.shared ?? null]),
    tc.preconditions,
    tc.module_value,
    tc.tags,
  ]);
}

/** The hold a finished submit leaves: every "unknown" result, named by the
 * title and the content of the row that was SENT at its index. Null when
 * there are none. `preExisting` is the ids of the PBI's test cases linked
 * before the upload began; they and every id a result reported go into
 * `ids`. */
export function holdFromResults(
  results: Pick<SubmitItemResult, "index" | "action" | "id">[],
  sent: TestCase[],
  since: string,
  preExisting: number[] = [],
): UploadHold | null {
  const held = results
    .filter((r) => r.action === "unknown")
    .map((r) => sent[r.index])
    .filter((c): c is TestCase => c != null);
  if (held.length === 0) return null;
  const reported = results
    .filter((r) => (r.action === "created" || r.action === "updated") && r.id != null)
    .map((r) => r.id as number);
  return {
    since,
    titles: held.map((c) => c.title),
    ids: [...new Set([...preExisting, ...reported])],
    sigs: held.map(holdSignature),
  };
}

/** What a Check tells Rust to leave out before it pairs anything: the
 * hold's recorded ids and every update id currently in the queue (a row
 * that already carries an id is that case, so it can be no held row's). */
export function checkExcludeIds(hold: UploadHold, queue: TestCase[]): number[] {
  const inQueue = queue.map((tc) => tc.update_id).filter((x): x is number => x != null);
  return [...new Set([...(hold.ids ?? []), ...inQueue])];
}

function countOf(list: string[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const k of list) out.set(k, (out.get(k) ?? 0) + 1);
  return out;
}

function take(left: Map<string, number>, k: string): boolean {
  const n = left.get(k) ?? 0;
  if (n === 0) return false;
  left.set(k, n - 1);
  return true;
}

/** Which queue rows are held: create rows only, as many per title as the
 * hold names. The rows whose content is exactly what was sent are marked
 * first - of two drafts sharing a title, the one that was held, wherever
 * it now sits. Then first come first marked by title, for a held row
 * edited since and for a hold stored before signatures existed. The same
 * order the file sync pairs rows in (fileSync.fileOwnedKeys). */
export function heldRows(queue: TestCase[], hold: UploadHold | null): boolean[] {
  if (!hold) return queue.map(() => false);
  const titlesLeft = countOf(hold.titles.map((t) => t.trim()));
  const sigsLeft = countOf(hold.sigs && hold.sigs.length === hold.titles.length ? hold.sigs : []);
  const out = queue.map(() => false);
  queue.forEach((tc, i) => {
    if (tc.update_id != null || sigsLeft.size === 0) return;
    const sig = holdSignature(tc);
    const title = tc.title.trim();
    if ((sigsLeft.get(sig) ?? 0) === 0 || (titlesLeft.get(title) ?? 0) === 0) return;
    take(sigsLeft, sig);
    take(titlesLeft, title);
    out[i] = true;
  });
  queue.forEach((tc, i) => {
    if (out[i] || tc.update_id != null) return;
    if (take(titlesLeft, tc.title.trim())) out[i] = true;
  });
  return out;
}

/** Which of the held rows are held because their title is ambiguous in
 * Azure DevOps (more matches there than rows checked), not merely unknown -
 * so the row should say that instead of the generic "check before
 * uploading again". Always a subset of `heldRows`. */
export function ambiguousRows(queue: TestCase[], hold: UploadHold | null): boolean[] {
  if (!hold || !hold.ambiguous || hold.ambiguous.length === 0) return queue.map(() => false);
  const held = heldRows(queue, hold);
  const left = countOf(hold.ambiguous.map((t) => t.trim()));
  return queue.map((tc, i) => held[i] && take(left, tc.title.trim()));
}

/** What is left of a hold after a Check: the rows whose title the answer
 * still calls ambiguous, each with its own signature, and the hold's ids
 * plus what this Check claimed. Null when nothing is left. */
export function narrowHold(h: UploadHold, ambiguous: string[], claimed: number[]): UploadHold | null {
  const still = new Set(ambiguous.map((t) => t.trim()));
  const keep = h.titles.map((_, i) => i).filter((i) => still.has(h.titles[i].trim()));
  if (keep.length === 0) return null;
  const titles = keep.map((i) => h.titles[i]);
  const ids = [...new Set([...(h.ids ?? []), ...claimed])];
  const sigs = h.sigs && h.sigs.length === h.titles.length ? keep.map((i) => h.sigs![i]) : undefined;
  const narrowed: UploadHold = { since: h.since, titles, ambiguous: titles, ids };
  return sigs ? { ...narrowed, sigs } : narrowed;
}

/** What a Check found, as "created" results indexed into `queue`. The
 * caller passes `queue` as both the sent and the live list to
 * `applyOutcome`, so `keepUploaded` numbers keys over the same full list
 * (see queueUploaded.ts, trap 3). `orphans`: found cases whose held row is
 * no longer in the queue. */
export function reconciledResults(
  queue: TestCase[],
  held: boolean[],
  found: ReconciledCase[],
): { results: SubmitItemResult[]; orphans: number } {
  const used = new Set<number>();
  const results: SubmitItemResult[] = [];
  let orphans = 0;
  for (const f of found) {
    const at = queue.findIndex((tc, i) => held[i] && !used.has(i) && tc.title.trim() === f.title.trim());
    if (at < 0) {
      orphans += 1;
      continue;
    }
    used.add(at);
    results.push({ index: at, title: queue[at].title, action: "created", id: f.id, error: null });
  }
  return { results, orphans };
}
