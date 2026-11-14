import { useCallback, useRef, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { commands, type DraftEdit, type TestCase } from "../../bindings";
import { toast } from "../../lib/toast";
import { fileName, fileOwners, patchWatch, type WatchedFile } from "../../lib/fileSync";
import { freshWatches, noteWritten, runOnFileChain } from "../../lib/draftWriteQueue";
import type { RenameTarget } from "../PowerRenameDialog";
import {
  changedKeys,
  restoreRows,
  revertBulk,
  useUndoToast,
  type BulkChange,
  type RemovedRow,
} from "./queueUndo";

const noun = (n: number) => `${n} case${n === 1 ? "" : "s"}`;

/**
 * Every way the queue changes under the user's hand - bulk edit, bulk
 * remove, rename, a single remove, a single save - and the write-back that
 * keeps the files the cases came from in step.
 */
export function useQueueEdits({
  queue,
  setQueue,
  watches,
  watchesRef,
  onWatchPatched,
  onQueueCleared,
  onQueueRestored,
  selected,
  setEditingIdx,
  uploading = false,
  scope = "",
}: {
  queue: TestCase[];
  setQueue: Dispatch<SetStateAction<TestCase[]>>;
  watches: WatchedFile[];
  watchesRef: MutableRefObject<WatchedFile[]>;
  onWatchPatched?: (path: string, fields: Partial<WatchedFile>) => void;
  onQueueCleared?: () => void;
  /** Undo of Remove all: the watches Clear dropped, to arm again. */
  onQueueRestored?: (dropped: WatchedFile[]) => void;
  selected: Set<number>;
  setEditingIdx: Dispatch<SetStateAction<number | null>>;
  /** An upload is running. Rows removed meanwhile get no Undo: the upload
   * stamps each created id onto its row, and a row that was out of the
   * queue at that moment would come back without one. */
  uploading?: boolean;
  /** Which PBI this queue is for; a pending Undo does not outlive it. */
  scope?: string;
}) {
  const announce = useUndoToast(scope, JSON.stringify(watches.map((w) => w.path)));
  /** A row a hand edit replaced, to the row that replaced it - how a bulk
   * edit's Undo still finds a row someone has since edited. */
  const successor = useRef(new WeakMap<TestCase, TestCase>());
  /** Write bulk changes back into the files the cases came from, so the
   * file says what the queue says - otherwise the next external save of
   * that file would quietly revert the bulk edit.
   *
   * `next` is aligned with `prev` (null = removed); ownership is computed
   * from the PRE-edit queue, because ownership is matched by title-derived
   * keys and a rename is exactly the operation that breaks that match.
   * Only files owning a changed case are written; each write returns the
   * file's new fingerprint, and the watch snapshot moves forward with it
   * so the watcher stays silent about our own write.
   *
   * One touched file at a time, through `runOnFileChain`: two write-backs
   * on the SAME file - a Remove followed by the next row's Remove landing
   * before the first one's IPC round trip has returned - must not both
   * build their `occurrence`s from the snapshot this render started with.
   * Each file's own task recomputes ownership right before it runs, against
   * `freshWatches` (this module's `watches` plus whatever the last write on
   * that path actually returned), so the SECOND write is paired against
   * what the FIRST one really did to the file, not against a guess made
   * before the first one had a chance to run. */
  const writeBackOwned = async (
    prev: TestCase[],
    next: (TestCase | null)[],
    changed: Set<number>,
  ) => {
    if (watches.length === 0) return;
    // Only used to find which files are touched - not for occurrences.
    // Ownership itself (WHICH file a row belongs to) does not shift when
    // an unrelated row elsewhere in the same file is removed, so today's
    // `watches` is fine for that; each path's own task below recomputes
    // the occurrences that actually matter, fresh, from whatever the
    // chain ahead of it has by then returned.
    const paths = new Set(fileOwners(prev, watches).flatMap((o) => (o.path ? [o.path] : [])));
    for (const path of paths) {
      try {
        await runOnFileChain(path, async () => {
          const owners = fileOwners(prev, freshWatches(watchesRef.current));
          const edits: DraftEdit[] = [];
          let touched = false;
          prev.forEach((before, i) => {
            const o = owners[i];
            if (!o || o.path !== path) return;
            edits.push({ before, after: next[i], occurrence: o.occurrence });
            if (changed.has(i)) touched = true;
          });
          if (!touched) return;
          const r = await commands.saveDraftCases(path, edits);
          if (r.status === "error") {
            // The queue HAS changed - saying so beats pretending nothing did.
            toast.warning(
              `The queue was updated, but ${fileName(path)} could not be: ${r.error}. ` +
                `The file still has the old values.`,
              { duration: 15000 },
            );
            return;
          }
          // The snapshot is what Rust says is now IN THE FILE, in file
          // order - not a queue-order slice of the rows this write touched.
          // Recorded here, before this chained task's promise resolves, so
          // a task queued behind it on this path sees it (`noteWritten`),
          // and so does a task that reads `watchesRef.current` before this
          // component has re-rendered with the parent's own state update.
          noteWritten(path, r.data);
          const fields = { stamp: r.data.stamp, snapshot: r.data.cases };
          watchesRef.current = patchWatch(watchesRef.current, path, fields);
          onWatchPatched?.(path, fields);
        });
      } catch (e) {
        // A THROW (not a `{status:"error"}` result) must not skip the
        // paths after it, nor leave the caller's fire-and-forget
        // `void writeBackOwned(...)` with an unhandled rejection.
        toast.warning(
          `The queue was updated, but ${fileName(path)} could not be written back: ` +
            `${e instanceof Error ? e.message : String(e)}. The file still has the old values.`,
          { duration: 15000 },
        );
      }
    }
  };

  const bulkApply = async (edit: (tc: TestCase) => TestCase) => {
    const prev = queue;
    const chosen = new Set(selected);
    const next: (TestCase | null)[] = prev.map((tc, i) => (chosen.has(i) ? edit(tc) : tc));
    setQueue(next.filter((x): x is TestCase => x != null));
    const changes: BulkChange[] = [];
    chosen.forEach((i) => {
      const after = next[i];
      if (!after || after === prev[i]) return;
      const keys = changedKeys(prev[i], after);
      if (keys.length > 0) changes.push({ index: i, before: prev[i], after, keys });
    });
    announce(
      "success",
      `Edited ${noun(chosen.size)}`,
      changes.length > 0 && !uploading ? () => undoBulk(changes) : undefined,
    );
    await writeBackOwned(prev, next, chosen);
  };

  /** Put back what the bulk edit set, where it still stands. The files the
   * cases came from follow, as they did for the edit. */
  const undoBulk = (changes: BulkChange[]) => {
    const { queue: now, writeBackOwned: writeBack, uploading: busy } = latest.current;
    if (busy) {
      toast.info("Undo is not available while an upload is running.");
      return;
    }
    const { queue: reverted, touched } = revertBulk(now, changes, successor.current);
    if (touched.size === 0) return;
    setQueue(reverted);
    void writeBack(now, reverted, touched);
  };

  const bulkRemove = async () => {
    const prev = queue;
    const removing = new Set(selected);
    const kept = prev.filter((_, i) => !removing.has(i));
    setQueue(kept);
    const gone = prev.flatMap((row, index) => (removing.has(index) ? [{ index, row }] : []));
    // Undone before the write-back below returns: the queue must not then be
    // reported as cleared (which drops the file watches) under the restored rows.
    let undone = false;
    let cleared = false;
    const dropped = watchesRef.current;
    announce(
      "info",
      `Removed ${noun(removing.size)}`,
      uploading || fileOwned(prev, removing)
        ? undefined
        : () => {
            undone = undoRemoval(gone);
            // The watches were already dropped: arm them again.
            if (undone && cleared && dropped.length > 0) onQueueRestored?.(dropped);
          },
      kept.length === 0 ? "[]" : undefined,
    );
    await writeBackOwned(
      prev,
      prev.map((tc, i) => (removing.has(i) ? null : tc)),
      removing,
    );
    if (kept.length === 0 && !undone) {
      cleared = true;
      onQueueCleared?.();
    }
  };

  /** Renaming drafts touches only this list and the files they came from,
   *  so undo here can never fail against Azure DevOps.
   *
   *  Rows are matched back to drafts by POSITION. A draft has no work item
   *  id and a rename can make two of them share a title, so the title is
   *  not an identity that survives the very operation being applied. The
   *  dialog covers just the selected rows, and the scope list maps the dialog's row indices back to queue positions. */
  // Only positions that still exist: the selection is a set of indices,
  // and "Clear queue" with rows selected emptied the queue under it - the
  // white window of 1.23.11 was `queue[qi].update_id` on the next render.
  const renameScope = [...selected].filter((i) => i < queue.length).sort((a, b) => a - b);
  const renameTarget: RenameTarget = {
    label: `${renameScope.length} selected draft${renameScope.length === 1 ? "" : "s"}`,
    cases: renameScope.map((qi) => ({ id: queue[qi].update_id, title: queue[qi].title })),
    undoable: true,
    apply: async (rows) => {
      // Every row must be accounted for: one that no longer maps to a
      // draft, or whose draft no longer carries the title the preview
      // showed - the queue can move under an open dialog - is reported,
      // never written onto whatever sits there now.
      const failed: typeof rows = [];
      const prev = queue;
      const next: (TestCase | null)[] = prev.map((tc) => tc);
      const changed = new Set<number>();
      for (const row of rows) {
        const qi = renameScope[row.index];
        const tc = qi != null ? prev[qi] : undefined;
        if (!tc || tc.title !== row.before) {
          failed.push(row);
          continue;
        }
        next[qi] = { ...tc, title: row.after };
        successor.current.set(tc, next[qi]!);
        changed.add(qi);
      }
      setQueue(next.filter((x): x is TestCase => x != null));
      // The rename reaches the files too - Undo comes back through here
      // with the rows reversed, so it writes the files back as well.
      await writeBackOwned(prev, next, changed);
      return failed;
    },
  };

  // Row callbacks. All stable: a row is memoised on its props, and a
  // callback that changed identity every render would re-render every
  // row on every render - which is exactly what made a 100-case queue
  // slow to switch to. The two that need the current queue and file
  // list read them through a ref instead of closing over them.
  const latest = useRef({ queue, writeBackOwned, uploading });
  latest.current = { queue, writeBackOwned, uploading };
  // The owning FILE follows a single removal exactly as it follows a bulk
  // one: otherwise the next outside save of that file sees the case in
  // both snapshots, not in the queue, and puts it back.
  //
  // By identity, not position: a second click that lands before the first
  // has re-rendered still carries index i, which by then names the NEXT
  // row. The row object is the identity; a row already on its way out is
  // not removed twice.
  const removing = useRef(new WeakSet<TestCase>());
  /** Put removed rows back where they stood. Nothing is sent anywhere: the
   * queue is local, and it is saved like any other change to it. */
  const undoRemoval = (gone: RemovedRow[]): boolean => {
    if (latest.current.uploading) {
      toast.info("Undo is not available while an upload is running.");
      return false;
    }
    for (const { row } of gone) removing.current.delete(row);
    setQueue((q) => restoreRows(q, gone));
    return true;
  };

  /** Whether any of these rows belongs to a watched file. Removing such a
   * row is written into the file, and the write-back has no way to put an
   * entry back, so Undo would leave queue and file disagreeing: a removal
   * of file-owned rows gets no Undo. It gets one once the file write-back
   * can re-insert an entry. */
  const fileOwned = (rows: TestCase[], at: Set<number>) => {
    if (watchesRef.current.length === 0) return false;
    const owners = fileOwners(rows, watchesRef.current);
    return [...at].some((i) => owners[i]?.path);
  };
  const removeRow = useCallback(
    (i: number) => {
      const { queue: prev, writeBackOwned: writeBack } = latest.current;
      const target = prev[i];
      if (!target || removing.current.has(target)) return;
      removing.current.add(target);
      setQueue((q) => q.filter((t) => t !== target));
      const gone = [{ index: i, row: target }];
      announce(
        "info",
        `Removed ${noun(1)}`,
        latest.current.uploading || fileOwned(prev, new Set([i])) ? undefined : () => void undoRemoval(gone),
      );
      void writeBack(
        prev,
        prev.map((t) => (t === target ? null : t)),
        new Set([i]),
      );
    },
    // announce and undoRemoval are stable in effect: they read through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [setQueue],
  );
  const saveRow = useCallback(
    (i: number, next: TestCase) => {
      const { queue: prev, writeBackOwned: writeBack } = latest.current;
      if (prev[i]) successor.current.set(prev[i], next);
      setQueue((q) => q.map((t, j) => (j === i ? next : t)));
      setEditingIdx(null);
      // The owning FILE follows the edit, through the same machinery as
      // bulk edits. This closes a real duplicate trap: ownership is
      // matched by title, so a rename that only the queue knew about
      // orphaned the row at stamp time - its created id was never written
      // back, and the next import of the file created the case again.
      void writeBack(
        prev,
        prev.map((t, j) => (j === i ? next : t)),
        new Set([i]),
      );
      toast.success("Queued case updated.");
    },
    [setQueue],
  );

  /** Remove all: the rows go; Undo puts them back ahead of anything added
   * since, and `restoreMarks` puts the caller's uploaded / failed marks
   * back. The files they came from are left as they are, as before. */
  const clearQueue = (restoreMarks?: () => void) => {
    const rows = latest.current.queue;
    const gone = rows.map((row, index) => ({ index, row }));
    // Clearing drops the file watches (onQueueCleared); keep them so Undo
    // can arm them again and the restored rows are owned as before.
    const dropped = watchesRef.current;
    setQueue([]);
    onQueueCleared?.();
    announce(
      "info",
      `Removed all ${noun(rows.length)}`,
      uploading
        ? undefined
        : () => {
            if (!undoRemoval(gone)) return;
            if (dropped.length > 0) onQueueRestored?.(dropped);
            restoreMarks?.();
          },
      "[]",
    );
  };

  return { bulkApply, bulkRemove, clearQueue, renameTarget, removeRow, saveRow };
}
