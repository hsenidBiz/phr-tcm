/**
 * The Test map's tree, built here so the app has ONE grouping rule: a case
 * with an `area` path lands under that path; a case without one falls back
 * to the title grouping every grouped screen already uses. The finished
 * tree goes to Rust, which only writes the page (test_map.rs).
 */
import type { MapCase, MapNode, TestCase } from "../bindings";
import { groupIndices } from "./grouping";

export const UNGROUPED = "Ungrouped";

/** "Manage Events / Create / Validation" -> ["Manage Events", "Create", "Validation"].
 * The slash is the separator; spaces around it are optional; empty
 * segments are dropped. */
export function splitArea(area: string | null | undefined): string[] {
  return (area ?? "")
    .split("/")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

type Draft = { name: string; cases: MapCase[]; children: Map<string, Draft> };

const toMapCase = (tc: TestCase): MapCase => ({
  id: tc.update_id ?? null,
  title: tc.title,
  steps: tc.steps,
  preconditions: tc.preconditions,
  tags: tc.tags,
  automation_status: tc.automation_status,
});

/** The child named `name`, made if missing. Names compare without case,
 * and the first spelling seen is the one shown. */
function child(parent: Map<string, Draft>, name: string): Draft {
  const key = name.toLowerCase();
  let d = parent.get(key);
  if (!d) {
    d = { name, cases: [], children: new Map() };
    parent.set(key, d);
  }
  return d;
}

function byName(a: MapNode, b: MapNode): number {
  if (a.name === UNGROUPED) return 1;
  if (b.name === UNGROUPED) return -1;
  return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
}

function finish(d: Draft): MapNode {
  const children = [...d.children.values()].map(finish).sort(byName);
  const count = d.cases.length + children.reduce((n, c) => n + c.count, 0);
  return { name: d.name, count, cases: d.cases, children };
}

export function buildTestMap(cases: TestCase[]): MapNode[] {
  const root = new Map<string, Draft>();
  const untagged: TestCase[] = [];
  for (const tc of cases) {
    const path = splitArea(tc.area);
    if (path.length === 0) {
      untagged.push(tc);
      continue;
    }
    let level = root;
    let node: Draft | null = null;
    for (const segment of path) {
      node = child(level, segment);
      level = node.children;
    }
    node!.cases.push(toMapCase(tc));
  }
  // The fallback, one level deep: a title group joins an area root of the
  // same name rather than sitting beside it.
  for (const g of groupIndices(untagged.map((c) => c.title))) {
    const node = child(root, g.name || UNGROUPED);
    for (const i of g.indices) node.cases.push(toMapCase(untagged[i]));
  }
  return [...root.values()].map(finish).sort(byName);
}
