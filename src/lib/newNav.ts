// The sidebar's "newly enabled" marks: a row a person has just revealed by
// switching a module on from the Modules page carries a highlight until
// they open it. Remembered per machine (like prefs.ts), so a restart before
// the first visit keeps the mark.
//
// Only `noteModuleSwitch` ever creates a mark, and only the Modules page
// calls it, after a person's own switch. Hydration from Rust, an update's or
// a fresh install's defaults, capture mode and demo mode never mark
// anything.
import { useSyncExternalStore } from "react";
import type { Section } from "../components/Sidebar";
import { isCaptureMode } from "../dev/capture";
import { sectionModule, sectionShown, type Module } from "./modules";

export const NEW_NAV_KEY = "tcm-v2-new-nav";

/** Section -> the modules whose switch-on marked it. A section is marked
 * while its list is not empty, so switching one of two modules off (both
 * add cards to AI Bridge) leaves the other's mark in place. */
type Marks = Readonly<Record<string, readonly Module[]>>;

const EMPTY: Marks = {};
let marks: Marks = EMPTY;
let loaded = false;
const listeners = new Set<() => void>();

/** The row each module reveals. Auto Run, API Templates and Playwright own
 * a tab each; Database Access and Writing Style add cards to AI Bridge. */
export function moduleNavItem(m: Module): Section {
  switch (m) {
    case "db_access":
    case "writing_style":
      return "ai";
    case "auto_run":
      return "autorun";
    case "api_templates":
      return "apitemplates";
    case "playwright":
      return "playwright";
  }
}

function load(): Marks {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(NEW_NAV_KEY) ?? "{}");
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) return EMPTY;
    const out: Record<string, Module[]> = {};
    for (const [section, list] of Object.entries(raw as Record<string, unknown>)) {
      if (!Array.isArray(list)) continue;
      const mods = list.filter((v): v is Module => typeof v === "string");
      if (mods.length > 0) out[section] = [...new Set(mods)];
    }
    return out;
  } catch {
    return EMPTY;
  }
}

function current(): Marks {
  if (!loaded) {
    marks = load();
    loaded = true;
  }
  return marks;
}

function commit(next: Marks): void {
  marks = next;
  loaded = true;
  try {
    localStorage.setItem(NEW_NAV_KEY, JSON.stringify(next));
  } catch {
    // storage unavailable -> session-only
  }
  for (const l of [...listeners]) l();
}

function demoMode(): boolean {
  if (!import.meta.env.DEV) return false;
  try {
    return localStorage.getItem("tcm-v2-dev-demo") === "on";
  } catch {
    return false;
  }
}

/** A person switched a module on or off from the Modules page and Rust
 * answered: `before` and `after` are the modules really on either side of
 * it. Every module that came on marks the row it reveals (a tab only when
 * it was not already showing); every module that went off takes its mark
 * away. */
export function noteModuleSwitch(before: ReadonlySet<Module>, after: ReadonlySet<Module>): void {
  const was = current();
  const next: Record<string, Module[]> = {};
  for (const [s, list] of Object.entries(was)) next[s] = [...list];
  let changed = false;
  // Off first: a module that went off takes its mark wherever it is.
  for (const m of before) {
    if (after.has(m)) continue;
    for (const s of Object.keys(next)) {
      if (!next[s].includes(m)) continue;
      next[s] = next[s].filter((x) => x !== m);
      if (next[s].length === 0) delete next[s];
      changed = true;
    }
  }
  if (!isCaptureMode() && !demoMode()) {
    for (const m of after) {
      if (before.has(m)) continue;
      const s = moduleNavItem(m);
      // A tab that was already in the rail is not new. AI Bridge always
      // is, and is marked for the cards it gains.
      if (sectionModule(s) !== null && sectionShown(s, before)) continue;
      const list = next[s] ?? [];
      if (list.includes(m)) continue;
      next[s] = [...list, m];
      changed = true;
    }
  }
  if (changed) commit(next);
}

/** The row has been opened: its mark is done. */
export function clearNewNav(section: string): void {
  const was = current();
  if (!(section in was)) return;
  const next: Record<string, readonly Module[]> = { ...was };
  delete next[section];
  commit(next);
}

/** Drop marks left by modules that are no longer on (switched off another
 * way, or reset by an update). Never adds one. */
export function pruneNewNav(on: ReadonlySet<Module>): void {
  const was = current();
  const next: Record<string, readonly Module[]> = {};
  let changed = false;
  for (const [s, list] of Object.entries(was)) {
    const kept = list.filter((m) => on.has(m));
    if (kept.length !== list.length) changed = true;
    if (kept.length > 0) next[s] = kept;
  }
  if (changed) commit(next);
}

export function subscribeNewNav(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** The marked sections. A stable reference between changes. */
export function newNavSnapshot(): Marks {
  return current();
}

export function isNewNav(section: string, m: Marks = current()): boolean {
  return (m[section]?.length ?? 0) > 0;
}

export function useNewNav(): Marks {
  return useSyncExternalStore(subscribeNewNav, newNavSnapshot);
}

/** Tests only: forget the in-memory copy, so the next read comes from
 * storage again (as a restart would). */
export function resetNewNavForTest(): void {
  marks = EMPTY;
  loaded = false;
}
