/**
 * Serialises write-backs to the same draft file, for the writes queued
 * back to back on it - and ONLY for those.
 *
 * Two write-backs can touch the same file within one `save_draft_cases`
 * round trip - a Remove followed by another Remove on the row that just
 * slid up under the cursor, an edit racing the post-upload id stamp. Each
 * one building its `occurrence`s from whatever watch snapshot its own
 * React closure captured when it was called would be correct for the first
 * write and stale for a second one that starts before the first's result
 * has flowed back through state. Rust would then pair a row with the wrong
 * same-titled entry and could delete or overwrite the wrong twin.
 *
 * This module holds, per path, a promise chain (so a write only starts
 * once every write already queued for that file has settled) and the
 * `{stamp, cases}` the last one actually returned - read by the next
 * QUEUED write instead of a possibly-stale prop. Module-scoped, not
 * component state, because a write-back can outlive the component that
 * started it (see the "mount may be gone" write-back in QueueSection,
 * mirroring `submitRun.ts`).
 *
 * The remembered result must never outlive the burst it was recorded for.
 * An outside edit (an assistant's own change, a review-page comment, a
 * spec or run-order save - none of them go through this module) moves the
 * file on without this module's knowledge, and a write paired against an
 * older copy of the file silently skips a row it should own: a queued
 * upload's id then never lands in the file, and a re-import of it creates
 * a duplicate. Two rules keep it current:
 * - the remembered `{stamp, cases}` is deleted the moment the LAST write
 *   currently queued for that path settles (so it lives only while a
 *   burst of back-to-back writes is still draining), and
 * - even while it is remembered, `freshWatches` only prefers it over the
 *   watch's own snapshot while the watch's OWN current stamp still equals
 *   the stamp that write returned - the moment anything else (an outside
 *   sync, one of the write paths that bypass this module) moves the watch
 *   on, the remembered snapshot stops applying, queued or not.
 */

import type { TestCase } from "../bindings";
import type { WatchedFile } from "./fileSync";

type Written = { stamp: string; cases: TestCase[] };

const chains = new Map<string, Promise<unknown>>();
const latest = new Map<string, Written>();

/** Run `task` only after every task already queued for `path` has settled,
 * success or failure - so two write-backs to the same file never overlap,
 * and each one sees what the last one actually wrote. A rejected task does
 * not jam the chain: later tasks queued for the same path still run.
 *
 * Once `task` settles, if nothing has been queued behind it since (this is
 * still the newest entry in `chains` for `path`), the path's remembered
 * `{stamp, cases}` is dropped: the chain has drained, so the NEXT write on
 * this path - whenever it comes - must pair against the watch's own
 * snapshot, not a copy of the file from however long ago this write was. */
export function runOnFileChain<T>(path: string, task: () => Promise<T>): Promise<T> {
  const previous = chains.get(path) ?? Promise.resolve();
  const settled = previous.then(task, task);
  // The stored tail must never itself reject, or the NEXT `.then` on it
  // (the next queued task) would skip straight to being rejected instead
  // of running.
  const tail = settled.then(
    () => {},
    () => {},
  );
  chains.set(path, tail);
  void tail.then(() => {
    if (chains.get(path) === tail) {
      chains.delete(path);
      latest.delete(path);
    }
  });
  return settled;
}

/** What a write-back for `path` actually returned, recorded the moment it
 * returns - before the queued task after it can possibly run, since that
 * task is chained after this one. Cleared automatically once the chain for
 * `path` drains (see `runOnFileChain`). */
export function noteWritten(path: string, result: Written): void {
  latest.set(path, result);
}

/** `watches`, with each entry's snapshot replaced by the one this module
 * remembers for its path - ONLY while that memory is both still queued
 * (see `runOnFileChain`) and still current: the watch's OWN stamp for that
 * path must still equal the stamp the remembered write returned. The
 * moment anything moves the watch on without going through this module - an
 * outside sync, a comment/spec/run-order save, a PBI switch - the stamps
 * disagree and this falls back to the watch's own snapshot, exactly as if
 * this module had never seen the file. Everything a write-back computes an
 * `occurrence` from (`fileOwners`) should read this instead of `watches`
 * directly. */
export function freshWatches(watches: WatchedFile[]): WatchedFile[] {
  return watches.map((w) => {
    const written = latest.get(w.path);
    return written && written.stamp === w.stamp ? { ...w, snapshot: written.cases } : w;
  });
}

/** Test-only: forget everything this module remembers. Paths are unique
 * files on disk in the real app, so nothing ever needs this outside a
 * test - but two tests reusing the same made-up path (unremarkable; real
 * component tests do it constantly) would otherwise leak one test's writes
 * into the next one's assertions, since this state lives at module scope
 * on purpose (see the file doc comment). */
export function resetFileWriteQueueForTests(): void {
  chains.clear();
  latest.clear();
}
