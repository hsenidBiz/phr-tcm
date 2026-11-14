import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import PowerRenameDialog from "./PowerRenameDialog";
import { commands, type SubmitItemResult, type TestCase } from "../bindings";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { diffCase } from "../lib/caseDiff";
import { keysFor, patchWatch, type SyncChange, type WatchedFile } from "../lib/fileSync";
import {
  registerQueueWriter,
  submitPhaseSnapshot,
  subscribeSubmit,
} from "../lib/submitRun";
import { onlineSnapshot, subscribeOnline } from "../lib/network";
import { sidebarCollapsedSnapshot, stickyLeftPx, subscribeSidebar } from "../lib/sidebarState";
import { unwrap } from "../lib/ipc";
import { duplicateWarning, qualityWarnings, validateCase } from "../lib/validate";
import QueueBulkEditDialog from "./QueueBulkEditDialog";
import QueueRow from "./QueueRow";
import { Button } from "./ui/button";
import ScanProgress from "./ScanProgress";
import { IconCollapseAll } from "../lib/actionIcons";
import { applyOutcome, type Outcome } from "./queue/applyOutcome";
import QueueActionBar from "./queue/QueueActionBar";
import QueueAreaGroup from "./queue/QueueAreaGroup";
import QueueBulkBar from "./queue/QueueBulkBar";
import QueueHeader from "./queue/QueueHeader";
import QueueHoldNotice, { ReleaseConfirmModal } from "./queue/QueueHoldNotice";
import QueueOrderBar from "./queue/QueueOrderBar";
import QueueResults from "./queue/QueueResults";
import RecentImportRow from "./queue/RecentImportRow";
import { useNoteSync } from "./queue/useNoteSync";
import { useQueueEdits } from "./queue/useQueueEdits";
import { useQueueReport } from "./queue/useQueueReport";
import { useAreaGrouping, useQueueSelection } from "./queue/useQueueSelection";
import { useQueueUpload } from "./queue/useQueueUpload";
import { useReviewGate } from "./queue/useReviewGate";
import { useRowViewState } from "./queue/useRowViewState";
import { useUploadHold } from "./queue/useUploadHold";

export { PLAIN_STEPS_REFUSAL } from "./queue/constants";

/** The shared pending-creation queue with the review gate, live progress and
 * exports. Manual Entry and Import Test Cases both render this under their own
 * input areas (v1: every tab feeds one queue). */
