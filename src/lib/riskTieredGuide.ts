// The AI Bridge tab's "Risk-tiered test design (trial)" switch: whether the
// writing guide the assistant reads carries the team's risk-tiering rules
// (tiers, design techniques, a scenario list approved before drafting, a
// budget per story, trace/tier/run-category tags) instead of the plain
// granularity and edge-case sections.
//
// Kept on this machine and pushed to the bridge with the other switches, the
// same shape as the API templates switch (apiTemplates.ts), which this file
// mirrors.

/** "1" only when it is on, and absent otherwise - a fresh profile and a
 * cleared one both read off, so the plain guide is the default. */
const RISK_TIERED_KEY = "tcm-v2-risk-tiered-guide";

const listeners = new Set<() => void>();

function notify(): void {
  for (const l of listeners) l();
}

/** Subscription so App re-pushes the bridge context the moment the switch
 * changes, and the assistant's next read of the guide sees it. */
export function subscribeRiskTiered(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Whether the writing guide carries the risk-tiered rules. Off unless it
 * was explicitly switched on. */
export function loadRiskTiered(): boolean {
  try {
    return localStorage.getItem(RISK_TIERED_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveRiskTiered(on: boolean): void {
  try {
    if (on) localStorage.setItem(RISK_TIERED_KEY, "1");
    else localStorage.removeItem(RISK_TIERED_KEY);
  } catch {
    // storage unavailable -> the choice lasts for this session only
  }
  notify();
}

/** A primitive, so `useSyncExternalStore` is happy to re-read it. */
export function riskTieredSnapshot(): boolean {
  return loadRiskTiered();
}
