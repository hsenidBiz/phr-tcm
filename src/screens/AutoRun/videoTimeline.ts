// What the video player draws: a case's steps (and the actions inside each)
// or a recording's actions, each at the millisecond it started on the video.
// The shape matches the timeline the run report is given, and the maths is
// the report player's (src-tauri/web/autorun-report-player.js): keep the two
// in step. Every marker comes from a recorded start, never from summed
// durations.

import type { CaseRecord, RecordingEntry } from "../../bindings";

export type TimelineMark = { label: string; at_ms: number };
export type TimelineStep = { name: string; at_ms: number; failed: boolean; actions: TimelineMark[] };
export type Timeline = { title: string; length_ms: number | null; steps: TimelineStep[]; marks: TimelineMark[] };

/** What a case's clean-up step is called on the list. */
export const CLEANUP_LABEL = "Clean-up";
/** How far an arrow key skips. */
export const SKIP_MS = 5000;
/** Past this far into a step, Previous step goes to the start of that step. */
export const PREV_GRACE_MS = 1000;
export const SPEEDS = [0.5, 1, 2] as const;

/** What a step's line is called in the review: the sign-in, the runner's
 * own trip to the case's module, or the case's own step. */
export function stepLabel(stepNumber: number): string {
  if (stepNumber === 0) return "Sign in";
  if (stepNumber === -1) return "Module";
  return `Step ${stepNumber}`;
}

/** Entries with no usable start are dropped, negatives become 0, the rest are
 * sorted by start (ties keep their order), and a missing name becomes
 * "<fallback> N" by its place in the list. The report player does the same. */
function tidy<T extends { at_ms: number }>(list: T[], key: keyof T, fallback: string): T[] {
  const kept: { item: T; i: number }[] = [];
  list.forEach((m, i) => {
    if (!m || typeof m.at_ms !== "number" || !Number.isFinite(m.at_ms)) return;
    const name = m[key];
    kept.push({ item: { ...m, at_ms: Math.max(0, m.at_ms), [key]: typeof name === "string" && name ? name : `${fallback} ${i + 1}` }, i });
  });
  kept.sort((a, b) => a.item.at_ms - b.item.at_ms || a.i - b.i);
  return kept.map((k) => k.item);
}

/** The player's timeline for a case; empty when it kept no timings. */
export function caseTimeline(c: CaseRecord): Timeline {
  const marks = c.video_marks;
  return {
    title: `Video of #${c.case_id} ${c.title}`,
    length_ms: marks?.length_ms ?? null,
    steps: tidy(
      (marks?.steps ?? []).map((s) => ({
        name: s.cleanup ? CLEANUP_LABEL : stepLabel(s.step_number),
        at_ms: s.at_ms,
        failed: Boolean(s.failed),
        actions: tidy((s.actions ?? []).map((a) => ({ at_ms: a.at_ms, label: a.label })), "label", "Action"),
      })),
      "name",
      "Step",
    ),
    marks: [],
  };
}

/** The player's timeline for a kept recording: a flat list of its actions. */
export function recordingTimeline(e: RecordingEntry): Timeline {
  return {
    title: `Recording: ${e.title}`,
    length_ms: e.duration_ms,
    steps: [],
    marks: tidy((e.marks ?? []).map((m) => ({ at_ms: m.at_ms, label: m.label })), "label", "Action"),
  };
}

/** Where the red stretch starts: the first failed step's start. */
export function failedFrom(t: Timeline): number | null {
  return t.steps.find((s) => s.failed)?.at_ms ?? null;
}

/** The index of the last start at or before `ms` (a later one wins a tie);
 * -1 before the first. */
export function currentAt(starts: number[], ms: number): number {
  let at = -1;
  for (let i = 0; i < starts.length; i++) if (starts[i] <= ms) at = i;
  return at;
}

/** The starts the keys move between: the steps', else the actions'. */
export function startsOf(t: Timeline): number[] {
  return (t.steps.length ? t.steps : t.marks).map((s) => s.at_ms);
}

/** `]` goes to the next start after `ms`; `[` to the last start more than
 * `PREV_GRACE_MS` before it. Null when there is none. */
export function stepTarget(t: Timeline, ms: number, dir: -1 | 1): number | null {
  const starts = startsOf(t);
  if (dir > 0) return starts.find((s) => s > ms) ?? null;
  let found: number | null = null;
  for (const s of starts) if (s < ms - PREV_GRACE_MS) found = s;
  return found;
}

/** Where `ms` sits on a bar `lengthMs` long, as a percentage held to 0-100;
 * 0 when the length is not a positive, finite number. */
export function timelinePercent(ms: number, lengthMs: number): number {
  if (!Number.isFinite(ms) || !Number.isFinite(lengthMs) || lengthMs <= 0) return 0;
  return Math.min(100, Math.max(0, (ms * 100) / lengthMs));
}

/** `83000` ms as `1:23`. */
export function clockText(ms: number): string {
  const total = Math.floor(Math.max(0, Number.isFinite(ms) ? ms : 0) / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}
