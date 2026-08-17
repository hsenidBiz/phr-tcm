/**
 * Default tags for Manual Entry, per org/project.
 *
 * Tags are project-scoped in Azure DevOps, so the default set is too - a
 * "smoke; HRM" default for one project would be noise in another. Stored
 * as the same semicolon-joined string the TestCase model carries, so it
 * drops straight into the tags field with no conversion.
 */

const key = (org: string, project: string) => `tcm-v2-default-tags:${org}/${project}`;

export function loadDefaultTags(org: string, project: string): string {
  if (!org || !project) return "";
  try {
    return localStorage.getItem(key(org, project)) ?? "";
  } catch {
    return "";
  }
}

export function saveDefaultTags(org: string, project: string, value: string): void {
  if (!org || !project) return;
  try {
    const trimmed = value.trim();
    if (trimmed === "") localStorage.removeItem(key(org, project));
    else localStorage.setItem(key(org, project), trimmed);
  } catch {
    // storage unavailable -> session-only
  }
}
