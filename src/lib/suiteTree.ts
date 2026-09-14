import type { SuiteRef } from "../bindings";

export type SuiteNode = { suite: SuiteRef; children: SuiteNode[] };

/** One level of indent for a suite shown at depth in a flat dropdown.
 * Non-breaking spaces: an ordinary space collapses in an `<option>` (and
 * most other HTML text), so plain spaces would render flush regardless of
 * depth. */
export const INDENT = "    ";

/** `name` indented for `depth`, the way `flattenTree` rows are shown. */
export function indented(name: string, depth: number): string {
  return INDENT.repeat(depth) + name;
}

/** Rebuild the multi-level suite tree from parent links. The plan's root
 * suite is already stripped in Rust, so a null (or unknown) parent means
 * top level. Shared by the Suite Lookup browser and the Suite Management
 * picker so both draw the same tree. */
export function buildTree(suites: SuiteRef[]): SuiteNode[] {
  const nodes = new Map<number, SuiteNode>();
  for (const s of suites) nodes.set(s.id, { suite: s, children: [] });
  const roots: SuiteNode[] = [];
  for (const n of nodes.values()) {
    const pid = n.suite.parent_id;
    if (pid != null && nodes.has(pid)) nodes.get(pid)!.children.push(n);
    else roots.push(n);
  }
  return roots;
}

/** Depth-first rows for a dropdown: each suite with how deep it sits, so
 * an option can be indented to read like the tree it came from. */
export function flattenTree(nodes: SuiteNode[]): Array<{ suite: SuiteRef; depth: number }> {
  const out: Array<{ suite: SuiteRef; depth: number }> = [];
  const walk = (list: SuiteNode[], depth: number) => {
    for (const n of list) {
      out.push({ suite: n.suite, depth });
      walk(n.children, depth + 1);
    }
  };
  walk(nodes, 0);
  return out;
}
