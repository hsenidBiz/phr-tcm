// Where a case's setup approval stands, as the script editor and the case
// card both read it. One query key, so approving in the editor refreshes the
// card behind it.

import { commands, type SetupView } from "../../bindings";
import { unwrapStr } from "../../lib/ipc";
import { logUi } from "../../lib/uiLog";

/** The cache key of case `caseId`'s setup view. */
export const setupViewKey = (org: string, project: string, caseId: number) =>
  ["autorun-setup", org, project, caseId] as const;

/** Reads case `caseId`'s setup view: null when its saved script has no setup. */
export const loadSetupView = (org: string, project: string, caseId: number): Promise<SetupView | null> =>
  unwrapStr(commands.autoRunSetupView(org, project, caseId));

/** The approval in a person's words, for the card's line. */
export function approvalWords(approval: string): string {
  if (approval === "approved") return "approved";
  if (approval === "changed") return "changed since approved";
  return "not approved";
}

/** Said when Approve setup is refused because the setup moved under the
 * person: the view is read again so they see what they would approve. */
export const CHANGED_WHILE_LOOKING =
  "the setup changed while you were looking at it - review it again before approving";

/** Of `caseIds`, the cases whose setup is not approved for what is saved
 * now: a run Blocks each of them. A check that fails answers none, so it
 * never holds a run back; the run itself still Blocks such a case. */
export async function unapprovedSetups(org: string, project: string, caseIds: number[]): Promise<number[]> {
  if (caseIds.length === 0) return [];
  try {
    const ids = await unwrapStr(commands.autoRunUnapprovedSetups(org, project, caseIds));
    return Array.isArray(ids) ? ids : [];
  } catch (e) {
    logUi(`auto-run: the setup approvals could not be read: ${e instanceof Error ? e.message : String(e)}`);
    return [];
  }
}
