/**
 * The unattended run, as one module-scope store, so it outlives its window.
 *
 * An unattended run is one long IPC call (`auto_run_replay`) with progress
 * and reset pauses arriving as events. It used to live in the dialog that
 * started it, so leaving Auto Run unmounted the only thing listening: the
 * run went on in Rust, but its progress and its finish were lost. Here the
 * call, its promise and its listeners belong to the module for the run's
 * whole life, and every view (the run window, the title-bar pill, the Auto
 * Run screen) only reads it.
 *
 * One run at a time. The store holds:
 * - `run`: the run being set up, going, or ended and not yet looked at;
 * - `review`: a finished run whose review the Auto Run screen should open
 *   (App takes the person there, the screen opens it and clears this).
 *
 * The same `useSyncExternalStore` shape as the app's other stores
 * (sessionExpired.ts): a snapshot, a subscribe, and plain functions that
 * change it.
 */
import { useSyncExternalStore } from "react";
import {
  commands,
  events,
  type AutorunResetNeeded,
  type PbiHit,
  type PlanView,
  type ReplayProgress,
  type StepRecord,
} from "../bindings";
import { DISCOVERY_BUSY } from "./discoveryActive";

/** A case's written step, as the script editor shows it. */
export type WrittenStep = { action: string; expected: string; shared?: number | null };

/** One case of the run, with what the window shows of it. `module` is the
 * case's Module field. */
export type RunCase = { id: number; title: string; module?: string; steps?: WrittenStep[] };

/** Where a run is:
 * - setup: the window is open on its choices, nothing started yet;
 * - running: the call is pending (a reset pause is `resetNeeded` on top);
 * - finished: it ended while its window was closed, waiting for Review;
 * - failed: it could not start or stopped with an error. */
export type RunPhase = "setup" | "running" | "finished" | "failed";

export type BackgroundRun = {
  org: string;
  project: string;
  pbi: PbiHit;
  /** The selection, in run order. */
  cases: RunCase[];
  /** The plan the setup shows: phases and reset points. */
  plan: PlanView | null;
  phase: RunPhase;
  /** Whether the run window is showing. */
  open: boolean;
  /** The run's id, fixed by the first progress event after Start. */
  runId: string | null;
  /** Status text per case id. */
  rows: Record<number, string>;
  /** The last phase seen per case id: "done" marks a finished case. */
  phases: Record<number, string>;
  /** The newest progress event's case and where it is. */
  latest: { caseId: number; phase: string; step: number } | null;
  /** The selection's own "case N of M" - null until the first event. */
  position: { index: number; total: number } | null;
  /** A finished case's recorded steps, read from the run file. */
  records: Record<number, StepRecord[]>;
  /** The reset point the run is paused at, while it waits for an answer. */
  resetNeeded: AutorunResetNeeded | null;
  /** An answer to the pause is on its way. */
  answering: boolean;
  /** Stop was pressed: the run ends after its current step. */
  stopping: boolean;
  /** The finished run's id. */
  resultId: string | null;
  /** Why the run failed, in the words the call gave. */
  error: string;
};

/** A finished run whose review the Auto Run screen should open. */
export type ReviewRequest = {
  org: string;
  project: string;
  pbi: PbiHit;
  runId: string;
  /** Opened from the pill or the toast: the screen moves to Past runs.
   * A run that finished with its window open goes straight to the review
   * wherever the screen already is, as it always has. */
  toPastRuns: boolean;
};

export type BackgroundRunState = { run: BackgroundRun | null; review: ReviewRequest | null };

/** What a run's end tells its listeners. */
export type RunEnded = { ok: boolean; inBackground: boolean; runId: string | null; error: string };

/** The run's choices, picked in the window's setup. */
export type RunOptions = {
  account: string | null;
  browserName: string;
  watch: boolean;
  retryTransient: boolean;
  dbReadAccess: boolean;
};

/** The one line a row shows for the phase an unattended step is in. */
export function statusOf(p: { phase: string; step_number: number; steps: number; proposed: string }): string {
  if (p.phase === "opening") return "Opening the browser";
  if (p.phase === "signing_in") return "Signing in";
  if (p.phase === "module") return "Going to the module";
  if (p.phase === "step") return `Step ${p.step_number} of ${p.steps}`;
  if (p.phase === "done") return p.proposed ? `Proposed: ${p.proposed}` : "Nothing proposed";
  return "Waiting";
}

let state: BackgroundRunState = { run: null, review: null };
const listeners = new Set<() => void>();
const endedListeners = new Set<(e: RunEnded) => void>();
/** Bumped by every start and every reset: a callback from an older run
 * (its promise, its events) checks this and does nothing. */
let generation = 0;
/** The event listeners of the run going now. */
let unlisteners: Promise<() => void>[] = [];

