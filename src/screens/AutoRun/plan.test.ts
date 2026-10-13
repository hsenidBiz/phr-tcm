import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test } from "vitest";
import {
  batchHeadingBefore,
  batchOf,
  fetchPlan,
  pauseLines,
  planBatches,
  recordedResetLines,
} from "./plan";

afterEach(() => clearMocks());

const plan = (order: number[]) => ({ order, phases: [order], pauses: [], counts: null, saved: false });

test("a plan whose order is not these cases is null, so the caller keeps list order", async () => {
  mockIPC(() => plan([2, 7]));
  expect(await fetchPlan("o", "p", 1, [1, 2])).toBeNull();
  mockIPC(() => plan([2, 2]));
  expect(await fetchPlan("o", "p", 1, [1, 2])).toBeNull();
  mockIPC(() => plan([2]));
  expect(await fetchPlan("o", "p", 1, [1, 2])).toBeNull();
});

test("a plan that is a reordering of the cases is returned as it is", async () => {
  mockIPC(() => plan([2, 1]));
  expect((await fetchPlan("o", "p", 1, [1, 2]))?.order).toEqual([2, 1]);
});

// ---- batches ----

const BATCHED = {
  order: [1, 3, 2],
  phases: [[1, 3], [2]],
  pauses: [
    { batch: 1, before_case_id: 1, set: [["Approval workflow", "ON"]] as [string, string][], names: [], changed_by: [] },
    {
      batch: 2,
      before_case_id: 2,
      set: [["Approval workflow", "OFF"]] as [string, string][],
      names: ["cycle published"],
      changed_by: [["cycle published", [1]]] as [string, number[]][],
    },
  ],
  counts: null,
  saved: false,
};

test("a plan without pauses has no batches to show; one with pauses has each with its pause", () => {
  expect(planBatches(plan([1, 2]))).toEqual([]);
  expect(batchOf(plan([1, 2]), 1)).toBeNull();
  expect(planBatches(BATCHED).map((b) => [b.n, b.ids, b.pause?.batch ?? null])).toEqual([
    [1, [1, 3], 1],
    [2, [2], 2],
  ]);
  expect(batchOf(BATCHED, 3)).toEqual({ n: 1, of: 2 });
  expect(batchOf(BATCHED, 2)).toEqual({ n: 2, of: 2 });
});

test("a pause is worded settings first, then what to put back", () => {
  const titleOf = (id: number) => (id === 1 ? "Publish the cycle" : undefined);
  expect(pauseLines(BATCHED.pauses[0], titleOf)).toEqual(["Make sure Approval workflow is ON"]);
  expect(pauseLines(BATCHED.pauses[1], titleOf)).toEqual([
    "Set Approval workflow to OFF",
    'Reset: revert "cycle published" (changed by #1 Publish the cycle)',
  ]);
});

test("a recorded pause reads as the report has it, and a run is headed by batch only once it has them", () => {
  expect(
    recordedResetLines({ before_case_id: 1, batch: 1, set: [["Approval workflow", "ON"]], outcome: "continued" }),
  ).toEqual(["Before Batch 1: make sure Approval workflow is ON - continued"]);
  expect(
    recordedResetLines({ before_case_id: 2, batch: 2, set: [["Rating scale", "5 point"]], names: ["x"], outcome: "stopped" }),
  ).toEqual(["Set Rating scale to 5 point - stopped", 'Reset: revert "x" - stopped']);
  const cases = [{ case_id: 1 }, { case_id: 3 }, { case_id: 2 }];
  const run = { cases, resets: [{ before_case_id: 2, batch: 2, names: ["x"], outcome: "continued" }] };
  expect(batchHeadingBefore(run, 1)).toBe("Batch 1");
  expect(batchHeadingBefore(run, 3)).toBeNull();
  expect(batchHeadingBefore(run, 2)).toBe("Batch 2");
  // A run recorded before batches: no headings.
  expect(batchHeadingBefore({ cases, resets: [{ before_case_id: 2, names: ["x"] }] }, 1)).toBeNull();
});
