import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { commands, type PlanWithSuites } from "../bindings";
import { CACHE, cacheEntry, cacheKeys, persistentQuery } from "../lib/cache";
import { readSuiteSeed, type SuiteSeed, writeSuiteSeed } from "../lib/suiteSeed";
import { unwrap } from "../lib/ipc";

/** Warms what the next screen will ask for, in the background, so it is
 * ready when the user navigates there. Never over the walkthrough. */
export function useWarmCaches(
  signedIn: boolean,
  org: string,
  project: string,
  pbiId: number | undefined,
  tourOpen: boolean,
) {
  const qc = useQueryClient();

  // Warm the Search Suites data in the background so the screen is ready
  // when the user navigates there (same key/staleTime as the screen).
  useEffect(() => {
    if (tourOpen) return;
    if (!signedIn || !org || !project) return;
    const key = cacheKeys.plansSuites(org, project);
    // A fresh disk seed means the screen already has its data - warming
    // over the network would spend an expensive scan for nothing.
    const seed = cacheEntry<PlanWithSuites[]>(key, CACHE.structure.ttlMs);
    if (seed && Date.now() - seed.at < CACHE.structure.staleMs) {
      qc.setQueryData(["plans-suites", org, project], seed.data, { updatedAt: seed.at });
      return;
    }
    qc.prefetchQuery({
      queryKey: ["plans-suites", org, project],
      ...persistentQuery({
        key,
        fetcher: () => unwrap(commands.listPlansWithSuites(org, project)),
        ...CACHE.structure,
      }),
      // Without a long gcTime the unobserved prefetch is garbage-collected
      // after 5 minutes and the screen loads from scratch again.
      gcTime: 60 * 60_000,
    });
  }, [signedIn, org, project, qc, tourOpen]);

  // Warm Run Tests: resolve the PBI's suite via the READ-ONLY finder
  // (never creates a plan/suite - creation stays on the Run screen),
  // seed the same cache RunPanel uses, then prefetch its test points.
  useEffect(() => {
    if (tourOpen) return;
    if (!signedIn || !org || !project || pbiId == null) return;
    (async () => {
      let suite: SuiteSeed | null = readSuiteSeed(org, pbiId) ?? null;
      if (!suite) {
        // Before asking the network: the Suites screen's plan tree is
        // already on disk per PROJECT and carries every requirement
        // suite's PBI id - switching PBIs must not re-list every plan's
        // suites when one cached inventory answers for all of them.
        const tree = cacheEntry<PlanWithSuites[]>(cacheKeys.plansSuites(org, project), CACHE.structure.ttlMs);
        for (const { plan, suites } of tree?.data ?? []) {
          const hit = suites.find(
            (s) => s.requirement_id === pbiId && s.suite_type === "requirementTestSuite",
          );
          if (hit) {
            suite = { plan_id: plan.id, plan_name: plan.name, suite_id: hit.id };
            writeSuiteSeed(org, pbiId, suite);
            break;
          }
        }
        // A FRESH tree with no match is an answer, not a miss: this PBI
        // has no suite yet, and scanning every plan again would only
        // confirm that. (Run Tests still find-or-creates on demand.)
        if (!suite && tree && Date.now() - tree.at < CACHE.structure.staleMs) return;
      }
      if (!suite) {
        const r = await commands.findPbiSuite(org, project, pbiId).catch(() => null);
        if (r && r.status === "ok" && r.data) {
          suite = r.data;
          writeSuiteSeed(org, pbiId, suite);
        }
      }
      if (!suite) return;
      const s = suite;
      qc.setQueryData(["suite", org, project, pbiId], s);
      qc.prefetchQuery({
        queryKey: ["points", org, project, s.plan_id, s.suite_id],
        queryFn: () => unwrap(commands.listTestPoints(org, project, s.plan_id, s.suite_id)),
        gcTime: 30 * 60_000,
      });
    })();
  }, [signedIn, org, project, pbiId, qc, tourOpen]);
}
