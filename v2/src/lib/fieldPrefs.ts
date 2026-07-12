import type { FieldRef } from "../bindings";

/** Org-specific module/preconditions reference names, per project. */
export type FieldPrefs = {
  moduleRef: string | null;
  preconditionsRef: string | null;
};

const key = (org: string, project: string) => `tcm-v2-fields:${org}/${project}`;

export function loadFieldPrefs(org: string, project: string): FieldPrefs | null {
  try {
    const raw = localStorage.getItem(key(org, project));
    if (!raw) return null;
    const p = JSON.parse(raw);
    return { moduleRef: p.moduleRef ?? null, preconditionsRef: p.preconditionsRef ?? null };
  } catch {
    return null;
  }
}

export function saveFieldPrefs(org: string, project: string, prefs: FieldPrefs) {
  try {
    localStorage.setItem(key(org, project), JSON.stringify(prefs));
  } catch {
    // storage unavailable -> session-only
  }
}

/** v1 config_screen auto-pick: first field whose display name contains
 * "module" / "prerequisite" or "precondition". Skip when nothing matches. */
export function autoPick(fields: FieldRef[]): FieldPrefs {
  const moduleRef =
    fields.find((f) => f.name.toLowerCase().includes("module"))?.reference_name ?? null;
  const preconditionsRef =
    fields.find((f) => {
      const n = f.name.toLowerCase();
      return n.includes("prerequisite") || n.includes("precondition");
    })?.reference_name ?? null;
  return { moduleRef, preconditionsRef };
}
