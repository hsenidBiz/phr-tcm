/**
 * The webview's one cache: persistent storage over localStorage, plus the
 * React Query options that seed a query from it.
 *
 * Azure DevOps rate-limits per user, so every read the app can answer from
 * disk is budget handed back. React Query's own cache is memory-only - every
 * launch (and every dev reload) starts empty - so this holds the slow-moving
 * reads (org/project lists, team members, plan trees) and the immutable ones
 * (pipeline runs of finished PRs) across restarts.
 *
 * How to use it:
 * - A query whose data should survive a restart: spread
 *   `persistentQuery({ key: cacheKeys.x(...), fetcher, ...CACHE.preset })`
 *   into `useQuery`. That is the whole integration.
 * - Merge logic a plain query can't express (PrPanel's finished pipelines):
 *   `cacheRead` / `cacheWrite`, still with a `cacheKeys` key and a `CACHE`
 *   shelf life.
 * - A new key goes in `cacheKeys`, a new shelf life in `CACHE`. cache.test.ts
 *   fails on a hand-written key, a raw TTL, or a second implementation.
 *
 * The Rust backend has its own cache (src-tauri/src/cache) for data the AI
 * bridge reads too; the two are separate processes and share nothing.
 *
 * Disabled entirely in demo mode: demo data must never be served to a real
 * session, and vice versa. Bounded to MAX_ENTRIES; oldest entries fall out
 * first (localStorage is ~5 MB and shared with everything else the app
 * persists).
 */

const PREFIX = "tcm-v2-cache:";
const OWNER_KEY = "tcm-v2-cache-owner";
const MAX_ENTRIES = 150;

const HOUR = 60 * 60_000;
const DAY = 24 * HOUR;

type Entry<T> = { at: number; data: T };

/** Common shelf lives, named so call sites read as intent. Pick `staleMs`
 * by how much the data actually moves. */
export const CACHE = {
  /** Plans/suites structure: refetch at most every 6h, keep for a week. */
  structure: { ttlMs: 7 * DAY, staleMs: 6 * HOUR },
  /** Run outcomes, comments, item details: seed instantly, always revalidate. */
  outcomes: { ttlMs: 7 * DAY, staleMs: 0 },
  /** Org/project lists, team members: a day from disk with no request, so
   * most app starts cost nothing here. */
  reference: { ttlMs: DAY, staleMs: DAY },
  /** Pipelines of a finished PR: they never change again. */
  finished: { ttlMs: 30 * DAY, staleMs: Infinity },
} as const;

/** Every key the webview caches under. The strings are what earlier
 * versions stored - changing one throws that data away for every user. */
export const cacheKeys = {
  orgs: () => "orgs",
  projects: (org: string) => `projects:${org}`,
  members: (org: string, project: string) => `members:${org}/${project}`,
  workItemDetail: (org: string, project: string, id: number) => `wi-detail:${org}/${project}/${id}`,
  workItemComments: (org: string, project: string, id: number) =>
    `wi-comments:${org}/${project}/${id}`,
  plansSuites: (org: string, project: string) => `plans-suites:${org}/${project}`,
  runHistory: (org: string, project: string, planId: number | undefined) =>
    `run-history:${org}/${project}/${planId}`,
  points: (org: string, project: string, planId: number | undefined, suiteId: number | undefined) =>
    `points:${org}/${project}/${planId}/${suiteId}`,
  boardPrs: (org: string, project: string) => `board-prs:${org}/${project}`,
  prPipeline: (org: string, project: string, prId: number, mergeCommit: string) =>
    `pipe:${org}/${project}:${prId}:${mergeCommit}`,
};

/** Non-reversible tag for an account, so the identity check never needs the
 * address itself written to disk. Collisions only cost a needless wipe. */
function tag(account: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < account.length; i++) {
    h = ((h ^ account.charCodeAt(i)) * 0x01000193) >>> 0;
  }
  return h.toString(36);
}

/**
 * Hand the cache to the signed-in account, discarding anything the previous
 * one left behind.
 *
 * Every key here is scoped by org and project, which is not the same as
 * being scoped by PERSON. Two accounts on one Windows profile - someone
 * signing out and back in as a service or test account - saw each other's
 * plan and suite trees for up to the structure TTL, painted instantly from
 * `initialData` before any request could have been refused. The data was
 * fetched with a token the second account never held.
 */
