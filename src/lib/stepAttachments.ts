import type { Step, TestCase } from "../bindings";

/** The GUIDs of the attachments Azure DevOps already holds that a list of steps carries. */
function heldIds(steps: Step[]): Set<string> {
  const ids = new Set<string>();
  for (const s of steps) for (const a of s.attachments ?? []) if (a.id) ids.add(a.id);
  return ids;
}

/** `tc` with `steps` in place, and every existing attachment that no longer
 * sits on any step recorded in `removed_attachments` (so the upload removes
 * it). A file not yet uploaded is simply gone with its chip. An id that is
 * back on a step is taken off the list again, and no id is listed twice.
 * Every host of the step editor sets its steps through this. */
export function withSteps(tc: TestCase, steps: Step[]): TestCase {
  const before = heldIds(tc.steps);
  const after = heldIds(steps);
  const removed = new Set(tc.removed_attachments ?? []);
  for (const id of before) if (!after.has(id)) removed.add(id);
  for (const id of after) removed.delete(id);
  const next: TestCase = { ...tc, steps };
  if (removed.size > 0) next.removed_attachments = [...removed];
  else delete next.removed_attachments;
  return next;
}
