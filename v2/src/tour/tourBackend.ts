/**
 * While the tour runs, the app's reads answer from `tourData` instead of
 * Azure DevOps. The originals are saved and put back when it ends, so a
 * tour that is skipped, finished or interrupted can never leave the app
 * serving an organisation that does not exist.
 *
 * Reads only. The app is locked while the tour is up, so nothing can ask
 * to write - and a write that somehow got through would find the real
 * call, which needs a real organisation and would refuse.
 */
import { commands } from "../bindings";
import {
  TOUR_BOARD,
  TOUR_BRIDGE,
  TOUR_CASES,
  TOUR_CASE_SUMMARIES,
  TOUR_DB_PRESETS,
  TOUR_HISTORY,
  TOUR_ORGS,
  TOUR_PBI,
  TOUR_PLANS,
  TOUR_POINTS,
  TOUR_PROJECTS,
  TOUR_PR_OVERVIEW,
  TOUR_SUITE,
  TOUR_TOOLS,
} from "./tourData";

const ok = <T,>(data: T) => Promise.resolve({ status: "ok" as const, data });

type Commands = typeof commands;
let saved: Record<string, unknown> | null = null;

function standIns(): Record<string, unknown> {
  return {
    // Scope pickers.
    listOrgs: () => ok(TOUR_ORGS),
    listProjects: () => ok(TOUR_PROJECTS),
    searchPbis: () => ok([TOUR_PBI]),

    // Manual Entry's field pickers.
    listTestCaseFields: () => ok([]),
    testCaseFieldValues: () => ok(["Checkout", "Basket", "Sign in"]),
    listProjectTags: () => ok(["Checkout", "Regression", "Payments"]),
    classificationPaths: () => ok(["Website", "Website\\Checkout"]),
    listIterations: () => ok([{ path: "Website", start_date: null, finish_date: null }]),
    activityValues: () => ok(["Testing"]),

    // Cases.
    pbiTestCases: () => ok(TOUR_CASE_SUMMARIES),
    pbiTestCasesFull: () => ok(TOUR_CASES),
    testCasesByIds: () => ok(TOUR_CASES),

    // Suites and runs.
    ensurePbiSuite: () => ok(TOUR_SUITE),
    findPbiSuite: () => ok(null),
    listPlansWithSuites: () => ok(TOUR_PLANS),
    listTestPoints: () => ok(TOUR_POINTS),
    runHistory: () => ok(TOUR_HISTORY),

    // Work Manager.
    fetchBoard: () => ok(TOUR_BOARD),
    boardPrLinks: () => ok([]),
    prOverview: () => ok(TOUR_PR_OVERVIEW),

    // AI Bridge.
    bridgeStatus: () => ok(TOUR_BRIDGE),
    detectAiTools: () => Promise.resolve(TOUR_TOOLS),
    dbServerPresets: () => Promise.resolve(TOUR_DB_PRESETS),
  };
}

export function tourBackendInstalled(): boolean {
  return saved !== null;
}

export function installTourBackend(): void {
  if (saved) return; // already on - saving now would trap the stand-ins
  const fakes = standIns();
  const keep: Record<string, unknown> = {};
  const target = commands as unknown as Record<string, unknown>;
  for (const name of Object.keys(fakes)) keep[name] = target[name];
  saved = keep;
  Object.assign(commands as unknown as Commands, fakes);
}

export function restoreTourBackend(): void {
  if (!saved) return;
  Object.assign(commands as unknown as Record<string, unknown>, saved);
  saved = null;
}
