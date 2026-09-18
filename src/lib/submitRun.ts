/**
 * Submit progress that survives the screen it started on.
 *
 * The upload runs in Rust and takes minutes on a big queue; the person
 * watching it is exactly the person who wanders to another tab meanwhile.
 * Progress used to live in QueueSection's own state, so navigating away
 * reset the bar to nothing and coming back showed an app doing work with
 * no sign of it. This module holds the live phase at MODULE scope: any
 * mount of the queue reads it, so returning mid-upload shows the bar
 * exactly where it is.
 *
 * It also carries the part that must not die with the component: after a
 * submit finishes, the created cases in the queue have to carry their new
 * work item ids. When the screen is mounted its own setQueue does that
 * (registered here); when it is not, the stamped queue is written straight
 * into the persisted draft, because a created case sitting in a queue
 * without its id is one Upload away from a duplicate work item - and this
 * app cannot delete one.
 */

import type { TestCase } from "../bindings";
import type { WatchedFile } from "./fileSync";

export type SubmitPhase = {
  /** Which submit this is. Only the run that set the phase may clear it. */
  run: number;
  org: string;
  pbiId: number;
  done: number;
  total: number;
  /** The title currently being written, for the bar's caption. */
  title: string;
} | null;

let phase: SubmitPhase = null;
let nextRun = 1;
const listeners = new Set<() => void>();

function emit() {
  for (const l of listeners) l();
}

export function subscribeSubmit(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function submitPhaseSnapshot(): SubmitPhase {
  return phase;
}

/** Start showing a submit and return its run id - or null when a submit is
 * already running. Rust takes one at a time and refuses the second, so a
 * second start used to overwrite the first's progress and then, when Rust
 * refused, clear it while the first was still uploading. */
export function submitStarted(org: string, pbiId: number, total: number): number | null {
  if (phase) return null;
  const run = nextRun++;
  phase = { run, org, pbiId, done: 0, total, title: "" };
  emit();
  return run;
}

export function submitProgressed(done: number, total: number, title: string): void {
  if (!phase) return; // a progress event with no submit is a stray
  phase = { ...phase, done, total, title };
  emit();
}

/** Clear the phase - only if it is still this run's. */
export function submitFinished(run: number): void {
  if (!phase || phase.run !== run) return;
  phase = null;
  emit();
}

/** The mounted queue's own state setter, so a finish that lands while the
 * screen is up flows through React state exactly as it always has. Keyed
 * by scope: a submit for PBI 42 must never write PBI 7's queue.
 *
 * `patchWatch` is the mounted screen's own watch-list update, for the same
 * reason: the mount that STARTED a submit may be gone by the time it
 * finishes, and its callback would update a screen nobody is looking at. */
type QueueWriter = {
  org: string;
  pbiId: number;
  setQueue: (updater: (q: TestCase[]) => TestCase[]) => void;
  patchWatch?: (path: string, fields: Partial<WatchedFile>) => void;
};

let writer: QueueWriter | null = null;

export function registerQueueWriter(w: QueueWriter): () => void {
  writer = w;
  return () => {
    if (writer === w) writer = null;
  };
}

export function queueWriterFor(org: string, pbiId: number): QueueWriter | null {
  return writer && writer.org === org && writer.pbiId === pbiId ? writer : null;
}
