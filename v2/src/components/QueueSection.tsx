import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { ChevronDown, ChevronRight, MessageSquare } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import PowerRenameDialog, { type RenameTarget } from "./PowerRenameDialog";
import { commands, events, type SubmitItemResult, type TestCase } from "../bindings";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { diffCase, diffSummary } from "../lib/caseDiff";
import { exportPathFor, rememberExportPath } from "../lib/exportDir";
import { cn } from "../lib/cn";
import { caseKey, fileName, keysFor, ownerPaths, type WatchedFile } from "../lib/fileSync";
import { pruneCreated } from "../lib/queuePrune";
import { iterationDetails } from "../lib/iterations";
import { setPbiGlow } from "../lib/pbiGlow";
import { copyText } from "../lib/clipboard";
import { unwrap } from "../lib/ipc";
import { duplicateWarning, validateCase } from "../lib/validate";
import AstryxIsland from "./AstryxIsland";
import InlineDiff from "./InlineDiff";
import Combobox from "./ui/combobox";
import { pagePalette } from "../lib/reportTheme";
import CaseStepsTable from "./CaseStepsTable";
import QueueCaseEditor from "./QueueCaseEditor";
import StepDiffLines from "./StepDiffLines";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Select } from "./ui/select";
import {
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
export default function QueueSection({
  org,
  project,
  pbiId,
  queue,
  setQueue,
  flash,
  watches = [],
  onQueueCleared,
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
}) {
  const qc = useQueryClient();
  const { prefs } = useFieldRefs(org, project);
  const [results, setResults] = useState<SubmitItemResult[] | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [areaPath, setAreaPath] = useState("");
  const [iterationPath, setIterationPath] = useState("");

  // Final confirmation stage: the first Confirm click arms the submit and
  // spotlights the PBI chip; only the explicit second click writes.
  const [armed, setArmed] = useState(false);
  const arm = (on: boolean) => {
    setArmed(on);
    setPbiGlow(on);
  };
  // Never leave the chip glowing if this screen unmounts mid-confirmation.
  useEffect(() => () => setPbiGlow(false), []);

  // An emptied queue (Remove all, removing the last item) has nothing to
  // review - leave review mode so the confirm controls disappear too.
  useEffect(() => {
    if (queue.length === 0) setReviewing(false);
  }, [queue.length]);

  // Classification trees load lazily, only once the review gate opens.
  const areas = useQuery({
    queryKey: ["classification", org, project, "areas"],
    queryFn: () => unwrap(commands.classificationPaths(org, project, "areas")),
    enabled: reviewing,
    staleTime: 60 * 60_000,
  });
  const iterations = useQuery({
    queryKey: ["iterations-dated", org, project],
    queryFn: () => unwrap(commands.listIterations(org, project)),
    enabled: reviewing,
    staleTime: 60 * 60_000,
  });

  const existing = useQuery({
    queryKey: ["pbi-tc-titles", org, pbiId],
    queryFn: () => unwrap(commands.pbiTestCases(org, pbiId)),
    retry: false,
  });
  const existingTitles = (existing.data ?? []).map((t) => t.title);

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
  const currentById = new Map((currentCases.data ?? []).map((c) => [c.id, c]));

  /** Rows this submit has nothing to write for.
   *
   * The review gate already computes this and prints "no-op - nothing will
   * change" on the row; until now it printed that and then wrote the case
   * anyway. On a queue of 81 imported cases where ten had really changed,
   * that was 71 pointless PATCHes plus 71 x the half-second pacing gap -
   * about a minute of waiting, and a minute of someone's rate-limit budget,
   * to say nothing.
   *
   * Fails SAFE in both directions that matter:
   *  - only an UPDATE can be a no-op; a create always writes.
   *  - it needs the server's current values. If that fetch failed, or has
   *    not landed, the row is NOT skipped - "we could not check" must never
   *    read as "nothing to do".
   */
  const isNoop = (tc: TestCase): boolean => {
    if (tc.update_id == null) return false;
    const cur = currentById.get(tc.update_id);
    if (!cur) return false;
    return diffCase(tc, cur, {
      moduleRef: prefs.moduleRef,
      preconditionsRef: prefs.preconditionsRef,
    }).noop;
  };
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

  const unlistenRef = useRef<(() => void) | null>(null);
  useEffect(() => () => unlistenRef.current?.(), []);

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
    onError: (e) => toast.error(`Could not open the report: ${e.message}`),
  });

  /**
   * Push a comment edited on the card into the JSON file the case came
   * from, so the card, the browser view and the file agree.
   *
   * Addressed by the case as it was BEFORE the edit: the same form can
   * rename a case, and the file still holds it under the old title.
   * Nothing else on the card is written through - an in-app edit has never
   * propagated to the file, and widening that is not this feature's job.
   *
   * A REFUSAL is surfaced. This used to be silent on every failure, with
   * the reasoning that a transient sync error is noise - fair enough. But
   * a refusal is not transient: two drafts sharing a title cannot be told
   * apart in the file, so that comment will NEVER reach it, and the card
   * would go on showing text the file does not have. The comment stays in
   * the queue either way, so the toast is informational, not a rollback.
   */
  const writeCommentThrough = (before: TestCase, text: string) => {
    if ((before.comment ?? "") === text) return;
    const owner = ownerPaths([before], watches)[0];
    if (!owner) return;
    void commands
      .saveDraftComment(owner, before.update_id, before.title, text)
      .then((r) => {
        if (r.status === "error") {
          toast.warning(`Kept in the queue, but not written to the file: ${r.error}`, {
            duration: 15000,
          });
        }
      })
      .catch(() => {});
  };

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
      // Everything that actually has something to write. The skipped rows
      // are still in the queue and still on screen - they are just not
      // sent, and they are pruned alongside the written ones afterwards.
      const toSend = queue.filter((tc) => !isNoop(tc));
      const skippedRows = queue.filter((tc) => isNoop(tc));
      const skipped = skippedRows.length;
      if (toSend.length === 0) {
        return { results: [], sent: [], sentFor: pbiId, skipped, skippedRows };
      }
      setProgress({ done: 0, total: toSend.length });
      const unProgress = await events.submitProgress.listen((e) => {
        setProgress({ done: e.payload.index + 1, total: e.payload.total });
      });
      // Fired before the upload loop when the PBI had no test plan and one
      // was created on the fly - surface it so plans never appear silently.
      const unPlan = await events.planCreated.listen((e) => {
        toast.info(
          `This PBI had no test plan - created "${e.payload.plan_name}" first, now uploading the test cases.`,
        );
      });
      unlistenRef.current = () => {
        unProgress();
        unPlan();
      };
      const r = await commands.submitQueue(
        org,
        project,
        pbiId,
        toSend,
        prefs.moduleRef,
        prefs.preconditionsRef,
        areaPath || null,
        iterationPath || null,
      );
      if (r.status === "error") throw new Error(r.error);
      // The exact rows that were sent, and the PBI they were sent for.
      // onSuccess runs later, by which time the user may have switched PBI
      // or a watched file may have rewritten the queue - so neither the
      // indices nor "the current queue" still mean what they meant here.
      // `sent` is the FILTERED list: every result index is an index into
      // it, and pruneCreated matches on that list. Passing the full queue
      // here would shift every index by the number skipped.
      return { results: r.data, sent: toSend, sentFor: pbiId, skipped, skippedRows };
    },
    onSettled: () => {
      unlistenRef.current?.();
      unlistenRef.current = null;
      setProgress(null);
    },
    onSuccess: ({ results, sent, sentFor, skipped, skippedRows }) => {
      setResults(results);
      setReviewing(false);
      // Keep failed items AND anything the loop never reached (cancelled).
      const done = results.filter((r) => r.action !== "failed");
      const ok = done.length;
      const failedCount = results.length - ok;

      if (sentFor !== pbiId) {
        // The queue on screen is not the one that was submitted. Leave it
        // completely alone and say so, rather than guess.
        toast.info(
          `${ok} test case(s) processed for PBI #${sentFor}. Switch back to it to see what is left.`,
        );
      } else {
        // The whole calculation lives in lib/queuePrune.ts, with the four
        // ways it has been wrong written down as tests. It was inline here
        // for all four of them, on a path with no test at all.
        let stranded = 0;
        let emptied = false;
        setQueue((q) => {
          const pruned = pruneCreated(sent, q, results);
          stranded = pruned.unmatched;
          let next = pruned.queue;
          // The rows deliberately skipped are finished too - nothing was
          // written because nothing needed to be. Leaving them queued
          // would end an 81-case submit with 71 still on screen and no
          // way to tell them from work outstanding.
          //
          // Pruned through the same tested function rather than a key
          // filter: two cases can share a title, and matching by key alone
          // is exactly how this went wrong four times before.
          if (skippedRows.length > 0) {
            next = pruneCreated(
              skippedRows,
              next,
              skippedRows.map((_, index) => ({ index, action: "skipped" })),
            ).queue;
          }
          emptied = next.length === 0;
          return next;
        });
        // A finished import has nothing left to watch. The files fed this
        // queue; with the queue gone, a later save to one of them would
        // refill a list the user has already dealt with.
        if (emptied && stranded === 0) onQueueCleared?.();
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
      }
      qc.invalidateQueries({ queryKey: ["pbi-tcs", org, sentFor] });
      qc.invalidateQueries({ queryKey: ["pbi-tc-titles", org, sentFor] });
    },
    onError: (e) => toast.error(`Submit failed: ${e.message}`),
  });

  // Same occurrence-aware keys the file sync reports changes under, so a
  // second case sharing a title still lights up its own row.
  const rowKeys = keysFor(queue);
  const [renameOpen, setRenameOpen] = useState(false);

  /** Renaming drafts touches nothing outside this list - they are in memory
   *  until Create runs - so undo here can never fail.
   *
   *  Rows are matched back to drafts by POSITION. A draft has no work item
   *  id and a rename can make two of them share a title, so the title is
   *  not an identity that survives the very operation being applied. */
  const renameTarget: RenameTarget = {
    label: "the queued drafts",
    cases: queue.map((tc) => ({ id: tc.update_id, title: tc.title })),
    undoable: true,
    apply: async (rows) => {
      // By POSITION, not by title. Title matching failed in exactly the
      // case this feature makes likely: rename one draft onto another's
      // title and the two stop being distinguishable, so Undo put the old
      // title back on whichever one it reached first and left titles
      // sitting on the wrong steps - reported as "Put back 1 title".
      const byIndex = new Map(rows.map((r) => [r.index, r]));
      // Every row must be accounted for, so the loop is driven by ROWS as
      // well as by the queue. Walking only the queue meant a row whose
      // index no longer exists - the queue shrank while the dialog was
      // open - was never visited, so it counted as succeeded and Undo then
      // had nothing to put back for it.
      const written = new Set<number>();
      const failed: typeof rows = [];
      setQueue((q) =>
        q.map((tc, i) => {
          const row = byIndex.get(i);
          if (!row) return tc;
          // The queue can move underneath an open dialog - a watched file
          // syncing, another tab adding a case. If this is no longer the
          // draft the preview showed, leave it alone and report it rather
          // than writing that title onto something else.
          if (tc.title !== row.before) {
            failed.push(row);
            return tc;
          }
          written.add(row.index);
          return { ...tc, title: row.after };
        }),
      );
      for (const r of rows) {
        if (!written.has(r.index) && !failed.includes(r)) failed.push(r);
      }
      return failed;
    },
  };

  const problems = queue.map((tc) => validateCase(tc));
  const duplicates = queue.map((tc) => duplicateWarning(tc, existingTitles));
  const hasBlockers = problems.some(Boolean);

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
            disabled={queue.length === 0 || share.isPending}
            title="Upload the draft as a one-time share link a teammate can import for review"
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
          <Button
            variant="outline"
            size="sm"
            disabled={queue.length === 0 || submit.isPending}
            onClick={() => setRenameOpen(true)}
          >
            <IconRename aria-hidden />
            Power Rename
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

      {queue.length === 0 && (
        <AstryxIsland>
          <EmptyState
            title="Nothing queued yet"
            description="Add test cases above - they gather here for review before anything is created in Azure DevOps."
          />
        </AstryxIsland>
      )}

      {queue.length > 0 && (
        <ul className="space-y-1">
          {queue.map((tc, i) => {
            const cur = tc.update_id != null ? currentById.get(tc.update_id) : undefined;
            const diff =
              reviewing && cur
                ? diffCase(tc, cur, {
                    moduleRef: prefs.moduleRef,
                    preconditionsRef: prefs.preconditionsRef,
                  })
                : null;
            const diffFailed =
              reviewing && tc.update_id != null && !cur && currentCases.isError;
            const touched = flash?.[rowKeys[i]];
            return (
              <li
                key={i}
                className={cn(
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
                      setQueue((q) => q.map((t, j) => (j === i ? next : t)));
                      setEditingIdx(null);
                      writeCommentThrough(tc, next.comment ?? "");
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
                    <CaseStepsTable steps={tc.steps} preconditions={tc.preconditions} />
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

      {reviewing && (
        <div className="flex flex-wrap gap-3">
          <label className="flex flex-col gap-1 text-xs text-muted">
            Area path for new cases
            <Select
              className="w-64 py-1.5"
              value={areaPath}
              onChange={(e) => setAreaPath(e.target.value)}
            >
              <option value="">Same as PBI</option>
              {(areas.data ?? []).map((p) => (
                <option key={p}>{p}</option>
              ))}
            </Select>
          </label>
          <label className="flex flex-col gap-1 text-xs text-muted">
            Iteration for new cases
            <Combobox
              ariaLabel="Iteration for new cases"
              className="w-72"
              placeholder="Same as PBI"
              value={iterationPath}
              options={(iterations.data ?? []).map((i) => i.path)}
              details={iterationDetails(iterations.data ?? [])}
              onChange={setIterationPath}
            />
          </label>
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
            if (!armed) {
              return (
                <>
                  <Button
                    disabled={queue.length === 0 || hasBlockers || submit.isPending}
                    onClick={() => arm(true)}
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
                  and this app can only move them to the recycle bin.
                </p>
                <div className="flex items-center gap-2">
                  <Button
                    disabled={submit.isPending}
                    onClick={() => {
                      arm(false);
                      submit.mutate();
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
    </section>
  );
}
