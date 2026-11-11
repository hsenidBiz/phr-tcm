/**
 * Turns the journal Rust writes before an upload's first batch into the
 * existing "Outcome unknown" hold, when the upload never finished (the app
 * closed, crashed or lost power mid-write). A finished upload clears its own
 * journal; whatever is still here belongs to one that did not.
 */
import { commands } from "../bindings";
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
