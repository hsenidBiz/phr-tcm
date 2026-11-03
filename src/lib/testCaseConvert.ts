import type { TestCase, TestCaseFull } from "../bindings";

/** A loaded case as the writable model: `id` becomes update_id so a save
 * updates that exact work item. */
export function toTestCase(c: TestCaseFull): TestCase {
  return {
    title: c.title,
    steps: c.steps,
    tags: c.tags,
    automation_status: c.automation_status,
    module_value: c.module_value,
    preconditions: c.preconditions,
    update_id: c.id,
    // The real value from Azure DevOps, so an edit round-trips it. Null
    // (a work item with none) leaves it alone on save.
    priority: c.priority,
  };
}

/** The same case with no step files, and no removals. For edits that never
 *  touch steps (bulk edit, Power Rename): the save then takes the exact path
 *  of a case without files, so it cannot add, remove or re-point one. */
export function withoutStepFiles(tc: TestCase): TestCase {
  const { removed_attachments: _removed, ...rest } = tc;
  return {
    ...rest,
    steps: tc.steps.map(({ attachments: _files, ...step }) => step),
  };
}
