/**
 * The Queue's "Group by area": a case's `area` path ("Manage Events /
 * Create") read as nested groups, one level per segment.
 *
 * Display only. Nothing here reorders the queue: each group lists the
 * queue INDICES of its cases, so a row keeps its real position and
 * selection, editing and upload order stay keyed exactly as before.
 *
 * Order is the queue's own, not A-Z: a group appears where its first case
 * appears, cases inside a group keep queue order, and cases with no area
 * sit under "No area", last (not "Ungrouped": a real area may be called
 * that, and must not read as the same heading). Segments compare ignoring case and the
 * spaces around them, so "Display" and "display " are one group, shown
 * with the first spelling seen.
 *
 * The page View in browser writes builds the same tree in Rust
 * (`import_parser::area_groups`), and the two are tested on the same cases.
 */

export const NO_AREA = "No area";

export type AreaGroup = {
  /** This level's segment, as first spelled in the queue. */
  name: string;
  /** The whole path, as first spelled: "Manage Events / Create". */
  path: string;
  /** The path folded for comparison (lower case, trimmed segments). What a
   * folded group is remembered by, so a later spelling does not unfold it.
   * Empty for "No area", which no real path can be. */
  key: string;
  /** Every case under this group, nested ones included. */
  count: number;
  /** Queue indices of the cases directly in this group, in queue order. */
  indices: number[];
  children: AreaGroup[];
};

/** "Manage Events/Create " -> ["Manage Events", "Create"]: the same rule as
 * `import_parser::normalise_area` on the Rust side. */
export function splitArea(raw: string): string[] {
  return raw
    .split("/")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

const fold = (segment: string) => segment.toLowerCase();

/** The tree of areas for these cases, in queue order. */
export function buildAreaGroups(cases: readonly { area?: string | null }[]): AreaGroup[] {
  const roots: AreaGroup[] = [];
  let noArea: AreaGroup | null = null;
  cases.forEach((tc, i) => {
    const segments = splitArea(tc.area ?? "");
    if (segments.length === 0) {
      noArea ??= { name: NO_AREA, path: NO_AREA, key: "", count: 0, indices: [], children: [] };
      noArea.indices.push(i);
      return;
    }
    let siblings = roots;
    let parent: AreaGroup | null = null;
    for (const segment of segments) {
      const folded = fold(segment);
      let node = siblings.find((g) => fold(g.name) === folded);
      if (!node) {
        node = {
          name: segment,
          path: parent ? `${parent.path} / ${segment}` : segment,
          key: parent ? `${parent.key} / ${folded}` : folded,
          count: 0,
          indices: [],
          children: [],
        };
        siblings.push(node);
      }
      parent = node;
      siblings = node.children;
    }
    parent!.indices.push(i);
  });
  const out = noArea ? [...roots, noArea] : roots;
  const tally = (g: AreaGroup): number =>
    (g.count = g.indices.length + g.children.reduce((n, c) => n + tally(c), 0));
  out.forEach(tally);
  return out;
}

/** Every queue index under `group`, nested included, in the order the
 * grouped Queue shows them: the group's own cases, then each subgroup's. */
export function groupIndices(group: AreaGroup): number[] {
  return [...group.indices, ...group.children.flatMap(groupIndices)];
}

/** The queue indices on screen, top to bottom, when the groups whose keys
 * are in `folded` are folded away - what a Shift-click range runs over. */
export function visibleOrder(groups: readonly AreaGroup[], folded: ReadonlySet<string>): number[] {
  return groups.flatMap((g) =>
    folded.has(g.key) ? [] : [...g.indices, ...visibleOrder(g.children, folded)],
  );
}
