// What the Playwright header's specs pill says, and the store it is kept in.
//
// The Playwright screen watches the clone's spec files (`useSpecsWatch`) and
// hands what it saw to `specsPill`; the pill beside the page title, which App
// draws outside that screen, reads the answer from this module-scope store.
// The store is empty (no pill) whenever the screen is not shown or no clone
// is set.

import { useSyncExternalStore } from "react";
import type { PillTone } from "../components/StatusPill";

/** What `pw_repo_specs_status` says when `suites/` is missing. Quoted from
 * SUITES_MISSING in pw_repo/specs_status.rs (tests/suite/pw_repo_specs_status.rs
 * keeps them equal): the one error the pill shows as "No specs found". */
export const SUITES_MISSING = "The clone has no suites folder.";

/** How often the shown tab looks at the clone's spec files again, in ms. */
export const SPECS_CHECK_MS = 60_000;

export type SpecsPill = { tone: PillTone; label: string; why: string };

/** What the last look at the clone found. */
export type SpecsWalk = { count: number; fingerprint: string } | { error: string };

/** Everything the pill is worked out from. */
export type SpecsView = {
  /** The last look at the spec files; null until the first one answers. */
  walk: SpecsWalk | null;
  /** The fingerprint the clone's data was last read again at. */
  processed: string | null;
  /** Reading the clone again is going now. */
  processing: boolean;
  /** The fingerprint a re-read failed at, and why. */
  failed: { fingerprint: string; error: string } | null;
};

const specsWord = (n: number) => (n === 1 ? "1 spec" : `${n} specs`);

/** The pill for `v`, or null when there is nothing to say yet. */
export function specsPill(v: SpecsView): SpecsPill | null {
  const walk = v.walk;
  if (!walk) return null;
  if ("error" in walk) {
    if (walk.error === SUITES_MISSING) return { tone: "danger", label: "No specs found", why: walk.error };
    return { tone: "danger", label: "Specs could not be read", why: walk.error };
  }
  if (walk.count === 0) {
    return {
      tone: "danger",
      label: "No specs found",
      why: "The clone's suites folder holds no spec files (*.spec.ts or *.spec.js).",
    };
  }
  if (v.failed && v.failed.fingerprint === walk.fingerprint) {
    return {
      tone: "danger",
      label: "Specs could not be read",
      why: `${specsWord(walk.count)} found, but reading them again failed: ${v.failed.error} Press Refresh to try again.`,
    };
  }
  if (v.processing || (v.processed !== null && v.processed !== walk.fingerprint)) {
    return {
      tone: "warning",
      label: "Updating specs…",
      why: `The spec files changed. Reading the clone again (${specsWord(walk.count)} found).`,
    };
  }
  return {
    tone: "success",
    label: `${specsWord(walk.count)} ready`,
    why: `${specsWord(walk.count)} found in the clone's suites folder and read. They are checked every minute and when this window comes back into focus.`,
  };
}

let current: SpecsPill | null = null;
const listeners = new Set<() => void>();

export function subscribeSpecsPill(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** The pill as the Playwright screen last set it. */
export function getSpecsPill(): SpecsPill | null {
  return current;
}

/** Sets the pill (null removes it). Keeps the same object when nothing changed. */
export function setSpecsPill(next: SpecsPill | null): void {
  const same =
    next === current ||
    (next !== null && current !== null && next.tone === current.tone && next.label === current.label && next.why === current.why);
  if (same) return;
  current = next;
  for (const l of listeners) l();
}

export function useSpecsPill(): SpecsPill | null {
  return useSyncExternalStore(subscribeSpecsPill, getSpecsPill);
}
