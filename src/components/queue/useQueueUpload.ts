import { useMutation } from "@tanstack/react-query";
import { useEffect, useRef, type Dispatch, type SetStateAction } from "react";
import {
  commands,
  events,
  type OrderHint,
  type SubmitItemResult,
  type TestCase,
  type TestCaseFull,
} from "../../bindings";
import { toast } from "../../lib/toast";
import { diffCase, type CaseDiff } from "../../lib/caseDiff";
import { hasTesterNotes, testerNotes } from "../../lib/testerNotes";
import { detach } from "../../lib/detach";
import { heldRows, holdSignature, loadHold } from "../../lib/uploadHold";
import {
  submitFinished,
  submitProgressed,
  submitStarted,
  submitUploading,
} from "../../lib/submitRun";
import { finishJournal, recoverJournal } from "../../lib/uploadJournal";
import { logUi } from "../../lib/uiLog";
import { unwrap } from "../../lib/ipc";
import type { FieldPrefs } from "../../lib/fieldPrefs";
import type { Outcome } from "./applyOutcome";
import { HOLD_REFUSAL, PLAIN_STEPS_REFUSAL } from "./constants";

/** The upload itself: the pre-flight, the write, and the guarded entry point the confirm button uses. */
export function useQueueUpload({
  org,
  project,
  pbiId,
  queue,
  prefs,
  holdActive,
  applyOutcome,
  setResults,
  setChangeNotes,
  arm,
  setReviewing,
  refreshTitles,
  acceptedDups,
}: {
  org: string;
  project: string;
  pbiId: number;
  queue: TestCase[];
  prefs: FieldPrefs;
  holdActive: boolean;
  applyOutcome: (outcome: Outcome) => void;
  setResults: Dispatch<SetStateAction<SubmitItemResult[] | null>>;
  setChangeNotes: Dispatch<SetStateAction<string | null>>;
  arm: (on: boolean) => void;
  setReviewing: Dispatch<SetStateAction<boolean>>;
  refreshTitles: () => Promise<{ id: number; title: string }[]>;
  acceptedDups: string[];
}) {
  // "Upload anyway" after PLAIN_STEPS_REFUSAL: the next submit (and only
  // that one) tells Rust the person accepted plain-text steps.
  const allowPlainNext = useRef(false);
  const lastPreExisting = useRef<number[]>([]);
  // The "Upload anyway" toast stays until dismissed, so it must not outlive
  // what it was offered for: leaving the screen, or a change to the queue,
  // the PBI or the organization, closes it (its action then never runs).
  const plainToast = useRef<string | null>(null);
  useEffect(() => {
    return () => {
      if (plainToast.current != null) toast.dismiss(plainToast.current);
      plainToast.current = null;
    };
  }, [org, pbiId, queue]);
  const submit = useMutation({
    /** `preExisting`: the ids of the PBI's test cases before this upload. */
    mutationFn: async (preExisting: number[] = []) => {
      const allowPlainSteps = allowPlainNext.current;
      allowPlainNext.current = false;
      lastPreExisting.current = preExisting;
      // A journal still on disk is an upload that never finished. Rust's
      // next write would overwrite it, so it is turned into a hold FIRST,
      // and if that hold marks a row here the upload does not start.
      if ((await recoverJournal(org, pbiId)) === "recovered" && heldRows(queue, loadHold(org, pbiId)).some(Boolean)) {
        throw new Error(HOLD_REFUSAL);
      }
      // The buttons are disabled while a hold stands; this is the backstop.
      if (holdActive) throw new Error(HOLD_REFUSAL);
      // Claimed BEFORE the pre-flight fetch below, not after it: a second
      // mount whose OWN pre-flight outlasts this mount's whole upload must
      // find the slot still taken when it gets there - claiming only after
      // the pre-flight left a window where the slot read as free while this
      // submit was still checking what to send, and a remount that reached
      // its own (slower) pre-flight during that window could claim it AND
      // resend the very creates this submit was about to make.
      const run = submitStarted(org, pbiId, queue.length);
      if (run == null) {
        throw new Error("another upload is still running. Wait for it to finish, then try again.");
      }
      let unProgress: (() => void) | undefined;
      let unPlan: (() => void) | undefined;
      let unSuite: (() => void) | undefined;
      let unRunOrder: (() => void) | undefined;
      try {
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
        // Something to write: from here on the screen says it is uploading.
        submitUploading(run);
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
          {
            order_hint: orderHint,
            allow_plain_steps: allowPlainSteps,
            pre_existing: preExisting,
            hold_sigs: toSend.map(holdSignature),
          },
        );
        if (r.status === "error") throw new Error(r.error);
        // The outcome is applied HERE, inside the promise, not in
        // onSuccess: the hook's callbacks die with the component, and the
        // person who navigated away mid-upload still needs the created
        // cases in their queue to carry their new ids when the loop
        // finishes. `sent` is the FILTERED list: every result index is an
        // index into it, and keepUploaded matches on that list.
        applyOutcome({ results: r.data, sent: toSend, sentFor: pbiId, skipped, prevQueue: queue, since, preExisting });
        // The hold (if any) is saved by now, so the journal can go: until
        // here it is what turns a crash into a hold.
        // The upload already happened: a failure to tidy the journal up
        // (an IPC error as much as an error result) must not turn it into a
        // failed upload.
        await finishJournal(
          org,
          pbiId,
          toSend.filter((c) => c.update_id == null).map((c) => c.title),
          r.data
            .filter((x) => (x.action === "created" || x.action === "updated") && x.id != null)
            .map((x) => x.id as number),
        ).catch((e) => logUi(`upload journal not finished: ${String(e)}`));
        return { results: r.data, sent: toSend, sentFor: pbiId, skipped, diffs };
      } finally {
        // Inside the promise for the same reason: onSettled may never run.
        // Covers the whole body, including the pre-flight above and the
        // all-no-op early return: the run is claimed before either runs, so
        // it must be released regardless of which path leaves by.
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
    onError: (e) => {
      // Nothing was sent: the current steps of the updates could not be
      // read. The person decides whether plain-text steps are acceptable.
      if (e.message === PLAIN_STEPS_REFUSAL) {
        plainToast.current = toast.warning(PLAIN_STEPS_REFUSAL, {
          duration: Infinity,
          action: {
            label: "Upload anyway",
            onClick: () => {
              plainToast.current = null;
              allowPlainNext.current = true;
              submit.mutate(lastPreExisting.current);
            },
          },
        });
        return;
      }
      toast.error(`Submit failed: ${e.message}`);
    },
  });

  /** The write. Re-verifies first: moving the check to the review opened a
   * window - the queue can be edited, and someone else can create the same
   * case, while a hundred cases are being read. It stops only for a clash
   * nobody has been shown, so the common path never asks twice. */
  const guardedSubmit = async () => {
    if (holdActive) {
      toast.warning(HOLD_REFUSAL);
      return;
    }
    const onPbi = await refreshTitles();
    const have = new Set(onPbi.map((t) => t.title.trim().toLowerCase()));
    const ok = new Set(acceptedDups.map((t) => t.trim().toLowerCase()));
    const surprises = queue.filter((tc) => {
      const key = tc.title.trim().toLowerCase();
      return tc.update_id == null && have.has(key) && !ok.has(key);
    });
    // The panel and the disabled button both read `dupsPending`, which the
    // refresh above has just updated - there is nothing else to set.
    if (surprises.length > 0) return;
    // The PBI's cases as they stand before this upload: a hold it leaves
    // records their ids, so a later Check can never claim one of them.
    submit.mutate(onPbi.map((t) => t.id));
  };

  return { submit, guardedSubmit };
}
