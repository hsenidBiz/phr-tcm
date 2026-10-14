// The brick game's best score on this machine. The game's frame is
// sandboxed and has no storage of its own, so the modal keeps it here, in
// the app's own localStorage namespace (carried by backups with the rest).
// A preference, not cached data: lib/cache.ts is not the place for it.

export const BREAKER_BEST_KEY = "tcm-v2-breaker-best";

/** No real game gets near this; anything at or above it is not a score. */
export const BREAKER_BEST_CAP = 10_000_000;

/** A whole, finite, non-negative number below the cap. */
export function isValidBest(v: unknown): v is number {
  return typeof v === "number" && Number.isInteger(v) && v >= 0 && v < BREAKER_BEST_CAP;
}

/** The best kept so far, or 0 when there is none or it is unreadable. */
export function loadBreakerBest(): number {
  try {
    const raw = localStorage.getItem(BREAKER_BEST_KEY);
    if (raw === null) return 0;
    const n: unknown = JSON.parse(raw);
    return isValidBest(n) ? n : 0;
  } catch {
    return 0;
  }
}

/** Keeps the higher of the stored best and `v`, ignoring an invalid `v`.
 * Returns the best now kept. */
export function recordBreakerBest(v: unknown): number {
  const current = loadBreakerBest();
  if (!isValidBest(v) || v <= current) return current;
  try {
    localStorage.setItem(BREAKER_BEST_KEY, JSON.stringify(v));
  } catch {
    // storage unavailable -> this session only
  }
  return v;
}