function emit() {
  for (const l of listeners) l();
}

function setState(next: BackgroundRunState) {
  state = next;
  emit();
}

/** Changes the run in place, if there still is one. */
function patchRun(patch: Partial<BackgroundRun> | ((r: BackgroundRun) => Partial<BackgroundRun>)) {
  const r = state.run;
  if (!r) return;
  setState({ ...state, run: { ...r, ...(typeof patch === "function" ? patch(r) : patch) } });
}

function stopListening() {
  for (const un of unlisteners) un.then((f) => f()).catch(() => {});
  unlisteners = [];
}

function announceEnd(e: RunEnded) {
  for (const l of endedListeners) l(e);
}

export function subscribeBackgroundRun(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function backgroundRunSnapshot(): BackgroundRunState {
  return state;
}

/** The store, for a component. */
export function useBackgroundRun(): BackgroundRunState {
  return useSyncExternalStore(subscribeBackgroundRun, backgroundRunSnapshot);
}

/** Whether a run is going (or paused at a reset point): every Run button
 * waits for it. */
export function runIsGoing(s: BackgroundRunState = state): boolean {
  return s.run?.phase === "running";
}

export function useRunGoing(): boolean {
  return runIsGoing(useBackgroundRun());
}

/** What a Run button that waits for the run says. */
export const RUN_GOING_REASON = "An unattended run is already going. Wait for it, or stop it.";

/** Why a Run button (or Replay to step, or Open browser) waits, or
 * undefined when nothing holds it: a run going says so first, then a
 * discovery holding the Auto Run browser. One sentence per cause. */
export function runBlockedReason(s: BackgroundRunState, discovering: boolean): string | undefined {
  if (runIsGoing(s)) return RUN_GOING_REASON;
  if (discovering) return DISCOVERY_BUSY;
  return undefined;
}

/** Called once per run, as it ends. Returns the unsubscribe. */
export function onRunEnded(cb: (e: RunEnded) => void): () => void {
  endedListeners.add(cb);
  return () => {
    endedListeners.delete(cb);
  };
}

/** Opens the run window on its setup for this selection. Refused while a
 * run is going: there is only ever one. A run ended and not looked at is
 * replaced; it is in Past runs. */
export function openRunSetup(s: {
  org: string;
  project: string;
  pbi: PbiHit;
  cases: RunCase[];
  plan: PlanView | null;
}): boolean {
  if (runIsGoing()) return false;
  setState({
    ...state,
    run: {
      ...s,
      phase: "setup",
      open: true,
      runId: null,
      rows: {},
      phases: {},
      latest: null,
      position: null,
      records: {},
      resetNeeded: null,
      answering: false,
      stopping: false,
      resultId: null,
      error: "",
    },
  });
  return true;
}

/** A setup nobody started goes with the screen that opened it. */
export function cancelRunSetup(): void {
  if (state.run?.phase === "setup") setState({ ...state, run: null });
}

function onProgress(gen: number, p: ReplayProgress) {
  const r = state.run;
  if (gen !== generation || !r) return;
  // The first event fixes which run this is: a stray event from another
  // run (one that outlived its window, say) must not change what shows.
  const runId = r.runId ?? p.run_id;
  if (p.run_id !== runId) return;
  patchRun((cur) => ({
    runId,
    rows: { ...cur.rows, [p.case_id]: statusOf(p) },
    phases: { ...cur.phases, [p.case_id]: p.phase },
    latest: { caseId: p.case_id, phase: p.phase, step: p.step_number },
    position: { index: p.index, total: p.total },
    // A case moving again means the pause is over.
    resetNeeded: p.phase !== "done" ? null : cur.resetNeeded,
  }));
  if (p.phase === "done") {
    // The run file is saved after every case. Never blocks the run: a
    // failed load leaves the row on its status line.
    commands
      .autoRunLoadRun(runId)
      .then((res) => {
        if (gen !== generation || state.run?.runId !== runId) return;
        if (res.status !== "ok" || !res.data) return;
        const rec = res.data.cases.find((x) => x.case_id === p.case_id);
        if (rec) patchRun((cur) => ({ records: { ...cur.records, [p.case_id]: rec.steps } }));
      })
      .catch(() => {});
  }
}

function onResetNeeded(gen: number, reset: AutorunResetNeeded) {
  const r = state.run;
  if (gen !== generation || !r || r.phase !== "running") return;
  if (r.runId !== null && reset.run_id !== r.runId) return;
  patchRun({ resetNeeded: reset, answering: false });
}

/** Starts the run set up in the window. The store keeps the call's promise,
 * so its end is caught whether or not anything is on screen. */
export async function startRun(opts: RunOptions): Promise<void> {
  const r = state.run;
  if (!r || (r.phase !== "setup" && r.phase !== "failed")) return;
  stopListening();
  generation += 1;
  const gen = generation;
  patchRun({
    phase: "running",
    runId: null,
    rows: {},
    phases: {},
    latest: null,
    position: null,
    records: {},
    resetNeeded: null,
    answering: false,
    stopping: false,
    resultId: null,
    error: "",
  });
  unlisteners = [
    events.replayProgress.listen((e) => onProgress(gen, e.payload)),
    events.autorunResetNeeded.listen((e) => onResetNeeded(gen, e.payload)),
  ];
  // Listening before the call starts, so the first event is never missed.
  await Promise.allSettled(unlisteners);
  if (gen !== generation) return;

  const fail = (error: string) => {
    if (gen !== generation || !state.run) return;
    stopListening();
    const inBackground = !state.run.open;
    patchRun({ phase: "failed", error, resetNeeded: null, answering: false, stopping: false });
    announceEnd({ ok: false, inBackground, runId: state.run?.runId ?? null, error });
  };
  try {
    const res = await commands.autoRunReplay(
      r.org,
      r.project,
      r.pbi.id,
      r.cases.map((c) => ({ case_id: c.id, title: c.title, module: c.module?.trim() || null })),
      opts.account,
      opts.browserName,
      opts.watch,
      opts.retryTransient,
      opts.dbReadAccess,
    );
    if (res.status === "error") {
      fail(res.error);
      return;
    }
    const cur = state.run;
    if (gen !== generation || !cur) return;
    stopListening();
    const id = res.data.id;
    if (cur.open) {
      // Its window is open: straight into the review, as always.
      setState({
        run: null,
        review: { org: cur.org, project: cur.project, pbi: cur.pbi, runId: id, toPastRuns: false },
      });
      announceEnd({ ok: true, inBackground: false, runId: id, error: "" });
    } else {
      patchRun({ phase: "finished", resultId: id, resetNeeded: null, answering: false, stopping: false });
      announceEnd({ ok: true, inBackground: true, runId: id, error: "" });
    }
  } catch (e) {
    // The generated wrapper rethrows an Error rather than resolving to
    // {status: "error"} when the call itself rejects.
    fail(e instanceof Error ? e.message : String(e));
  }
}

/** Run in background: the window closes, the run carries on. */
export function sendRunToBackground(): void {
  const r = state.run;
  if (r && r.phase !== "setup") patchRun({ open: false });
}

/** The pill: shows the run's window again, or opens a finished run's
 * review. */
export function openRunWindow(): void {
  const r = state.run;
  if (!r) return;
  if (r.phase === "finished") {
    reviewFinishedRun();
    return;
  }
  if (!r.open) patchRun({ open: true });
}

/** The window's own way out: a run going carries on in the background; a
 * setup, or a run that failed, is done with. */
export function closeRunWindow(): void {
  const r = state.run;
  if (!r) return;
  if (r.phase === "running") sendRunToBackground();
  else setState({ ...state, run: null });
}

/** Asks the run to stop after the step it is on. */
export async function stopRun(): Promise<void> {
  if (!runIsGoing()) return;
  // Said the instant it is pressed: the run itself only stops after its
  // current step, and there is nothing else to tell until it does.
  patchRun({ stopping: true });
  await commands.autoRunReplayCancel().catch(() => {});
}

/** The answer at the reset point: Continue runs the next phase, Stop ends
 * the run there. A refused answer (the run stopped meanwhile) leaves the
 * run to end on its own. */
export async function answerRunReset(continueRun: boolean): Promise<void> {
  const reset = state.run?.resetNeeded;
  if (!reset) return;
  patchRun((cur) => ({ answering: true, stopping: cur.stopping || !continueRun }));
  let error = "";
  try {
    const res = await commands.autoRunAnswerReset(reset.run_id, continueRun);
    if (res.status === "error") error = res.error;
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  }
  patchRun((cur) => ({
    resetNeeded: cur.resetNeeded?.run_id === reset.run_id ? null : cur.resetNeeded,
    answering: false,
    error: error || cur.error,
  }));
}

/** Review on the pill or the toast: hands the finished run to the Auto Run
 * screen, on Past runs, and lets the run go. */
export function reviewFinishedRun(): void {
  const r = state.run;
  if (!r || r.phase !== "finished" || !r.resultId) return;
  setState({
    run: null,
    review: { org: r.org, project: r.project, pbi: r.pbi, runId: r.resultId, toPastRuns: true },
  });
}

/** The Auto Run screen opened the review. */
export function clearReviewRequest(): void {
  if (state.review) setState({ ...state, review: null });
}

/** Forgets everything, listeners included. For tests. */
export function resetBackgroundRun(): void {
  generation += 1;
  stopListening();
  setState({ run: null, review: null });
}
