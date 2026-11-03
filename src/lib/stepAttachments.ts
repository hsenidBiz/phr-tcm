import type { AttachedFile, Step, TestCase } from "../bindings";

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

/** `tc` after a save that `sent` it: each uploaded file's local path is swapped
 * for the id Azure DevOps gave it (so saving again cannot attach it twice),
 * and, when the save applied its attachment work (`applied`: it had nothing
 * to say about it), the removals it carried are cleared. What was typed since
 * the save stays. */
export function stampSaved(tc: TestCase, sent: TestCase, attached: AttachedFile[], applied: boolean): TestCase {
  const steps = tc.steps.map((s) => ({ ...s }));
  for (const a of attached) {
    const list = steps[a.step]?.attachments;
    const k = list ? list.findIndex((x) => x.path === a.path) : -1;
    if (!list || k < 0) continue;
    const size = list[k].size;
    steps[a.step] = {
      ...steps[a.step],
      attachments: list.map((x, m) => (m === k ? { id: a.id, name: a.name, ...(size != null ? { size } : {}) } : x)),
    };
  }
  const next: TestCase = { ...tc, steps };
  if (applied) {
    const done = new Set(sent.removed_attachments ?? []);
    const left = (tc.removed_attachments ?? []).filter((id) => !done.has(id));
    if (left.length > 0) next.removed_attachments = left;
    else delete next.removed_attachments;
  }
  return next;
}
