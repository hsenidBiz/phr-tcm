// Where a case's setup approval stands, as the script editor and the case
// card both read it. One query key, so approving in the editor refreshes the
// card behind it.

import { commands, type SetupView } from "../../bindings";
import { unwrapStr } from "../../lib/ipc";

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
