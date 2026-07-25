/**
 * How hard the app is allowed to hit Azure DevOps.
 *
 * ADO's throughput limit is per USER, not per application: the app spends
 * the same budget as the person's browser tabs and git operations, and
 * once the account crosses the threshold ADO delays *everything* they do.
 * The app can't see that budget, so instead it lets the user hand some of
 * it back - useful when working in the app and in ADO at the same time.
 *
 * The pacing itself lives in Rust (one gap between every request); this
 * only persists the choice and pushes it down.
 */
import { commands } from "../bindings";

export type RateLevel = "full" | "balanced" | "gentle";

const KEY = "tcm-v2-ado-rate";

export const RATE_LEVELS: { id: RateLevel; label: string; hint: string }[] = [
  { id: "full", label: "Full speed", hint: "No delay. Fastest, but competes with your browser." },
  { id: "balanced", label: "Balanced", hint: "A small gap between requests. Recommended." },
  { id: "gentle", label: "Gentle", hint: "Wide gaps. Use when you are working in Azure DevOps too." },
];

export function getRateLevel(): RateLevel {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "full" || v === "balanced" || v === "gentle") return v;
  } catch {
    // storage unavailable
  }
  return "balanced";
}

export function setRateLevel(level: RateLevel): void {
  try {
    localStorage.setItem(KEY, level);
  } catch {
    // session-only
  }
  applyRateLevel(level);
}

/** Pushes the level into the Rust pacer. Safe to call before sign-in. */
export function applyRateLevel(level: RateLevel = getRateLevel()): void {
  commands.setAdoRateLevel(level).catch(() => {
    // Non-fatal: the backend keeps its default pacing.
  });
}
