// The plans and suites of every project, as one query both Search Suites and
// Suite Management use: the same key, the same fetcher, so a walk one of
// them paid for is the other's too.

import { useQuery, type QueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { commands } from "../bindings";
import { CACHE, cacheKeys, cacheRemove, persistentQuery } from "./cache";
import { unwrap } from "./ipc";
import { fetchPlansAll } from "./plansAll";

/** The combined plans query's key. */
export const plansAllKey = (org: string) => ["plans-suites-all", org] as const;

/**
 * The organization's projects (the same query as the context bar's, so one
 * request), then every project's plans, one project at a time, each through
 * its own per-project cache entry. Nothing is read while `enabled` is false.
 *
 * `scanNote` says which project is being read right now ("Reading test plans
 * in Payroll (2 of 9 projects)") and is null when no walk is running.
 */
export function usePlansAll(org: string, enabled: boolean) {
  const [scanNote, setScanNote] = useState<string | null>(null);
  const projectList = useQuery({
    queryKey: ["projects", org],
    ...persistentQuery({
      key: cacheKeys.projects(org),
      fetcher: () => unwrap(commands.listProjects(org)),
      ...CACHE.reference,
    }),
    enabled: enabled && Boolean(org),
  });
  const projectNames = useMemo(() => (projectList.data ?? []).map((p) => p.name), [projectList.data]);
  const plansAll = useQuery({
    queryKey: plansAllKey(org),
    // The query's signal ends a walk that was cancelled (a second Refresh, a
    // new suite, the screen closing): it stops before the next project.
    queryFn: async ({ signal }) => {
      const force = forced.delete(org);
      try {
        return await fetchPlansAll(
          org,
          projectNames,
          (p, i, n) => setScanNote(`Reading test plans in ${p} (${i} of ${n} projects)`),
          { signal, force },
        );
      } finally {
        // A cancelled walk must not wipe the note of the walk that replaced it.
        if (!signal.aborted) setScanNote(null);
      }
    },
    enabled: enabled && Boolean(org && projectNames.length),
    staleTime: CACHE.structure.staleMs,
    gcTime: 60 * 60_000,
    retry: false,
  });
  // The list came back and it is empty: there is nothing to walk, so the
  // screen says so instead of staying blank.
  const noProjects = enabled && Boolean(org) && projectList.isSuccess && projectNames.length === 0;
  return { projectList, projectNames, plansAll, scanNote, noProjects };
}

/** The sentence both screens show for an organization whose project list is empty. */
export const NO_PROJECTS_SENTENCE = "This organization has no projects you can read.";

/** Organizations whose next walk re-reads every project, fresh cache or not. */
const forced = new Set<string>();

/** Refresh: read every project again. Each project's entry is replaced only
 * after its own read succeeds, so a walk that fails partway keeps the trees
 * it had not reached. */
export function refreshPlansAll(qc: QueryClient, org: string): void {
  forced.add(org);
  qc.invalidateQueries({ queryKey: plansAllKey(org) });
}

/** A project's plans changed (a suite was created): forget what was kept for
 * it, so the combined walk reads it again instead of trusting its cache. */
export function forgetProjectPlans(qc: QueryClient, org: string, project: string): void {
  cacheRemove(cacheKeys.plansSuites(org, project));
  qc.invalidateQueries({ queryKey: plansAllKey(org) });
}
