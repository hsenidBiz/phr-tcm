import type { QueryClient } from "@tanstack/react-query";
import type { Dispatch, SetStateAction } from "react";
import { commands, type SubmitItemResult, type TestCase } from "../../bindings";
import { toast } from "../../lib/toast";
import { fileName, fileOwners, loadWatches, patchWatch, saveWatches, type WatchedFile } from "../../lib/fileSync";
import { loadDraftQueue, saveDraftQueue } from "../../hooks/useQueue";
import { keepUploaded } from "../../lib/queueUploaded";
import { holdFromResults, saveHold } from "../../lib/uploadHold";
import { queueWriterFor } from "../../lib/submitRun";
import { noteSyncPairs, stampFileSlices, unstampedCreated } from "../../lib/queueStamp";
import { freshWatches, noteWritten, runOnFileChain } from "../../lib/draftWriteQueue";
import { cacheKeys, cacheRemove } from "../../lib/cache";
import { saveNote } from "../../lib/caseNotes";

/** What an upload outcome needs from the screen that started it. */
export type OutcomeContext = {
  qc: QueryClient;
  org: string;
  project: string;
  setFailedRows: Dispatch<SetStateAction<Set<string>>>;
  setUploadedIds: Dispatch<SetStateAction<Set<number>>>;
};

export type Outcome = {
  results: SubmitItemResult[];
  sent: TestCase[];
  sentFor: number;
  skipped: number;
  prevQueue: TestCase[];
  /** When this upload started - set by a submit, absent for a Check. */
  since?: string;
  /** The PBI's test case ids before this upload - set by a submit. */
  preExisting?: number[];
};

/** Everything a finished submit owes the user, wherever they are now.
 * Runs inside the mutation promise, so navigating away cannot skip it. */
