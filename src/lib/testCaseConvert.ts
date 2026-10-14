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
