// Cross-window notification: the runner announces each recorded (or reset)
// test point over a Tauri event, and the main window patches its cached
// Run Tests rows in place - the table repaints on every Next without
// refetching the whole suite, and without turning window focus churn into
// network traffic (refetchOnWindowFocus is off app-wide on purpose).

import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TestPoint } from "../bindings";

export type PointRecorded = {
  org: string;
  project: string;
  planId: number;
  suiteId: number;
  testCaseId: number;
  /** "" = reset to Active (the never-run bucket); else the recorded outcome. */
  outcome: string;
  runId: number | null;
  resultId: number | null;
};

const EVENT = "runner:point-recorded";

export function emitPointRecorded(p: PointRecorded): void {
  // Fire-and-forget: a lost repaint self-heals on the next refetch.
  emit(EVENT, p).catch(() => {});
}

export function onPointRecorded(cb: (p: PointRecorded) => void): Promise<UnlistenFn> {
  return listen<PointRecorded>(EVENT, (e) => cb(e.payload));
}

/** The cache patch, pure so it can be unit-tested: update the matching
 * case's row the way a refetch would report it. A reset keeps the last
 * run/result reference (ADO does too - only the outcome is cleared). */
export function patchPointRows(
  rows: TestPoint[] | undefined,
  p: PointRecorded,
): TestPoint[] | undefined {
  return rows?.map((r) =>
    r.test_case_id === p.testCaseId
      ? {
          ...r,
          last_outcome: p.outcome,
          last_run_id: p.runId ?? r.last_run_id,
          last_result_id: p.resultId ?? r.last_result_id,
        }
      : r,
  );
}
