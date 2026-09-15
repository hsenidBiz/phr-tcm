/**
 * What an upload does to the queue: nothing leaves it.
 *
 * Every row stays until the user removes it. A row that was CREATED gains
 * its new work item id, so it is now an update of the case it made - which
 * is what keeps a queue that still holds it from creating it twice, and
 * what keeps it lined up with its watched file, which gets the same id
 * written back (see queueStamp.ts).
 *
 * This used to remove the uploaded rows instead, and that calculation was
 * wrong four separate ways, every one of which shipped. Matching a result
 * back to its row has the same traps whether the row is removed or stamped,
 * so the discipline is unchanged:
 *
 *   1. NOT BY INDEX. An index only means something against the list that
 *      was sent. A watched-file sync can reorder the queue mid-upload.
 *
 *   2. NOT BY OBJECT REFERENCE. A watched file saved during the upload makes
 *      syncFromFile hand back a NEW object for the case it changed.
 *
 *   3. NOT BY KEYS NUMBERED OVER A SUBSET. `keysFor` numbers repeats by
 *      position within the list it is handed; numbering only the succeeded
 *      rows shifted every later occurrence onto the wrong row.
 *
 *   4. What is here: content keys numbered over the SENT list and over the
 *      LIVE list, each in full, consumed one at a time so repeats match one
 *      each.
 */
import { keysFor } from "./fileSync";
import type { TestCase } from "../bindings";

/** One row of what the submit reported back. */
export type SubmitOutcome = {
  /** Position in the list that was SENT, not in the queue now. */
  index: number;
  /** "created" | "updated" | "failed" */
  action: string;
  /** The work item id; the NEW one for a created case. */
  id?: number | null;
};

export type KeepResult = {
  /** The queue as it should now be - every row still in it. */
  queue: TestCase[];
  /** Created cases whose row could not be found to give the id to. Never
   * silently dropped: if the row is still there under another title, it has
   * no id, and uploading it again creates a duplicate this app cannot
   * delete. */
  unmatched: number;
  /** Keys, over `queue`, of the rows that failed. */
  failed: Set<string>;
  /** Work item ids written by this upload, created and updated alike. A
   * row carrying one of these was uploaded; an id is exact, where a key's
   * occurrence number moves as rows are edited or removed. */
  uploadedIds: Set<number>;
};

/** Live index per sent result, matched by key and consumed one at a time. */
function matchLive(sentKeys: string[], liveKeys: string[], indices: number[]): (number | null)[] {
  const byKey = new Map<string, number[]>();
  liveKeys.forEach((k, i) => byKey.set(k, [...(byKey.get(k) ?? []), i]));
  return indices.map((si) => {
    const k = sentKeys[si];
    const slots = k == null ? undefined : byKey.get(k);
    return slots && slots.length > 0 ? slots.shift()! : null;
  });
}

/**
 * Apply a finished upload to the queue without removing anything.
 *
 * `sent` is the snapshot that was submitted; `live` is the queue as it
 * stands now, which may differ - a watched file may have synced, or the
 * user may have added a case.
 */
export function keepUploaded(sent: TestCase[], live: TestCase[], results: SubmitOutcome[]): KeepResult {
  const sentKeys = keysFor(sent);
  const liveKeys = keysFor(live);
  const slots = matchLive(
    sentKeys,
    liveKeys,
    results.map((r) => r.index),
  );

  const queue = [...live];
  const failedAt: number[] = [];
  const uploadedIds = new Set<number>();
  let unmatched = 0;

  results.forEach((r, n) => {
    const at = slots[n];
    if (r.action === "created") {
      if (r.id == null || at == null) {
        unmatched += 1;
        return;
      }
      queue[at] = { ...queue[at], update_id: r.id };
      uploadedIds.add(r.id);
    } else if (r.action === "updated") {
      const id = sent[r.index]?.update_id ?? r.id;
      if (id != null) uploadedIds.add(id);
    } else if (r.action === "failed" && at != null) {
      failedAt.push(at);
    }
  });

  // Keyed over the stamped queue, the list the rows render from: a created
  // row's key changes to its id, which renumbers same-titled rows after it.
  const queueKeys = keysFor(queue);
  const failed = new Set(failedAt.map((i) => queueKeys[i]));
  return { queue, unmatched, failed, uploadedIds };
}
