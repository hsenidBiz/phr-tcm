// The JSON files imported recently, so an empty queue can offer them back
// ("Recent JSON Imports") instead of a dead-end empty state.
//
// Paths only - never case content. The file on disk stays the single
// source of truth; reopening one runs the real importer again, so a file
// edited since it was last imported comes back as it is NOW.

const KEY = "tcm-v2-recent-imports";

/** Enough to be useful, few enough to scan at a glance. */
const MAX = 8;

export type RecentImport = { path: string; when: number };

export function loadRecentImports(): RecentImport[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (e): e is RecentImport =>
        typeof e === "object" &&
        e !== null &&
        typeof (e as RecentImport).path === "string" &&
        typeof (e as RecentImport).when === "number",
    );
  } catch {
    return [];
  }
}

function persist(list: RecentImport[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    // storage unavailable -> the list lasts this session only
  }
}

/** Record an import that actually succeeded. Re-importing a known file
 * moves it to the top rather than duplicating it. */
export function recordRecentImport(path: string, when = Date.now()): RecentImport[] {
  const rest = loadRecentImports().filter((e) => e.path !== path);
  const next = [{ path, when }, ...rest].slice(0, MAX);
  persist(next);
  return next;
}

/** Drop one path - used when the file turns out to be gone, and by the
 * per-row remove button. */
export function forgetRecentImport(path: string): RecentImport[] {
  const next = loadRecentImports().filter((e) => e.path !== path);
  persist(next);
  return next;
}
