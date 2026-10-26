// "Treat all projects as one": the switch, the effective project, and the
// helpers every screen shares so a person's saved choices survive the
// switch being on.
//
// Settings holds the switch. While it is on there is no project picker:
// each PBI carries its own project, and what has no PBI falls back to the
// home project (prefs.project). The store mirrors networkTools.ts without
// the environment scoping - it is a property of the profile, not of an
// environment.

import { useSyncExternalStore } from "react";

import { tourRunningSnapshot } from "../tour/tourState";

/** "0" = off; anything else, or absent = on. */
export const ALL_PROJECTS_KEY = "tcm-v2-all-projects";
/** Written the first time the one-time notice is asked about. */
export const ALL_PROJECTS_NOTICE_KEY = "tcm-v2-all-projects-notice";

/** The `<org>/*` key segment used by recents, lanes, pins and tracked
 * pull request repositories while the projects show together. */
export const COMBINED = "*";

const listeners = new Set<() => void>();

function notify(): void {
  for (const l of listeners) l();
}

export function subscribeAllProjects(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Whether the projects show together. On unless it was switched off;
 * storage that cannot be read reads on too. */
export function loadAllProjects(): boolean {
  try {
    return localStorage.getItem(ALL_PROJECTS_KEY) !== "0";
  } catch {
    return true;
  }
}

/** Saves the switch. The tour runs on sample data and must not change a
 * real choice, so it is refused while the tour is running. */
export function saveAllProjects(on: boolean): void {
  if (tourRunningSnapshot()) return;
  try {
    localStorage.setItem(ALL_PROJECTS_KEY, on ? "1" : "0");
  } catch {
    // A failed write leaves storage as it was, and the snapshot re-reads
    // storage, so the switch shows its stored value again.
  }
  notify();
}

/** A primitive, so `useSyncExternalStore` is happy to re-read it. */
export function allProjectsSnapshot(): boolean {
  return loadAllProjects();
}

export function useAllProjects(): boolean {
  return useSyncExternalStore(subscribeAllProjects, allProjectsSnapshot);
}

/** The project every PBI-scoped screen and write uses: the PBI's own while
 * the projects show together, otherwise the home project. `||` rather than
 * `??`, so a PBI saved with an empty project still lands somewhere. */
export function effectiveProject(
  allProjects: boolean,
  pbi: { project?: string } | null,
  homeProject: string,
): string {
  return allProjects ? pbi?.project || homeProject : homeProject;
}

/** True exactly once per profile, for an existing install: the notice key
 * is absent, the switch was never set, and a project was already chosen.
 * The notice key is written the first time this is asked, so a fresh
 * install (nothing chosen yet) never sees the notice later. */
export function takeAllProjectsNotice(hadProject: boolean): boolean {
  try {
    if (localStorage.getItem(ALL_PROJECTS_NOTICE_KEY) !== null) return false;
    const switchSet = localStorage.getItem(ALL_PROJECTS_KEY) !== null;
    localStorage.setItem(ALL_PROJECTS_NOTICE_KEY, "1");
    return hadProject && !switchSet;
  } catch {
    return false;
  }
}

/** Whether a list holds items of more than one project - the cue to show
 * each item's project beside it. Blanks do not count. */
export function spansProjects(items: ReadonlyArray<{ project?: string | null }>): boolean {
  const seen = new Set<string>();
  for (const item of items) {
    if (item.project) seen.add(item.project);
    if (seen.size > 1) return true;
  }
  return false;
}

/** The segment of a `<org>/<segment>` key: `*` while the projects show
 * together, otherwise the project. */
export function scopeSegment(project: string, allProjects: boolean): string {
  return allProjects ? COMBINED : project;
}

/** Copies what a person saved under a per-project key into the combined
 * key, once: only when the combined key is absent and the source holds a
 * value. The source is never removed (switching off must find it exactly
 * as it was) and an existing combined key is never overwritten. `fill`
 * reshapes the parsed value on the way over; a value that does not parse,
 * or a `fill` that throws, copies nothing. Refused during the tour. */
export function seedCombinedKey(
  combinedKey: string,
  fromKey: string,
  fill?: (value: unknown) => unknown,
): void {
  if (tourRunningSnapshot()) return;
  try {
    if (localStorage.getItem(combinedKey) !== null) return;
    const raw = localStorage.getItem(fromKey);
    if (raw === null) return;
    const value = fill ? JSON.stringify(fill(JSON.parse(raw))) : raw;
    // A reshaper that returns undefined has nothing to store.
    if (value === undefined) return;
    localStorage.setItem(combinedKey, value);
  } catch {
    // unreadable storage or bad saved data -> nothing is copied
  }
}
