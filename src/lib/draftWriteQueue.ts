/**
 * Serialises write-backs to the same draft file.
 *
 * Two write-backs can touch the same file within one `save_draft_cases`
 * round trip - a Remove followed by another Remove on the row that just
 * slid up under the cursor, an edit racing the post-upload id stamp. Each
 * one used to build its `occurrence`s from whatever watch snapshot its own
 * React closure captured when it was called: correct for the first write,
 * stale for a second one that starts before the first's result has flowed
 * back through state. Rust then pairs a row with the wrong same-titled
 * entry and can delete or overwrite the wrong twin - the exact case Task 5
 * set out to fix, reopened by a race between two of its own writes.
 *
 * This module holds, per path, a promise chain (so a write only starts
 * once every write already queued for that file has settled) and the cases
 * the last one actually returned - read by the next write instead of a
 * possibly-stale prop. Module-scoped, not component state, because a
 * write-back can outlive the component that started it (see the "mount
 * may be gone" write-back in QueueSection, mirroring `submitRun.ts`).
 */

import type { TestCase } from "../bindings";
import type { WatchedFile } from "./fileSync";

const chains = new Map<string, Promise<unknown>>();
const latest = new Map<string, TestCase[]>();

/** Run `task` only after every task already queued for `path` has settled,
 * success or failure - so two write-backs to the same file never overlap,
 * and each one sees what the last one actually wrote. A rejected task does
 * not jam the chain: later tasks queued for the same path still run. */
export function runOnFileChain<T>(path: string, task: () => Promise<T>): Promise<T> {
  const previous = chains.get(path) ?? Promise.resolve();
  const settled = previous.then(task, task);
  // The stored tail must never itself reject, or the NEXT `.then` on it
  // (the next queued task) would skip straight to being rejected instead
  // of running.
  chains.set(
    path,
    settled.then(
      () => {},
      () => {},
    ),
  );
  return settled;
}

/** What the last successful write-back for `path` actually left in the
 * file, if this module has seen one. `noteWritten` records it the moment a
 * write returns - before the queued task after it can possibly run, since
 * that task is chained after this one. */
export function noteWritten(path: string, cases: TestCase[]): void {
  latest.set(path, cases);
}

/** `watches`, with each entry's snapshot replaced by the freshest one this
 * module knows for its path, if any. Everything a write-back computes an
 * `occurrence` from (`fileOwners`) should read this instead of `watches`
 * directly, so a write queued behind another on the same file is built
 * from what that other write actually returned - not from the prop this
 * component had when the write was first requested. */
export function freshWatches(watches: WatchedFile[]): WatchedFile[] {
  return watches.map((w) => {
    const cases = latest.get(w.path);
    return cases ? { ...w, snapshot: cases } : w;
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