export function applyOutcome(
  { qc, org, project, setFailedRows, setUploadedIds }: OutcomeContext,
  { results, sent, sentFor, skipped, prevQueue, since, preExisting }: Outcome,
) {
  // Only what Azure DevOps confirmed counts as done. "unknown" is neither
  // done nor failed: it may exist, so it is held until someone checks.
  const ok = results.filter((r) => r.action === "created" || r.action === "updated").length;
  const failedCount = results.filter((r) => r.action === "failed").length;
  const unknownCount = results.filter((r) => r.action === "unknown").length;
  const newHold = since ? holdFromResults(results, sent, since, preExisting) : null;
  if (newHold) saveHold(org, sentFor, newHold);

  // The whole calculation lives in lib/queueUploaded.ts, with the ways
  // matching a result back to its row has gone wrong written down as
  // tests. Every row stays; created ones gain their new id.
  let stranded = 0;
  const keep = (q: TestCase[]) => {
    const kept = keepUploaded(sent, q, results);
    stranded = kept.unmatched;
    return kept.queue;
  };

  // Which rows failed and which were uploaded, for the marks on the rows.
  // Read off the sent list rather than from inside the state updater,
  // which must stay pure: the queue as submitted is the sent rows plus the
  // skipped no-ops, and those carry ids already, so title-keyed rows
  // number the same way in both.
  const marks = keepUploaded(sent, sent, results);
  setFailedRows(marks.failed);
  setUploadedIds(marks.uploadedIds);

  const writer = queueWriterFor(org, sentFor);
  if (writer) {
    // The submitted queue is on screen (this mount or a fresh one):
    // through React state, exactly as it always went.
    writer.setQueue(keep);
  } else {
    // Nobody is looking at that queue right now - the user navigated
    // away, or switched PBI. The persisted draft is the queue they will
    // see on return; stamp THAT, so a created case is never sitting there
    // without its id, one Upload away from a duplicate.
    saveDraftQueue(org, sentFor, keep(loadDraftQueue(org, sentFor)));
  }

  if (failedCount === 0 && unknownCount === 0 && stranded === 0) {
    toast.success(
      skipped > 0
        ? `${ok} test case(s) processed, ${skipped} already up to date.`
        : `${ok} test case(s) processed.`,
    );
  } else if (failedCount > 0) {
    toast.warning(`${ok} processed, ${failedCount} failed - the failed items are marked in the queue.`);
  }
  if (unknownCount > 0) {
    toast.warning(
      `${unknownCount} case(s) may or may not have been created - the upload was interrupted and ` +
        `Azure DevOps could not be asked. They are marked in the queue; check them before uploading again.`,
      { duration: 30000 },
    );
  }
  if (stranded > 0) {
    // Never silent: a created case still sitting in the queue is one
    // Create away from a duplicate work item, and this app cannot
    // delete one.
    toast.warning(
      `${stranded} case(s) were created but could not be matched back to the queue - ` +
        `check the queue before creating again, or you will get duplicates.`,
      { duration: 20000 },
    );
  }
  // The FILES learn what the submit made real: every succeeded case is
  // written back with its work item id and exactly the uploaded content,
  // so re-importing the file yields no-op updates instead of a duplicate
  // set. Failed cases stay as drafts. Comments are never the price:
  // per-case comments ride on the cases, and the file-level comments
  // block survives because merge_cases_into_draft keeps every top-level
  // key it does not own.
  const outcomes = results.map((r) => ({ index: r.index, action: r.action, id: r.id, attached: r.attached, error: r.error }));
  // The watch list as it is at the moment of asking: the screen showing
  // this queue, if one is mounted, and storage only when none is (or it
  // watches nothing). Storage alone is not enough - a failed storage
  // write is swallowed, so it can hold nothing, or a list from before an
  // outside sync, and a row whose file it misses would silently go
  // unstamped. The orphan check and every stamp write read THIS, so they
  // always agree on which file owns a row.
  const currentWatches = (): WatchedFile[] => {
    const live = queueWriterFor(org, sentFor)?.watches?.() ?? [];
    return live.length > 0 ? live : loadWatches(org, sentFor);
  };
  void (async () => {
    const known = currentWatches();
    // One pairing pass serves both the orphan check below and the stamp
    // write-back after it, instead of computing it twice.
    const owned = known.length > 0 ? fileOwners(prevQueue, known) : prevQueue.map(() => null);
    // The other half of the promise "the file learns what the submit made
    // real": say it LOUDLY when a created case's id could not be recorded
    // anywhere - no owning file matched it, or there is no file at all.
    // Both duplicate incidents to date were this situation, silent.
    const owners = owned.map((o) => o?.path ?? "");
    const orphaned = unstampedCreated(prevQueue, owners, sent, outcomes);
    if (orphaned.length > 0) {
      const named = orphaned.slice(0, 3).join("; ");
      toast.warning(
        `${orphaned.length} created case(s) have no file recording their new ids ` +
          `(${named}${orphaned.length > 3 ? "; …" : ""}). Their ids exist only in Azure ` +
          `DevOps now - importing the same drafts again will create duplicates. ` +
          `View Test Cases has the created set.`,
        { duration: 25000 },
      );
    }
    if (known.length > 0) {
      // Same rule as `writeBackOwned`: one write at a time per file, each
      // one recomputing its own pairing against whatever the write ahead
      // of it on that path actually returned - not against `known`,
      // which this whole callback can be holding well after it was
      // captured (a submit takes as long as the upload does).
      for (const path of new Set(owners.filter((p): p is string => !!p))) {
        try {
          await runOnFileChain(path, async () => {
            // Read fresh, not the closed-over `known`: this callback can
            // run well after it was captured. Both sources are patched
            // synchronously below the moment a write lands, which is
            // what lets `freshWatches`' stamp check tell a write still
            // queued behind another apart from one that is not.
            const freshOwned = fileOwners(prevQueue, freshWatches(currentWatches()));
            const files = stampFileSlices(
              prevQueue,
              freshOwned.map((o) => o.path),
              sent,
              outcomes,
              freshOwned.map((o) => o.occurrence),
            );
            const f = files.get(path);
            if (!f || !f.changed) return;
            const r = await commands.saveDraftCases(path, f.edits);
            if (r.status === "error") {
              toast.warning(
                `Uploaded, but ${fileName(path)} could not be updated with the new ids: ${r.error}. ` +
                  `Importing it again would create duplicates - fix the file before re-importing.`,
                { duration: 20000 },
              );
              return;
            }
            // Storage always: the mount that started this submit may be
            // gone, and a setter on an unmounted screen never runs its
            // persist step. Then the screen showing this queue NOW, if
            // any - not this closure's own callback, which may belong to
            // that gone mount.
            //
            // The snapshot is what Rust says is now IN THE FILE, in file
            // order - not a queue-order slice of the rows this write
            // touched. Recorded here, before this task's own promise
            // resolves, so a write queued behind it on the same path is
            // built from what THIS write actually did.
            noteWritten(path, r.data);
            const fields = { stamp: r.data.stamp, snapshot: r.data.cases };
            saveWatches(org, sentFor, patchWatch(loadWatches(org, sentFor), path, fields));
            queueWriterFor(org, sentFor)?.patchWatch?.(path, fields);
          });
        } catch (e) {
          // A THROW must not skip the paths after it, nor leave this
          // fire-and-forget callback with an unhandled rejection.
          toast.warning(
            `Uploaded, but ${fileName(path)} could not be updated with the new ids: ` +
              `${e instanceof Error ? e.message : String(e)}. Importing it again would ` +
              `create duplicates - fix the file before re-importing.`,
            { duration: 20000 },
          );
        }
      }
    }
    // And the same comment now shows on the case where it LIVES: the
    // View Test Cases notes store learns every succeeded case's comment
    // under its (new) work item id.
    for (const pair of noteSyncPairs(sent, outcomes)) {
      saveNote(org, pair.id, pair.comment);
    }
  })();

  qc.invalidateQueries({ queryKey: ["pbi-tcs", org, sentFor] });
  qc.invalidateQueries({ queryKey: ["pbi-tc-titles", org, sentFor] });
  // The rows diff against the server's copy fetched for the review. An
  // update just changed that copy, so drop it: read again, the updated
  // rows show "no-op" instead of the changes they have already made.
  if (results.some((r) => r.action === "updated")) {
    qc.invalidateQueries({ queryKey: ["diff-cases", org] });
  }
  // A created case can move the suite's spec order and/or the PBI's
  // suggested run order (design doc §4.1, §4.2). Run Tests and Suite
  // Management must not go on serving what they cached before the
  // upload - drop both the query and the persisted copy, and widen the
  // suite-cases invalidation to every suite for this org/project since
  // this screen does not know which suite the upload landed in.
  if (results.some((r) => r.action === "created")) {
    qc.invalidateQueries({ queryKey: ["run-order", org, project, sentFor] });
    cacheRemove(cacheKeys.runOrder(org, project, sentFor));
    qc.invalidateQueries({ queryKey: ["suite-cases", org, project] });
  }
}