export function claimCacheFor(account: string | null): void {
  if (!account) return;
  try {
    const now = tag(account);
    if (localStorage.getItem(OWNER_KEY) === now) return;
    for (const k of Object.keys(localStorage)) {
      if (k.startsWith(PREFIX)) localStorage.removeItem(k);
    }
    localStorage.setItem(OWNER_KEY, now);
  } catch {
    // storage unavailable - nothing was cached to leak
  }
}

function demoMode(): boolean {
  try {
    return localStorage.getItem("tcm-v2-dev-demo") === "on";
  } catch {
    return false;
  }
}

/** The guided tour shows sample data: while it runs nothing may be read
 * from disk (real data would appear inside the tour) and nothing written
 * to it (sample data would outlive the tour). */
let suspended = false;

export function suspendCache(value: boolean): void {
  suspended = value;
}

function off(): boolean {
  return suspended || demoMode();
}

/** The entry WITH its age, for callers that need to tell React Query how
 * old the seed is (so it can decide whether to revalidate). */
export function cacheEntry<T>(key: string, maxAgeMs: number): { data: T; at: number } | null {
  if (off()) return null;
  try {
    const raw = localStorage.getItem(PREFIX + key);
    if (!raw) return null;
    const entry = JSON.parse(raw) as Entry<T>;
    if (typeof entry?.at !== "number") return null;
    if (Date.now() - entry.at > maxAgeMs) return null;
    return entry;
  } catch {
    return null;
  }
}

export function cacheRead<T>(key: string, maxAgeMs: number): T | null {
  return cacheEntry<T>(key, maxAgeMs)?.data ?? null;
}

export function cacheWrite<T>(key: string, data: T): void {
  if (off()) return;
  try {
    localStorage.setItem(PREFIX + key, JSON.stringify({ at: Date.now(), data }));
    prune();
  } catch {
    // Quota or unavailable - drop everything we own and carry on; a cache
    // that cannot write must never break the feature it accelerates.
    try {
      for (const k of Object.keys(localStorage)) {
        if (k.startsWith(PREFIX)) localStorage.removeItem(k);
      }
    } catch {
      // storage fully unavailable
    }
  }
}

/**
 * React Query options backed by the cache, so a query survives an app
 * restart instead of re-hitting Azure DevOps.
 *
 * Seeding `initialData` from disk - WITH its real age via
 * `initialDataUpdatedAt` - lets React Query paint instantly and then decide
 * for itself whether the seed is stale enough to revalidate in the
 * background.
 */
export function persistentQuery<T>(opts: {
  /** From `cacheKeys` - must encode every scope the data depends on. */
  key: string;
  fetcher: () => Promise<T>;
  /** How long a seed may be served at all. */
  ttlMs: number;
  /** How long a seed is considered fresh (no background refetch). */
  staleMs: number;
  /** Whether a fetched result may be written; it is returned either way. */
  store?: (data: T) => boolean;
}) {
  const { key, fetcher, ttlMs, staleMs, store } = opts;
  return {
    queryFn: async () => {
      const data = await fetcher();
      if (!store || store(data)) cacheWrite(key, data);
      return data;
    },
    initialData: () => cacheEntry<T>(key, ttlMs)?.data,
    // Without the real timestamp React Query would treat the seed as
    // fetched "now" and never refresh it.
    initialDataUpdatedAt: () => cacheEntry<T>(key, ttlMs)?.at,
    staleTime: staleMs,
  };
}

function prune(): void {
  const keys: { k: string; at: number }[] = [];
  for (const k of Object.keys(localStorage)) {
    if (!k.startsWith(PREFIX)) continue;
    try {
      keys.push({ k, at: (JSON.parse(localStorage.getItem(k) ?? "") as Entry<unknown>).at ?? 0 });
    } catch {
      localStorage.removeItem(k); // unreadable entry - not a cache anymore
    }
  }
  if (keys.length <= MAX_ENTRIES) return;
  keys.sort((a, b) => a.at - b.at);
  for (const { k } of keys.slice(0, keys.length - MAX_ENTRIES)) {
    localStorage.removeItem(k);
  }
}
