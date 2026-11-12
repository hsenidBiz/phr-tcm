/**
 * Turns the journal Rust writes before an upload's first batch into the
 * existing "Outcome unknown" hold, when the upload never finished (the app
 * closed, crashed or lost power mid-write). A finished upload clears the
 * journal, but first turns any older, unrecovered rows in it (from an earlier
 * upload that never finished) into a hold, so Check can still account for
 * them; whatever is still here at launch belongs to an upload that did not.
 */
import { commands, type UploadJournal } from "../bindings";
import { submitPhaseSnapshot } from "./submitRun";
import { holdFromJournal, loadHold, mergeHolds, saveHold } from "./uploadHold";
import { logUi } from "./uiLog";

export type RecoverOutcome = "none" | "running" | "recovered";

/** Is an upload in THIS window for this very PBI? One on another PBI must
 * not hold recovery here. */
function runningHere(org: string, pbiId: number): boolean {
  const p = submitPhaseSnapshot();
  return p != null && p.org === org && p.pbiId === pbiId;
}

/** Read the PBI's journal and, if one is left over, save it as a hold and
 * only then clear it - the hold is on disk before the journal goes. */
export async function recoverJournal(org: string, pbiId: number): Promise<RecoverOutcome> {
  if (runningHere(org, pbiId)) return "running";
  let read;
  try {
    read = await commands.uploadJournalRead(org, pbiId);
  } catch {
    return "none";
  }
  if (!read || read.status === "error") return "none";
  const found = read.data;
  if (!found) return "none";
  if (found.kind === "running") return "running";
  if (found.kind !== "found") return "none";
  // The read crossed IPC: an upload may have started meanwhile.
  if (runningHere(org, pbiId)) return "running";
  saveHold(org, pbiId, mergeHolds(loadHold(org, pbiId), holdFromJournal(found.journal)));
  try {
    const cleared = await commands.uploadJournalClear(org, pbiId);
    if (cleared && cleared.status === "error") logUi(`upload journal not cleared: ${cleared.error}`);
  } catch (e) {
    logUi(`upload journal not cleared: ${String(e)}`);
  }
  return "recovered";
}

/** The journal's rows that did NOT come from the upload that just finished.
 * Rust merges a new upload's create titles after the older ones, so when this
 * upload's journal write succeeded they are the journal's trailing rows.
 * `thisUploadTitles` is the SENT rows with no update id, in order (how Rust
 * builds them; empty when the upload created nothing). They are subtracted
 * only when the journal's trailing titles equal them exactly: if this
 * upload's own write failed, the journal is purely the older upload's and is
 * held whole. Null when nothing older is left. */
export function olderPartOf(j: UploadJournal, thisUploadTitles: string[]): UploadJournal | null {
  const n = thisUploadTitles.length;
  const tail = j.titles.slice(j.titles.length - n);
  const ours = n > 0 && n <= j.titles.length && tail.every((t, i) => t === thisUploadTitles[i]);
  const older = ours ? j.titles.length - n : j.titles.length;
  if (older <= 0) return null;
  return {
    ...j,
    titles: j.titles.slice(0, older),
    sigs: j.sigs.length === j.titles.length ? j.sigs.slice(0, older) : [],
  };
}

/** After a finished upload: keep any older, unrecovered upload's rows as a
 * hold, then clear the journal. Never throws: the upload already happened. */
export async function finishJournal(
  org: string,
  pbiId: number,
  thisUploadTitles: string[],
  createdIds: number[],
): Promise<void> {
  try {
    const read = await commands.uploadJournalRead(org, pbiId);
    if (read && read.status !== "error" && read.data && read.data.kind === "found") {
      const older = olderPartOf(read.data.journal, thisUploadTitles);
      if (older) {
        const merged = mergeHolds(loadHold(org, pbiId), holdFromJournal(older));
        // The hold applyOutcome just saved carries this upload's preExisting,
        // fetched AFTER the older upload ran, so it already lists the cases
        // the older upload created; unioning those ids would hide them from
        // Check (checkExcludeIds) and let a duplicate through. The right
        // exclusion set is the older upload's baseline plus the ids THIS
        // upload confirmed.
        saveHold(org, pbiId, { ...merged, ids: [...new Set([...older.ids, ...createdIds])] });
      }
    } else if (read && read.status === "error") {
      logUi(`upload journal not read: ${read.error}`);
    }
  } catch (e) {
    logUi(`upload journal not read: ${String(e)}`);
  }
  try {
    const cleared = await commands.uploadJournalClear(org, pbiId);
    if (cleared.status === "error") logUi(`upload journal not cleared: ${cleared.error}`);
  } catch (e) {
    logUi(`upload journal not cleared: ${String(e)}`);
  }
}
