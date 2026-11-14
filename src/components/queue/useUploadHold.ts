import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { commands, events, type TestCase } from "../../bindings";
import { toast } from "../../lib/toast";
import { detach } from "../../lib/detach";
import {
  ambiguousRows,
  checkExcludeIds,
  claimsForCheck,
  heldRows,
  loadHold,
  narrowHold,
  reconciledResults,
  saveHold,
  subscribeHold,
  withClaimedFiles,
} from "../../lib/uploadHold";
import { recoverJournal } from "../../lib/uploadJournal";
import type { Outcome } from "./applyOutcome";

/**
 * Creates from an interrupted upload that Azure DevOps could not confirm
 * (lib/uploadHold), and the Check / Release that settle them.
 */
export function useUploadHold({
  org,
  project,
  pbiId,
  queue,
  applyOutcome,
}: {
  org: string;
  project: string;
  pbiId: number;
  queue: TestCase[];
  applyOutcome: (outcome: Outcome) => void;
}) {
  // Persisted per PBI: a restart must not lift the one
  // thing between the user and a duplicate.
  const hold = useSyncExternalStore(subscribeHold, () => loadHold(org, pbiId));
  const [checkingHold, setCheckingHold] = useState(false);

  // A journal left by an upload that never finished becomes a hold: on
  // mount, and again whenever an upload for this PBI ends (by any route).
  // recoverJournal itself stands down while this window is uploading it.
  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | undefined;
    void recoverJournal(org, pbiId).catch(() => {});
    void events.uploadEnded
      .listen((e) => {
        if (e.payload.organization === org && e.payload.pbi_id === pbiId) {
          void recoverJournal(org, pbiId).catch(() => {});
        }
      })
      .then((u) => {
        if (live) unlisten = u;
        else detach(u);
      })
      .catch(() => {});
    return () => {
      live = false;
      detach(unlisten);
    };
  }, [org, pbiId]);

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
      // Never claimable for a held row: what the hold recorded (the PBI's
      // cases before the upload, the upload's own ids) and every id a row
      // in the queue already carries.
      const r = await commands.reconcileUpload(org, project, pbiId, h.since, h.titles, checkExcludeIds(h, queue));
      if (r.status === "error") {
        toast.error(`Could not check with Azure DevOps: ${r.error} The cases stay marked - try again.`);
        return;
      }
      const { found, ambiguous } = r.data;
      const reconciled = reconciledResults(queue, heldRows(queue, h), found);
      const { orphans } = reconciled;
      // A claimed create may carry files the interrupted upload attached:
      // ask which ones the case holds, so each path becomes its stored id and
      // a second upload cannot attach it again. A failed read claims nothing.
      let createdResults = reconciled.results;
      const claims = claimsForCheck(queue, createdResults);
      if (claims.length > 0) {
        const got = await commands.claimCreatedStepFiles(org, claims);
        if (got.status === "ok") createdResults = withClaimedFiles(createdResults, got.data);
        else {
          toast.warning(
            "Could not check which step files those cases already have in Azure DevOps. Uploading them again would attach their files a second time.",
            { duration: 20000 },
          );
        }
      }
      // A smaller hold keeps its ids, plus what this Check just claimed,
      // and each still-held row keeps its own content signature.
      const next = narrowHold(h, ambiguous, found.map((f) => f.id));
      const stillHeld = next?.titles ?? [];
      saveHold(org, pbiId, next);
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

  return { hold, held, ambiguous, holdActive, checkingHold, checkHold, releaseConfirm, setReleaseConfirm };
}
