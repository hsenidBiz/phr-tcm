// Persisted app context (org / project / PBI / tab / mode). One loader and
// one writer so no path forgets to persist or trusts corrupt storage.

import type { PbiHit } from "../bindings";
import type { Section } from "../components/Sidebar";
import type { SuiteScope } from "./scope";

export const PREFS_KEY = "tcm-v2-prefs";

export type Prefs = {
  org: string;
  project: string;
  section: Section;
  pbi: PbiHit | null;
  /** The test suite in the top bar, remembered the way the PBI is. Never set with a PBI. */
  suite: SuiteScope | null;
  workMode: boolean;
};

// Every value of the Section union - kept in sync with Sidebar.tsx's own
// CASE_ITEMS plus "settings". Missing one here silently downgrades a saved
// tab to "manual" on the next launch (that hid a real bug in Auto Test: see
// lib/modules.ts's shouldLeaveHidden and its test).
const SECTIONS: Section[] = [
  "manual",
  "import",
  "edit",
  "view",
  "run",
  "autorun",
  "suites",
  "manage",
  "ai",
  "apitemplates",
  "playwright",
  "crystal-reports",
  "settings",
];

function readSuite(raw: unknown, pbi: unknown): SuiteScope | null {
  // A PBI and a suite are never both current: the PBI wins if a file has both.
  if (pbi || !raw || typeof raw !== "object") return null;
  const s = raw as Record<string, unknown>;
  if (typeof s.planId !== "number" || typeof s.suiteId !== "number" || typeof s.title !== "string") return null;
  return {
    planId: s.planId,
    suiteId: s.suiteId,
    title: s.title,
    planName: typeof s.planName === "string" ? s.planName : "",
    ...(typeof s.project === "string" && s.project ? { project: s.project } : {}),
  };
}

export function loadPrefs(): Prefs {
  const defaults: Prefs = {
    org: "",
    project: "",
    section: "manual",
    pbi: null,
    suite: null,
    workMode: false,
  };
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (raw) {
      const p = JSON.parse(raw);
      return {
        org: p.org ?? "",
        project: p.project ?? "",
        section: SECTIONS.includes(p.section) ? p.section : "manual",
        pbi:
          p.pbi && typeof p.pbi.id === "number" && typeof p.pbi.title === "string"
            ? {
                id: p.pbi.id,
                title: p.pbi.title,
                work_item_type: p.pbi.work_item_type ?? "",
                // A PBI saved before PBIs carried a project belongs to the
                // project that was picked then.
                project: typeof p.pbi.project === "string" && p.pbi.project ? p.pbi.project : (p.project ?? ""),
              }
            : null,
        suite: readSuite(p.suite, p.pbi),
        workMode: Boolean(p.workMode),
      };
    }
  } catch {
    // corrupted prefs -> defaults
  }
  return defaults;
}

export function savePrefs(p: Prefs): void {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(p));
  } catch {
    // storage unavailable -> session-only
  }
}
