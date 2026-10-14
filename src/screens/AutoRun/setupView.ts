// A case's setup as the script editor and the case card both read it. One
// query key, so a script saved anywhere refreshes both.

import { commands, type SetupView } from "../../bindings";
import { unwrapStr } from "../../lib/ipc";

/** The cache key of case `caseId`'s setup view. */
export const setupViewKey = (org: string, project: string, caseId: number) =>
  ["autorun-setup", org, project, caseId] as const;

/** Reads case `caseId`'s setup view: null when its saved script has no setup. */
export const loadSetupView = (org: string, project: string, caseId: number): Promise<SetupView | null> =>
  unwrapStr(commands.autoRunSetupView(org, project, caseId));
