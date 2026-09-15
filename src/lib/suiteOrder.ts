import { groupIndices } from "./grouping";

/** One test case as the Suite Management list shows it. */
export type SuiteCase = { id: number; title: string };

/** A copy of `list` with the item at `from` moved to `to`. Anything out of
 * range, or a move onto itself, returns an equal copy. */
export function moveItem<T>(list: T[], from: number, to: number): T[] {
  const out = list.slice();
  if (from === to || from < 0 || to < 0 || from >= out.length || to >= out.length) return out;
  const [item] = out.splice(from, 1);
  out.splice(to, 0, item);
  return out;
}

/** True when both lists carry the same ids in the same order. Titles do
 * not matter: the order is what gets saved. */
export function sameOrder(a: SuiteCase[], b: SuiteCase[]): boolean {
  return a.length === b.length && a.every((c, i) => c.id === b[i].id);
}

/** Everything in `ids` gathered into one block, in its current relative
 * order, and dropped at `targetId` the way `moveItem` drops one row: after
 * the target when the block came from above it, before it when the block
 * came from below. A target inside the block, or not in the list, is a
 * no-op (a fresh copy either way). */
export function moveBlock(list: SuiteCase[], ids: ReadonlySet<number>, targetId: number): SuiteCase[] {
  const targetAt = list.findIndex((c) => c.id === targetId);
  if (targetAt < 0 || ids.has(targetId)) return list.slice();
  const block = list.filter((c) => ids.has(c.id));
  if (block.length === 0) return list.slice();
  const rest = list.filter((c) => !ids.has(c.id));
  const firstAt = list.findIndex((c) => ids.has(c.id));
  const restTargetAt = rest.findIndex((c) => c.id === targetId);
  const at = firstAt < targetAt ? restTargetAt + 1 : restTargetAt;
  return [...rest.slice(0, at), ...block, ...rest.slice(at)];
}

/** As moveBlock, but always inserts the block BEFORE targetId, regardless
 * of which direction the block came from. moveBlock's up/down asymmetry
 * (after the target from above, before it from below) is right for an
 * ordinary row - it drops in like any other item - but a section header
 * is a boundary, not a row: dropping a block onto it must place the block
 * before that section every time, or a drop from above ends up INSIDE
 * the section instead of ahead of it. A target inside the block, or not
 * in the list, is a no-op (a fresh copy either way). */
export function moveBlockBefore(list: SuiteCase[], ids: ReadonlySet<number>, targetId: number): SuiteCase[] {
  const rest = list.filter((c) => !ids.has(c.id));
  const restTargetAt = rest.findIndex((c) => c.id === targetId);
  if (restTargetAt < 0) return list.slice();
  const block = list.filter((c) => ids.has(c.id));
  if (block.length === 0) return list.slice();
  return [...rest.slice(0, restTargetAt), ...block, ...rest.slice(restTargetAt)];
}

/** The block one step up or down - what Move up / Move down do on a
 * ticked row. A scattered selection is gathered at its first member. */
export function nudgeBlock(list: SuiteCase[], ids: ReadonlySet<number>, dir: "up" | "down"): SuiteCase[] {
  const block = list.filter((c) => ids.has(c.id));
  if (block.length === 0) return list.slice();
  const rest = list.filter((c) => !ids.has(c.id));
  const firstAt = list.findIndex((c) => ids.has(c.id));
  // Where the block sits in `rest` terms: how many non-members precede it.
  const before = list.slice(0, firstAt).filter((c) => !ids.has(c.id)).length;
  const at = dir === "up" ? Math.max(0, before - 1) : Math.min(rest.length, before + 1);
  return [...rest.slice(0, at), ...block, ...rest.slice(at)];
}

/** Each case's title group, as Run Tests and View Test Cases see them:
 * "" for a case in no group. */
export function groupKeys(cases: SuiteCase[]): string[] {
  const keys = new Array<string>(cases.length).fill("");
  for (const g of groupIndices(cases.map((c) => c.title))) {
    for (const i of g.indices) keys[i] = g.name;
  }
  return keys;
}

export type Section = { name: string; ids: number[] };

/** The list as runs of one group, in the CURRENT order: a group split by a
 * drag shows as two sections rather than being silently re-joined. */
export function sectionsOf(cases: SuiteCase[]): Section[] {
  const keys = groupKeys(cases);
  const out: Section[] = [];
  cases.forEach((c, i) => {
    const last = out[out.length - 1];
    if (last && last.name === keys[i]) last.ids.push(c.id);
    else out.push({ name: keys[i], ids: [c.id] });
  });
  return out;
}

function arrangeByGroup(cases: SuiteCase[], groupOrder: (names: string[]) => string[]): SuiteCase[] {
  const keys = groupKeys(cases);
  const firstSeen: string[] = [];
  for (const k of keys) if (k && !firstSeen.includes(k)) firstSeen.push(k);
  const out: SuiteCase[] = [];
  for (const name of groupOrder(firstSeen)) {
    cases.forEach((c, i) => {
      if (keys[i] === name) out.push(c);
    });
  }
  cases.forEach((c, i) => {
    if (!keys[i]) out.push(c);
  });
  return out;
}

/** Every group contiguous, groups in the order they first appear, cases
 * keeping their order inside; ungrouped cases last. What the Group by
 * title switch does when turned on. */
export function orderByGroups(cases: SuiteCase[]): SuiteCase[] {
  return arrangeByGroup(cases, (names) => names);
}

/** As orderByGroups, with the groups sorted by name. */
export function orderGroupsAZ(cases: SuiteCase[]): SuiteCase[] {
  return arrangeByGroup(cases, (names) =>
    [...names].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase())),
  );
}

export type FileForOrder = { name: string; cases: Array<{ update_id: number | null }> };

/** The order a set of draft files asks for. Each file is one block in the
 * file's ROW order (tester_order plays no part - the numbers are what
 * jumbled a suite once); blocks follow the files' order; a case named by
 * two files goes with the first and is counted in `duplicates`; every
 * suite case in no file trails in its current order. `placed[i]` is how
 * many suite cases file i placed, so the dialog can flag a file that has
 * nothing to say about this suite. */
export function orderFromFiles(
  current: SuiteCase[],
  files: FileForOrder[],
): { order: SuiteCase[]; placed: number[]; duplicates: number } {
  const byId = new Map(current.map((c) => [c.id, c]));
  const taken = new Set<number>();
  const blocks: SuiteCase[] = [];
  const placed: number[] = [];
  let duplicates = 0;
  for (const f of files) {
    let n = 0;
    const seenHere = new Set<number>();
    for (const fc of f.cases) {
      const id = fc.update_id;
      if (id == null || !byId.has(id) || seenHere.has(id)) continue;
      seenHere.add(id);
      if (taken.has(id)) {
        duplicates += 1;
        continue;
      }
      taken.add(id);
      blocks.push(byId.get(id)!);
      n += 1;
    }
    placed.push(n);
  }
  const rest = current.filter((c) => !taken.has(c.id));
  return { order: [...blocks, ...rest], placed, duplicates };
}

/** Removed in Task 4 of the suite-ordering plan - SuiteCases still imports it. */
export const orderFromFile = (current: SuiteCase[], fileCases: Array<{ update_id: number | null; tester_order?: number | null }>) => {
  const r = orderFromFiles(current, [{ name: "", cases: fileCases }]);
  return { order: r.order, matched: r.placed[0] ?? 0 };
};
