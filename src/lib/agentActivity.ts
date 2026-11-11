// What an assistant is doing, in words: shared by the Assistants card and the
// activity strip so the two never say it differently.

import type { AgentActivity } from "../bindings";

/** What a row says its assistant is doing. */
export function activityText(a: AgentActivity, idleMinutes: number): string {
  switch (a.kind) {
    case "idle":
      return idleMinutes > 0 ? `Idle for ${idleMinutes} min` : "Idle";
    case "discovering":
      return a.area ? `Discovering ${a.area}` : "Discovering";
    case "mapping":
      return `Mapping menus (${a.modules.join(", ")})`;
    case "exploring":
      return "Exploring";
    case "replaying":
      // Before the first step the count is 0 of 0, which is not worth saying.
      return a.step === 0 && a.of === 0
        ? `Replaying case ${a.case_id} (starting)`
        : `Replaying case ${a.case_id} (step ${a.step} of ${a.of})`;
    case "working":
      return "Working";
  }
}

/** Whether the assistant is doing something, which is what brings the card
 * and the strip up. */
export function isWorking(a: AgentActivity): boolean {
  return a.kind !== "idle";
}
