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
 *   catches the common forms of a hand-written key, a raw TTL, or a second
 *   implementation.
 *
 * The Rust backend has its own cache (src-tauri/src/cache) for data the AI
 * bridge reads too; the two are separate processes and share nothing.
 *
 * Disabled entirely in demo mode: demo data must never be served to a real
 * session, and vice versa. Bounded to MAX_ENTRIES; oldest entries fall out
 * first.
 *
 * Where it lives (backlog A1.6): in IndexedDB, with a copy in memory.
 * It used to live in localStorage, which is about 5 MB for the whole app
 * and shared with the queue drafts, notes and settings - a big plan tree
 * could fill it, and then a queue draft could no longer be saved. Reads
 * stay synchronous: `initCache` (awaited in main.tsx before the first
 * render) loads every entry into memory, and writes go to memory at once
 * and to IndexedDB in the background, batched. Nothing crosses IPC. Where
 * IndexedDB cannot be opened the cache works for the session only.
 * Entries an earlier version left in localStorage are moved over on the
 * first launch and removed from there.
 */

const PREFIX = "tcm-v2-cache:";
const OWNER_KEY = "tcm-v2-cache-owner";
/** The localStorage index an earlier version kept; removed on migration. */
const LEGACY_INDEX_KEY = "tcm-v2-cache-index";
const MAX_ENTRIES = 150;
/** The `plans-suites:` entries (see cacheKeys.plansSuites) have a cap of
 * their own: one per project the combined walk may read (100), plus headroom. */
const PLAN_TREES = "plans-suites:";
const MAX_PLAN_TREES = 120;

const DB_NAME = "tcm-v2-cache";
const STORE = "entries";

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
  /** The area paths of every project (one request per project): an hour
   * with no request, then shown from disk while it is read again. */
  areas: { ttlMs: DAY, staleMs: HOUR },
  /** Pipelines of a finished PR: they never change again. */
  finished: { ttlMs: 30 * DAY, staleMs: Infinity },
  /** Choices a person made and expects back (the Playwright tab's ticks):
   * kept for a year, never refetched. */
  picks: { ttlMs: 365 * DAY, staleMs: Infinity },
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
  suiteCases: (org: string, project: string, planId: number, suiteId: number) =>
    `suite-cases:${org}/${project}/${planId}/${suiteId}`,
  runHistory: (org: string, project: string, planId: number | undefined) =>
    `run-history:${org}/${project}/${planId}`,
  // Per suite: the history is asked for that suite's cases only.
  suiteRunHistory: (org: string, project: string, planId: number | undefined, suiteId: number | undefined) =>
    `run-history:${org}/${project}/${planId}/${suiteId}`,
  points: (org: string, project: string, planId: number | undefined, suiteId: number | undefined) =>
    `points:${org}/${project}/${planId}/${suiteId}`,
  boardPrs: (org: string, project: string) => `board-prs:${org}/${project}`,
  boardPrsAll: (org: string) => `board-prs-all:${org}`,
  areasAll: (org: string) => `areas-all:${org}`,
  prPipeline: (org: string, project: string, prId: number, mergeCommit: string) =>
    `pipe:${org}/${project}:${prId}:${mergeCommit}`,
  suiteSeed: (org: string, pbiId: number) => `suite-seed:${org}/${pbiId}`,
  runOrder: (org: string, project: string, pbiId: number) => `run-order:${org}/${project}/${pbiId}`,
  sharedStepTitle: (org: string, id: number) => `shared-step:${org}/${id}`,
  // The steps inside a Shared Steps item, as the runner shows them (shelf: CACHE.outcomes).
  sharedStepSteps: (org: string, id: number) => `shared-step-steps:${org}/${id}`,
  pwTicks: (clone: string) => `pw-ticks:${clone}`,
  // What a person last answered for the fields a Bug requires (shelf: CACHE.picks).
  bugFieldValues: (org: string, project: string) => `bug-fields:${org}/${project}`,
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

/** Every entry, by key (without PREFIX). The one place reads come from. */
const memory = new Map<string, Entry<unknown>>();

/** Writes not yet in IndexedDB: an entry to put, or null to delete. */
const pending = new Map<string, Entry<unknown> | null>();
let wipePending = false;
let flushTimer: ReturnType<typeof setTimeout> | null = null;
let db: IDBDatabase | null = null;

