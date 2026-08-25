import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, MessageSquare, X } from "lucide-react";
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { createPortal } from "react-dom";
import { toast } from "sonner";
import PowerRenameDialog, { type RenameTarget } from "./PowerRenameDialog";
import { commands, events, type SubmitItemResult, type TestCase, type TestCaseFull } from "../bindings";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { diffCase, diffSummary } from "../lib/caseDiff";
import { exportPathFor, rememberExportPath } from "../lib/exportDir";
import { cn } from "../lib/cn";
import { caseKey, fileName, keysFor, loadWatches, ownerPaths, patchWatch, saveWatches, type WatchedFile } from "../lib/fileSync";
import { loadDraftQueue, saveDraftQueue } from "../hooks/useQueue";
import { pruneCreated } from "../lib/queuePrune";
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
import { OFFLINE_HINT, onlineSnapshot, subscribeOnline } from "../lib/network";
import { sidebarCollapsedSnapshot, stickyLeftPx, subscribeSidebar } from "../lib/sidebarState";
import { loadNotes, saveNote } from "../lib/caseNotes";
import { setPbiGlow } from "../lib/pbiGlow";
import { copyText } from "../lib/clipboard";
import { unwrap } from "../lib/ipc";
import { duplicateWarning, validateCase } from "../lib/validate";
import InlineDiff from "./InlineDiff";
import { pagePalette } from "../lib/reportTheme";
import CaseStepsTable from "./CaseStepsTable";
import QueueBulkEditDialog from "./QueueBulkEditDialog";
import QueueCaseEditor from "./QueueCaseEditor";
import StepDiffLines from "./StepDiffLines";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Checkbox } from "./ui/checkbox";
import {
  IconCollapseAll,
  IconBack,
  IconClear,
  IconConfirm,
  IconExport,
  IconOpenInBrowser,
  IconRemove,
  IconReview,
  IconShare,
  IconStop,
  IconRename,
} from "../lib/actionIcons";

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
  const [reviewing, setReviewing] = useState(false);
  // Progress lives at MODULE scope (lib/submitRun), not in this component:
  // the upload takes minutes and the person watching it is exactly the
  // person who wanders to another tab meanwhile. Any mount of this screen
  // reads the same phase, so coming back shows the bar where it really is.
  const phase = useSyncExternalStore(subscribeSubmit, submitPhaseSnapshot);
  const progress = phase && phase.org === org && phase.pbiId === pbiId ? phase : null;
  // Writes PAUSE while offline instead of failing: attempting a submit on
  // a dead connection is known-doomed, and a paused button with a reason
  // beats an error toast. Reads pause on their own (React Query's
  // networkMode) and resume when the connection returns.
  const online = useSyncExternalStore(subscribeOnline, onlineSnapshot);
  // For the sticky Collapse all's left offset - it parks bottom LEFT like
  // every other screen's, clearing the sidebar at its current width.
  const sidebarCollapsed = useSyncExternalStore(subscribeSidebar, sidebarCollapsedSnapshot);

  // While mounted, this screen's own setQueue handles the post-submit
  // prune (through React state, as always). When it is NOT mounted at the
  // finish, the fallback in the mutation writes the persisted draft
  // directly - a created case still sitting in a queue is one Create away
  // from a duplicate work item.
  useEffect(
    () =>
      registerQueueWriter({
        org,
        pbiId,
        setQueue: (updater) => setQueue(updater),
        onCleared: () => onQueueCleared?.(),
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [org, pbiId],
  );
  // A case imported WITH its work item id and a comment is a comment
  // about a case that already lives in Azure DevOps - surface it in View
  // Test Cases too. Fill only EMPTY slots: a note typed in View is never
  // overwritten by a file import.
  useEffect(() => {
    const notes = loadNotes(org);
    for (const tc of queue) {
      const comment = tc.comment ?? "";
      if (tc.update_id != null && comment.trim() && !notes[String(tc.update_id)]) {
        saveNote(org, tc.update_id, comment);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queue, org]);

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
  const [dupGate, setDupGate] = useState<string[] | null>(null);
  const arm = (on: boolean) => {
    setArmed(on);
    setPbiGlow(on);
  };
  // Never leave the chip glowing if this screen unmounts mid-confirmation.
  useEffect(() => () => setPbiGlow(false), []);

  // An emptied queue (Remove all, removing the last item) has nothing to
  // review - leave review mode so the confirm controls disappear too. A
  // changed queue also invalidates a duplicate check taken against it.
  useEffect(() => {
    if (queue.length === 0) setReviewing(false);
    setDupGate(null);
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
  const existingCases = (existing.data ?? []).map((t) => ({ id: t.id, title: t.title }));

  // Diff-preview (spec EDT-B): once the review gate opens, fetch the
  // current server values for every queued UPDATE in one batch so rows
  // can show what will actually change. Failure degrades to "diff
  // unavailable" - it never blocks submitting.
  const updateIds = queue
    .map((tc) => tc.update_id)
    .filter((x): x is number => x != null);
  const currentCases = useQuery({
    queryKey: ["diff-cases", org, [...updateIds].sort(), prefs.moduleRef, prefs.preconditionsRef],
    queryFn: () =>
      unwrap(commands.testCasesByIds(org, updateIds, prefs.moduleRef, prefs.preconditionsRef)),
    enabled: reviewing && updateIds.length > 0,
    staleTime: 60_000,
    retry: false,
  });
  const [expandedDiffs, setExpandedDiffs] = useState<Set<number>>(new Set());
  const toggleDiff = (i: number) =>
    setExpandedDiffs((s) => {
      const next = new Set(s);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
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
  const toggleSteps = (i: number) =>
    setExpandedSteps((s) => {
      const next = new Set(s);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });


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
        watches.map((w) => ({
          path: w.path,
          label: fileName(w.path),
          comment: w.comment ?? "",
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
          watches.map((w) => ({
            path: w.path,
            label: fileName(w.path),
            comment: w.comment ?? "",
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
      const { id, title, text } = e.payload;
      const key = id != null ? `id:${id}` : `t:${title.trim().toLowerCase()}`;
      setQueue((q) => q.map((c) => (caseKey(c) === key ? { ...c, comment: text } : c)));
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [setQueue]);

  const submit = useMutation({
    mutationFn: async () => {
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
      // sent, and they are pruned alongside the written ones afterwards.
      const toSend = queue.filter((tc) => !noopNow(tc));
      const skippedRows = queue.filter((tc) => noopNow(tc));
      const skipped = skippedRows.length;
      if (toSend.length === 0) {
        return { results: [], sent: [], sentFor: pbiId, skipped, skippedRows };
      }
      submitStarted(org, pbiId, toSend.length);
      const unProgress = await events.submitProgress.listen((e) => {
        submitProgressed(e.payload.index + 1, e.payload.total, e.payload.title);
      });
      // Fired before the upload loop when the PBI had no test plan and one
      // was created on the fly - surface it so plans never appear silently.
      const unPlan = await events.planCreated.listen((e) => {
        toast.info(
          `This PBI had no test plan - created "${e.payload.plan_name}" first, now uploading the test cases.`,
        );
      });
      try {
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
        );
        if (r.status === "error") throw new Error(r.error);
        // The outcome is applied HERE, inside the promise, not in
        // onSuccess: the hook's callbacks die with the component, and the
        // person who navigated away mid-upload still needs the created
        // cases OUT of their queue when the loop finishes. `sent` is the
        // FILTERED list: every result index is an index into it, and
        // pruneCreated matches on that list.
        applyOutcome({ results: r.data, sent: toSend, sentFor: pbiId, skipped, skippedRows, prevQueue: queue });
        return { results: r.data, sent: toSend, sentFor: pbiId, skipped, skippedRows };
      } finally {
        // Inside the promise for the same reason: onSettled may never run.
        detach(unProgress);
        detach(unPlan);
        submitFinished();
      }
    },
    // Only the parts a mounted screen can show. Everything that must
    // happen - pruning, toasts, invalidations - already ran inside the
    // mutation itself, because these callbacks die with the component.
    onSuccess: ({ results }) => {
      setResults(results);
      setReviewing(false);
    },
    onError: (e) => toast.error(`Submit failed: ${e.message}`),
  });

  /** The final-click gate: re-check ADO for the titles of every case about
   * to be CREATED, and stop the submit if any already exist - the user must
   * explicitly choose duplicates. Runs against a FRESH fetch (the cached
   * list can be minutes old, and both real incidents happened inside that
   * window); an unreachable ADO falls back to the cache rather than
   * blocking, since the submit itself would surface the outage anyway. */
  const guardedSubmit = async () => {
    let titles = existingCases.map((t) => t.title);
    try {
      const fresh = await existing.refetch();
      if (fresh.data) titles = fresh.data.map((t) => t.title);
    } catch {
      // keep the cached list
    }
    const have = new Set(titles.map((t) => t.trim().toLowerCase()));
    const dups = queue
      .filter((tc) => tc.update_id == null && have.has(tc.title.trim().toLowerCase()))
      .map((tc) => tc.title);
    if (dups.length > 0) {
      setDupGate(dups);
      return;
    }
    submit.mutate();
  };

  /** Everything a finished submit owes the user, wherever they are now.
   * Runs inside the mutation promise, so navigating away cannot skip it. */
  function applyOutcome({
    results,
    sent,
    sentFor,
    skipped,
    skippedRows,
    prevQueue,
  }: {
    results: SubmitItemResult[];
    sent: TestCase[];
    sentFor: number;
    skipped: number;
    skippedRows: TestCase[];
    prevQueue: TestCase[];
  }) {
    // Keep failed items AND anything the loop never reached (cancelled).
    const done = results.filter((r) => r.action !== "failed");
    const ok = done.length;
    const failedCount = results.length - ok;

    // The whole calculation lives in lib/queuePrune.ts, with the four
    // ways it has been wrong written down as tests.
    let stranded = 0;
    let emptied = false;
    const prune = (q: TestCase[]) => {
      const pruned = pruneCreated(sent, q, results);
      stranded = pruned.unmatched;
      let next = pruned.queue;
      // The rows deliberately skipped are finished too - nothing was
      // written because nothing needed to be. Pruned through the same
      // tested function rather than a key filter: two cases can share a
      // title, and matching by key alone went wrong four times before.
      if (skippedRows.length > 0) {
        next = pruneCreated(
          skippedRows,
          next,
          skippedRows.map((_, index) => ({ index, action: "skipped" })),
        ).queue;
      }
      emptied = next.length === 0;
      return next;
    };

    const writer = queueWriterFor(org, sentFor);
    if (writer) {
      // The submitted queue is on screen (this mount or a fresh one):
      // through React state, exactly as it always went.
      writer.setQueue(prune);
      if (emptied && stranded === 0) writer.onCleared();
    } else {
      // Nobody is looking at that queue right now - the user navigated
      // away, or switched PBI. The persisted draft is the queue they will
      // see on return; prune THAT, so a created case is never still
      // sitting in a queue one Create away from a duplicate.
      saveDraftQueue(org, sentFor, prune(loadDraftQueue(org, sentFor)));
      // A finished import has nothing left to watch: with the queue gone,
      // a later save to one of its files would refill a list already
      // dealt with. (Any live backend watcher reconciles on next mount.)
      if (emptied && stranded === 0) saveWatches(org, sentFor, []);
    }

    if (failedCount === 0 && stranded === 0) {
      toast.success(
        skipped > 0
          ? `${ok} test case(s) processed, ${skipped} already up to date.`
          : `${ok} test case(s) processed.`,
      );
    } else if (failedCount > 0) {
      toast.warning(`${ok} processed, ${failedCount} failed - failed items stay queued.`);
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
          const r = await commands.saveDraftCases(path, f.slice);
          if (r.status === "error") {
            toast.warning(
              `Uploaded, but ${fileName(path)} could not be updated with the new ids: ${r.error}. ` +
                `Importing it again would create duplicates - fix the file before re-importing.`,
              { duration: 20000 },
            );
            continue;
          }
          // Mounted: through the owner's state, as bulk edits do. Away:
          // straight into the persisted watch list - a state setter on an
          // unmounted screen never runs its persist step.
          if (queueWriterFor(org, sentFor) && onWatchPatched) {
            onWatchPatched(path, { stamp: r.data, snapshot: f.slice });
          } else {
            saveWatches(
              org,
              sentFor,
              patchWatch(loadWatches(org, sentFor), path, { stamp: r.data, snapshot: f.slice }),
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
  const rowKeys = keysFor(queue);
  const [renameOpen, setRenameOpen] = useState(false);
  const [bulkOpen, setBulkOpen] = useState(false);

  // Selection is by POSITION, like Power Rename: drafts have no id, and a
  // bulk edit can make two share a title, so the title is not an identity
  // that survives the operation being applied. Any change in queue LENGTH
  // drops the selection - after a removal or a file sync the indices point
  // at different cases, and a stale selection silently bulk-edits the
  // wrong rows.
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [selAnchor, setSelAnchor] = useState<number | null>(null);
  useEffect(() => {
    setSelected(new Set());
    setSelAnchor(null);
  }, [queue.length]);

  const toggleSelect = (i: number, shift: boolean) => {
    setSelected((s) => {
      const next = new Set(s);
      if (shift && selAnchor != null) {
        const [lo, hi] = selAnchor < i ? [selAnchor, i] : [i, selAnchor];
        for (let k = lo; k <= hi; k++) next.add(k);
      } else if (next.has(i)) {
        next.delete(i);
      } else {
        next.add(i);
      }
      return next;
    });
    setSelAnchor(i);
  };

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
    const files = new Map<string, { slice: TestCase[]; touched: boolean }>();
    prev.forEach((_, i) => {
      const p = owners[i];
      if (!p) return;
      const f = files.get(p) ?? { slice: [], touched: false };
      const out = next[i];
      if (out) f.slice.push(out);
      if (changed.has(i)) f.touched = true;
      files.set(p, f);
    });
    for (const [path, f] of files) {
      if (!f.touched) continue;
      const r = await commands.saveDraftCases(path, f.slice);
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
  const renameScope =
    selected.size > 0 ? [...selected].sort((a, b) => a - b) : queue.map((_, i) => i);
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

  const problems = queue.map((tc) => validateCase(tc));
  const duplicates = queue.map((tc) => duplicateWarning(tc, existingCases));
  const hasBlockers = problems.some(Boolean);

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
      const diff =
        reviewing && cur
          ? diffCase(tc, cur, { moduleRef: prefs.moduleRef, preconditionsRef: prefs.preconditionsRef })
          : null;
      const diffFailed = reviewing && tc.update_id != null && !cur && currentCases.isError;
      return { diff, diffFailed };
    });
  }, [queue, currentCases.data, reviewing, prefs.moduleRef, prefs.preconditionsRef, currentCases.isError]);

  // An empty queue is not a queue - the whole section stays out of the
  // way until a case exists. The "Queue for PBI" header, its five action
  // buttons and the Review button all act on cases, and with zero cases
  // every one of them was disabled furniture; even the old "Nothing
  // queued yet" island was a paragraph explaining an absence. So: Manual
  // Entry (no recents wiring) renders nothing below the form, and the
  // Import tab shows Recent JSON Imports alone - its way back in. Kept
  // whole mid-submit (progress/results/reviewing): pruning empties the
  // queue as items are created, and the progress bar must not vanish
  // with it.
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
            Power Rename
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
                Power Rename {selected.size}
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
            const touched = flash?.[rowKeys[i]];
            return (
              <li
                key={i}
                className={cn(
                  // The open editor hosts a non-portaled Combobox dropdown that must
                  // paint past the row's box - content-visibility's paint containment
                  // would clip it, so drop cv-row while this row is being edited.
                  editingIdx !== i && "cv-row",
                  "rounded-md border text-sm transition-colors",
                  touched === "added"
                    ? "border-success/50 bg-success/5"
                    : touched === "changed"
                      ? "border-warning/50 bg-warning/5"
                      : "border-border",
                )}
              >
                <div className="flex items-center justify-between px-3 py-1.5">
                  <span className="text-text">
                    {/* Capture-phase wrapper: the checkbox's own click never
                        fires, so shift-ranges can be read off the event. */}
                    <span
                      className="mr-2 inline-block align-middle"
                      onClickCapture={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        toggleSelect(i, e.shiftKey);
                      }}
                    >
                      <Checkbox
                        ariaLabel={`Select ${tc.title}`}
                        checked={selected.has(i)}
                        onCheckedChange={() => {}}
                      />
                    </span>
                    <button
                      aria-label={
                        expandedSteps.has(i) ? `Collapse steps of ${tc.title}` : `Expand steps of ${tc.title}`
                      }
                      title={expandedSteps.has(i) ? "Hide steps" : "Check the steps before submitting"}
                      className="mr-2 align-middle text-muted hover:text-accent"
                      onClick={() => toggleSteps(i)}
                    >
                      {expandedSteps.has(i) ? (
                        <ChevronDown size={14} />
                      ) : (
                        <ChevronRight size={14} />
                      )}
                    </button>
                    {tc.update_id != null ? (
                      <Badge className="mr-2 bg-warning/20 text-warning">
                        UPDATE #{tc.update_id}
                      </Badge>
                    ) : (
                      <Badge className="mr-2 bg-success/20 text-success">NEW</Badge>
                    )}
                    {tc.title}
                    <span className="ml-2 text-xs text-faint">{tc.steps.length} steps</span>
                    {diff?.noop && (
                      <Badge className="ml-2 bg-warning/20 text-warning">
                        no-op — nothing will change
                      </Badge>
                    )}
                    {diff && !diff.noop && (
                      <button
                        className="ml-2 text-xs text-accent hover:underline"
                        onClick={() => toggleDiff(i)}
                      >
                        {diffSummary(diff)} {expandedDiffs.has(i) ? "▾" : "▸"}
                      </button>
                    )}
                    {diffFailed && (
                      <span className="ml-2 text-xs text-faint">diff unavailable</span>
                    )}
                    {reviewing && problems[i] && (
                      <span className="ml-2 text-xs text-danger">{problems[i]}</span>
                    )}
                    {reviewing && !problems[i] && duplicates[i] && (
                      <span className="ml-2 text-xs text-warning">{duplicates[i]}</span>
                    )}
                  </span>
                  <span className="flex items-center gap-3">
                    <button
                      className="text-xs text-faint hover:text-accent disabled:opacity-50"
                      disabled={submit.isPending}
                      onClick={() => setEditingIdx((cur) => (cur === i ? null : i))}
                    >
                      {editingIdx === i ? "Close" : "Edit"}
                    </button>
                    <button
                      className="text-xs text-faint hover:text-danger disabled:opacity-50"
                      disabled={submit.isPending}
                      onClick={() => setQueue((q) => q.filter((_, j) => j !== i))}
                    >
                      Remove
                    </button>
                  </span>
                </div>
                {/* In-app note from the JSON file - never sent to ADO. */}
                {(tc.comment ?? "").trim() !== "" && (
                  <p className="flex items-start gap-1.5 border-t border-border/60 px-3 py-1 text-xs text-muted">
                    <MessageSquare size={12} className="mt-0.5 shrink-0" />
                    <span className="min-w-0 flex-1 whitespace-pre-wrap">{tc.comment}</span>
                  </p>
                )}
                {editingIdx === i && (
                  <QueueCaseEditor
                    original={tc}
                    org={org}
                    project={project}
                    onSave={(next) => {
                      const prev = queue;
                      setQueue((q) => q.map((t, j) => (j === i ? next : t)));
                      setEditingIdx(null);
                      // The owning FILE follows the edit, through the same
                      // machinery as bulk edits. This closes a real duplicate
                      // trap: ownership is matched by title, so a rename that
                      // only the queue knew about orphaned the row at stamp
                      // time - its created id was never written back, and the
                      // next import of the file created the case again.
                      void writeBackOwned(
                        prev,
                        prev.map((t, j) => (j === i ? next : t)),
                        new Set([i]),
                      );
                      toast.success("Queued case updated.");
                    }}
                    onCancel={() => setEditingIdx(null)}
                  />
                )}
                {expandedSteps.has(i) && (
                  <div className="border-t border-border">
                    {/* Shared with the watched-file change report, which
                        needed the same "read the case start to finish"
                        view - see CaseStepsTable. */}
                    <CaseStepsTable
                      steps={tc.steps}
                      preconditions={tc.preconditions}
                      reviewerNotes={tc.reviewer_notes}
                    />
                  </div>
                )}
                {diff && !diff.noop && expandedDiffs.has(i) && (
                  <div className="space-y-1 border-t border-border px-3 py-2 text-xs">
                    {/* Word-level, like the step lines below: editing one
                        word of a title must not read as the whole title
                        being replaced. */}
                    {diff.fields.map((f) => (
                      <div key={f.name}>
                        <span className="font-medium text-muted">{f.name}:</span>{" "}
                        <InlineDiff old={f.old} next={f.new} />
                      </div>
                    ))}
                    {diff.steps.detail.length > 0 && (
                      <div className="space-y-1">
                        <span className="font-medium text-muted">Steps:</span>
                        {/* git word-diff style: -/+ lines with only the
                            actually-changed words highlighted. */}
                        {diff.steps.detail.map((d) => (
                          <StepDiffLines key={d.index} d={d} />
                        ))}
                      </div>
                    )}
                    {diff.blankSkipped.length > 0 && (
                      <div className="text-faint">
                        Left untouched (blank in import): {diff.blankSkipped.join(", ")}
                      </div>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}

      {progress && (
        <div className="space-y-1">
          <div className="h-1.5 overflow-hidden rounded-full bg-surface-2">
            <div
              className="h-full rounded-full bg-accent transition-all"
              style={{ width: `${(progress.done / Math.max(progress.total, 1)) * 100}%` }}
            />
          </div>
          <div className="flex items-center gap-3">
            <p className="text-xs text-muted">
              Processing {progress.done}/{progress.total}
            </p>
            <Button
              variant="danger"
              size="sm"
              onClick={() => {
                commands.cancelSubmit();
                toast.info("Stopping after the current item");
              }}
            >
              <IconStop aria-hidden />
              Cancel
            </Button>
          </div>
        </div>
      )}

      <div className="flex items-center gap-3">
        {!reviewing ? (
          <Button disabled={queue.length === 0} onClick={() => setReviewing(true)}>
            <IconReview aria-hidden />
            Review {queue.length} test case{queue.length === 1 ? "" : "s"}
          </Button>
        ) : (
          (() => {
            const updates = queue.filter((tc) => tc.update_id != null).length;
            const creates = queue.length - updates;
            const label = [
              creates > 0 && `create ${creates}`,
              updates > 0 && `update ${updates}`,
            ]
              .filter(Boolean)
              .join(" · ");
            if (dupGate) {
              return (
                <div className="w-full space-y-2 rounded-md border border-danger/50 bg-danger/10 p-3">
                  <p className="text-sm font-semibold text-text">
                    Stopped: {dupGate.length} case{dupGate.length === 1 ? "" : "s"} with the same
                    title already exist{dupGate.length === 1 ? "s" : ""} on PBI #{pbiId}.
                  </p>
                  <ul className="max-h-32 space-y-0.5 overflow-y-auto text-xs text-muted">
                    {dupGate.map((t) => (
                      <li key={t}>• {t}</li>
                    ))}
                  </ul>
                  <p className="text-xs text-muted">
                    If you meant to update the existing cases, import a file that includes their
                    ids (View Test Cases → Export JSON has them). Creating anyway makes
                    duplicates - and cleaning those up needs delete permission.
                  </p>
                  <div className="flex items-center gap-2">
                    <Button size="sm" onClick={() => setDupGate(null)}>
                      <IconBack aria-hidden />
                      Stop — take me back
                    </Button>
                    <Button
                      variant="danger"
                      size="sm"
                      disabled={submit.isPending || !online}
                      onClick={() => {
                        setDupGate(null);
                        submit.mutate();
                      }}
                    >
                      Create duplicates anyway
                    </Button>
                  </div>
                </div>
              );
            }
            if (!armed) {
              return (
                <>
                  <Button
                    disabled={queue.length === 0 || hasBlockers || submit.isPending || !online}
                    title={online ? undefined : OFFLINE_HINT}
                    // Pure updates go straight to the (still dup-guarded)
                    // submit - see the pureUpdates note above.
                    onClick={() => (pureUpdates ? void guardedSubmit() : arm(true))}
                  >
                    <IconConfirm aria-hidden />
                    {submit.isPending ? "Processing" : `Confirm & ${label || "create 0"}`}
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => setReviewing(false)}>
                    <IconBack aria-hidden />
                    Back
                  </Button>
                  {hasBlockers && (
                    <span className="text-xs text-danger">Fix the flagged items first.</span>
                  )}
                </>
              );
            }
            return (
              <div className="w-full space-y-2 rounded-md border border-warning/50 bg-warning/10 p-3">
                <p className="text-sm text-text">
                  Check the highlighted PBI above — everything here will be written to{" "}
                  <span className="font-semibold">PBI #{pbiId}</span>. Removing them afterwards{" "}
                  <span className="font-semibold">needs delete permission</span> in Azure DevOps,
                  and deleting a test case in Azure DevOps is permanent.
                </p>
                <div className="flex items-center gap-2">
                  <Button
                    disabled={submit.isPending || !online}
                    title={online ? undefined : OFFLINE_HINT}
                    onClick={() => {
                      arm(false);
                      // Not straight to the submit: the duplicate-title gate
                      // re-checks ADO first and may stop to ask.
                      void guardedSubmit();
                    }}
                  >
                    <IconConfirm aria-hidden />
                    {submit.isPending ? "Processing" : `Yes — ${label}`}
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => arm(false)}>
                    <IconBack aria-hidden />
                    Back
                  </Button>
                </div>
              </div>
            );
          })()
        )}
      </div>

      {results && (
        <div className="space-y-2">
          <ul className="space-y-0.5 text-sm">
            {results.map((r) => (
              <li
                key={r.index}
                className={
                  r.action === "failed"
                    ? "text-danger"
                    : // Created or updated, but something after the write
                      // went wrong - the case exists, so this is a warning
                      // to act on, not a failure to retry.
                      r.error
                      ? "text-warning"
                      : "text-success"
                }
              >
                {r.action === "created" && `Created #${r.id}: ${r.title}`}
                {r.action === "updated" && `Updated #${r.id}: ${r.title}`}
                {r.action === "failed" && `Failed: ${r.title} - ${r.error}`}
                {r.action !== "failed" && r.error && ` - ${r.error}`}
              </li>
            ))}
          </ul>
          {/* Dismiss the results once read - the button goes with them. */}
          <Button variant="outline" size="sm" onClick={() => setResults(null)}>
            <IconClear aria-hidden />
            Clear results
          </Button>
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
    </section>
  );
}
