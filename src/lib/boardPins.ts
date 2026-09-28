// The areas pinned to the top of the Work Manager board's scope list: the
// few teams someone looks at most, out of a list that runs to dozens.
// Remembered per org/project on this machine, in the order they were
// pinned.

export function pinnedAreasKey(org: string, project: string): string {
  return `tcm-v2-board-pinned-areas:${org}/${project}`;
}

/** This org/project's pinned areas, oldest pin first. Anything unreadable
 * is ignored rather than trusted. */
export function loadPinnedAreas(org: string, project: string): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(pinnedAreasKey(org, project)) ?? "[]");
    if (!Array.isArray(raw)) return [];
    return [...new Set(raw.filter((v): v is string => typeof v === "string" && v.trim() !== ""))];
  } catch {
    return [];
  }
}

export function savePinnedAreas(org: string, project: string, areas: string[]): void {
  try {
    localStorage.setItem(pinnedAreasKey(org, project), JSON.stringify(areas));
  } catch {
    // session-only
  }
}

/** Pin an area at the end of the pinned list, or unpin it. */
export function togglePin(pinned: string[], area: string): string[] {
  return pinned.includes(area) ? pinned.filter((a) => a !== area) : [...pinned, area];
}