function openDb(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    try {
      if (typeof indexedDB === "undefined") return resolve(null);
      const req = indexedDB.open(DB_NAME, 1);
      req.onupgradeneeded = () => {
        if (!req.result.objectStoreNames.contains(STORE)) req.result.createObjectStore(STORE);
      };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(null);
      req.onblocked = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
}

function readAll(store: IDBDatabase): Promise<[string, Entry<unknown>][]> {
  return new Promise((resolve) => {
    try {
      const tx = store.transaction(STORE, "readonly");
      const os = tx.objectStore(STORE);
      const keys = os.getAllKeys();
      const values = os.getAll();
      tx.oncomplete = () =>
        resolve((keys.result as IDBValidKey[]).map((k, i) => [String(k), values.result[i] as Entry<unknown>]));
      tx.onerror = () => resolve([]);
      tx.onabort = () => resolve([]);
    } catch {
      resolve([]);
    }
  });
}

function isEntry(v: unknown): v is Entry<unknown> {
  return typeof v === "object" && v !== null && typeof (v as Entry<unknown>).at === "number";
}

/** Entries an earlier version kept in localStorage: into memory (and so to
 * IndexedDB), and out of localStorage, where the room is needed. */
function migrateLegacy(): void {
  try {
    const keys = Object.keys(localStorage).filter((k) => k.startsWith(PREFIX));
    for (const k of keys) {
      try {
        const entry = JSON.parse(localStorage.getItem(k) ?? "") as unknown;
        if (isEntry(entry)) {
          const key = k.slice(PREFIX.length);
          const have = memory.get(key);
          if (!have || have.at < entry.at) {
            memory.set(key, entry);
            pending.set(key, entry);
          }
        }
      } catch {
        // unreadable - not a cache entry any more
      }
      localStorage.removeItem(k);
    }
    localStorage.removeItem(LEGACY_INDEX_KEY);
  } catch {
    // storage unavailable
  }
}

/** One entry an earlier version left in localStorage, moved over on its
 * first read: `migrateLegacy` moves them all at launch, and this covers an
 * entry put back afterwards (a restored older backup carries them). */
function legacyEntry(key: string): Entry<unknown> | undefined {
  try {
    const raw = localStorage.getItem(PREFIX + key);
    if (raw == null) return undefined;
    localStorage.removeItem(PREFIX + key);
    const entry = JSON.parse(raw) as unknown;
    if (!isEntry(entry)) return undefined;
    put(key, entry);
    return entry;
  } catch {
    return undefined;
  }
}

let ready: Promise<void> | null = null;

/** While `initCache` reads the disk: a wipe or a removal made meanwhile must
 * not be undone by the older disk copy it then loads (an account switch
 * when IndexedDB is slower than main.tsx's 1.5 s wait). */
let loading = false;
let wipedDuringLoad = false;
const removedDuringLoad = new Set<string>();

/**
 * Load the cache into memory. main.tsx awaits this before the first render,
 * so the first queries can seed from it. Never rejects; with no IndexedDB
 * the cache simply starts empty for this session.
 */
export function initCache(): Promise<void> {
  ready ??= (async () => {
    loading = true;
    migrateLegacy();
    db = await openDb();
    if (db) {
      const disk = await readAll(db);
      // A wipe while loading: the disk holds what was wiped.
      if (!wipedDuringLoad) {
        for (const [key, entry] of disk) {
          // A write or a removal made while this was loading is newer than
          // the disk.
          if (!isEntry(entry) || memory.has(key) || removedDuringLoad.has(key)) continue;
          memory.set(key, entry);
        }
      }
    }
    loading = false;
    wipedDuringLoad = false;
    removedDuringLoad.clear();
    pruneAll();
    scheduleFlush();
  })();
  return ready;
}

function scheduleFlush(): void {
  if (flushTimer != null) return;
  flushTimer = setTimeout(flush, 50);
}

/** Write the pending changes to IndexedDB in one transaction. A failed
 * write costs only the next launch's head start: memory still has it. */
function flush(): void {
  flushTimer = null;
  if (!db || (!wipePending && pending.size === 0)) return;
  if (wipePending) {
    // The wipe goes in a transaction of its own, and is tried again until it
    // lands: a lost clear() would load the previous account's entries at the
    // next launch, under the new owner. Nothing else is written before it.
    const store = db;
    try {
      const tx = store.transaction(STORE, "readwrite");
      tx.objectStore(STORE).clear();
      tx.onabort = tx.onerror = () => {
        // Every entry held now is written again after the retried clear.
        wipePending = true;
        for (const [key, entry] of memory) pending.set(key, entry);
        scheduleFlush();
      };
      wipePending = false;
    } catch {
      scheduleFlush();
      return;
    }
  }
  if (pending.size === 0) return;
  try {
    const tx = db.transaction(STORE, "readwrite");
    const os = tx.objectStore(STORE);
    for (const [key, entry] of pending) {
      if (entry) os.put(entry, key);
      else os.delete(key);
    }
  } catch {
    // quota or a closed database - memory still serves this session
  }
  pending.clear();
}

function put(key: string, entry: Entry<unknown>): void {
  memory.set(key, entry);
  pending.set(key, entry);
  scheduleFlush();
}

function drop(key: string): void {
  memory.delete(key);
  pending.set(key, null);
  scheduleFlush();
}

function wipe(): void {
  if (loading) wipedDuringLoad = true;
  memory.clear();
  pending.clear();
  wipePending = true;
  scheduleFlush();
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
 *
 * Returns whether it actually wiped (a different account claimed the
 * cache), not merely whether an account was given - callers with their own
 * per-account state (the notification bell, the mentions baseline) key off
 * that to forget theirs at the exact same moment.
 */
export function claimCacheFor(account: string | null): boolean {
  if (!account) return false;
  try {
    const now = tag(account);
    if (localStorage.getItem(OWNER_KEY) === now) return false;
    wipe();
    localStorage.setItem(OWNER_KEY, now);
    return true;
  } catch {
    // storage unavailable - nothing was cached to leak
    return false;
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
  const entry = (memory.get(key) ?? legacyEntry(key)) as Entry<T> | undefined;
  if (!entry) return null;
  if (Date.now() - entry.at > maxAgeMs) return null;
  return entry;
}

export function cacheRead<T>(key: string, maxAgeMs: number): T | null {
  return cacheEntry<T>(key, maxAgeMs)?.data ?? null;
}

/** Remove one entry. Not gated by `off()`: removing is always safe, tour
 * or demo mode or not. */
export function cacheRemove(key: string): void {
  // Also while loading, when memory may not have it yet but the disk does.
  if (loading) removedDuringLoad.add(key);
  if (memory.has(key) || loading) drop(key);
}

export function cacheWrite<T>(key: string, data: T): void {
  if (off()) return;
  put(key, { at: Date.now(), data });
  prune();
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

/**
 * Keep at most MAX_ENTRIES, dropping the oldest. Plan trees (one per
 * project the combined walk reads) are counted and evicted among
 * themselves, so a walk over many projects never pushes out other screens'
 * entries, nor they the trees. All in memory: no entry is read from disk.
 */
function prune(): void {
  if (memory.size <= Math.min(MAX_ENTRIES, MAX_PLAN_TREES)) return;
  pruneAll();
}

function pruneAll(): void {
  const keys = [...memory.keys()];
  trimOldest(keys.filter((k) => !k.startsWith(PLAN_TREES)), MAX_ENTRIES);
  trimOldest(keys.filter((k) => k.startsWith(PLAN_TREES)), MAX_PLAN_TREES);
}

/** Drops the oldest of `keys` until at most `max` remain. */
function trimOldest(keys: string[], max: number): void {
  if (keys.length <= max) return;
  keys.sort((a, b) => memory.get(a)!.at - memory.get(b)!.at);
  for (const k of keys.slice(0, keys.length - max)) drop(k);
}

/** The keys held now, sorted. For tests and the developer panel. */
export function cacheStoredKeys(): string[] {
  return [...memory.keys()].sort();
}

/** Drop every entry (the developer panel's Clear app storage). */
export function clearCache(): void {
  wipe();
}

/** Test hook: forget memory and the open database, as a restart would.
 * The database itself is left as it is on disk. */
export function resetCacheForTests(): void {
  if (flushTimer != null) clearTimeout(flushTimer);
  flushTimer = null;
  memory.clear();
  pending.clear();
  wipePending = false;
  loading = false;
  wipedDuringLoad = false;
  removedDuringLoad.clear();
  db?.close();
  db = null;
  ready = null;
}

/** Test hook: store `data` as fetched at `at` (epoch ms), so a test can
 * age an entry without a clock of its own. */
export function cacheWriteAtForTests<T>(key: string, data: T, at: number): void {
  put(key, { at, data });
}

/** Test hook: write pending changes now and wait for them to land. */
export function flushCacheForTests(): Promise<void> {
  if (flushTimer != null) clearTimeout(flushTimer);
  const store = db;
  flush();
  if (!store) return Promise.resolve();
  return new Promise((resolve) => {
    const tx = store.transaction(STORE, "readonly");
    tx.oncomplete = () => resolve();
    tx.onerror = () => resolve();
  });
}
