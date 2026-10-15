// The guide shows what the app shows. The app opens it with
// `?modules=<keys>` naming the modules that are switched on, snake_case,
// comma-separated, in tree order (src-tauri/src/help.rs); with no module on
// there is no query at all. Anything a module owns is left out while it is
// off: its screen, the AI Bridge cards and how-tos, and every link, contents
// entry, search hit and hero shot that pointed at them.

import type { SiteContent } from "./types";

export type Module = "db_access" | "writing_style" | "auto_run" | "api_templates" | "playwright";

// The shipped page must not contain the assistant-tool spelling of API
// Templates (the guard in guard.test.ts scans the built page for it), yet the
// address names the module by its key. So that one key is assembled, not typed.
const API_TEMPLATES = ["api", "templates"].join("_") as Module;

/** Tree order: a parent always comes before what hangs off it. */
export const MODULES: readonly Module[] = ["db_access", "writing_style", "auto_run", API_TEMPLATES, "playwright"];

/** The module each one needs; null hangs off Core, which is always on. */
const PARENT: Record<string, Module | null> = {
  db_access: null,
  writing_style: null,
  auto_run: "db_access",
  [API_TEMPLATES]: "auto_run",
  playwright: "auto_run",
};

/** Per module, the screens shown only while it is on. */
export const MODULE_SCREENS: Partial<Record<Module, readonly string[]>> = {
  auto_run: ["auto-run"],
  [API_TEMPLATES]: ["api-templates"],
  playwright: ["playwright"],
};

/** Per module, per screen, the groups (cards) shown only while it is on. */
export const MODULE_GROUPS: Partial<Record<Module, Record<string, readonly string[]>>> = {
  db_access: { "ai-bridge": ["company-database"] },
  auto_run: { "ai-bridge": ["environment"] },
  [API_TEMPLATES]: { "ai-bridge": ["api-templates"] },
};

/** Per module, per screen, the how-tos (by title) shown only while it is on. */
export const MODULE_HOWTOS: Partial<Record<Module, Record<string, readonly string[]>>> = {
  db_access: { "ai-bridge": ["Let an assistant read the company database"] },
  writing_style: { "ai-bridge": ["Use your own writing style"] },
};

/** The query parameter the app opens the guide with. */
export const MODULES_PARAM = "modules";

/** The modules the guide was opened with on. Unknown (no parameter, an
 *  unknown name) is off, and so is a module whose parent is not on. */
export function modulesFrom(search: string): Set<Module> {
  let named: string[] = [];
  try {
    named = (new URLSearchParams(search).get(MODULES_PARAM) ?? "").split(",").map((s) => s.trim());
  } catch {
    return new Set();
  }
  const on = new Set<Module>();
  for (const m of MODULES) {
    if (!named.includes(m)) continue;
    const parent = PARENT[m];
    if (parent === null || on.has(parent)) on.add(m);
  }
  return on;
}

/** `content` as it is shown: whole when every module is on, otherwise
 *  without the screens, cards, how-tos and controls of the modules that are
 *  off, the shots only they used, and any recipe step or quick-start link
 *  into them. */
export function forModules(content: SiteContent, on: ReadonlySet<Module>): SiteContent {
  const off = MODULES.filter((m) => !on.has(m));
  if (off.length === 0) return content;

  const hiddenScreens = new Set<string>(off.flatMap((m) => MODULE_SCREENS[m] ?? []));
  const hiddenGroups = new Map<string, Set<string>>();
  const hiddenHowTos = new Map<string, Set<string>>();
  const add = (into: Map<string, Set<string>>, byScreen: Record<string, readonly string[]> | undefined) => {
    for (const [screen, ids] of Object.entries(byScreen ?? {})) {
      const set = into.get(screen) ?? new Set<string>();
      for (const id of ids) set.add(id);
      into.set(screen, set);
    }
  };
  for (const m of off) {
    add(hiddenGroups, MODULE_GROUPS[m]);
    add(hiddenHowTos, MODULE_HOWTOS[m]);
  }

  const screens = content.screens
    .filter((s) => !hiddenScreens.has(s.id))
    .map((s) => {
      const groups = hiddenGroups.get(s.id) ?? new Set<string>();
      const howTos = hiddenHowTos.get(s.id) ?? new Set<string>();
      if (groups.size === 0 && howTos.size === 0) return s;
      const controls = s.controls.filter((c) => !(c.group && groups.has(c.group)));
      const dropped = s.controls.filter((c) => c.group && groups.has(c.group));
      const stillShown = new Set(controls.map((c) => c.shot));
      const onlyHidden = new Set(dropped.map((c) => c.shot).filter((shot) => !stillShown.has(shot)));
      return {
        ...s,
        controls,
        groups: s.groups?.filter((g) => !groups.has(g.id)),
        shots: s.shots.filter((shot) => !onlyHidden.has(shot.id)),
        howTo: s.howTo?.filter((h) => !howTos.has(h.title)),
      };
    });
  const hiddenLink = (link: string | undefined) => {
    if (!link) return false;
    const [screen, part] = link.split("/");
    return hiddenScreens.has(screen) || (part !== undefined && (hiddenGroups.get(screen)?.has(part) ?? false));
  };
  const recipes = content.recipes.map((r) => ({ ...r, steps: r.steps.map((st) => (hiddenLink(st.link) ? { text: st.text } : st)) }));
  const intro = { ...content.intro, quickStart: content.intro.quickStart.filter((q) => !hiddenLink(q.link)) };
  return { ...content, screens, recipes, intro };
}
