// Whether the Auto Run screen has what a run needs, worked out from answers
// the screen already reads - no command of its own.
//
// Every input separates "not known yet" from "none": `undefined` (the site)
// or `null` (the rest) means the answer has not come, or could not be read.
// Neither is ever counted as missing, so a slow or failed read can never
// open the Setup panel by itself - a failed read shows its own error on its
// Setup row instead.

import { useMemo } from "react";
import type { CaseScript } from "../../bindings";

/** The screen's two tabs. Setup is a panel on the Test cases tab. */
export type AutoRunTab = "cases" | "runs";

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
    // Keyed by the lower-cased name so one file asked for in two spellings
    // counts once; the first spelling seen is the one reported.
    const missing = new Map<string, string>();
    for (const script of scripts) {
      for (const step of script?.steps ?? []) {
        for (const action of step.actions) {
          if (action.kind !== "upload") continue;
          const key = action.file.toLowerCase();
          if (!have.has(key) && !missing.has(key)) missing.set(key, action.file);
        }
      }
    }
    return [...missing.values()].sort((a, b) => a.localeCompare(b));
  }, [scripts, testFileNames]);

  return { loaded, essentialMissing, missingTestFiles };
}
