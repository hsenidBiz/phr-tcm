/**
 * React Query options backed by the local cache, so a query survives an
 * app restart instead of re-hitting Azure DevOps.
 *
 * React Query's own cache is memory-only: every launch (and every dev
 * reload) starts empty, which is why an expensive scan like the test-plan
 * tree got re-fetched despite `staleTime: Infinity`. Seeding `initialData`
 * from disk - WITH its real age via `initialDataUpdatedAt` - lets React
 * Query paint instantly and then decide for itself whether the seed is
 * stale enough to revalidate in the background.
 *
 * Pick `staleMs` by how much the data actually moves:
 *   structure (plans, suites)  -> hours; it barely changes
 *   outcomes (points, history) -> 0; show the seed, refresh immediately
 */
import { cacheEntry, cacheWrite } from "./localCache";

export function persistentQuery<T>(opts: {
  /** Cache key - must encode every scope the data depends on (org, ids). */
  key: string;
  fetcher: () => Promise<T>;
  /** How long a seed may be served at all. */
  ttlMs: number;
  /** How long a seed is considered fresh (no background refetch). */
  staleMs: number;
}) {
  const { key, fetcher, ttlMs, staleMs } = opts;
  return {
    queryFn: async () => {
      const data = await fetcher();
      cacheWrite(key, data);
      return data;
    },
    initialData: () => cacheEntry<T>(key, ttlMs)?.data,
    // Without the real timestamp React Query would treat the seed as
    // fetched "now" and never refresh it.
    initialDataUpdatedAt: () => cacheEntry<T>(key, ttlMs)?.at,
    staleTime: staleMs,
  };
}

/** Common shelf lives, named so call sites read as intent. */
export const CACHE = {
  /** Plans/suites structure: refetch at most every 6h, keep for a week. */
  structure: { ttlMs: 7 * 24 * 60 * 60_000, staleMs: 6 * 60 * 60_000 },
  /** Run outcomes: seed instantly, always revalidate. */
  outcomes: { ttlMs: 7 * 24 * 60 * 60_000, staleMs: 0 },
} as const;