export default function QueueSection({
  org,
  project,
  pbiId,
  queue,
  setQueue,
  flash,
  fileChanges,
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
  /** What each open change report says the file did to a CHANGED row, by
   * caseKey. Lets a NEW row show its own edit until the report is dismissed. */
  fileChanges?: Record<string, SyncChange>;
  /** The JSON files this queue was imported from, so a comment typed in
   * the browser view knows which file to be written back into. Manual
   * Entry passes none - its cases live only in the app. */
  watches?: WatchedFile[];
  /** Called when Clear queue empties the queue. The Import screen uses it
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
  // Freshest known watches: kept in step with the prop on every render (so
  // an outside sync is seen), and ALSO updated the instant one of THIS
  // component's own writes lands (`writeBackOwned`, and the post-upload id
  // stamp through the registered `patchWatch`) - without waiting for
  // that write's result to travel all the way back through the parent's
  // state and down through props again first. A write chained right behind
  // another on the same file must see the first one's result the moment it
  // is known, not whenever this component next happens to re-render.
  const watchesRef = useRef(watches);
  watchesRef.current = watches;
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
  // Progress lives at MODULE scope (lib/submitRun), not in this component:
  // the upload takes minutes and the person watching it is exactly the
  // person who wanders to another tab meanwhile. Any mount of this screen
  // reads the same phase, so coming back shows the bar where it really is.
  const phase = useSyncExternalStore(subscribeSubmit, submitPhaseSnapshot);
  const progress = phase && phase.org === org && phase.pbiId === pbiId ? phase : null;

  // Everything a finished submit owes the user, wherever they are now.
  const finishUpload = (outcome: Outcome) =>
    applyOutcome({ qc, org, project, setFailedRows, setUploadedIds }, outcome);

  // Creates from an interrupted upload that Azure DevOps could not confirm,
  // and the Check / Release that settle them.
  const {
    hold,
    held,
    ambiguous,
    holdActive,
    checkingHold,
    checkHold,
    releaseConfirm,
    setReleaseConfirm,
  } = useUploadHold({ org, project, pbiId, queue, applyOutcome: finishUpload });

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
        patchWatch: (path, fields) => {
          watchesRef.current = patchWatch(watchesRef.current, path, fields);
          watchPatched.current?.(path, fields);
        },
        watches: () => watchesRef.current,
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [org, pbiId],
  );
  useNoteSync({ org, pbiId, queue, setQueue, watches, watchesRef, onWatchPatched });

  // The review stage: arming, and the duplicate check before anything is written.
  const gate = useReviewGate({ org, pbiId, queue });
  const { reviewing, setReviewing, armed, arm, pureUpdates, existingCases } = gate;

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

  // Same occurrence-aware keys the file sync reports changes under, so a
  // second case sharing a title still lights up its own row.
  const rowKeys = useMemo(() => keysFor(queue), [queue]);
  const {
    expandedDiffs,
    setExpandedDiffs,
    toggleDiff,
    expandedSteps,
    setExpandedSteps,
    toggleSteps,
    editingIdx,
    setEditingIdx,
    toggleEdit,
    cancelEdit,
  } = useRowViewState(queue, fileChanges, rowKeys);

  const { areaGrouped, changeGrouped, foldedAreas, toggleFoldedArea } = useAreaGrouping();

  const { share, viewHtml } = useQueueReport({ org, project, pbiId, queue, setQueue, watches, areaGrouped });

  const { submit, guardedSubmit } = useQueueUpload({
    org,
    project,
    pbiId,
    queue,
    prefs,
    holdActive,
    applyOutcome: finishUpload,
    setResults,
    setChangeNotes,
    arm,
    setReviewing,
    refreshTitles: gate.refreshTitles,
    acceptedDups: gate.acceptedDups,
  });

  const [renameOpen, setRenameOpen] = useState(false);
  const [bulkOpen, setBulkOpen] = useState(false);

  const {
    selected,
    setSelected,
    toggleGroup,
    areaGroups,
    rowsOnScreen,
    rowIds,
    tabRow,
    onRowFocus,
    select,
    onNavigate,
  } = useQueueSelection({ queue, areaGrouped, foldedAreas, editingIdx });

  const { bulkApply, bulkRemove, clearQueue, renameTarget, removeRow, saveRow } = useQueueEdits({
    queue,
    setQueue,
    watches,
    watchesRef,
    onWatchPatched,
    onQueueCleared,
    selected,
    setEditingIdx,
    uploading: submit.isPending || progress != null,
  });

  const problems = useMemo(() => queue.map((tc) => validateCase(tc)), [queue]);
  const duplicates = useMemo(
    () => queue.map((tc) => duplicateWarning(tc, existingCases)),
    [queue, existingCases],
  );

  // Advice only: never reaches hasBlockers. Joined so a row's memo compares
  // it by value.
  const qualities = useMemo(() => queue.map((tc) => qualityWarnings(tc).join("\n")), [queue]);

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
  // The row, held as a plain node so it can be scrolled to. ActionDock owns
  // the on/off-screen watch that decides when its floating copy shows and
  // keeps that node in its own state instead of handing it back, so this
  // ref exists purely for scrollIntoView below.
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

  // What Collapse all would fold that the user can SEE: steps and diffs
  // open on rows inside a folded group are not counted (the row being
  // edited always shows, folded or not), plus the editor itself.
  const onScreen = new Set(rowsOnScreen);
  const shown = (i: number) => onScreen.has(i);
  const openOnScreen =
    [...expandedSteps].filter(shown).length + [...expandedDiffs].filter(shown).length + (editingIdx != null ? 1 : 0);

  /** One queued case, by its real queue index - the same row whether the
   * Queue is flat or grouped by area. */
  const renderRow = (i: number) => {
    const tc = queue[i];
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
        fileChange={tc.update_id == null ? fileChanges?.[rowKeys[i]] : undefined}
        reviewing={reviewing}
        problem={problems[i]}
        duplicate={duplicates[i]}
        quality={qualities[i]}
        diff={diff}
        diffFailed={diffFailed}
        busy={submit.isPending}
        onToggleSelect={select}
        idBase={rowIds}
        tabStop={tabRow === i}
        onNavigate={onNavigate}
        onRowFocus={onRowFocus}
        onToggleSteps={toggleSteps}
        onToggleDiff={toggleDiff}
        onToggleEdit={toggleEdit}
        onRemove={removeRow}
        onSave={saveRow}
        onCancelEdit={cancelEdit}
      />
    );
  };

  // An empty queue is not a queue - the whole section stays out of the
  // way until a case exists. The "Queue for PBI" header, its five action
  // buttons and the Review button all act on cases, and with zero cases
  // every one of them was disabled furniture; even the old "Nothing
  // queued yet" island was a paragraph explaining an absence. So: Manual
  // Entry (no recents wiring) renders nothing below the form, and the
  // Import tab's Recent JSON Imports section is absent too until a file
  // has actually been imported - no empty-state box explaining its own
  // absence. Kept whole mid-submit (progress/results/reviewing): the user
  // can remove rows while an upload runs, and the progress bar and the
  // results must not vanish with them.
  if (queue.length === 0 && !progress && !results && !reviewing) {
    if (!onOpenRecent || recentImports.length === 0) return null;
    return (
      <section className="space-y-2 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">Recent JSON Imports</h2>
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
      </section>
    );
  }

  return (
    <section className="space-y-3 rounded-md border border-border bg-surface p-4">
      <QueueHeader
        pbiId={pbiId}
        queueLength={queue.length}
        online={online}
        viewPending={viewHtml.isPending}
        sharePending={share.isPending}
        submitPending={submit.isPending}
        onView={() => viewHtml.mutate()}
        onShare={() => share.mutate()}
        onClear={() => {
          const failed = failedRows;
          const uploaded = uploadedIds;
          clearQueue(() => {
            setFailedRows(failed);
            setUploadedIds(uploaded);
          });
          setFailedRows(new Set());
          setUploadedIds(new Set());
        }}
      />

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

      <QueueOrderBar
        queue={queue}
        setQueue={setQueue}
        areaGrouped={areaGrouped}
        onGroupedChange={changeGrouped}
      />

      {/* No empty-state island: with zero cases (and no submit running)
          the whole section early-returns above, so this point is only
          reached with cases to show or a submit in flight. */}

      {queue.length > 0 && (
        <QueueBulkBar
          queueLength={queue.length}
          selected={selected}
          setSelected={setSelected}
          onBulkEdit={() => setBulkOpen(true)}
          onRename={() => setRenameOpen(true)}
          onRemove={() => bulkRemove()}
        />
      )}

      {/* Click a case to select it, Ctrl+click to add or remove one,
          Shift+click for a range - the rows are a multi-select grid. */}
      {queue.length > 0 && !areaGroups && (
        <div role="grid" aria-multiselectable="true" aria-label="Queued cases" className="space-y-1">
          {queue.map((_, i) => renderRow(i))}
        </div>
      )}

      {/* Grouped by area: the same rows, under one heading per level of
          their area. Display only - each row is handed its real queue
          index, so selection, editing and upload order never notice. */}
      {queue.length > 0 && areaGroups && (
        <div className="space-y-1">
          {areaGroups.map((g) => (
            <QueueAreaGroup
              key={g.key}
              group={g}
              hidden={false}
              foldedAreas={foldedAreas}
              toggleFoldedArea={toggleFoldedArea}
              editingIdx={editingIdx}
              selected={selected}
              toggleGroup={toggleGroup}
              renderRow={renderRow}
            />
          ))}
        </div>
      )}

      {/* The same glowing bar the test-suite scan uses. Before the first
          batch answers there is no count to show - the suite is being
          resolved and the batch is in flight - so it sweeps; once a batch
          lands it fills to the count. */}
      {progress && (
        <ScanProgress
          label={
            progress.stage === "checking"
              ? "Checking what changed"
              : progress.done === 0
                ? "Processing the upload"
                : "Uploading"
          }
          done={progress.done > 0 ? progress.done : undefined}
          total={progress.done > 0 ? progress.total : undefined}
        />
      )}

      {hold && !progress && (
        <QueueHoldNotice
          hold={hold}
          online={online}
          checkingHold={checkingHold}
          onCheck={() => void checkHold()}
          onRelease={() => setReleaseConfirm(true)}
        />
      )}

      {releaseConfirm && (
        <ReleaseConfirmModal org={org} pbiId={pbiId} onClose={() => setReleaseConfirm(false)} />
      )}

      <QueueActionBar
        pbiId={pbiId}
        queueLength={queue.length}
        progress={progress}
        reviewing={reviewing}
        armed={armed}
        arm={arm}
        setReviewing={setReviewing}
        holdActive={holdActive}
        hasBlockers={hasBlockers}
        submitPending={submit.isPending}
        online={online}
        pureUpdates={pureUpdates}
        checkingDups={gate.checkingDups}
        dupsPending={gate.dupsPending}
        setAcceptedDups={gate.setAcceptedDups}
        actionLabel={actionLabel}
        actionRowEl={actionRowEl}
        onOpenReview={gate.openReview}
        onSubmit={() => void guardedSubmit()}
      />

      {results && (
        <QueueResults
          results={results}
          changeNotes={changeNotes}
          onClear={() => {
            setResults(null);
            setChangeNotes(null);
            setFailedRows(new Set());
            setUploadedIds(new Set());
          }}
        />
      )}

      {/* Sticky Collapse all, bottom LEFT like every other screen's, once
          anything in the queue is unfolded (steps, diffs, or the inline
          editor). Portalled because this renders inside the screen fade,
          whose transform would make `fixed` mean the scroll region
          instead of the viewport. */}
      {openOnScreen > 0 &&
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
              Collapse all ({openOnScreen})
            </Button>
          </div>,
          document.body,
        )}

    </section>
  );
}
