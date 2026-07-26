/**
 * Small persistent cache over localStorage - the app's "cache db".
 *
 * Azure DevOps rate-limits per user, so every request the app can answer
 * from disk is budget handed back to the person's browser. This holds the
 * slow-moving reads (org/project lists, team members) across restarts and
 * the immutable ones (pipeline runs of finished PRs) indefinitely.
 *
 * Disabled entirely in demo mode: demo data must never be served to a real
 * session, and vice versa. Bounded to MAX_ENTRIES; oldest entries fall out
 * first (localStorage is ~5 MB and shared with everything else the app
 * persists).
 */

const PREFIX = "tcm-v2-cache:";
const MAX_ENTRIES = 150;

type Entry<T> = { at: number; data: T };

function demoMode(): boolean {
  try {
    return localStorage.getItem("tcm-v2-dev-demo") === "on";
  } catch {
    return false;
  }
}

/** The entry WITH its age, for callers that need to tell React Query how
 * old the seed is (so it can decide whether to revalidate). */
export function cacheEntry<T>(key: string, maxAgeMs: number): { data: T; at: number } | null {
  if (demoMode()) return null;
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
  if (demoMode()) return;
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

/** Serve from cache when fresh enough, else fetch and remember. */
export async function cached<T>(
  key: string,
  maxAgeMs: number,
  fetcher: () => Promise<T>,
): Promise<T> {
  const hit = cacheRead<T>(key, maxAgeMs);
  if (hit !== null) return hit;
  const data = await fetcher();
  cacheWrite(key, data);
  return data;
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
