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

/** Best field for a term: an exact name match beats a name that starts
 * with the term, which beats one merely containing it - so "Module" wins
 * over "Sub Module" when both exist. */
function bestMatch(fields: FieldRef[], terms: string[]): string | null {
  const ranked = [
    (n: string) => terms.some((t) => n === t),
    (n: string) => terms.some((t) => n.startsWith(t)),
    (n: string) => terms.some((t) => n.includes(t)),
  ];
  for (const matches of ranked) {
    const hit = fields.find((f) => matches(f.name.toLowerCase()));
    if (hit) return hit.reference_name;
  }
  return null;
}

/** v1 config_screen auto-pick, ranked (exact > prefix > contains). Skip
 * when nothing matches. */
export function autoPick(fields: FieldRef[]): FieldPrefs {
  return {
    moduleRef: bestMatch(fields, ["module"]),
    preconditionsRef: bestMatch(fields, ["prerequisite", "precondition"]),
  };
}
