import type { TestCase } from "../bindings";

/** Client-side mirror of the Rust TestCase::is_valid rules, so the review
 * gate can flag problems before anything is submitted. The Rust check still
 * runs server-side on submit - this is preview only. */
export function validateCase(tc: TestCase): string | null {
  const title = tc.title.trim();
  if (!title) return "Title is required.";
  if (title.length > 255) return `Title is ${title.length} characters - max 255.`;
  if (tc.steps.length === 0) return "At least one step is required.";
  const bad = tc.steps.findIndex((s) => !s.action.trim());
  if (bad >= 0) return `Step ${bad + 1} action is empty.`;
  if (!["Not Automated", "Planned"].includes(tc.automation_status))
    return `Invalid automation status: '${tc.automation_status}'`;
  if (tc.tags.includes(","))
    return "Tags must be separated with semicolons - commas are not allowed.";
  return null;
}

/** Duplicate-title warning (v1 rule: warn only, never implicit update). */
export function duplicateWarning(tc: TestCase, existingTitles: string[]): string | null {
  if (tc.update_id != null) return null; // explicit update, not a duplicate
  const t = tc.title.trim().toLowerCase();
  return existingTitles.some((e) => e.trim().toLowerCase() === t)
    ? "A test case with this title already exists on the PBI - this will create a duplicate, not update it."
    : null;
}
