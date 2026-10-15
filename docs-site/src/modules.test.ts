// The guide shows the pages of the modules that are on, and nothing of the
// ones that are off; unknown means hidden. The app opens it with
// `?modules=<snake_case keys>` (src-tauri/src/help.rs).

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import positions from "../shots/positions.json";
import { MODULES, MODULE_GROUPS, MODULE_HOWTOS, MODULE_SCREENS, forModules, modulesFrom, type Module } from "./modules";
import { intro, recipes, screens } from "./content";
import { render } from "./render/layout";
import type { Positions, SiteContent } from "./types";

const content: SiteContent = { screens, recipes, intro, positions: positions as Positions, available: [] };
const ALL = new Set<Module>(MODULES);
const without = (...off: Module[]) => new Set<Module>(MODULES.filter((m) => !off.includes(m)));

const PARENT: Record<Module, Module | null> = {
  db_access: null,
  writing_style: null,
  auto_run: "db_access",
  api_templates: "auto_run",
  playwright: "auto_run",
};
const below = (x: Module): Module[] => [x, ...MODULES.filter((c) => PARENT[c] === x).flatMap(below)];

describe("the address", () => {
  test("?modules= names the modules that are on; unknown names are ignored", () => {
    expect([...modulesFrom("?modules=db_access,writing_style,auto_run,api_templates,playwright")]).toEqual([...MODULES]);
    expect([...modulesFrom("?modules=db_access,auto_run")]).toEqual(["db_access", "auto_run"]);
    expect([...modulesFrom("?x=2&modules=db_access,nonsense")]).toEqual(["db_access"]);
  });

  test("anything else, or nothing, is off; the old ?advanced=1 turns nothing on", () => {
    for (const search of ["", "?modules=", "?advanced=1", "?modules", "?other=1", "?%zz"]) {
      expect(modulesFrom(search).size, search).toBe(0);
    }
  });

  test("a module whose parent is not named stays off", () => {
    expect([...modulesFrom("?modules=auto_run,api_templates,playwright")]).toEqual([]);
    expect([...modulesFrom("?modules=db_access,api_templates")]).toEqual(["db_access"]);
  });
});

describe("the lists cannot go stale", () => {
  test("every named screen, card and how-to exists", () => {
    for (const ids of Object.values(MODULE_SCREENS)) for (const id of ids ?? []) expect(screens.some((s) => s.id === id), id).toBe(true);
    for (const byScreen of Object.values(MODULE_GROUPS)) {
      for (const [screen, groups] of Object.entries(byScreen ?? {})) {
        const s = screens.find((x) => x.id === screen);
        expect(s, screen).toBeTruthy();
        for (const g of groups) expect(s!.groups?.some((x) => x.id === g), `${screen}/${g}`).toBe(true);
      }
    }
    for (const byScreen of Object.values(MODULE_HOWTOS)) {
      for (const [screen, titles] of Object.entries(byScreen ?? {})) {
        const s = screens.find((x) => x.id === screen);
        for (const t of titles) expect(s?.howTo?.some((h) => h.title === t), `${screen}: ${t}`).toBe(true);
      }
    }
  });

  test("the five modules are covered", () => {
    expect(MODULES).toEqual(["db_access", "writing_style", "auto_run", "api_templates", "playwright"]);
    expect(Object.keys(MODULE_SCREENS).sort()).toEqual(["api_templates", "auto_run", "playwright"]);
    expect(MODULE_SCREENS.auto_run).toEqual(["auto-run"]);
    expect(MODULE_SCREENS.api_templates).toEqual(["api-templates"]);
    expect(MODULE_SCREENS.playwright).toEqual(["playwright"]);
    expect(MODULE_GROUPS.auto_run?.["ai-bridge"]).toEqual(["environment"]);
    expect(MODULE_GROUPS.api_templates?.["ai-bridge"]).toEqual(["api-templates"]);
    expect(MODULE_GROUPS.db_access?.["ai-bridge"]).toEqual(["company-database"]);
  });
});

