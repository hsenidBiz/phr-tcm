// The Modules page's words, verbatim from the spec
// (docs/superpowers/specs/2026-10-15-modules-design.md). Sentence case.
import type { Module } from "../../lib/modules";

export interface ModuleInfo {
  module: Module;
  name: string;
  /** The module this one needs, or null when it hangs off Core. */
  parent: Module | null;
  description: string;
}

/** Tree order: a parent always comes before what hangs off it. */
export const MODULE_INFO: readonly ModuleInfo[] = [
  {
    module: "db_access",
    name: "Database Access",
    parent: null,
    description:
      "Connect TCM to your test environments' databases. Your assistant can look data up, and Auto Run uses it to check preconditions and to notice when the VPN drops.",
  },
  {
    module: "writing_style",
    name: "Writing Style",
    parent: null,
    description:
      "Your own rules for how test cases are worded and split into steps. Your assistant follows them instead of the standard guide when it writes cases.",
  },
  {
    module: "auto_run",
    name: "Auto Run",
    parent: "db_access",
    description:
      "Script a test case once, then run it in a browser, watched or unattended, and review the results before they go to Azure DevOps. Includes environments, accounts, areas and test files.",
  },
  {
    module: "api_templates",
    name: "API Templates",
    parent: "auto_run",
    description:
      "Create the test data a case needs, such as cycles, drafts and settings, through the app's own pages before the case runs, so every run starts from a known state.",
  },
  {
    module: "playwright",
    name: "Playwright",
    parent: "auto_run",
    description:
      "Export passing Auto Run scripts into the team's Playwright repo, run its tests, manage its test accounts and keep exported cases in sync.",
  },
  {
    module: "crystal_reports",
    name: "Crystal Reports",
    parent: null,
    description:
      "Lets your AI assistant open, format and preview Crystal Reports files, and change the stored procedures behind them on the Dev login.",
  },
];

export const CORE_NAME = "Core";
export const CORE_LINE = "Test cases, suites, runs and the board";

export function infoOf(m: Module): ModuleInfo {
  return MODULE_INFO.find((i) => i.module === m)!;
}

export const STATUS_ON = "On";
export const STATUS_OFF = "Off · click to switch on";
export function statusLocked(parent: Module): string {
  return `Locked · needs ${infoOf(parent).name}`;
}

/** "A", "A and B", "A, B and C". */
export function joinNames(names: readonly string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

export function switchOffWarning(name: string, below: readonly string[]): string {
  return `Switching off ${name} also switches off ${joinNames(below)}.`;
}
