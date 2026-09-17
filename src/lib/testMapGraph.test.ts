/**
 * The Test map's graph helpers live in a plain browser script embedded in
 * the generated page (src-tauri/web/test-map-graph.js). They have no DOM
 * dependency, so this loads the file as-is and exercises them here.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, test } from "vitest";

type Node = {
  kind: "area" | "case";
  i: number;
  x: number;
  y: number;
  r: number;
  hidden: boolean;
  name?: string;
  count?: number;
  folded?: boolean;
  parent?: Node | null;
  area?: Node;
  children?: Node[];
  cases?: Node[];
  ci?: number;
  data?: { id: number | null; title: string };
};
type Edge = { a: Node; b: Node };
type Graph = { nodes: Node[]; edges: Edge[] };
type Helpers = {
  ROW: number;
  LEVEL: number;
  buildGraph: (tree: unknown[]) => Graph;
  layoutTree: (graph: Graph) => { width: number; height: number };
  fold: (graph: Graph, area: Node, folded: boolean) => void;
  pathTo: (node: Node) => Node[];
  labelAlpha: (kind: "case-id" | "case-title", scale: number, reduced?: boolean) => number;
  fitTransform: (nodes: Node[], w: number, h: number, pad: number) => { scale: number; tx: number; ty: number };
  fitWidthTransform: (
    nodes: Node[],
    w: number,
    pad: number,
    labelSpace: number,
  ) => { scale: number; tx: number; ty: number };
  caseLabel: (node: Node, withTitle: boolean) => string;
};

let G: Helpers;

const mapCase = (id: number | null, title: string) => ({
  id,
  title,
  steps: [],
  preconditions: "",
  tags: "",
  automation_status: "",
});

const tree = () => [
  {
    name: "Manage Events",
    count: 3,
    cases: [mapCase(81310, "Page navigation")],
    children: [
      {
        name: "Create",
        count: 2,
        cases: [mapCase(81314, "Validation & limits"), mapCase(null, "Fill details")],
        children: [],
      },
    ],
  },
  { name: "Reports", count: 1, cases: [mapCase(90001, "Export")], children: [] },
];

beforeAll(() => {
  // import.meta.url, not `__dirname`: this file is ESM under vitest.
  const here = dirname(fileURLToPath(import.meta.url));
  const src = readFileSync(resolve(here, "../../src-tauri/web/test-map-graph.js"), "utf8");
  new Function(src)();
  G = (window as unknown as { testMap: Helpers }).testMap;
});

const visible = (g: Graph) => g.nodes.filter((n) => !n.hidden);
const label = (n: Node) => n.name ?? n.data!.title;
const isUnder = (n: Node, a: Node): boolean => {
  let p: Node | null | undefined = n.kind === "case" ? n.area : n.parent;
  while (p) {
    if (p === a) return true;
    p = p.parent;
  }
  return false;
};

describe("buildGraph", () => {
  test("one node per area and case, one edge per membership, cases numbered in walk order", () => {
    const g = G.buildGraph(tree());
    const areas = g.nodes.filter((n) => n.kind === "area");
    const cases = g.nodes.filter((n) => n.kind === "case");
    expect(areas.map((a) => a.name)).toEqual(["Manage Events", "Create", "Reports"]);
    expect(cases.map((c) => c.data!.title)).toEqual([
      "Page navigation",
      "Validation & limits",
      "Fill details",
      "Export",
    ]);
    expect(cases.map((c) => c.ci)).toEqual([0, 1, 2, 3]);
    // 4 case→area edges + 1 area→area edge (Create under Manage Events).
    expect(g.edges).toHaveLength(5);
    expect(g.edges.filter((e) => e.a.kind === "area" && e.b.kind === "area")).toHaveLength(1);
    g.nodes.forEach((n, i) => expect(n.i).toBe(i));
    expect(areas[1].parent).toBe(areas[0]);
    expect(areas[2].parent).toBeNull();
  });

  test("area radius grows with count and is clamped", () => {
    const big = G.buildGraph([{ name: "Huge", count: 1000, cases: [], children: [] }]);
    expect(big.nodes[0].r).toBe(16);
    const tiny = G.buildGraph([{ name: "Tiny", count: 0, cases: [], children: [] }]);
    expect(tiny.nodes[0].r).toBe(7);
    const mid = G.buildGraph([{ name: "Mid", count: 16, cases: [], children: [] }]);
    expect(mid.nodes[0].r).toBe(11);
  });
});

describe("layoutTree", () => {
  test("every visible case has its own row; depth sets the column", () => {
    const g = G.buildGraph(tree());
    G.layoutTree(g);
    const cases = g.nodes.filter((n) => n.kind === "case");
    const rows = cases.map((c) => c.y);
    expect(new Set(rows).size).toBe(rows.length);
    // File order top to bottom.
    expect(rows).toEqual([...rows].sort((a, b) => a - b));
    // Adjacent rows are one ROW apart within a tree.
    expect(rows[1] - rows[0]).toBe(G.ROW);
    expect(rows[2] - rows[1]).toBe(G.ROW);
    const [manage, create, reports] = g.nodes.filter((n) => n.kind === "area");
    expect(manage.x).toBe(0);
    expect(create.x).toBe(G.LEVEL);
    expect(reports.x).toBe(0);
    // A case sits one level right of its area.
    expect(cases[0].x).toBe(G.LEVEL);
    expect(cases[1].x).toBe(2 * G.LEVEL);
  });

  test("an area sits at the midpoint of the rows beneath it", () => {
    const g = G.buildGraph(tree());
    G.layoutTree(g);
    const [manage, create] = g.nodes.filter((n) => n.kind === "area");
    const under = (a: Node) => visible(g).filter((n) => n !== a && isUnder(n, a)).map((n) => n.y);
    const m = under(manage);
    expect(manage.y).toBeCloseTo((Math.min(...m) + Math.max(...m)) / 2);
    const c = under(create);
    expect(create.y).toBeCloseTo((Math.min(...c) + Math.max(...c)) / 2);
  });

  test("top-level areas stack as separate trees with a gap, and never overlap", () => {
    const g = G.buildGraph(tree());
    const size = G.layoutTree(g);
    const manage = g.nodes[0];
    const reports = g.nodes.find((n) => n.name === "Reports")!;
    const manageRows = visible(g).filter((n) => n === manage || isUnder(n, manage)).map((n) => n.y);
    expect(reports.y).toBeGreaterThan(Math.max(...manageRows) + G.ROW);
    const rows = visible(g).map((n) => n.y);
    expect(size.height).toBeGreaterThanOrEqual(Math.max(...rows));
    expect(size.width).toBeGreaterThanOrEqual(2 * G.LEVEL);
  });

  test("a folded area collapses to one row and the layout closes the gap", () => {
    const g = G.buildGraph(tree());
    const manage = g.nodes[0];
    const before = G.layoutTree(g).height;
    G.fold(g, manage, true);
    const after = G.layoutTree(g).height;
    expect(after).toBeLessThan(before);
    expect(visible(g).map(label)).toEqual(["Manage Events", "Reports", "Export"]);
    const reports = g.nodes.find((n) => n.name === "Reports")!;
    expect(reports.y - manage.y).toBeLessThan(3 * G.ROW + 40);
    G.fold(g, manage, false);
    expect(G.layoutTree(g).height).toBe(before);
  });

  test("layout is deterministic", () => {
    const a = G.buildGraph(tree());
    const b = G.buildGraph(tree());
    G.layoutTree(a);
    G.layoutTree(b);
    expect(a.nodes.map((n) => [n.x, n.y])).toEqual(b.nodes.map((n) => [n.x, n.y]));
  });
});

describe("fold", () => {
  test("hides a subtree and its edges, and unfolding restores it", () => {
    const g = G.buildGraph(tree());
    const manage = g.nodes[0];
    G.fold(g, manage, true);
    expect(manage.folded).toBe(true);
    expect(manage.hidden).toBe(false);
    expect(visible(g).map(label)).toEqual(["Manage Events", "Reports", "Export"]);
    G.fold(g, manage, false);
    expect(g.nodes.every((n) => !n.hidden)).toBe(true);
  });

  test("a folded child stays folded when its parent is unfolded", () => {
    const g = G.buildGraph(tree());
    const manage = g.nodes[0];
    const create = g.nodes.find((n) => n.name === "Create")!;
    G.fold(g, create, true);
    G.fold(g, manage, true);
    G.fold(g, manage, false);
    expect(create.hidden).toBe(false);
    expect(create.folded).toBe(true);
    expect(create.cases!.every((c) => c.hidden)).toBe(true);
  });
});

describe("labelAlpha", () => {
  test("is 0 below, ramps across ±0.1, and is 1 above each threshold", () => {
    expect(G.labelAlpha("case-id", 0.3)).toBe(0);
    expect(G.labelAlpha("case-id", 0.4)).toBe(0);
    expect(G.labelAlpha("case-id", 0.5)).toBeCloseTo(0.5);
    expect(G.labelAlpha("case-id", 0.6)).toBe(1);
    expect(G.labelAlpha("case-title", 0.6)).toBe(0);
    expect(G.labelAlpha("case-title", 0.75)).toBeCloseTo(0.25);
    expect(G.labelAlpha("case-title", 0.9)).toBe(1);
  });

  test("reduced motion makes the ramp a step at the threshold", () => {
    expect(G.labelAlpha("case-id", 0.49, true)).toBe(0);
    expect(G.labelAlpha("case-id", 0.5, true)).toBe(1);
    expect(G.labelAlpha("case-title", 0.79, true)).toBe(0);
    expect(G.labelAlpha("case-title", 0.8, true)).toBe(1);
  });
});

describe("fitTransform", () => {
  test("keeps every visible node inside the viewport with the padding, capped at 1.5", () => {
    const g = G.buildGraph(tree());
    G.layoutTree(g);
    const t = G.fitTransform(g.nodes, 800, 600, 40);
    expect(t.scale).toBeLessThanOrEqual(1.5);
    for (const n of g.nodes) {
      const sx = n.x * t.scale + t.tx;
      const sy = n.y * t.scale + t.ty;
      expect(sx - n.r * t.scale).toBeGreaterThanOrEqual(40 - 1e-6);
      expect(sx + n.r * t.scale).toBeLessThanOrEqual(800 - 40 + 1e-6);
      expect(sy - n.r * t.scale).toBeGreaterThanOrEqual(40 - 1e-6);
      expect(sy + n.r * t.scale).toBeLessThanOrEqual(600 - 40 + 1e-6);
    }
  });

  test("ignores hidden nodes and copes with a single node", () => {
    const g = G.buildGraph(tree());
    G.layoutTree(g);
    g.nodes.forEach((n, i) => (n.hidden = i !== 0));
    const t = G.fitTransform(g.nodes, 400, 300, 40);
    expect(t.scale).toBe(1.5);
    expect(g.nodes[0].x * t.scale + t.tx).toBeCloseTo(200);
    expect(g.nodes[0].y * t.scale + t.ty).toBeCloseTo(150);
  });

  test("a 0x0 viewport clamps to the zoom floor instead of going negative", () => {
    const g = G.buildGraph(tree());
    G.layoutTree(g);
    const t = G.fitTransform(g.nodes, 0, 0, 40);
    expect(t.scale).toBe(0.2);
    expect(Number.isFinite(t.tx)).toBe(true);
    expect(Number.isFinite(t.ty)).toBe(true);
  });
});

describe("fitWidthTransform", () => {
  test("fits the tree's width plus label room, never above 100%, top-left with the padding", () => {
    const g = G.buildGraph(tree());
    G.layoutTree(g);
    const t = G.fitWidthTransform(g.nodes, 2000, 40, 300);
    expect(t.scale).toBe(1);
    const minX = Math.min(...g.nodes.map((n) => n.x - n.r));
    const minY = Math.min(...g.nodes.map((n) => n.y - n.r));
    expect(minX * t.scale + t.tx).toBeCloseTo(40);
    expect(minY * t.scale + t.ty).toBeCloseTo(40);
  });

  test("shrinks a wide tree to the viewport width, down to the zoom floor", () => {
    const g = G.buildGraph(tree());
    G.layoutTree(g);
    const maxX = Math.max(...g.nodes.map((n) => n.x + n.r));
    const minX = Math.min(...g.nodes.map((n) => n.x - n.r));
    const t = G.fitWidthTransform(g.nodes, 400, 40, 300);
    expect(t.scale).toBeCloseTo(Math.max(0.2, (400 - 80) / (maxX - minX + 300)));
    expect(G.fitWidthTransform(g.nodes, 0, 40, 300).scale).toBe(0.2);
  });
});

describe("caseLabel", () => {
  test("shows the id, the title alone for a new case, and never cuts a long title", () => {
    const long = "A very long title that keeps going well past seventy characters and is shown whole";
    const g = G.buildGraph([
      { name: "A", count: 2, cases: [mapCase(7, "Short"), mapCase(null, long)], children: [] },
    ]);
    const [, c1, c2] = g.nodes;
    expect(G.caseLabel(c1, false)).toBe("#7");
    expect(G.caseLabel(c1, true)).toBe("#7  Short");
    // A new case has no id: nothing at the id-only level, no "NEW" prefix.
    expect(G.caseLabel(c2, false)).toBe("");
    expect(G.caseLabel(c2, true)).toBe(long);
  });
});

describe("pathTo", () => {
  test("is the chain from the top-level area down to the node, inclusive", () => {
    const g = G.buildGraph(tree());
    const [manage, create, reports] = g.nodes.filter((n) => n.kind === "area");
    const cases = g.nodes.filter((n) => n.kind === "case");
    expect(G.pathTo(cases[2]).map(label)).toEqual(["Manage Events", "Create", "Fill details"]);
    expect(G.pathTo(cases[0]).map(label)).toEqual(["Manage Events", "Page navigation"]);
    expect(G.pathTo(create).map(label)).toEqual(["Manage Events", "Create"]);
    expect(G.pathTo(manage)).toEqual([manage]);
    expect(G.pathTo(cases[3]).map(label)).toEqual(["Reports", "Export"]);
    expect(G.pathTo(reports)).toEqual([reports]);
  });
});
