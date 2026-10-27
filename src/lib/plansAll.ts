// The test plans and suites of every project of an organization, for the
// screens that browse suites without a PBI ("treat all projects as one").
//
// Azure DevOps has no organization-level route for a project's test plans,
// and the Rust `list_plans_with_suites` command has no cache of its own, so
// the combined list is built here: one project at a time, each through the
// same per-project webview cache the single-project Suites screen uses
// (`cacheKeys.plansSuites`). No key of its own - a project scanned before,
// by this screen or the other, is already warm.
//
// The walk is kept honest the way ado/fanout.rs keeps the Rust ones:
// capped, never a silent partial list. A project the person may not read is
// skipped and counted out of `shown`; any other failure (an expired
// sign-in, a throttle, a server error) fails the whole call.

import type { PlanWithSuites } from "../bindings";
import { commands } from "../bindings";
import { CACHE, cacheEntry, cacheKeys, cacheWrite } from "./cache";
import { describeAdoError } from "./ipc";

/** The most projects a fan-out reads. Mirrors `MAX_FANOUT_PROJECTS` in
 * src-tauri/src/ado/fanout.rs; an organization with more is read for its
 * first projects by name and says so. */
export const MAX_FANOUT_PROJECTS = 100;

export type ProjectPlans = { project: string; plans: PlanWithSuites[] };

/** What was read and how much of the organization that is: `shown <
 * total` means the screen must say so. `groups` holds only projects that
 * have plans, in project-name order. */
export type PlansAll = { groups: ProjectPlans[]; shown: number; total: number };

/**
 * Plans and suites from every project in `projects`, name order, at most
 * `MAX_FANOUT_PROJECTS` of them.
 *
 * A project whose cached tree is younger than the structure shelf life is
 * used as it is; any other is read (and its answer cached under its own
 * key). `onProject` is told about each project that is actually read, with
 * its place among the projects covered (1-based), so the screen can say
 * "Reading test plans in X (2 of 9 projects)"; a cached project is not
 * announced, as nothing is read for it.
 *
 * A project that answers Forbidden or NotFound is skipped and not counted
 * in `shown`. A project with no plans counts as shown but gets no group.
 */
export async function fetchPlansAll(
  org: string,
  projects: string[],
  onProject?: (project: string, index: number, count: number) => void,
): Promise<PlansAll> {
  const total = projects.length;
  const covered = [...projects]
    .sort((a, b) => {
      const x = a.toLowerCase();
      const y = b.toLowerCase();
      return x < y ? -1 : x > y ? 1 : 0;
    })
    .slice(0, MAX_FANOUT_PROJECTS);

  const groups: ProjectPlans[] = [];
  let shown = 0;
  for (let i = 0; i < covered.length; i++) {
    const project = covered[i];
    const key = cacheKeys.plansSuites(org, project);
    const seed = cacheEntry<PlanWithSuites[]>(key, CACHE.structure.ttlMs);
    let plans: PlanWithSuites[];
    if (seed && Date.now() - seed.at < CACHE.structure.staleMs) {
      plans = seed.data;
    } else {
      onProject?.(project, i + 1, covered.length);
      const result = await commands.listPlansWithSuites(org, project);
      if (result.status === "error") {
        const kind = result.error.kind;
        if (kind === "Forbidden" || kind === "NotFound") continue;
        throw new Error(describeAdoError(result.error));
      }
      plans = result.data;
      cacheWrite(key, plans);
    }
    shown++;
    if (plans.length > 0) groups.push({ project, plans });
  }
  return { groups, shown, total };
}

/** The project that owns a plan, or null when the plan is not in the list
 * (or there is no list yet). */
export function projectOfPlan(all: PlansAll | undefined, planId: number): string | null {
  for (const g of all?.groups ?? []) {
    if (g.plans.some((p) => p.plan.id === planId)) return g.project;
  }
  return null;
}
