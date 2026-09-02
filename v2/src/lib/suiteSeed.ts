import type { EnsuredSuite } from "../bindings";
import { tourRunningSnapshot } from "../tour/tourState";

/**
 * The plan/suite already resolved for a PBI, cached so Run Tests (and
 * App's own warm-up prefetch) do not re-scan every plan in the project on
 * each visit. Two different writers feed this key - see `SuiteSeed` below
 * for why its shape is looser than `EnsuredSuite`.
 */
const key = (org: string, pbiId: number) => `tcm-v2-suite:${org}/${pbiId}`;

/**
 * `EnsuredSuite`'s `created_plan` flag only means something coming out of
 * `ensurePbiSuite` (Run Tests' own resolver, the one allowed to create a
 * plan) or the read-only `findPbiSuite` finder - App's background warm-up
 * also builds a seed straight from the Suites screen's already-cached plan
 * tree, where there is no such flag to report. Rather than force a value
 * neither of those has, the field is optional here: both writers are
 * honest about what they actually know.
 */
export type SuiteSeed = Pick<EnsuredSuite, "plan_id" | "plan_name" | "suite_id"> &
  Partial<Pick<EnsuredSuite, "created_plan">>;

export function readSuiteSeed(org: string, pbiId: number): SuiteSeed | undefined {
  try {
    const raw = localStorage.getItem(key(org, pbiId));
    return raw ? (JSON.parse(raw) as SuiteSeed) : undefined;
  } catch {
    return undefined;
  }
}

export function writeSuiteSeed(org: string, pbiId: number, seed: SuiteSeed): void {
  // The tour's PBI does not exist in any real org - a plan/suite "resolved"
  // for it must not sit on disk once the tour is gone. Every writer of
  // this key routes through here, so this one check covers all of them.
  if (tourRunningSnapshot()) return;
  try {
    localStorage.setItem(key(org, pbiId), JSON.stringify(seed));
  } catch {
    // cache is best-effort
  }
}

export function clearSuiteSeed(org: string, pbiId: number): void {
  try {
    localStorage.removeItem(key(org, pbiId));
  } catch {
    // cache is best-effort
  }
}
