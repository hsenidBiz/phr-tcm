import {
  useCallback,
  useEffect,
  useState,
  useSyncExternalStore,
  type Dispatch,
  type SetStateAction,
} from "react";
import type { TestCase } from "../bindings";
import { TOUR_QUEUE } from "../tour/tourData";
import { subscribeTour, tourRunningSnapshot } from "../tour/tourState";

const draftKey = (org: string, pbiId: number) => `tcm-v2-draft:${org}/${pbiId}`;

/** Direct draft access for code that outlives the screen - the submit
 * finishing after navigation writes the pruned queue here, and the next
 * mount of useQueue loads it. */
export function loadDraftQueue(org: string, pbiId: number): TestCase[] {
  return loadDraft(org, pbiId);
}

export function saveDraftQueue(org: string, pbiId: number, queue: TestCase[]): void {
  try {
    if (queue.length === 0) localStorage.removeItem(draftKey(org, pbiId));
    else localStorage.setItem(draftKey(org, pbiId), JSON.stringify(queue));
  } catch {
    // storage unavailable -> session-only
  }
}

function loadDraft(org: string, pbiId: number): TestCase[] {
  try {
    const raw = localStorage.getItem(draftKey(org, pbiId));
    return raw ? (JSON.parse(raw) as TestCase[]) : [];
  } catch {
    return [];
  }
}

/** What a queue in state belongs to. The tour is a scope of its own, and
 * one no draft key can ever spell (keys are `org/pbiId`). */
const TOUR_SCOPE = "tour";
const scopeOf = (tour: boolean, org: string, pbiId: number | null) =>
  tour ? TOUR_SCOPE : pbiId == null ? "" : `${org}/${pbiId}`;

function loadFor(tour: boolean, org: string, pbiId: number | null): TestCase[] {
  // While the tour runs the queue is the fixture and storage is not
  // touched AT ALL - not read, not written, not cleared. The tour's whole
  // promise is to leave nothing behind, and the user's saved draft is the
  // last place to be clever about that: a hook that never touches it
  // cannot lose it.
  if (tour) return [...TOUR_QUEUE];
  return pbiId != null ? loadDraft(org, pbiId) : [];
}

/** The ONE pending-creation queue, shared by Manual Entry and Import File
 * (v1: every tab feeds the same queue) and persisted per PBI so a closed
 * app never loses queued work.
 *
 * The queue is held WITH the scope it was loaded for, and that pairing is
 * what makes saving safe. A scope change - a new PBI, or the tour handing
 * the app back - reaches the save effect one render BEFORE the reload
 * does, so for that render the queue in state belongs to the scope just
 * left. Writing it then would file one PBI's cases (or the tour's sample
 * ones) under another PBI's key. Mismatched pair -> no write; the save
 * happens on the next render, with the queue that key actually owns. */
export function useQueue(org: string, pbiId: number | null) {
  const tour = useSyncExternalStore(subscribeTour, tourRunningSnapshot);
  const scope = scopeOf(tour, org, pbiId);
  const [state, setState] = useState<{ scope: string; queue: TestCase[] }>(() => ({
    scope,
    queue: loadFor(tour, org, pbiId),
  }));

  // Scope switch (the tour's arrival and departure included) -> reload.
  useEffect(() => {
    setState((prev) => (prev.scope === scope ? prev : { scope, queue: loadFor(tour, org, pbiId) }));
  }, [scope, tour, org, pbiId]);

  useEffect(() => {
    if (tour || pbiId == null) return;
    if (state.scope !== scope) return; // belongs to the scope we just left
    saveDraftQueue(org, pbiId, state.queue);
  }, [state, scope, tour, org, pbiId]);

  const setQueue = useCallback<Dispatch<SetStateAction<TestCase[]>>>((update) => {
    setState((prev) => ({
      ...prev,
      queue: typeof update === "function" ? update(prev.queue) : update,
    }));
  }, []);

  return { queue: state.queue, setQueue };
}
