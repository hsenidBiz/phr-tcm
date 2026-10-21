// Which modules are switched on, as the webview sees them. Rust owns the
// answer (src-tauri/src/modules.rs: saved per machine in the app data dir,
// asked by every backend gate); this mirrors it so a screen shows only what
// is on. A module off hides its sidebar row, palette item, screen, AI
// Bridge cards and assistant tool rows - it never deletes anything.
//
// Rust also starts a development build with every module on, so there is no
// build-kind check here: the store simply follows Rust. Capture mode (the
// documentation screenshots) reports every module on but the one that is
// offered only on a machine that unlocked it.
import { useSyncExternalStore } from "react";
import { commands, type Module } from "../bindings";
import { isCaptureMode } from "../dev/capture";

export type { Module };

export const ALL_MODULES: readonly Module[] = [
  "db_access",
  "writing_style",
  "auto_run",
  "api_templates",
  "playwright",
  "crystal_reports",
];

/** The one module a machine is offered only once it has unlocked it in
 * Settings (Rust holds it back from the answer until then). */
export const UNLOCK_ONLY_MODULE: Module = "crystal_reports";

/** Capture mode (the documentation screenshots) shows every module but that
 * one: the published guide never shows it. */
const ALL: ReadonlySet<Module> = new Set(ALL_MODULES.filter((m) => m !== UNLOCK_ONLY_MODULE));
const NONE: ReadonlySet<Module> = new Set();

let on: ReadonlySet<Module> = NONE;
/** Bumped by every save, so a read that started before it cannot put the
 * old answer back when it lands late. */
let generation = 0;
/** True once `hydrateModules` has settled at least once, success or not.
 * An empty set reads the same as "still waiting on Rust", so callers that
 * must not act before Rust has answered (the saved-tab redirect) read this. */
let hydrated = false;
const listeners = new Set<() => void>();
const hydratedListeners = new Set<() => void>();

function notify(): void {
  for (const l of [...listeners]) l();
}

function publish(next: readonly Module[]): void {
  const set = new Set(next);
  if (set.size === on.size && [...set].every((m) => on.has(m))) return;
  on = set;
  notify();
}

function publishHydrated(): void {
  if (hydrated) return;
  hydrated = true;
  for (const l of [...hydratedListeners]) l();
}

export function subscribeModules(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

function subscribeModulesHydrated(cb: () => void): () => void {
  hydratedListeners.add(cb);
  return () => {
    hydratedListeners.delete(cb);
  };
}

/** The modules that are on right now. A stable reference between changes. */
export function modulesSnapshot(): ReadonlySet<Module> {
  return isCaptureMode() ? ALL : on;
}

/** The modules really on, without capture mode's all-on view. For what is
 * registered, saved or sent to Rust - never for what is drawn. */
export function realModulesSnapshot(): ReadonlySet<Module> {
  return on;
}

export function modulesHydratedSnapshot(): boolean {
  return isCaptureMode() || hydrated;
}

/** Ask Rust which modules are on. Outside the app (a test without IPC, a
 * browser preview) the answer is "none" - as a fresh install reads. Either
 * way, `hydrated` flips once this settles. */
export async function hydrateModules(): Promise<void> {
  const started = generation;
  try {
    const list = await commands.modulesGet();
    if (started === generation && Array.isArray(list)) publish(list);
  } catch {
    // no IPC here
  }
  if (started === generation) publishHydrated();
  // A save landed while this read was in flight and discarded its answer. If
  // that save was refused, nothing else will ever flip hydrated: ask again.
  else if (!hydrated) await hydrateModules();
}

/** Switch one module through Rust, then tell every screen what Rust says is
 * on now (switching one off can switch others off). Throws Rust's refusal -
 * changing nothing - when it will not. */
export async function setModule(m: Module, next: boolean): Promise<void> {
  generation += 1;
  const r = await commands.modulesSet(m, next);
  if (r.status === "error") throw new Error(r.error);
  publish(r.data);
  publishHydrated();
}

export function useModules(): ReadonlySet<Module> {
  return useSyncExternalStore(subscribeModules, modulesSnapshot);
}

export function useModule(m: Module): boolean {
  return useModules().has(m);
}

export function useModulesHydrated(): boolean {
  return useSyncExternalStore(subscribeModulesHydrated, modulesHydratedSnapshot);
}

/** The module a sidebar section belongs to, or null for one that is always
 * there. */
export function sectionModule(section: string): Module | null {
  switch (section) {
    case "autorun":
      return "auto_run";
    case "apitemplates":
      return "api_templates";
    case "playwright":
      return "playwright";
    case "crystal-reports":
      return "crystal_reports";
    default:
      return null;
  }
}

/** Whether a section is offered with these modules on. */
export function sectionShown(section: string, modules: ReadonlySet<Module> = modulesSnapshot()): boolean {
  const m = sectionModule(section);
  return m === null || modules.has(m);
}

/** Whether the App shell's redirect off a tab that is no longer offered
 * should fire right now. False until hydration settles, so a saved last tab
 * of an off-by-default module is not bounced to Manual Entry before Rust's
 * answer arrives (and a module that is on is never bounced at all). */
export function shouldLeaveHidden(section: string, shown: boolean, hydrated: boolean): boolean {
  return hydrated && !shown && sectionModule(section) !== null;
}

/** Tests only: back to none on and un-hydrated, without telling anyone. */
export function resetModulesStore(): void {
  on = NONE;
  generation = 0;
  hydrated = false;
}

/** Tests only: put exactly these modules on and mark the store hydrated. */
export function setModulesForTest(next: readonly Module[]): void {
  generation += 1;
  publish(next);
  publishHydrated();
}
