import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { createPortal } from "react-dom";
import { toast } from "sonner";
import PowerRenameDialog, { type RenameTarget } from "./PowerRenameDialog";
import {
  commands,
  events,
  type DraftEdit,
  type OrderHint,
  type SubmitItemResult,
  type TestCase,
  type TestCaseFull,
} from "../bindings";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { useOnScreen } from "../hooks/useOnScreen";
import { diffCase, type CaseDiff } from "../lib/caseDiff";
import { hasTesterNotes, testerNotes } from "../lib/testerNotes";
import { exportPathFor, rememberExportPath } from "../lib/exportDir";
import { cn } from "../lib/cn";
import { fileName, keysFor, loadWatches, ownerPaths, patchWatch, saveWatches, type WatchedFile } from "../lib/fileSync";
import { loadDraftQueue, saveDraftQueue } from "../hooks/useQueue";
import { keepUploaded } from "../lib/queueUploaded";
import { summariseSubmit } from "../lib/submitSummary";
import {
  ambiguousRows,
  heldRows,
  holdFromResults,
  loadHold,
  reconciledResults,
  saveHold,
  subscribeHold,
} from "../lib/uploadHold";
import {
  queueWriterFor,
  registerQueueWriter,
  submitFinished,
  submitPhaseSnapshot,
  submitProgressed,
  submitStarted,
  subscribeSubmit,
} from "../lib/submitRun";
import { noteSyncPairs, stampFileSlices, unstampedCreated } from "../lib/queueStamp";
import { cacheKeys, cacheRemove } from "../lib/cache";
import { OFFLINE_HINT, onlineSnapshot, subscribeOnline } from "../lib/network";
import { sidebarCollapsedSnapshot, stickyLeftPx, subscribeSidebar } from "../lib/sidebarState";
import { loadNotes, saveNote } from "../lib/caseNotes";
import { setPbiGlow } from "../lib/pbiGlow";
import { copyText } from "../lib/clipboard";
import { unwrap } from "../lib/ipc";
import { duplicateWarning, validateCase } from "../lib/validate";
import { pagePalette } from "../lib/reportTheme";
import QueueBulkEditDialog from "./QueueBulkEditDialog";
import QueueRow from "./QueueRow";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import ScanProgress from "./ScanProgress";
import { Checkbox } from "./ui/checkbox";
import { Modal } from "./ui/modal";
import {
  IconCollapseAll,
  IconBack,
  IconClear,
  IconConfirm,
  IconExport,
  IconOpenInBrowser,
  IconRemove,
  IconReview,
  IconCopy,
  IconShare,
  IconRename,
  IconRefresh,
  IconRelease,
  IconCancel,
} from "../lib/actionIcons";

const HOLD_REFUSAL =
  "Some cases from the last upload have an unknown outcome - check them before uploading again.";

/** The shared pending-creation queue with the review gate, live progress and
 * exports. Manual Entry and Import File both render this under their own
 * input areas (v1: every tab feeds one queue). */
/** One recently imported file. The row asks the disk whether the file
 * still exists (fileStamp is null for a missing one) and says so instead
 * of offering a click that can only fail - deleted files happen, and the
 * row's job is to be honest about them. The X forgets the entry either
 * way. */
function RecentImportRow({
  path,
  when,
  onOpen,
  onForget,
}: {
  path: string;
  when: number;
  onOpen: () => void;
  onForget: () => void;
}) {
  const exists = useQuery({
    queryKey: ["file-exists", path],
    queryFn: () => commands.fileStamp(path),
    // A file can come back (restored from the bin, a re-synced share) -
    // re-ask on mount rather than trusting a stale "gone".
    staleTime: 0,
    retry: false,
  });
  const missing = exists.isSuccess && exists.data === null;
  return (
    <div
      className={cn(
        "flex items-center gap-2 rounded-md border border-border bg-surface px-3 py-1.5 text-xs",
        missing && "opacity-70",
      )}
    >
      <span className="min-w-0 flex-1 truncate">
        <span className="text-text">{fileName(path)}</span>
        <span className="ml-2 text-faint" title={path}>
          {path}
        </span>
      </span>
      <span className="shrink-0 text-faint">{new Date(when).toLocaleDateString()}</span>
      {missing ? (
        <span className="pill-label shrink-0 rounded-full bg-warning/15 px-2 text-[10px] font-medium text-warning">
          File no longer exists
        </span>
      ) : (
        <Button
          size="sm"
          variant="outline"
          aria-label={`Reopen ${fileName(path)}`}
          disabled={exists.isLoading}
          onClick={onOpen}
        >
          Open
        </Button>
      )}
      <button
        aria-label={`Remove ${fileName(path)} from recent imports`}
        title="Remove from recent imports"
        className="shrink-0 rounded p-1 text-muted transition-colors hover:text-danger"
        onClick={onForget}
      >
        <X size={13} />
      </button>
    </div>
  );
}

