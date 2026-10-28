// What the Playwright header's specs pill says, and the store it is kept in.
//
// The pill counts the rule files the script pipeline follows - the agent
// files, gen-test, the skills and the repo's CLAUDE.md - not the test specs:
// those are the clone's tests, and only drive the quiet re-read of its data.
//
// The Playwright screen watches the clone (`useSpecsWatch`) and
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

/** The pipeline's rule files the clone holds and lacks, by repo path
 * (`.claude/agents/playwright-test-healer.md`, `CLAUDE.md`). */
export type PipelineFiles = { found: string[]; missing: string[] };

/** What the last look at the clone found. */
export type SpecsWalk = { count: number; fingerprint: string; pipeline: PipelineFiles } | { error: string };

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
const filesWord = (n: number) => (n === 1 ? "1 pipeline file" : `${n} pipeline files`);

/** A rule file as a person knows it: `playwright-test-healer.md`,
 * `edit-spec/SKILL.md`, `gen-test.md`, `CLAUDE.md`. */
export function ruleFileName(rel: string): string {
  const parts = rel.split("/");
  const last = parts[parts.length - 1];
  return last === "SKILL.md" && parts.length > 1 ? parts.slice(-2).join("/") : last || rel;
}

/** `a`, `a and b`, `a, b and c`. */
function sayList(items: string[]): string {
  return items.length < 2 ? (items[0] ?? "") : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/** What the found files are: "6 agents, gen-test, 3 skills and CLAUDE.md". */
function foundKinds(found: string[]): string {
  const agents = found.filter((f) => f.startsWith(".claude/agents/")).length;
  const skills = found.filter((f) => f.startsWith(".claude/skills/")).length;
  const gen = found.some((f) => f.startsWith(".claude/commands/"));
  const repo = found.includes("CLAUDE.md");
  return sayList([
    ...(agents ? [agents === 1 ? "1 agent" : `${agents} agents`] : []),
    ...(gen ? ["gen-test"] : []),
    ...(skills ? [skills === 1 ? "1 skill" : `${skills} skills`] : []),
    ...(repo ? ["CLAUDE.md"] : []),
  ]);
}

/** The pill for `v`, or null when there is nothing to say yet. */
export function specsPill(v: SpecsView): SpecsPill | null {
  const walk = v.walk;
  if (!walk) return null;
  if ("error" in walk) {
    if (walk.error === SUITES_MISSING) return { tone: "danger", label: "No specs found", why: walk.error };
    return { tone: "danger", label: "Specs could not be read", why: walk.error };
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
  const { found, missing } = walk.pipeline;
  const checked = "They are checked every minute and when this window comes back into focus.";
  if (missing.length > 0) {
    const names = sayList(missing.map(ruleFileName));
    return {
      tone: "warning",
      label: `${filesWord(missing.length)} missing`,
      why: `${names} ${missing.length === 1 ? "is" : "are"} not in the clone, so the copy shipped with this version of Test Case Manager is used instead. ${checked}`,
    };
  }
  return {
    tone: "success",
    label: `${filesWord(found.length)} ready`,
    why: `${found.length === 1 ? "1 file" : `${found.length} files`} the script pipeline follows ${found.length === 1 ? "was" : "were"} found in the clone: ${foundKinds(found)}. ${checked}`,
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