describe("the content", () => {
  test("all on, nothing is left out", () => {
    expect(forModules(content, ALL)).toBe(content);
  });

  test("none on, every module's screens, cards and how-tos go", () => {
    const shown = forModules(content, new Set());
    const ids = shown.screens.map((s) => s.id);
    for (const id of ["auto-run", "api-templates", "playwright"]) expect(ids).not.toContain(id);
    const bridge = shown.screens.find((s) => s.id === "ai-bridge")!;
    for (const g of ["environment", "api-templates", "company-database"]) {
      expect(bridge.groups?.some((x) => x.id === g), g).toBe(false);
      expect(bridge.controls.some((c) => c.group === g), g).toBe(false);
    }
    expect(bridge.howTo?.some((h) => h.title === "Use your own writing style")).toBe(false);
    expect(bridge.howTo?.some((h) => h.title === "Let an assistant read the company database")).toBe(false);
  });

  test.each(MODULES.map((m) => [m] as const))("%s off removes exactly its own parts (and what hangs below it)", (m) => {
    const off = below(m);
    const shown = forModules(content, without(...off));
    const shownIds = shown.screens.map((s) => s.id);
    for (const mod of MODULES) {
      for (const id of MODULE_SCREENS[mod] ?? []) expect(shownIds.includes(id), `${m} off: ${id}`).toBe(!off.includes(mod));
      for (const [screen, groups] of Object.entries(MODULE_GROUPS[mod] ?? {})) {
        const s = shown.screens.find((x) => x.id === screen)!;
        for (const g of groups) expect(s.groups?.some((x) => x.id === g), `${m} off: ${screen}/${g}`).toBe(!off.includes(mod));
      }
      for (const [screen, titles] of Object.entries(MODULE_HOWTOS[mod] ?? {})) {
        const s = shown.screens.find((x) => x.id === screen)!;
        for (const t of titles) expect(s.howTo?.some((h) => h.title === t), `${m} off: ${t}`).toBe(!off.includes(mod));
      }
    }
    // Every control left still has its shot; nothing links into what is hidden.
    for (const s of shown.screens) for (const c of s.controls) expect(s.shots.some((x) => x.id === c.shot), `${s.id}/${c.id}`).toBe(true);
    const hiddenIds = new Set(off.flatMap((x) => MODULE_SCREENS[x] ?? []));
    const hidden = (link?: string) => !!link && [...hiddenIds].some((id) => link === id || link.startsWith(`${id}/`));
    for (const r of shown.recipes) for (const st of r.steps) expect(hidden(st.link), `${r.id}: ${st.link}`).toBe(false);
    for (const q of shown.intro.quickStart) expect(hidden(q.link), q.label).toBe(false);
  });

  test("the rendered page has no node of an off module", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const site = render(root, forModules(content, new Set<Module>(["db_access", "writing_style"])));
    try {
      for (const id of ["auto-run", "api-templates", "playwright"]) {
        expect(root.querySelector(`[id="${id}"]`), id).toBeNull();
        expect(root.querySelector(`a[href="#${id}"]`), id).toBeNull();
      }
      expect(root.querySelector('[id="ai-bridge/api-templates"]')).toBeNull();
      expect(root.querySelector('[id="ai-bridge/environment"]')).toBeNull();
      expect(root.querySelector('[id="ai-bridge/company-database"]')).not.toBeNull();
      expect(root.querySelector('[id="settings/modules"]')).not.toBeNull();
    } finally {
      site.destroy();
      root.remove();
    }
  });

  test("on, every module's section is there", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const site = render(root, forModules(content, ALL));
    try {
      for (const id of ["auto-run", "api-templates", "playwright"]) expect(root.querySelector(`[id="${id}"]`), id).not.toBeNull();
    } finally {
      site.destroy();
      root.remove();
    }
  });
});

describe("the Modules section and the old switch", () => {
  const text = JSON.stringify({ screens, recipes, intro });

  test("no help text names the Advanced Features switch", () => {
    expect(text).not.toMatch(/advanced feature/i);
    expect(text).not.toMatch(/\badvanced\b/i);
  });

  test("Settings documents the Modules page with the app's own wording", () => {
    const settings = screens.find((s) => s.id === "settings")!;
    expect(settings.groups?.some((g) => g.id === "modules")).toBe(true);
    const row = settings.controls.find((c) => c.id === "open-modules")!;
    expect(row.group).toBe("modules");
    expect(row.locate).toEqual({ role: "button", name: "Modules" });
    const copy = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "..", "..", "src", "screens", "Modules", "modulesCopy.ts"), "utf8");
    const section = JSON.stringify(settings);
    for (const line of ["Test cases, suites, runs and the board", "Off · click to switch on", "Locked · needs", "Switch on the parts of the app you need, such as Auto Run."]) {
      expect(section, line).toContain(line);
    }
    const descriptions = [...copy.matchAll(/^\s+"((?:[^"\\]|\\.)+)",?$/gm)].map((m) => m[1]);
    expect(descriptions.length).toBe(5);
    for (const d of descriptions) expect(section, d).toContain(d);
  });
});
