import { useEffect, useState } from "react";
import type { TestCase } from "../bindings";

const draftKey = (org: string, pbiId: number) => `tcm-v2-draft:${org}/${pbiId}`;

function loadDraft(org: string, pbiId: number): TestCase[] {
  try {
    const raw = localStorage.getItem(draftKey(org, pbiId));
    return raw ? (JSON.parse(raw) as TestCase[]) : [];
  } catch {
    return [];
  }
}

/** The ONE pending-creation queue, shared by Manual Entry and Import File
 * (v1: every tab feeds the same queue) and persisted per PBI so a closed
 * app never loses queued work. */
export function useQueue(org: string, pbiId: number | null) {
  const [queue, setQueue] = useState<TestCase[]>(() =>
    pbiId != null ? loadDraft(org, pbiId) : [],
  );

  // Scope switch -> reload that PBI's draft.
  useEffect(() => {
    setQueue(pbiId != null ? loadDraft(org, pbiId) : []);
  }, [org, pbiId]);

  useEffect(() => {
    if (pbiId == null) return;
    try {
      if (queue.length === 0) localStorage.removeItem(draftKey(org, pbiId));
      else localStorage.setItem(draftKey(org, pbiId), JSON.stringify(queue));
    } catch {
      // storage unavailable -> session-only
    }
  }, [queue, org, pbiId]);

  return { queue, setQueue };
}