export default function QueueSection({
  org,
  project,
  pbiId,
  queue,
  setQueue,
  flash,
  watches = [],
  onQueueCleared,
  onWatchPatched,
  recentImports = [],
  onOpenRecent,
  onForgetRecent,
}: {
  org: string;
  project: string;
  pbiId: number;
  queue: TestCase[];
  setQueue: React.Dispatch<React.SetStateAction<TestCase[]>>;
  /** Rows a watched-file sync just touched, by caseKey - tinted so the
   * change report's counts can be traced to actual rows. */
  flash?: Record<string, "added" | "changed">;
  /** The JSON files this queue was imported from, so a comment typed in
   * the browser view knows which file to be written back into. Manual
   * Entry passes none - its cases live only in the app. */
  watches?: WatchedFile[];
  /** Called when Remove all empties the queue. The Import screen uses it
   * to stop watching the files that fed it: the queue was the only reason
   * those watches existed, so leaving them armed means a later save to a
   * finished file quietly refills a queue the user deliberately emptied. */
  onQueueCleared?: () => void;
  /** Called after a bulk change is written back into a watched file, with
   * the file's new fingerprint and snapshot - the owner of the watch list
   * moves it forward so the watcher stays silent about our own write. */
  onWatchPatched?: (path: string, fields: Partial<WatchedFile>) => void;
  /** Recently imported JSON files, shown while the queue is empty so the
   * screen offers a way back in rather than a dead end. Manual Entry
   * passes none and keeps the plain empty state. */
  recentImports?: { path: string; when: number }[];
  onOpenRecent?: (path: string) => void;
  onForgetRecent?: (path: string) => void;
}) {
  const qc = useQueryClient();
  const { prefs } = useFieldRefs(org, project);
  const [results, setResults] = useState<SubmitItemResult[] | null>(null);
  // The last upload's "what changed" note for testers, or null when it
  // changed nothing a tester would act on. Built from diffs taken just
  // before the write - once the upload lands, the queue matches the server
  // and there is nothing left to diff.
  const [changeNotes, setChangeNotes] = useState<string | null>(null);
  // What the last submit did to each row. Nothing leaves the queue on an
  // upload - the user removes rows when they are finished with them - so
  // without these marks an uploaded row and a failed one look the same.
  // Failed rows by key; uploaded rows by work item id, which every one of
  // them now carries and which does not shift as rows around it change.
  const [failedRows, setFailedRows] = useState<Set<string>>(() => new Set());
  const [uploadedIds, setUploadedIds] = useState<Set<number>>(() => new Set());
  const [reviewing, setReviewing] = useState(false);
  // Progress lives at MODULE scope (lib/submitRun), not in this component:
  // the upload takes minutes and the person watching it is exactly the
  // person who wanders to another tab meanwhile. Any mount of this screen
  // reads the same phase, so coming back shows the bar where it really is.
  const phase = useSyncExternalStore(subscribeSubmit, submitPhaseSnapshot);
  const progress = phase && phase.org === org && phase.pbiId === pbiId ? phase : null;

  // Creates from an interrupted upload that Azure DevOps could not confirm
  // (lib/uploadHold). Persisted per PBI: a restart must not lift the one
  // thing between the user and a duplicate.
  const hold = useSyncExternalStore(subscribeHold, () => loadHold(org, pbiId));
  const [checkingHold, setCheckingHold] = useState(false);

  // An upload runs to the end once started - there is no Stop. The case
  // in flight could never be taken back (created cases cannot be deleted),
  // so stopping only ever left a half-done set. While it runs the action
  // button reads "Processing" and does nothing; the bar above it is what
  // shows that something is happening.
  // Writes PAUSE while offline instead of failing: attempting a submit on
  // a dead connection is known-doomed, and a paused button with a reason
  // beats an error toast. Reads pause on their own (React Query's
  // networkMode) and resume when the connection returns.
  const online = useSyncExternalStore(subscribeOnline, onlineSnapshot);
  // For the sticky Collapse all's left offset - it parks bottom LEFT like
  // every other screen's, clearing the sidebar at its current width.
  const sidebarCollapsed = useSyncExternalStore(subscribeSidebar, sidebarCollapsedSnapshot);

  // While mounted, this screen's own setQueue applies the post-submit ids
  // (through React state, as always). When it is NOT mounted at the finish,
  // the fallback in the mutation writes the persisted draft directly - a
  // created case left in a queue without its id is one Upload away from a
  // duplicate work item. The watch-list update is registered the same way,
  // through a ref so the registration does not churn on every render.
  const watchPatched = useRef(onWatchPatched);
  watchPatched.current = onWatchPatched;
  useEffect(
    () =>
      registerQueueWriter({
        org,
        pbiId,
        setQueue: (updater) => setQueue(updater),
        patchWatch: (path, fields) => watchPatched.current?.(path, fields),
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [org, pbiId],
  );
  // Comments sync both ways between a file and the View Test Cases notes,
  // joined on the work item id - a case imported for update IS the case
  // the note is about.
  //
  // File -> notes: a case imported WITH its id and a comment, whose id has
  // no note yet, gives the note its text. A note typed in View is never
  // overwritten by an import.
  //
  // Notes -> card: the FIRST time a row with that id appears in this queue
  // with an empty comment, the note fills it - onto the card and into the
  // file the case came from. Only then: once the row has been seen, the
  // card is the user's, and a comment they cleared stays cleared.
  //
  // Card -> notes: a later change to that comment (the editor, the browser
  // page, a file sync) is written to the note, debounced, so View Test
  // Cases shows what the queue shows.
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
    queue.forEach((tc, i) => {
      if (tc.update_id == null) return;
      const id = tc.update_id;
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
    const known = watches ?? [];
    if (known.length === 0) return;
    const owners = ownerPaths(queue, known);
    void (async () => {
      for (const [i, text] of fills) {
        const path = owners[i];
        if (!path) continue;
        const tc = queue[i];
        const r = await commands.saveDraftComment(path, tc.update_id, tc.title, text);
        if (r.status !== "ok" || !onWatchPatched) continue;
        const w = known.find((x) => x.path === path);
        onWatchPatched(path, {
          stamp: r.data,
          snapshot: (w?.snapshot ?? []).map((c) =>
            c.update_id === tc.update_id ? { ...c, comment: text } : c,
          ),
        });
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queue, org, pbiId]);

  // Final confirmation stage: the first Confirm click arms the submit and
  // spotlights the PBI chip; only the explicit second click writes.
  //
  // Skipped for a queue that is NOTHING but updates: an update PATCHes its
  // own work item where it already lives and never reads the selected PBI
  // (only creates link to it, take its area/iteration, and populate its
  // suite). The check-the-PBI stage exists to stop creates landing under
  // the wrong PBI - for pure updates it is friction with nothing to catch.
  const pureUpdates = queue.length > 0 && queue.every((tc) => tc.update_id != null);
  const [armed, setArmed] = useState(false);
  // The last safeguard before anything is written: titles of the CREATE
  // rows that already exist on the PBI, checked FRESH against ADO at the
  // final click. Set = the submit is stopped until the user explicitly
  // chooses. The per-row hint was scrollable-past; 43 duplicates once
  // sailed through it.
  //
  // Checked when REVIEW OPENS rather than at the final click. The review
  // already showed a per-row duplicate hint, but computed off a cache the
  // comment above admits can be minutes old - so it warned using data it
  // did not trust and then re-asked properly at the worst moment, after a
  // button that said it would create. Fetching once up front makes the
  // hints true and puts the wait where someone is reading.
  //
  // Titles as the fetch last saw them; null until it has answered.
  const [freshTitles, setFreshTitles] = useState<string[] | null>(null);
  const [checkingDups, setCheckingDups] = useState(false);
  // Titles the user has looked at and chosen to duplicate anyway.
  const [acceptedDups, setAcceptedDups] = useState<string[]>([]);
  const arm = (on: boolean) => {
    setArmed(on);
    setPbiGlow(on);
  };
  // Never leave the chip glowing if this screen unmounts mid-confirmation.
  useEffect(() => () => setPbiGlow(false), []);

  // An emptied queue (Remove all, removing the last item) has nothing to
  // review - leave review mode so the confirm controls disappear too.
  //
  // The duplicate answer needs no clearing here: `dupsPending` is derived
  // from the current queue, so removing the offending row clears the
  // warning by itself, and an acceptance stays attached to the title it
  // was given for.
  useEffect(() => {
    // Remove all stays enabled during review, so this is a real exit from
    // the armed confirmation too - the chip must stop glowing with it.
    if (queue.length === 0) {
      arm(false);
      setReviewing(false);
    }
  }, [queue.length]);

  const existing = useQuery({
    queryKey: ["pbi-tc-titles", org, pbiId],
    queryFn: () => unwrap(commands.pbiTestCases(org, pbiId)),
    retry: false,
    // Cached-first: a tab flip inside a minute renders instantly from
    // cache; after that the cached list still paints while a background
    // refetch updates it. Refresh buttons and mutation invalidations
    // bypass this and always hit the network.
    staleTime: 60_000,
  });
  const existingCases = useMemo(
    () => (existing.data ?? []).map((t) => ({ id: t.id, title: t.title })),
    [existing.data],
  );

  // Diff-preview (spec EDT-B): as soon as the queue holds an UPDATE,
  // fetch the current server values for every queued update in one batch
  // so rows show what will actually change - while the person is still
  // editing, not only once the review gate opens. One request per
  // distinct id set, cached a minute, so an in-place edit re-diffs
  // against the copy already here. Failure degrades to "diff unavailable"
  // - it never blocks submitting.
  const updateIds = queue
    .map((tc) => tc.update_id)
    .filter((x): x is number => x != null);
  const currentCases = useQuery({
    queryKey: ["diff-cases", org, [...updateIds].sort(), prefs.moduleRef, prefs.preconditionsRef],
    queryFn: () =>
      unwrap(commands.testCasesByIds(org, updateIds, prefs.moduleRef, prefs.preconditionsRef)),
    enabled: updateIds.length > 0,
    staleTime: 60_000,
    retry: false,
  });
  const [expandedDiffs, setExpandedDiffs] = useState<Set<number>>(new Set());
  const toggleDiff = useCallback(
    (i: number) =>
      setExpandedDiffs((s) => {
        const next = new Set(s);
        if (next.has(i)) next.delete(i);
        else next.add(i);
        return next;
      }),
    [],
  );
  // One row at a time is editable in place; queue-length changes (remove,
  // import, submit) shift indices, so any of them closes the editor.
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  // Whether the browser report has been opened this session - see the
  // re-export effect below.
  const [reportOpen, setReportOpen] = useState(false);
  useEffect(() => {
    setEditingIdx(null);
  }, [queue.length]);

  // Per-row steps preview (collapsed by default): check what will actually
  // be written before submitting, for creates and updates alike.
  const [expandedSteps, setExpandedSteps] = useState<Set<number>>(new Set());
  const toggleSteps = useCallback(
    (i: number) =>
      setExpandedSteps((s) => {
        const next = new Set(s);
        if (next.has(i)) next.delete(i);
        else next.add(i);
        return next;
      }),
    [],
  );


  // Share-for-review: the draft travels through ADO as a PBI attachment
  // (one-time-use link; nothing is created in ADO). The link lands on the
  // clipboard, ready for Teams.
  const share = useMutation({
    mutationFn: async () => {
      const r = await commands.shareQueue(org, project, pbiId, queue);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: (link) => {
      copyText(link)
        .then(() => toast.success("Share link copied - send it to your reviewer."))
        .catch(() => toast.success(`Share link ready: ${link}`));
    },
    onError: (e) => toast.error(`Could not share: ${e.message}`),
  });

  const exportJson = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: exportPathFor("test-case-queue.json"),
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      rememberExportPath(path);
      const r = await commands.exportQueueJson(path, queue);
      if (r.status === "error") throw new Error(r.error);
      toast.success("Queue exported.");
    },
    onError: (e) => toast.error(`Export failed: ${e.message}`),
  });

  // v1's "View": render to a temp file and open the browser - no download.
  //
  // The draft page, not the one used for cases that already exist in Azure
  // DevOps: here every case gets a comment box (a draft has no work item id
  // to key an app-side note by), the text is the case's own `comment` -
  // the same field the card below edits - and it is written back into the
  // JSON file the case came from.
  const viewHtml = useMutation({
    mutationFn: async () => {
      const r = await commands.viewDraftHtml(
        queue,
        `PBI #${pbiId}`,
        ownerPaths(queue, watches),
        // Each row's key and this PBI ride on the page, so a comment typed
        // there comes back to exactly that row - and to nothing when the
        // page is for a PBI no longer on screen.
        keysFor(queue),
        pbiId,
        watches.map((w) => ({
          path: w.path,
          label: fileName(w.path),
          comment: w.comment ?? "",
          specs: w.specs ?? [],
        })),
        // Read at click time so the page opens in the theme in front of
        // the user; it carries both schemes and its own switch.
        pagePalette(),
      );
      if (r.status === "error") throw new Error(r.error);
    },
    onSuccess: () => setReportOpen(true),
    onError: (e) => toast.error(`Could not open the report: ${e.message}`),
  });

  // Keep an already-open report in step with the queue.
  //
  // The page is a file on disk: nothing pushes to it, so the app rewrites
  // the same file and bumps a revision the page polls; the page then
  // pulls the fresh content over the loopback listener and swaps itself
  // in place - no reload, no lost scroll position.
  //
  // REFRESH, never VIEW: this used to call the same command as the
  // button, and the open_path at the end of that meant every comment
  // save and every queue change opened ANOTHER browser tab on a file the
  // reviewer already had open.
  //
  // Only after they have opened it once: re-exporting for a report
  // nobody asked for would write a temp file on every keystroke.
  useEffect(() => {
    if (!reportOpen || queue.length === 0) return;
    // Debounced, because a rename or a bulk edit lands as a burst of
    // queue updates and each one would otherwise rewrite the file.
    const t = window.setTimeout(() => {
      void commands
        .refreshDraftHtml(
          queue,
          `PBI #${pbiId}`,
          ownerPaths(queue, watches),
          // Each row's key and this PBI ride on the page, so a comment typed
          // there comes back to exactly that row - and to nothing when the
          // page is for a PBI no longer on screen.
          keysFor(queue),
          pbiId,
          watches.map((w) => ({
            path: w.path,
            label: fileName(w.path),
            comment: w.comment ?? "",
            specs: w.specs ?? [],
          })),
          pagePalette(),
        )
        // Silent on failure: this is a background refresh of something
        // the developer is not necessarily looking at, and the report
        // they have is still readable.
        .catch(() => {});
    }, 800);
    return () => window.clearTimeout(t);
    // watches/pbiId are read, not depended on: a watch comment changing
    // is not a reason to rewrite the page.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queue, reportOpen]);


  // A comment typed in that page comes back here, so the card and the
  // browser tab never disagree. The file is already written by the time
  // this fires - this is only the app catching up.
  useEffect(() => {
    const un = events.draftCommentSaved.listen((e) => {
      const { key, pbi_id, text } = e.payload;
      // A page for a PBI this screen is not showing: its rows are not here.
      if (pbi_id !== pbiId || !key) return;
      // The full occurrence key: two rows sharing a title are two rows.
      setQueue((q) => {
        const keys = keysFor(q);
        return q.map((c, i) => (keys[i] === key ? { ...c, comment: text } : c));
      });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [setQueue, pbiId]);

  const submit = useMutation({
    mutationFn: async () => {
      // The buttons are disabled while a hold stands; this is the backstop.
      if (holdActive) throw new Error(HOLD_REFUSAL);
      // Rows this submit has nothing to write for. The review gate prints
      // "no-op - nothing will change" per row; on a queue of 81 imported
      // cases where ten had really changed, writing anyway meant 71
      // pointless PATCHes plus 71 x the half-second pacing gap - a minute
      // of waiting and rate-limit budget to say nothing.
      //
      // The check needs the server's CURRENT values, fetched HERE at
      // submit time: the review's cached baseline can be minutes old, and
      // the pure-update fast path (no check-the-PBI stage) can reach this
      // point before the review's own diff fetch has landed at all.
      // Fails SAFE in both directions that matter: only an UPDATE can be
      // a no-op (a create always writes), and an unreadable baseline
      // skips nothing - "we could not check" must never read as "nothing
      // to do".
      const idsToCheck = queue
        .map((tc) => tc.update_id)
        .filter((x): x is number => x != null);
      const freshById = new Map<number, TestCaseFull>();
      if (idsToCheck.length > 0) {
        try {
          const fresh = await unwrap(
            commands.testCasesByIds(org, idsToCheck, prefs.moduleRef, prefs.preconditionsRef),
          );
          for (const c of fresh) freshById.set(c.id, c);
        } catch {
          // fail safe: skip nothing
        }
      }
      const noopNow = (tc: TestCase): boolean => {
        if (tc.update_id == null) return false;
        const cur = freshById.get(tc.update_id);
        if (!cur) return false;
        return diffCase(tc, cur, {
          moduleRef: prefs.moduleRef,
          preconditionsRef: prefs.preconditionsRef,
        }).noop;
      };
      // Everything that actually has something to write. The skipped rows
      // are still in the queue and still on screen - they are just not
      // sent, and they stay there afterwards like every other row.
      //
      // IN THE ORDER ON SCREEN, which is the order the user chose: the
      // upload order is the suite order, and the "Order:" bar above the
      // queue sorts the queue itself. This used to re-sort by tester_order
      // here, which silently overrode a queue laid out any other way -
      // pick "Down the spec", and the cases still landed in the suite in
      // tester order with nothing on screen to explain it.
      // `sent` below is THIS list, and every result index and the file
      // stamping key off it.
      const toSend = queue.filter((tc) => !noopNow(tc));
      const skipped = queue.filter((tc) => noopNow(tc)).length;
      // The rows left out still hold their place in the file. The order
      // set after the upload needs them, or one new case in a re-uploaded
      // file is ordered as if it were the only one - to the top of the
      // suite. `index` is the row's place on screen before the filter.
      const orderHint: OrderHint[] = queue.flatMap((tc, index) =>
        tc.update_id != null && noopNow(tc)
          ? [
              {
                index,
                id: tc.update_id,
                spec_order: tc.spec_order ?? null,
                tester_order: tc.tester_order ?? null,
                area: tc.area ?? "",
              },
            ]
          : [],
      );
      if (toSend.length === 0) {
        return { results: [], sent: [], sentFor: pbiId, skipped, diffs: [] };
      }
      // What each sent update is about to change, from the same fresh
      // baseline - kept for the "Copy changes" note, since after the write
      // the server already holds the new values.
      const diffs: (CaseDiff | null)[] = toSend.map((tc) => {
        const cur = tc.update_id != null ? freshById.get(tc.update_id) : undefined;
        return cur
          ? diffCase(tc, cur, { moduleRef: prefs.moduleRef, preconditionsRef: prefs.preconditionsRef })
          : null;
      });
      // The lower bound for a later "what did this upload create" check,
      // early by the same five minutes Rust allows for clock drift.
      const since = new Date(Date.now() - 5 * 60_000).toISOString();
      const run = submitStarted(org, pbiId, toSend.length);
      if (run == null) {
        throw new Error("another upload is still running. Wait for it to finish, then try again.");
      }
      let unProgress: (() => void) | undefined;
      let unPlan: (() => void) | undefined;
      let unSuite: (() => void) | undefined;
      let unRunOrder: (() => void) | undefined;
      try {
        // Inside the try: a listen that rejects must still reach the
        // finally, or the phase stays on "Processing" until a restart.
        unProgress = await events.submitProgress.listen((e) => {
          submitProgressed(e.payload.index + 1, e.payload.total, e.payload.title);
        });
        // Fired before the upload loop when the PBI had no test plan and one
        // was created on the fly - surface it so plans never appear silently.
        unPlan = await events.planCreated.listen((e) => {
          toast.info(
            `This PBI had no test plan - created "${e.payload.plan_name}" first, now uploading the test cases.`,
          );
        });
        // The PBI had no requirement suite and one could not be created -
        // typically no permission on the plans for its area. The cases still
        // upload and link to the PBI, but Run Tests will not see them until a
        // suite exists, so this stays up long enough to be read. It used to be
        // a warning in the log only, which is how 197 cases once landed with
        // no suite and nobody knew.
        unSuite = await events.suiteNotCreated.listen((e) => {
          toast.warning(`Uploaded, but no test suite could be created for this PBI. ${e.payload.reason}`, {
            duration: 30000,
          });
        });
        // The upload went through, but the suite's spec order or the
        // suggested run order was not saved. The reason says which, and
        // that Suite Management can set it.
        unRunOrder = await events.runOrderNotSaved.listen((e) => {
          toast.warning(e.payload.reason);
        });
        const r = await commands.submitQueue(
          org,
          project,
          pbiId,
          toSend,
          prefs.moduleRef,
          prefs.preconditionsRef,
          // Always the PBI's own area and iteration. These were dropdown
          // overrides once; cases landing anywhere but beside their PBI
          // was never wanted, so null (= inherit) is now the only value.
          null,
          null,
          orderHint,
        );
        if (r.status === "error") throw new Error(r.error);
        // The outcome is applied HERE, inside the promise, not in
        // onSuccess: the hook's callbacks die with the component, and the
        // person who navigated away mid-upload still needs the created
        // cases in their queue to carry their new ids when the loop
        // finishes. `sent` is the FILTERED list: every result index is an
        // index into it, and keepUploaded matches on that list.
        applyOutcome({ results: r.data, sent: toSend, sentFor: pbiId, skipped, prevQueue: queue, since });
        return { results: r.data, sent: toSend, sentFor: pbiId, skipped, diffs };
      } finally {
        // Inside the promise for the same reason: onSettled may never run.
        detach(unProgress);
        detach(unPlan);
        detach(unSuite);
        detach(unRunOrder);
        submitFinished(run);
      }
    },
    // Only the parts a mounted screen can show. Everything that must
    // happen - new ids, toasts, invalidations - already ran inside the
    // mutation itself, because these callbacks die with the component.
    onSuccess: ({ results, sent, sentFor, diffs }) => {
      setResults(results);
      setChangeNotes(
        hasTesterNotes(results, diffs) ? testerNotes({ pbiId: sentFor, sent, results, diffs }) : null,
      );
      // The glow says "about to create on this PBI" - once the write has
      // gone through there is nothing left to warn about.
      arm(false);
      setReviewing(false);
    },
    onError: (e) => toast.error(`Submit failed: ${e.message}`),
  });

  /** Ask ADO what titles it already has, and remember them.
   *
   * An unreachable ADO falls back to the cache rather than blocking: the
   * submit itself would surface the outage anyway, and refusing to open a
   * review because a check could not run helps nobody. */
  const refreshTitles = useCallback(async (): Promise<string[]> => {
    setCheckingDups(true);
    let titles = existingCases.map((t) => t.title);
    try {
      const fresh = await existing.refetch();
      if (fresh.data) titles = fresh.data.map((t) => t.title);
    } catch {
      // keep the cached list
    }
    setFreshTitles(titles);
    setCheckingDups(false);
    return titles;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [existingCases]);

  /** Which creates clash with a title already on the PBI, minus the ones
   * the user has looked at and accepted.
   *
   * Derived, not stored, so editing or removing a row during the review
   * updates it without another round trip - remove the offending case and
   * the warning clears itself. */
  const dupsPending = useMemo(() => {
    if (!freshTitles) return [];
    const have = new Set(freshTitles.map((t) => t.trim().toLowerCase()));
    const ok = new Set(acceptedDups.map((t) => t.trim().toLowerCase()));
    return queue
      .filter((tc) => {
        const key = tc.title.trim().toLowerCase();
        return tc.update_id == null && have.has(key) && !ok.has(key);
      })
      .map((tc) => tc.title);
  }, [queue, freshTitles, acceptedDups]);

  /** Open the review: arm the PBI check for a queue that creates anything,
   * and find out about duplicates now rather than after the last click. */
  const openReview = () => {
    setReviewing(true);
    if (!pureUpdates) arm(true);
    void refreshTitles();
  };

  /** The write. Re-verifies first: moving the check to the review opened a
   * window - the queue can be edited, and someone else can create the same
   * case, while a hundred cases are being read. It stops only for a clash
   * nobody has been shown, so the common path never asks twice. */
  const guardedSubmit = async () => {
    if (holdActive) {
      toast.warning(HOLD_REFUSAL);
      return;
    }
    const titles = await refreshTitles();
    const have = new Set(titles.map((t) => t.trim().toLowerCase()));
    const ok = new Set(acceptedDups.map((t) => t.trim().toLowerCase()));
    const surprises = queue.filter((tc) => {
      const key = tc.title.trim().toLowerCase();
      return tc.update_id == null && have.has(key) && !ok.has(key);
    });
    // The panel and the disabled button both read `dupsPending`, which the
    // refresh above has just updated - there is nothing else to set.
    if (surprises.length > 0) return;
    submit.mutate();
  };

  /** Ask Azure DevOps what an interrupted upload actually created. Found
   * cases go through applyOutcome exactly as created results do: id on the
   * row, the file and the notes learn it. The rest were never created and
   * are ordinary queued rows again.
   *
   * Deviation from the brief (controller ruling): `reconcile_upload` cannot
   * tell "not found" from "ambiguous" - a title with more unclaimed matches
   * in Azure DevOps than rows being checked comes back in `ambiguous`
   * rather than `found`. Those titles stay held (a new, smaller hold); a
   * check that fails outright keeps the whole hold as it was.
   *
   * Fix round 1: `ambiguous` names a TITLE once (it comes from a Rust
   * HashSet), not once per held ROW - a hold hit `titles: ["A", "A"]` still
   * only gets `ambiguous: ["A"]` back even though both rows are ambiguous.
   * The new hold is built by keeping every entry of the OLD `h.titles` whose
   * trimmed text is in the answer's ambiguous set, so a repeated title keeps
   * every one of its rows (and `missing` is counted per ROW, not per
   * distinct title) instead of losing all but one to the rebuild. */
  const checkHold = async () => {
    const h = loadHold(org, pbiId);
    if (!h) return;
    setCheckingHold(true);
    try {
      const r = await commands.reconcileUpload(org, project, pbiId, h.since, h.titles);
      if (r.status === "error") {
        toast.error(`Could not check with Azure DevOps: ${r.error} The cases stay marked - try again.`);
        return;
      }
      const { found, ambiguous } = r.data;
      const { results: createdResults, orphans } = reconciledResults(queue, heldRows(queue, h), found);
      const ambiguousSet = new Set(ambiguous.map((t) => t.trim()));
      const stillHeld = h.titles.filter((t) => ambiguousSet.has(t.trim()));
      saveHold(org, pbiId, stillHeld.length > 0 ? { since: h.since, titles: stillHeld, ambiguous: stillHeld } : null);
      if (createdResults.length > 0) {
        applyOutcome({ results: createdResults, sent: queue, sentFor: pbiId, skipped: 0, prevQueue: queue });
      }
      const missing = h.titles.length - found.length - stillHeld.length;
      toast.info(
        `${found.length} of ${h.titles.length} case(s) had been created` +
          (missing > 0 ? `; ${missing} had not and can be uploaded again.` : "."),
      );
      if (stillHeld.length > 0) {
        toast.warning(
          `${stillHeld.length} case(s) still cannot be confirmed - more than one test case with that ` +
            `title exists in Azure DevOps. Check there, then release them here.`,
          { duration: 20000 },
        );
      }
      if (orphans > 0) {
        toast.warning(`${orphans} created case(s) are no longer in the queue - View Test Cases has them.`, {
          duration: 20000,
        });
      }
    } finally {
      setCheckingHold(false);
    }
  };

  /** Everything a finished submit owes the user, wherever they are now.
   * Runs inside the mutation promise, so navigating away cannot skip it. */
  function applyOutcome({
    results,
    sent,
    sentFor,
    skipped,
    prevQueue,
    since,
  }: {
    results: SubmitItemResult[];
    sent: TestCase[];
    sentFor: number;
    skipped: number;
    prevQueue: TestCase[];
    /** When this upload started - set by a submit, absent for a Check. */
    since?: string;
  }) {
    // Only what Azure DevOps confirmed counts as done. "unknown" is neither
    // done nor failed: it may exist, so it is held until someone checks.
    const ok = results.filter((r) => r.action === "created" || r.action === "updated").length;
    const failedCount = results.filter((r) => r.action === "failed").length;
    const unknownCount = results.filter((r) => r.action === "unknown").length;
    const newHold = since ? holdFromResults(results, sent, since) : null;
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
    const outcomes = results.map((r) => ({ index: r.index, action: r.action, id: r.id }));
    void (async () => {
      const known = watches.length > 0 ? watches : loadWatches(org, sentFor);
      // The other half of the promise "the file learns what the submit made
      // real": say it LOUDLY when a created case's id could not be recorded
      // anywhere - no owning file matched it, or there is no file at all.
      // Both duplicate incidents to date were this situation, silent.
      const owners = known.length > 0 ? ownerPaths(prevQueue, known) : prevQueue.map(() => "");
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
        const files = stampFileSlices(prevQueue, ownerPaths(prevQueue, known), sent, outcomes);
        for (const [path, f] of files) {
          if (!f.changed) continue;
          const r = await commands.saveDraftCases(path, f.edits);
          if (r.status === "error") {
            toast.warning(
              `Uploaded, but ${fileName(path)} could not be updated with the new ids: ${r.error}. ` +
                `Importing it again would create duplicates - fix the file before re-importing.`,
              { duration: 20000 },
            );
            continue;
          }
          // Storage always: the mount that started this submit may be gone,
          // and a setter on an unmounted screen never runs its persist step.
          // Then the screen showing this queue NOW, if any - not this
          // closure's own callback, which may belong to that gone mount.
          const fields = { stamp: r.data, snapshot: f.slice };
          saveWatches(org, sentFor, patchWatch(loadWatches(org, sentFor), path, fields));
          queueWriterFor(org, sentFor)?.patchWatch?.(path, fields);
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

  /** Stop listening without letting jsdom's missing event internals turn a
   * cleanup into an unhandled rejection - same shape as App's detach. */
  function detach(unlisten: (() => void) | undefined): void {
    if (!unlisten) return;
    try {
      void Promise.resolve(unlisten() as unknown).catch(() => {});
    } catch {
      // threw synchronously - same conclusion
    }
  }

  // Same occurrence-aware keys the file sync reports changes under, so a
  // second case sharing a title still lights up its own row.
  const rowKeys = useMemo(() => keysFor(queue), [queue]);
  const held = useMemo(() => heldRows(queue, hold), [queue, hold]);
  const ambiguous = useMemo(() => ambiguousRows(queue, hold), [queue, hold]);
  // Fix round 1 (controller ruling): a hold refuses uploads only while one
  // of its rows is still actually in the queue. Removing or renaming every
  // held row lifts the refusal; the hold itself is left exactly as it was.
  //
  // Fix round 2 (CRITICAL): this used to also clear the hold from an effect
  // watching (hold, queue) once no row matched it - but `useQueue`
  // (src/hooks/useQueue.ts:60-93) delivers a PBI switch's new scope one
  // render BEFORE its reload: `hold` already reads the new PBI (it is keyed
  // on `pbiId` alone) while `queue` is still the OLD PBI's rows, or the
  // tour fixture's, which can happen to pair with a real PBI. In that one
  // render nothing in `queue` matches the new PBI's hold, so the effect
  // deleted a hold whose rows simply had not arrived yet. There is no such
  // effect now: a hold whose rows are absent is merely INERT (`holdActive`
  // below is false, so nothing is blocked), and if a same-titled row comes
  // back the hold applies again exactly as it was. Only a successful Check
  // and a confirmed Release ever remove one.
  const holdActive = hold != null && held.some(Boolean);
  const [releaseConfirm, setReleaseConfirm] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);
  const [bulkOpen, setBulkOpen] = useState(false);

  // Selection is by POSITION, like Power Rename: drafts have no id, and a
  // bulk edit can make two share a title, so the title is not an identity
  // that survives the operation being applied. Any change in queue LENGTH
  // drops the selection - after a removal or a file sync the indices point
  // at different cases, and a stale selection silently bulk-edits the
  // wrong rows.
  const [selected, setSelected] = useState<Set<number>>(new Set());
  // Positions are only meaningful while the rows exist. When the queue
  // shrinks beneath a selection (Remove all, an import that replaces the
  // queue, a single removal) the indices past its end are dropped.
  useEffect(() => {
    setSelected((prev) => {
      if (![...prev].some((i) => i >= queue.length)) return prev;
      return new Set([...prev].filter((i) => i < queue.length));
    });
  }, [queue.length]);
  // The shift-click anchor is only ever read inside toggleSelect, so it
  // lives in a ref: that keeps the callback's identity stable, which is
  // what lets the rows below stay memoised.
  const selAnchor = useRef<number | null>(null);
  useEffect(() => {
    setSelected(new Set());
    selAnchor.current = null;
  }, [queue.length]);

  const toggleSelect = useCallback((i: number, shift: boolean) => {
    const anchor = selAnchor.current;
    setSelected((s) => {
      const next = new Set(s);
      if (shift && anchor != null) {
        const [lo, hi] = anchor < i ? [anchor, i] : [i, anchor];
        for (let k = lo; k <= hi; k++) next.add(k);
      } else if (next.has(i)) {
        next.delete(i);
      } else {
        next.add(i);
      }
      return next;
    });
    selAnchor.current = i;
  }, []);

  /** Write bulk changes back into the files the cases came from, so the
   * file says what the queue says - otherwise the next external save of
   * that file would quietly revert the bulk edit.
   *
   * `next` is aligned with `prev` (null = removed); ownership is computed
   * from the PRE-edit queue, because ownership is matched by title-derived
   * keys and a rename is exactly the operation that breaks that match.
   * Only files owning a changed case are written; each write returns the
   * file's new fingerprint, and the watch snapshot moves forward with it
   * so the watcher stays silent about our own write. */
  const writeBackOwned = async (
    prev: TestCase[],
    next: (TestCase | null)[],
    changed: Set<number>,
  ) => {
    if (watches.length === 0) return;
    const owners = ownerPaths(prev, watches);
    // Per file: one edit per owned row IN QUEUE ORDER - the row BEFORE the
    // edit (how the file finds its own copy - a rename changes the title)
    // and after it (null = removed). Removals stay interleaved where they
    // happened, so the Nth same-titled row claims the Nth same-titled entry.
    // The file keeps everything else it holds.
    const files = new Map<string, { slice: TestCase[]; edits: DraftEdit[]; touched: boolean }>();
    prev.forEach((before, i) => {
      const p = owners[i];
      if (!p) return;
      const f = files.get(p) ?? { slice: [], edits: [], touched: false };
      const after = next[i];
      if (after) f.slice.push(after);
      f.edits.push({ before, after });
      if (changed.has(i)) f.touched = true;
      files.set(p, f);
    });
    for (const [path, f] of files) {
      if (!f.touched) continue;
      const r = await commands.saveDraftCases(path, f.edits);
      if (r.status === "error") {
        // The queue HAS changed - saying so beats pretending nothing did.
        toast.warning(
          `The queue was updated, but ${fileName(path)} could not be: ${r.error}. ` +
            `The file still has the old values.`,
          { duration: 15000 },
        );
      } else {
        onWatchPatched?.(path, { stamp: r.data, snapshot: f.slice });
      }
    }
  };

  const bulkApply = async (edit: (tc: TestCase) => TestCase) => {
    const prev = queue;
    const chosen = new Set(selected);
    const next: (TestCase | null)[] = prev.map((tc, i) => (chosen.has(i) ? edit(tc) : tc));
    setQueue(next.filter((x): x is TestCase => x != null));
    await writeBackOwned(prev, next, chosen);
    toast.success(`Updated ${chosen.size} queued case${chosen.size === 1 ? "" : "s"}.`);
  };

  const bulkRemove = async () => {
    const prev = queue;
    const removing = new Set(selected);
    const kept = prev.filter((_, i) => !removing.has(i));
    setQueue(kept);
    await writeBackOwned(
      prev,
      prev.map((tc, i) => (removing.has(i) ? null : tc)),
      removing,
    );
    if (kept.length === 0) onQueueCleared?.();
    toast.info(`Removed ${removing.size} queued case${removing.size === 1 ? "" : "s"}.`);
  };

  /** Renaming drafts touches only this list and the files they came from,
   *  so undo here can never fail against Azure DevOps.
   *
   *  Rows are matched back to drafts by POSITION. A draft has no work item
   *  id and a rename can make two of them share a title, so the title is
   *  not an identity that survives the very operation being applied. With
   *  a selection active, the dialog covers just the selected rows - the
   *  scope list maps the dialog's row indices back to queue positions. */
  // Only positions that still exist: the selection is a set of indices,
  // and "Remove all" with rows selected emptied the queue under it - the
  // white window of 1.23.11 was `queue[qi].update_id` on the next render.
  const renameScope =
    selected.size > 0
      ? [...selected].filter((i) => i < queue.length).sort((a, b) => a - b)
      : queue.map((_, i) => i);
  const renameTarget: RenameTarget = {
    label:
      selected.size > 0
        ? `${selected.size} selected draft${selected.size === 1 ? "" : "s"}`
        : "the queued drafts",
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
        changed.add(qi);
      }
      setQueue(next.filter((x): x is TestCase => x != null));
      // The rename reaches the files too - Undo comes back through here
      // with the rows reversed, so it writes the files back as well.
      await writeBackOwned(prev, next, changed);
      return failed;
    },
  };

  const problems = useMemo(() => queue.map((tc) => validateCase(tc)), [queue]);
  const duplicates = useMemo(
    () => queue.map((tc) => duplicateWarning(tc, existingCases)),
    [queue, existingCases],
  );

  // Row callbacks. All stable: a row is memoised on its props, and a
  // callback that changed identity every render would re-render every
  // row on every render - which is exactly what made a 100-case queue
  // slow to switch to. The two that need the current queue and file
  // list read them through a ref instead of closing over them.
  const latest = useRef({ queue, writeBackOwned });
  latest.current = { queue, writeBackOwned };
  const toggleEdit = useCallback((i: number) => setEditingIdx((cur) => (cur === i ? null : i)), []);
  const cancelEdit = useCallback(() => setEditingIdx(null), []);
  // The owning FILE follows a single removal exactly as it follows a bulk
  // one: otherwise the next outside save of that file sees the case in
  // both snapshots, not in the queue, and puts it back.
  const removeRow = useCallback(
    (i: number) => {
      const { queue: prev, writeBackOwned: writeBack } = latest.current;
      setQueue((q) => q.filter((_, j) => j !== i));
      void writeBack(
        prev,
        prev.map((t, j) => (j === i ? null : t)),
        new Set([i]),
      );
    },
    [setQueue],
  );
  const saveRow = useCallback(
    (i: number, next: TestCase) => {
      const { queue: prev, writeBackOwned: writeBack } = latest.current;
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
  const hasBlockers = problems.some(Boolean);

  // What the primary action is about to do, in words. Lifted out of the
  // review branch because the floating copy has to say the same thing.
  const updateCount = queue.filter((tc) => tc.update_id != null).length;
  const createCount = queue.length - updateCount;
  const actionLabel = [
    createCount > 0 && `create ${createCount}`,
    updateCount > 0 && `update ${updateCount}`,
  ]
    .filter(Boolean)
    .join(" · ");
  // -24px so the real row has to be properly in view, not just peeking
  // over the bottom edge, before the floating copy stands down. The ref
  // is the hook's own callback ref, because the action row is NOT in the
  // page while the queue is empty - and an empty queue growing long is
  // exactly the flow the floating copy exists for.
  const [actionRow, actionOnScreen] = useOnScreen("0px 0px -24px 0px");

  // The same row, held as a plain node so it can be scrolled to. The hook
  // above keeps its node in state for the observer and does not hand it
  // back, and reading it out of there would make this depend on when the
  // observer happens to re-render.
  const actionRowEl = useRef<HTMLDivElement | null>(null);

  // Opening the review, and arming the confirmation, both grow this row -
  // and on a long queue that put the next button below the fold. The
  // floating copy covers the first case but stands down for the armed
  // warning on purpose, since that one has to be read. So the row comes to
  // the reader. `block: "end"` rather than "center": the tail of the
  // review content stays visible above it instead of the button landing
  // mid-screen with the content it belongs to pushed off the top.
  //
  // Not on mount - only when one of these turns on. Moving the page under
  // someone who has not asked for anything is worse than the scroll it
  // saves.
  useEffect(() => {
    if (!reviewing && !armed) return;
    // Smoothly, so the page visibly travels and the reader keeps their
    // place: a jump saves the scrolling and spends it again on working out
    // where they were thrown to. Except under prefers-reduced-motion,
    // which this app honours everywhere else and which is not a
    // preference about taste.
    const still = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
    actionRowEl.current?.scrollIntoView({
      block: "end",
      behavior: still ? "auto" : "smooth",
    });
  }, [reviewing, armed]);

  // Diffing is word-level and runs per row - recomputing all of it on
  // every keystroke/selection render made an 80-case review sluggish.
  // Recomputed only when the queue, the fetched originals, or the field
  // refs actually change. The dependency is `currentCases.data` - which
  // react-query keeps referentially stable across renders that don't
  // change the result - and the id lookup Map is built INSIDE the
  // factory: a Map built per render would have a new identity every
  // time and defeat the memo. (The submit path builds its own fresh
  // baseline at submit time; see the mutation.)
  const reviewRows = useMemo(() => {
    const byId = new Map((currentCases.data ?? []).map((c) => [c.id, c]));
    return queue.map((tc) => {
      const cur = tc.update_id != null ? byId.get(tc.update_id) : undefined;
      const diff = cur
        ? diffCase(tc, cur, { moduleRef: prefs.moduleRef, preconditionsRef: prefs.preconditionsRef })
        : null;
      const diffFailed = tc.update_id != null && !cur && currentCases.isError;
      return { diff, diffFailed };
    });
  }, [queue, currentCases.data, prefs.moduleRef, prefs.preconditionsRef, currentCases.isError]);

  // An empty queue is not a queue - the whole section stays out of the
  // way until a case exists. The "Queue for PBI" header, its five action
  // buttons and the Review button all act on cases, and with zero cases
  // every one of them was disabled furniture; even the old "Nothing
  // queued yet" island was a paragraph explaining an absence. So: Manual
  // Entry (no recents wiring) renders nothing below the form, and the
  // Import tab shows Recent JSON Imports alone - its way back in. Kept
  // whole mid-submit (progress/results/reviewing): the user can remove
  // rows while an upload runs, and the progress bar and the results must
  // not vanish with them.
  if (queue.length === 0 && !progress && !results && !reviewing) {
    if (!onOpenRecent) return null;
    return (
      <section className="space-y-2 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">Recent JSON Imports</h2>
        {recentImports.length > 0 ? (
          <>
            <p className="text-xs text-faint">
              Reopen a file to import its cases again - the file is re-read as it is now.
            </p>
            {recentImports.map((r) => (
              <RecentImportRow
                key={r.path}
                path={r.path}
                when={r.when}
                onOpen={() => onOpenRecent(r.path)}
                onForget={() => onForgetRecent?.(r.path)}
              />
            ))}
          </>
        ) : (
          <p className="text-xs text-faint">
            Files you import appear here for quick reopening. Import a JSON file above to
            start a queue.
          </p>
        )}
      </section>
    );
  }

  return (
    <section className="space-y-3 rounded-md border border-border bg-surface p-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold text-text">
          Queue for PBI #{pbiId} ({queue.length} queued)
        </h2>
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={queue.length === 0 || viewHtml.isPending}
            onClick={() => viewHtml.mutate()}
          >
            <IconOpenInBrowser aria-hidden />
            View in browser
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={queue.length === 0 || share.isPending || !online}
            title={online ? "Upload the draft as a one-time share link a teammate can import for review" : OFFLINE_HINT}
            onClick={() => share.mutate()}
          >
            <IconShare aria-hidden />
            {share.isPending ? "Sharing" : "Share for review"}
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={queue.length === 0}
            onClick={() => exportJson.mutate()}
          >
            <IconExport aria-hidden />
            Export JSON
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={queue.length === 0 || submit.isPending}
            onClick={() => setRenameOpen(true)}
          >
            <IconRename aria-hidden />
            Rename
          </Button>
          {/* Last on purpose, and red on approach: this is the destroy
              action in a row of build actions, so it sits at the far end
              where a fast hand does not land on it by habit, and announces
              itself before the click. */}
          <Button
            variant="outline"
            size="sm"
            className="hover:border-danger hover:bg-danger/10 hover:text-danger"
            disabled={queue.length === 0 || submit.isPending}
            onClick={() => {
              const n = queue.length;
              setQueue([]);
              setFailedRows(new Set());
              setUploadedIds(new Set());
              onQueueCleared?.();
              toast.info(`Removed ${n} queued case${n === 1 ? "" : "s"}.`);
            }}
          >
            <IconRemove aria-hidden />
            Remove all
          </Button>
        </div>
      </div>

      {renameOpen && (
        <PowerRenameDialog
          target={renameTarget}
          onClose={() => setRenameOpen(false)}
          onDone={() => {}}
        />
      )}

      {bulkOpen && (
        <QueueBulkEditDialog
          org={org}
          project={project}
          count={selected.size}
          onClose={() => setBulkOpen(false)}
          onApply={(edit) => {
            setBulkOpen(false);
            void bulkApply(edit);
          }}
        />
      )}

      {/* The two readings of an optimized file: grouped so a tester changes
          environment as little as possible, or walking down the spec so a
          reviewer scrolls the document and the queue together. Only offered
          when EVERY case carries the field - a partial sort would interleave
          ordered cases with ones that have no opinion, which is neither
          reading. Reordering is real, not a view: the queue's order is the
          order the cases are created in. */}
      {/* Shown as soon as ANY case carries an order - a bar that only
          appears when everything is already stamped would hide the reason
          the buttons are disabled, which is the one thing a mixed queue
          needs explained. */}
      {queue.length > 1 &&
        (["tester_order", "spec_order"] as const).some((k) => queue.some((tc) => tc[k] != null)) && (
          <div className="flex items-center gap-2 text-xs text-muted">
            <span>Order:</span>
            {(
              [
                ["tester_order", "For testing", "Cases sharing a setup run together"],
                ["spec_order", "Down the spec", "Cases follow the specification document"],
              ] as const
            ).map(([key, label, hint]) => {
              const available = queue.every((tc) => tc[key] != null);
              const active =
                available && queue.every((tc, i) => i === 0 || (queue[i - 1][key] ?? 0) <= (tc[key] ?? 0));
              return (
                <Button
                  key={key}
                  variant="outline"
                  size="sm"
                  disabled={!available}
                  aria-pressed={active}
                  title={available ? hint : "Not every queued case has this order yet - it's added when an AI assistant optimizes the draft"}
                  className={active ? "border-accent text-accent" : undefined}
                  onClick={() =>
                    setQueue((q) => [...q].sort((a, b) => (a[key] ?? 0) - (b[key] ?? 0)))
                  }
                >
                  {label}
                </Button>
              );
            })}
          </div>
        )}

      {/* No empty-state island: with zero cases (and no submit running)
          the whole section early-returns above, so this point is only
          reached with cases to show or a submit in flight. */}

      {/* Bulk actions over a selection. Shift+click a checkbox to select a
          range. Every action here also updates the .json file each case
          came from - the file and the queue must not disagree. */}
      {queue.length > 0 && (
        <div className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-surface-2/50 px-3 py-1.5 text-xs">
          <label className="flex items-center gap-2 text-muted">
            <Checkbox
              ariaLabel="Select all queued cases"
              checked={selected.size === queue.length && queue.length > 0}
              onCheckedChange={(v) =>
                setSelected(v ? new Set(queue.map((_, i) => i)) : new Set())
              }
            />
            {selected.size > 0
              ? `${selected.size} of ${queue.length} selected`
              : "Select cases for bulk actions"}
          </label>
          {selected.size > 0 && (
            <>
              <Button variant="outline" size="sm" onClick={() => setBulkOpen(true)}>
                <IconConfirm aria-hidden />
                Bulk edit
              </Button>
              <Button variant="outline" size="sm" onClick={() => setRenameOpen(true)}>
                <IconRename aria-hidden />
                Rename {selected.size}
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="hover:border-danger hover:bg-danger/10 hover:text-danger"
                onClick={() => bulkRemove()}
              >
                <IconRemove aria-hidden />
                Remove {selected.size}
              </Button>
            </>
          )}
        </div>
      )}

      {queue.length > 0 && (
        <ul className="space-y-1">
          {queue.map((tc, i) => {
            const { diff, diffFailed } = reviewRows[i];
            return (
              <QueueRow
                key={i}
                tc={tc}
                index={i}
                org={org}
                project={project}
                isSelected={selected.has(i)}
                stepsOpen={expandedSteps.has(i)}
                diffOpen={expandedDiffs.has(i)}
                editing={editingIdx === i}
                failed={failedRows.has(rowKeys[i])}
                uploaded={tc.update_id != null && uploadedIds.has(tc.update_id)}
                held={held[i]}
                ambiguous={ambiguous[i]}
                touched={flash?.[rowKeys[i]]}
                reviewing={reviewing}
                problem={problems[i]}
                duplicate={duplicates[i]}
                diff={diff}
                diffFailed={diffFailed}
                busy={submit.isPending}
                onToggleSelect={toggleSelect}
                onToggleSteps={toggleSteps}
                onToggleDiff={toggleDiff}
                onToggleEdit={toggleEdit}
                onRemove={removeRow}
                onSave={saveRow}
                onCancelEdit={cancelEdit}
              />
            );
          })}
        </ul>
      )}

      {/* The same glowing bar the test-suite scan uses. Before the first
          batch answers there is no count to show - the suite is being
          resolved and the batch is in flight - so it sweeps; once a batch
          lands it fills to the count. */}
      {progress && (
        <ScanProgress
          label={progress.done === 0 ? "Processing the upload" : "Uploading"}
          done={progress.done > 0 ? progress.done : undefined}
          total={progress.done > 0 ? progress.total : undefined}
        />
      )}

      {hold && !progress && (
        <div className="space-y-2 rounded-md border border-warning/50 bg-warning/10 p-3">
          {hold.ambiguous && hold.ambiguous.length > 0 ? (
            <>
              <p className="text-sm font-semibold text-text">
                More than one test case with these titles exists in Azure DevOps. Check there, then
                release them here.
              </p>
              <p className="text-xs text-muted">
                Azure DevOps has more matches for {hold.ambiguous.length === 1 ? "this title" : "these titles"}{" "}
                than rows waiting on it, so this app cannot tell which one is genuinely this upload's.
                Once you have confirmed it there, Release lets{" "}
                {hold.ambiguous.length === 1 ? "that case" : "those cases"} be uploaded again.
              </p>
            </>
          ) : (
            <>
              <p className="text-sm font-semibold text-text">
                Outcome unknown for {hold.titles.length} case{hold.titles.length === 1 ? "" : "s"} - check
                before uploading again.
              </p>
              <p className="text-xs text-muted">
                The last upload was interrupted and Azure DevOps could not be asked what it had created.
                Uploading again before checking could create {hold.titles.length === 1 ? "it" : "them"} twice.
              </p>
            </>
          )}
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              disabled={checkingHold || !online}
              title={online ? undefined : OFFLINE_HINT}
              onClick={() => void checkHold()}
            >
              <IconRefresh aria-hidden />
              {checkingHold ? "Checking" : "Check with Azure DevOps"}
            </Button>
            {hold.ambiguous && hold.ambiguous.length > 0 && (
              <Button size="sm" variant="outline" onClick={() => setReleaseConfirm(true)}>
                <IconRelease aria-hidden />
                Release
              </Button>
            )}
          </div>
        </div>
      )}

      {releaseConfirm && (
        <Modal onClose={() => setReleaseConfirm(false)} className="w-full max-w-md space-y-4 p-5">
          <h2 className="text-sm font-semibold text-text">Release held cases?</h2>
          <p className="text-xs text-muted">
            I have checked in Azure DevOps — release these cases so they can be uploaded again?
          </p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={() => setReleaseConfirm(false)}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button
              size="sm"
              onClick={() => {
                saveHold(org, pbiId, null);
                setReleaseConfirm(false);
              }}
            >
              <IconRelease aria-hidden />
              Release
            </Button>
          </div>
        </Modal>
      )}

      <div
        // The tour rings this row, not the Review button inside it: the
        // row is what holds Review before review and the confirm/upload
        // button during it, so the stop makes sense either way.
        data-tour="queue-review"
        ref={(el) => {
          actionRowEl.current = el;
          actionRow(el);
        }}
        className="flex items-center gap-3"
      >
        {progress ? (
          <Button disabled>
            <IconConfirm aria-hidden />
            Processing
          </Button>
        ) : !reviewing ? (
          <Button disabled={queue.length === 0} onClick={openReview}>
            <IconReview aria-hidden />
            Review {queue.length} test case{queue.length === 1 ? "" : "s"}
          </Button>
        ) : (
          <div className="w-full space-y-2">
            {/* The duplicate warning sits ABOVE the way on rather than
                replacing it, and the button below is disabled while it
                stands - the per-row hint it replaced was scrollable-past,
                and 43 duplicates once sailed through that. */}
            {dupsPending.length > 0 && (
              <div className="space-y-2 rounded-md border border-danger/50 bg-danger/10 p-3">
                <p className="text-sm font-semibold text-text">
                  Stopped: {dupsPending.length} case{dupsPending.length === 1 ? "" : "s"} with the
                  same title already exist{dupsPending.length === 1 ? "s" : ""} on PBI #{pbiId}.
                </p>
                <ul className="max-h-32 space-y-0.5 overflow-y-auto text-xs text-muted">
                  {dupsPending.map((t) => (
                    <li key={t}>• {t}</li>
                  ))}
                </ul>
                <p className="text-xs text-muted">
                  If you meant to update the existing cases, import a file that includes their
                  ids (View Test Cases → Export JSON has them). Creating anyway makes
                  duplicates - and cleaning those up needs delete permission.
                </p>
                <div className="flex items-center gap-2">
                  <Button
                    size="sm"
                    onClick={() => {
                      // Backing out here abandons the armed confirmation
                      // too - the chip must stop glowing, same as Back.
                      arm(false);
                      setReviewing(false);
                    }}
                  >
                    <IconBack aria-hidden />
                    Stop — take me back
                  </Button>
                  <Button
                    variant="danger"
                    size="sm"
                    disabled={submit.isPending || !online}
                    // Accepting does NOT write. It records the choice and
                    // frees the button, so the last click is still the one
                    // that creates and still says so.
                    onClick={() => setAcceptedDups((prev) => [...prev, ...dupsPending])}
                  >
                    Create duplicates anyway
                  </Button>
                </div>
              </div>
            )}
            {armed && (
              <div className="rounded-md border border-warning/50 bg-warning/10 p-3">
                <p className="text-sm text-text">
                  Check the highlighted PBI above — everything here will be written to{" "}
                  <span className="font-semibold">PBI #{pbiId}</span>. Removing them afterwards{" "}
                  <span className="font-semibold">needs delete permission</span> in Azure DevOps,
                  and deleting a test case in Azure DevOps is permanent.
                </p>
              </div>
            )}
            <div className="flex items-center gap-2">
              <Button
                disabled={
                  queue.length === 0 ||
                  holdActive ||
                  hasBlockers ||
                  submit.isPending ||
                  !online ||
                  // While the check is in flight, and while a duplicate is
                  // waiting to be looked at.
                  checkingDups ||
                  dupsPending.length > 0
                }
                title={online ? undefined : OFFLINE_HINT}
                onClick={() => void guardedSubmit()}
              >
                <IconConfirm aria-hidden />
                {submit.isPending
                  ? "Processing"
                  : armed
                    ? `Yes — ${actionLabel}`
                    : `Confirm & ${actionLabel || "create 0"}`}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  arm(false);
                  setReviewing(false);
                }}
              >
                <IconBack aria-hidden />
                Back
              </Button>
              {hasBlockers && (
                <span className="text-xs text-danger">Fix the flagged items first.</span>
              )}
              {holdActive && (
                <span className="text-xs text-warning">Check the cases marked "Outcome unknown" first.</span>
              )}
            </div>
          </div>
        )}
      </div>

      {/* What just happened, as a panel rather than a paragraph of coloured
          text pasted under the queue. The headline answers "did it work"
          on its own; the rows below are for finding one case. */}
      {results && (
        <div className="space-y-2 rounded-md border border-border bg-surface-2 p-3">
          <div className="flex items-baseline justify-between gap-3">
            <h3 className="text-sm font-semibold text-text">
              {summariseSubmit(results).headline}
            </h3>
            {/* Dismiss the results once read - the button goes with them,
                and so do the marks on the rows: once the results are
                gone, the queue reads as a plain queue again. */}
            <div className="flex shrink-0 items-center gap-2">
              {/* For the tester: every updated case by id with what changed,
                  then the new ones - so they can tell whether a case they
                  already ran needs running again. */}
              {changeNotes && (
                <Button
                  variant="outline"
                  size="sm"
                  title="Copy the updated test case ids and what changed, to send to a tester"
                  onClick={() => {
                    copyText(changeNotes)
                      .then(() => toast.success("Changes copied."))
                      .catch(() => toast.error("Could not copy to clipboard."));
                  }}
                >
                  <IconCopy aria-hidden />
                  Copy changes
                </Button>
              )}
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  setResults(null);
                  setChangeNotes(null);
                  setFailedRows(new Set());
                  setUploadedIds(new Set());
                }}
              >
                <IconClear aria-hidden />
                Clear results
              </Button>
            </div>
          </div>
          <ul className="space-y-1 text-sm">
            {results.map((r) => (
              <li key={r.index} className="flex items-baseline gap-2">
                <Badge
                  className={cn(
                    "shrink-0",
                    r.action === "created"
                      ? "bg-success/20 text-success"
                      : r.action === "updated" || r.action === "unknown"
                        ? "bg-warning/20 text-warning"
                        : "bg-danger/20 text-danger",
                  )}
                >
                  {r.action === "created"
                    ? "NEW"
                    : r.action === "updated"
                      ? "UPDATED"
                      : r.action === "unknown"
                        ? "UNKNOWN"
                        : "FAILED"}
                </Badge>
                {r.id != null && <span className="id-mono shrink-0 text-faint">#{r.id}</span>}
                <span className="min-w-0 break-words text-text">{r.title}</span>
                {/* A case that WAS written and then hit trouble afterwards
                    still exists in Azure DevOps, so its note is a warning
                    beside it, not a failure badge in front of it. */}
                {r.error && (
                  <span
                    className={cn(
                      "min-w-0 break-words",
                      r.action === "failed" ? "text-danger" : "text-warning",
                    )}
                  >
                    {r.error}
                  </span>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Sticky Collapse all, bottom LEFT like every other screen's, once
          anything in the queue is unfolded (steps, diffs, or the inline
          editor). Portalled because this renders inside AnimatedContent,
          whose GSAP transform would make `fixed` mean the scroll region
          instead of the viewport. */}
      {(expandedSteps.size + expandedDiffs.size > 0 || editingIdx != null) &&
        createPortal(
          /* Left offset clears the sidebar at its CURRENT width - parked at
             left-6 this would sit exactly on the sidebar's Close button. */
          <div
            className="fixed bottom-6 z-40 rounded-full border border-accent bg-bg shadow-2xl transition-[left] duration-200"
            style={{ left: stickyLeftPx(sidebarCollapsed) }}
          >
            <Button
              size="sm"
              variant="ghost"
              className="rounded-full text-text hover:bg-surface-2 hover:text-text"
              onClick={() => {
                setExpandedSteps(new Set());
                setExpandedDiffs(new Set());
                setEditingIdx(null);
              }}
            >
              <IconCollapseAll aria-hidden />
              Collapse all (
              {expandedSteps.size + expandedDiffs.size + (editingIdx != null ? 1 : 0)})
            </Button>
          </div>,
          document.body,
        )}

      {/* The same main button, following the user down a long queue.
          Bottom RIGHT, keeping clear of Collapse all on the left. It does
          share that corner with the toasts, which render above it and can
          cover it for the few seconds one is up - the trade the placement
          makes, since the button is persistent and a toast is not.
          Portalled for the same reason Collapse all is - AnimatedContent's
          transform would make `fixed` mean this scroll region instead of
          the window. It is aria-hidden and unfocusable on purpose: it
          duplicates a control that is already in the page. It never covers
          the armed confirmation or the duplicate gate - those are there to
          be read before a write that cannot be undone. */}
      {queue.length > 0 &&
        !armed &&
        dupsPending.length === 0 &&
        createPortal(
          <div
            aria-hidden
            data-sticky-action
            className={cn(
              // No pill behind it: the button is its own affordance, and the
              // ring of background around it read as a second control.
              "fixed bottom-6 right-6 z-40 transition-all duration-200",
              actionOnScreen
                ? "pointer-events-none translate-y-3 opacity-0"
                : "translate-y-0 opacity-100",
            )}
          >
            {progress ? (
              <Button tabIndex={-1} disabled>
                <IconConfirm aria-hidden />
                Processing
              </Button>
            ) : !reviewing ? (
              <Button tabIndex={-1} onClick={openReview}>
                <IconReview aria-hidden />
                Review {queue.length} test case{queue.length === 1 ? "" : "s"}
              </Button>
            ) : (
              <Button
                tabIndex={-1}
                disabled={holdActive || hasBlockers || submit.isPending || !online}
                title={online ? undefined : OFFLINE_HINT}
                onClick={() => (pureUpdates ? void guardedSubmit() : arm(true))}
              >
                <IconConfirm aria-hidden />
                {submit.isPending ? "Processing" : `Confirm & ${actionLabel || "create 0"}`}
              </Button>
            )}
          </div>,
          document.body,
        )}
    </section>
  );
}
