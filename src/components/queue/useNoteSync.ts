import { useCallback, useEffect, useRef, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { commands, type TestCase } from "../../bindings";
import { freshWatches, noteWritten, runOnFileChain } from "../../lib/draftWriteQueue";
import { ownerPaths, patchWatch, type WatchedFile } from "../../lib/fileSync";
import { loadNotes, saveNote } from "../../lib/caseNotes";

/**
 * Comments sync both ways between a file and the View Test Cases notes,
 * joined on the work item id - a case imported for update IS the case
 * the note is about.
 *
 * File -> notes: a case imported WITH its id and a comment, whose id has
 * no note yet, gives the note its text. A note typed in View is never
 * overwritten by an import.
 *
 * Notes -> card: the FIRST time a row with that id appears in this queue
 * with an empty comment, the note fills it - onto the card and into the
 * file the case came from. Only then: once the row has been seen, the
 * card is the user's, and a comment they cleared stays cleared.
 *
 * Card -> notes: a later change to that comment (the editor, the browser
 * page, a file sync) is written to the note, debounced, so View Test
 * Cases shows what the queue shows.
 */
export function useNoteSync({
  org,
  pbiId,
  queue,
  setQueue,
  watches,
  watchesRef,
  onWatchPatched,
}: {
  org: string;
  pbiId: number;
  queue: TestCase[];
  setQueue: Dispatch<SetStateAction<TestCase[]>>;
  watches: WatchedFile[] | undefined;
  watchesRef: MutableRefObject<WatchedFile[]>;
  onWatchPatched?: (path: string, fields: Partial<WatchedFile>) => void;
}) {
  const noteSeen = useRef<{ scope: string; comments: Map<number, string> }>({
    scope: "",
    comments: new Map(),
  });
  const pendingNotes = useRef<{ org: string; texts: Map<number, string> }>({ org, texts: new Map() });
  const noteTimer = useRef<number | undefined>(undefined);
  const flushNotes = useCallback(() => {
    window.clearTimeout(noteTimer.current);
    noteTimer.current = undefined;
    const p = pendingNotes.current;
    for (const [id, text] of p.texts) saveNote(p.org, id, text);
    p.texts.clear();
  }, []);
  // An edit still waiting when the screen goes is written, not dropped.
  useEffect(() => flushNotes, [flushNotes]);
  useEffect(() => {
    const scope = `${org}/${pbiId}`;
    if (noteSeen.current.scope !== scope) {
      flushNotes();
      noteSeen.current = { scope, comments: new Map() };
    }
    const seen = noteSeen.current.comments;
    const notes = loadNotes(org);
    const fills = new Map<number, string>(); // queue index -> note
    let edited = false;
    const presentIds = new Set<number>();
    queue.forEach((tc, i) => {
      if (tc.update_id == null) return;
      const id = tc.update_id;
      presentIds.add(id);
      const comment = (tc.comment ?? "").trim();
      const before = seen.get(id);
      if (before === undefined) {
        const note = notes[String(id)];
        if (comment && !note) saveNote(org, id, comment);
        if (!comment && note) {
          fills.set(i, note);
          seen.set(id, note.trim());
        } else {
          seen.set(id, comment);
        }
        return;
      }
      if (comment === before) return;
      seen.set(id, comment);
      pendingNotes.current.org = org;
      pendingNotes.current.texts.set(id, comment);
      edited = true;
    });
    // A row that has left the queue (removed, kept-uploaded and swapped
    // for a fresh copy, etc.) is forgotten here: if that id comes back
    // later in this same mount, it is a first appearance again, not an
    // edit of what it had - the stored note fills it instead of an empty
    // comment being queued to blank the note out.
    for (const id of seen.keys()) {
      if (!presentIds.has(id)) seen.delete(id);
    }
    if (edited) {
      window.clearTimeout(noteTimer.current);
      noteTimer.current = window.setTimeout(flushNotes, 600);
    }
    if (fills.size === 0) return;

    // By work item id, not position: the queue can move between this
    // effect and the updater, and an id is exact.
    const fillById = new Map<number, string>();
    for (const [i, text] of fills) fillById.set(queue[i].update_id as number, text);
    setQueue((q) =>
      q.map((c) =>
        c.update_id != null && fillById.has(c.update_id) && !(c.comment ?? "").trim()
          ? { ...c, comment: fillById.get(c.update_id)! }
          : c,
      ),
    );
    // The file learns it too, one targeted comment patch per case (never
    // a whole-file rewrite for this), and the watch is told the stamp so
    // the app's own write is not reported back as an outside edit.
    //
    // Each patch waits its turn on the file's write chain, like every
    // write-back does. The snapshot it hands on is read from the watch as
    // it stands AFTER the patch landed, not from the list captured when
    // this effect ran: a Remove or an edit can finish on the same file
    // while this loop is still going, and putting the older case list back
    // at the file's newest stamp would make the next write pair same-titled
    // rows with the wrong entries.
    const known = watches ?? [];
    if (known.length === 0) return;
    const owners = ownerPaths(queue, known);
    void (async () => {
      for (const [i, text] of fills) {
        const path = owners[i];
        if (!path) continue;
        const tc = queue[i];
        try {
          await runOnFileChain(path, async () => {
            const r = await commands.saveDraftComment(path, tc.update_id, tc.title, text);
            if (r.status !== "ok") return;
            const w = freshWatches(watchesRef.current).find((x) => x.path === path);
            if (!w) return;
            const snapshot = w.snapshot.map((c) =>
              c.update_id === tc.update_id ? { ...c, comment: text } : c,
            );
            noteWritten(path, { stamp: r.data, cases: snapshot });
            const fields = { stamp: r.data, snapshot };
            watchesRef.current = patchWatch(watchesRef.current, path, fields);
            onWatchPatched?.(path, fields);
          });
        } catch {
          // The note is still on the card and in View Test Cases; a patch
          // that could not reach the file is not worth stopping the rest.
        }
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queue, org, pbiId]);
}
