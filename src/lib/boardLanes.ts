// Swimlanes on the Work Manager board: cards grouped by their DIRECT
// parent (tasks and bugs under their PBI, PBIs under their Feature), the
// way Azure DevOps' own "group by parent" does - no walking up the tree.
// Pure grouping and naming, plus the two preferences the view remembers
// on this machine.

import type { BoardItem, BoardParent } from "../bindings";

/** The lane id of cards with no parent. Work item ids start at 1. */
export const NO_PARENT = 0;

export type Lane = {
  /** The parent's work item id, or NO_PARENT. */
  id: number;
  /** Null for the No parent lane. Its title is empty when the parent
   * could not be read. */
  parent: BoardParent | null;
  items: BoardItem[];
};

/** A card's lane. `parent` is missing altogether on board data shaped
 * before swimlanes existed, which reads as no parent. */
export function laneIdOf(item: BoardItem): number {
  return item.parent?.id ?? NO_PARENT;
}

/** Group cards into lanes in the order given. The board's order is most
 * recently changed first, so each lane sits where its newest card would.
 * No parent always comes last. Only lanes with cards exist, so a lane the
 * filters emptied is simply not there. */
export function groupIntoLanes(items: BoardItem[]): Lane[] {
  const lanes = new Map<number, Lane>();
  for (const item of items) {
    const id = laneIdOf(item);
    const lane = lanes.get(id);
    if (lane) lane.items.push(item);
    else lanes.set(id, { id, parent: id === NO_PARENT ? null : (item.parent ?? null), items: [item] });
  }
  const all = [...lanes.values()];
  return [...all.filter((l) => l.id !== NO_PARENT), ...all.filter((l) => l.id === NO_PARENT)];
}

export function cardCount(n: number): string {
  return `${n} card${n === 1 ? "" : "s"}`;
}

/** What a lane is called: its parent's title, `#id` when the parent could
 * not be read, or "No parent". */
export function laneLabel(lane: Lane): string {
  if (lane.id === NO_PARENT) return "No parent";
  return lane.parent?.title || `#${lane.id}`;
}

/** The lane toggle's accessible name: "Leave requests, 4 cards, collapse". */
export function laneToggleName(lane: Lane, collapsed: boolean): string {
  return `${laneLabel(lane)}, ${cardCount(lane.items.length)}, ${collapsed ? "expand" : "collapse"}`;
}

export const SWIMLANES_KEY = "tcm-v2-board-swimlanes";

export function loadSwimlanes(): boolean {
  try {
    return localStorage.getItem(SWIMLANES_KEY) === "on";
  } catch {
    return false;
  }
}

export function saveSwimlanes(on: boolean): void {
  try {
    localStorage.setItem(SWIMLANES_KEY, on ? "on" : "off");
  } catch {
    // session-only
  }
}

export function collapsedLanesKey(org: string, project: string): string {
  return `tcm-v2-board-lanes-collapsed:${org}/${project}`;
}

/** The lanes collapsed on this org/project's board: parent ids, 0 for No
 * parent. Anything unreadable is ignored rather than trusted. */
export function loadCollapsedLanes(org: string, project: string): Set<number> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(collapsedLanesKey(org, project)) ?? "[]");
    if (!Array.isArray(raw)) return new Set();
    return new Set(raw.filter((v): v is number => Number.isInteger(v) && v >= 0));
  } catch {
    return new Set();
  }
}

export function saveCollapsedLanes(org: string, project: string, ids: Set<number>): void {
  try {
    localStorage.setItem(
      collapsedLanesKey(org, project),
      JSON.stringify([...ids].sort((a, b) => a - b)),
    );
  } catch {
    // session-only
  }
}
