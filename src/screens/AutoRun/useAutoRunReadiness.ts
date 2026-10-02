// Whether the Auto Run screen has what a run needs, worked out from answers
// the screen already reads - no command of its own.
//
// Every input separates "not known yet" from "none": `undefined` (the site)
// or `null` (the rest) means the answer has not come, or could not be read.
// Neither is ever counted as missing, so a slow or failed read can never
// send the screen to Setup by itself - a failed read shows its own error on
// its Setup row instead.

import { useMemo } from "react";
import type { CaseScript } from "../../bindings";

/** The screen's three tabs. */
export type AutoRunTab = "cases" | "runs" | "setup";

export function useAutoRunReadiness(input: {
  /** Where a run goes. `undefined` while unknown; `null` or "" is none. */
  siteUrl: string | null | undefined;
  /** A saved recipe, the built-in one, or no way to sign in at all. */
  signIn: "saved" | "builtin" | "none" | null;
  accountCount: number | null;
  areaCount: number | null;
  /** The listed cases' scripts, as their queries hold them. */
  scripts: (CaseScript | null | undefined)[];
  /** The names in the project's Test files folder. */
  testFileNames: string[] | null;
}): { loaded: boolean; essentialMissing: boolean; missingTestFiles: string[] } {
  const { siteUrl, signIn, accountCount, scripts, testFileNames } = input;

  // Only the three a run cannot go without decide anything. Areas and test
  // files are reported, but a project can run without either.
  const loaded = siteUrl !== undefined && signIn !== null && accountCount !== null;
  const essentialMissing =
    (siteUrl !== undefined && !siteUrl?.trim()) || signIn === "none" || accountCount === 0;

  // Every upload action of every listed case's script, against the folder.
  const missingTestFiles = useMemo(() => {
    if (testFileNames === null) return [];
    // Windows file names: cv.txt and CV.TXT are the same file.
    const have = new Set(testFileNames.map((n) => n.toLowerCase()));
    const missing = new Set<string>();
    for (const script of scripts) {
      for (const step of script?.steps ?? []) {
        for (const action of step.actions) {
          if (action.kind === "upload" && !have.has(action.file.toLowerCase())) missing.add(action.file);
        }
      }
    }
    return [...missing].sort((a, b) => a.localeCompare(b));
  }, [scripts, testFileNames]);

  return { loaded, essentialMissing, missingTestFiles };
}
