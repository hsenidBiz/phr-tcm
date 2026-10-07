// The AI Bridge tab's Writing style: Markdown the person edits, which the
// writing guide carries instead of its standard granularity and edge-case
// sections while it is switched on. Rust keeps it (writing-style.json in
// the app data folder) and the guide reads it on every call; this file
// holds the query key the card and App share, and the one-time move of
// the old trial switch.

import { commands } from "../bindings";
import { unwrapStr } from "./ipc";

/** The card's query, invalidated by App when the migration saves. */
export const WRITING_STYLE_QUERY = ["writing-style"] as const;

/** Where an older version kept the AI Bridge tab's "Risk-tiered test
 * design (trial)" switch: "1" when it was on. The trial's rules are now
 * the starting writing style, so an "on" here becomes that style switched
 * on. */
const OLD_TRIAL_KEY = "tcm-v2-risk-tiered-guide";

function readOldTrial(): string | null {
  try {
    return localStorage.getItem(OLD_TRIAL_KEY);
  } catch {
    return null;
  }
}

function dropOldTrial(): void {
  try {
    localStorage.removeItem(OLD_TRIAL_KEY);
  } catch {
    // storage unavailable -> nothing was stored to drop either
  }
}

/** Moves the old trial switch over once: when it was on, the saved style
 * (the trial's rules, unless the person has written their own) is saved
 * switched on, and the key goes. A failed save keeps the key, so the next
 * start tries again. Resolves true when it saved. App calls it at start,
 * so it runs even if the AI Bridge tab is never opened. */
export async function migrateOldTrialSwitch(): Promise<boolean> {
  const old = readOldTrial();
  if (old === null) return false;
  if (old !== "1") {
    dropOldTrial();
    return false;
  }
  try {
    const saved = await commands.writingStyleGet();
    if (!saved.enabled) await unwrapStr(commands.writingStyleSave({ enabled: true, text: saved.text }));
    dropOldTrial();
    return true;
  } catch {
    return false;
  }
}
