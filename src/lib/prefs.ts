// Persisted app context (org / project / PBI / tab / mode). One loader and
// one writer so no path forgets to persist or trusts corrupt storage.

import type { PbiHit } from "../bindings";
import type { Section } from "../components/Sidebar";

export const PREFS_KEY = "tcm-v2-prefs";

export type Prefs = {
  org: string;
  project: string;
  section: Section;
  pbi: PbiHit | null;
  workMode: boolean;
};

const SECTIONS: Section[] = ["manual", "import", "edit", "view", "run", "suites", "ai", "settings"];

export function loadPrefs(): Prefs {
  const defaults: Prefs = {
    org: "",
    project: "",
    section: "manual",
    pbi: null,
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
            ? { id: p.pbi.id, title: p.pbi.title, work_item_type: p.pbi.work_item_type ?? "" }
            : null,
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
