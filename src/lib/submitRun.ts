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
 * submit finishes, the created cases have to leave the queue. When the
 * screen is mounted its own setQueue does that (registered here); when it
 * is not, the pruned queue is written straight into the persisted draft,
 * because a created case still sitting in a queue is one Create away from
 * a duplicate work item - and this app cannot delete one.
 */

import type { TestCase } from "../bindings";

export type SubmitPhase = {
  org: string;
  pbiId: number;
  done: number;
  total: number;
  /** The title currently being written, for the bar's caption. */
  title: string;
} | null;

let phase: SubmitPhase = null;
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

export function submitStarted(org: string, pbiId: number, total: number): void {
  phase = { org, pbiId, done: 0, total, title: "" };
  emit();
}

export function submitProgressed(done: number, total: number, title: string): void {
  if (!phase) return; // a progress event with no submit is a stray
  phase = { ...phase, done, total, title };
  emit();
}

export function submitFinished(): void {
  phase = null;
  emit();
}

/** The mounted queue's own state setter, so a finish that lands while the
 * screen is up flows through React state exactly as it always has. Keyed
 * by scope: a submit for PBI 42 must never write PBI 7's queue. */
type QueueWriter = {
  org: string;
  pbiId: number;
  setQueue: (updater: (q: TestCase[]) => TestCase[]) => void;
  onCleared: () => void;
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
