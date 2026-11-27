// The video player's timeline: how a case or a recording becomes steps and
// marks, and the maths that places them on the bar.

import { expect, test } from "vitest";
import type { CaseRecord, RecordingEntry, VideoMarks } from "../../../src/bindings";
import {
  caseTimeline,
  clockText,
  currentAt,
  failedFrom,
  recordingTimeline,
  stepTarget,
  timelinePercent,
  type Timeline,
} from "../../../src/screens/AutoRun/videoTimeline";

function caseWith(video_marks?: VideoMarks | null): CaseRecord {
  return { case_id: 7, title: "Leave request", verdict: "failed", note: "", steps: [], video_marks } as unknown as CaseRecord;
}

test("caseTimeline names steps like the review and carries failures", () => {
  const t = caseTimeline(
    caseWith({
      length_ms: 20000,
      steps: [
        { step_number: 0, at_ms: 0 },
        { step_number: 1, at_ms: 4000, actions: [{ at_ms: 4200, label: "Click" }] },
        { step_number: 2, at_ms: 9000, failed: true },
        { step_number: 3, cleanup: true, at_ms: 15000 },
      ],
    } as VideoMarks),
  );
  expect(t.title).toBe("Video of #7 Leave request");
  expect(t.length_ms).toBe(20000);
  expect(t.steps.map((s) => s.name)).toEqual(["Sign in", "Step 1", "Step 2", "Clean-up"]);
  expect(t.steps[1].actions).toEqual([{ at_ms: 4200, label: "Click" }]);
  expect(t.steps.map((s) => s.failed)).toEqual([false, false, true, false]);
  expect(failedFrom(t)).toBe(9000);
});

test("a case without marks gives an empty timeline", () => {
  const t = caseTimeline(caseWith(undefined));
  expect(t.steps).toEqual([]);
  expect(t.marks).toEqual([]);
  expect(t.length_ms).toBeNull();
  expect(failedFrom(t)).toBeNull();
});

test("recordingTimeline is a flat list with the recording's length", () => {
  const e = {
    id: "r1",
    title: "Bug 1",
    created: "2026-11-27T10:00:00Z",
    duration_ms: 94000,
    file: "r1.mp4",
    marks: [{ at_ms: 3000, label: "Click" }],
  } as RecordingEntry;
  const t = recordingTimeline(e);
  expect(t.title).toBe("Recording: Bug 1");
  expect(t.length_ms).toBe(94000);
  expect(t.steps).toEqual([]);
  expect(t.marks).toEqual([{ at_ms: 3000, label: "Click" }]);
  expect(recordingTimeline({ ...e, marks: undefined }).marks).toEqual([]);
});

test("currentAt picks the last start at or before now, later on ties", () => {
  expect(currentAt([0, 0, 4000], 0)).toBe(1);
  expect(currentAt([1000, 4000], 500)).toBe(-1);
  expect(currentAt([1000, 4000], 4000)).toBe(1);
});

test("stepTarget moves to the next start, and back past the grace", () => {
  const t: Timeline = {
    title: "x",
    length_ms: 20000,
    marks: [],
    steps: [0, 4000, 9000].map((at_ms) => ({ name: "s", at_ms, failed: false, actions: [] })),
  };
  expect(stepTarget(t, 4500, 1)).toBe(9000);
  expect(stepTarget(t, 9200, -1)).toBe(4000);
  expect(stepTarget(t, 9000 + 1500, -1)).toBe(9000);
  expect(stepTarget(t, 9500, 1)).toBeNull();
});

test("stepTarget falls back to the marks of a recording", () => {
  const t: Timeline = { title: "x", length_ms: 9000, steps: [], marks: [{ label: "a", at_ms: 2000 }, { label: "b", at_ms: 6000 }] };
  expect(stepTarget(t, 0, 1)).toBe(2000);
  expect(stepTarget(t, 7500, -1)).toBe(6000);
});

test("timelinePercent clamps to 0..100", () => {
  expect(timelinePercent(25000, 20000)).toBe(100);
  expect(timelinePercent(5000, 20000)).toBe(25);
  expect(timelinePercent(-5, 20000)).toBe(0);
  expect(timelinePercent(5000, Number.NaN)).toBe(0);
  expect(timelinePercent(5000, Number.POSITIVE_INFINITY)).toBe(0);
  expect(timelinePercent(5000, 0)).toBe(0);
});

test("clockText", () => {
  expect(clockText(83000)).toBe("1:23");
  expect(clockText(-5)).toBe("0:00");
  expect(clockText(Number.NaN)).toBe("0:00");
});
