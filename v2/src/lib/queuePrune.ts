/**
 * Which queued drafts to remove after a submit.
 *
 * This one calculation has been wrong four separate ways in a single day,
 * and every one of them shipped, because it lived inline in a component
 * with no test on the path at all. So it lives here now, as a pure
 * function, with the four failures written down as tests below it.
 *
 *   1. BY INDEX. An index only means something against the list that was
 *      sent. The queue can be reordered by a watched-file sync or emptied
 *      by a PBI switch in the meantime, so filtering by index removed
 *      whichever rows happened to sit at those positions - which is how a
 *      submit on one PBI deleted another PBI's drafts.
 *
 *   2. BY OBJECT REFERENCE. Fixed (1), but a watched file saved DURING the
 *      create loop makes syncFromFile hand back a NEW object for the case
 *      it changed - the entire purpose of watching a file. The reference
 *      stopped matching, the created draft stayed queued, and the next
 *      Create made a duplicate work item.
 *
 *   3. BY CONTENT KEY, numbered over the SUCCEEDED subset. Fixed (2), and
 *      introduced the worst of the four. `keysFor` numbers repeats by their
 *      position within the list it is handed, so numbering a filtered
 *      subset and comparing it against the whole queue shifts every later
 *      occurrence: with three same-titled drafts where only the second
 *      succeeded, it deleted the FAILED draft the user still had to fix and
 *      kept the CREATED one, so the next Create wrote a second copy.
 *
 *   4. What is here now: content keys numbered over the SENT list, so both
 *      sides share one frame of reference, and consumed one at a time so
 *      repeats remove one each.
 *
 * The lesson is not any of the four - it is that identity derived from
 * content or position always has an aliasing case, and the only reason it
 * took four goes to notice is that nothing here was ever tested.
 */
import { keysFor } from "./fileSync";
import type { TestCase } from "../bindings";

/** One row of what the submit reported back. */
export type SubmitOutcome = {
  /** Position in the list that was SENT, not in the queue now. */
  index: number;
  /** "created" | "updated" | "failed" */
  action: string;
};

export type PruneResult = {
  /** The queue as it should now be. */
  queue: TestCase[];
  /** Created rows whose draft could not be found to remove. Never silently
   * dropped: a created case still sitting in the queue is one Create away
   * from a duplicate work item that this app cannot delete. */
  unmatched: number;
};

/**
 * Remove the drafts that were successfully created or updated.
 *
 * `sent` is the snapshot that was submitted; `live` is the queue as it
 * stands now, which may differ - a watched file may have synced, or the
 * user may have added a case. Keys are computed over BOTH lists in full so
 * the occurrence numbering means the same thing on each side.
 */
export function pruneCreated(
  sent: TestCase[],
  live: TestCase[],
  results: SubmitOutcome[],
): PruneResult {
  const sentKeys = keysFor(sent);
  // Numbered against the full sent list, THEN selected - not numbered over
  // the selection, which is what shifted the occurrences in (3).
  const created: string[] = results
    .filter((r) => r.action !== "failed")
    .map((r) => sentKeys[r.index])
    .filter((k): k is string => k != null);

  const remaining = new Map<string, number>();
  for (const k of created) remaining.set(k, (remaining.get(k) ?? 0) + 1);

  const liveKeys = keysFor(live);
  const queue = live.filter((_, i) => {
    const left = remaining.get(liveKeys[i]) ?? 0;
    if (left === 0) return true;
    remaining.set(liveKeys[i], left - 1);
    return false;
  });

  let unmatched = 0;
  for (const left of remaining.values()) unmatched += left;
  return { queue, unmatched };
}
