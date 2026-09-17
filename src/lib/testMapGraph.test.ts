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
  vx: number;
  vy: number;
  r: number;
  hidden: boolean;
  pinned: boolean;
  name?: string;
  count?: number;
  folded?: boolean;
  children?: Node[];
  cases?: Node[];
  ci?: number;
  data?: { id: number | null; title: string };
};
type Edge = { a: Node; b: Node; len: number };
type Graph = { nodes: Node[]; edges: Edge[] };
type Helpers = {
  buildGraph: (tree: unknown[]) => Graph;
  step: (nodes: Node[], edges: Edge[], alpha: number) => void;
  fold: (graph: Graph, area: Node, folded: boolean) => void;
  labelAlpha: (kind: "case-id" | "case-title", scale: number, reduced?: boolean) => number;
  fitTransform: (nodes: Node[], w: number, h: number, pad: number) => { scale: number; tx: number; ty: number };
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
  // import.meta.url, not __dirname: this file is ESM under vitest.
  const here = dirname(fileURLToPath(import.meta.url));
  const src = readFileSync(resolve(here, "../../src-tauri/web/test-map-graph.js"), "utf8");
  new Function(src)();
  G = (window as unknown as { testMap: Helpers }).testMap;
});

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
    const areaEdges = g.edges.filter((e) => e.a.kind === "area" && e.b.kind === "area");
    expect(areaEdges).toHaveLength(1);
    expect(areaEdges[0].len).toBe(110);
    expect(g.edges.filter((e) => e.b.kind === "case").every((e) => e.len === 40)).toBe(true);
    g.nodes.forEach((n, i) => expect(n.i).toBe(i));
  });

  test("area radius grows with count and is clamped; the start is deterministic", () => {
    const g1 = G.buildGraph(tree());
    const g2 = G.buildGraph(tree());
    expect(g1.nodes.map((n) => [n.x, n.y])).toEqual(g2.nodes.map((n) => [n.x, n.y]));
    const big = G.buildGraph([{ name: "Huge", count: 1000, cases: [], children: [] }]);
    expect(big.nodes[0].r).toBe(28);
    const tiny = G.buildGraph([{ name: "Tiny", count: 0, cases: [], children: [] }]);
    expect(tiny.nodes[0].r).toBe(10);
    // Two top-level areas start apart, not on top of each other.
    const [a, , b] = g1.nodes.filter((n) => n.kind === "area");
    expect(Math.hypot(a.x - b.x, a.y - b.y)).toBeGreaterThan(100);
  });
});

describe("step", () => {
  const energy = (nodes: Node[]) => nodes.reduce((s, n) => s + n.vx * n.vx + n.vy * n.vy, 0);

  test("settles: kinetic energy falls as alpha cools and every position stays finite", () => {
    const g = G.buildGraph(tree());
    let alpha = 1;
    let early = 0;
    for (let k = 0; k < 200; k++) {
      G.step(g.nodes, g.edges, alpha);
      alpha *= 0.97;
      if (k === 10) early = energy(g.nodes);
    }
    expect(energy(g.nodes)).toBeLessThan(early);
    expect(g.nodes.every((n) => Number.isFinite(n.x) && Number.isFinite(n.y))).toBe(true);
  });

  test("a pinned node does not move; a hidden node is left out", () => {
    const g = G.buildGraph(tree());
    const pinned = g.nodes[0];
    pinned.pinned = true;
    const hidden = g.nodes[1];
    hidden.hidden = true;
    const before = [pinned.x, pinned.y, hidden.x, hidden.y];
    for (let k = 0; k < 20; k++) G.step(g.nodes, g.edges, 1);
    expect([pinned.x, pinned.y, hidden.x, hidden.y]).toEqual(before);
  });

  test("two coincident nodes are pushed apart rather than dividing by zero", () => {
    const g = G.buildGraph([{ name: "A", count: 2, cases: [mapCase(1, "x"), mapCase(2, "y")], children: [] }]);
    const [, c1, c2] = g.nodes;
    c2.x = c1.x;
    c2.y = c1.y;
    for (let k = 0; k < 5; k++) G.step(g.nodes, g.edges, 1);
    expect(Math.hypot(c1.x - c2.x, c1.y - c2.y)).toBeGreaterThan(0);
    expect(g.nodes.every((n) => Number.isFinite(n.x) && Number.isFinite(n.y))).toBe(true);
  });
});

describe("fold", () => {
  test("hides a subtree and its edges, and unfolding restores it", () => {
    const g = G.buildGraph(tree());
    const manage = g.nodes[0];
    G.fold(g, manage, true);
    expect(manage.folded).toBe(true);
    expect(manage.hidden).toBe(false);
    const visible = g.nodes.filter((n) => !n.hidden).map((n) => n.name ?? n.data!.title);
    expect(visible).toEqual(["Manage Events", "Reports", "Export"]);
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
    expect(G.labelAlpha("case-id", 0.4)).toBe(0);
    expect(G.labelAlpha("case-id", 0.5)).toBe(0);
    expect(G.labelAlpha("case-id", 0.6)).toBeCloseTo(0.5);
    expect(G.labelAlpha("case-id", 0.7)).toBe(1);
    expect(G.labelAlpha("case-title", 1.0)).toBe(0);
    expect(G.labelAlpha("case-title", 1.15)).toBeCloseTo(0.25);
    expect(G.labelAlpha("case-title", 1.3)).toBe(1);
  });

  test("reduced motion makes the ramp a step at the threshold", () => {
    expect(G.labelAlpha("case-id", 0.59, true)).toBe(0);
    expect(G.labelAlpha("case-id", 0.6, true)).toBe(1);
    expect(G.labelAlpha("case-title", 1.19, true)).toBe(0);
    expect(G.labelAlpha("case-title", 1.2, true)).toBe(1);
  });
});

describe("fitTransform", () => {
  test("keeps every visible node inside the viewport with the padding, capped at 1.5", () => {
    const g = G.buildGraph(tree());
    for (let k = 0; k < 100; k++) G.step(g.nodes, g.edges, 0.5);
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
    g.nodes.forEach((n, i) => (n.hidden = i !== 0));
    const t = G.fitTransform(g.nodes, 400, 300, 40);
    expect(t.scale).toBe(1.5);
    expect(g.nodes[0].x * t.scale + t.tx).toBeCloseTo(200);
    expect(g.nodes[0].y * t.scale + t.ty).toBeCloseTo(150);
  });

  test("clamps to the zoom floor instead of going negative on a 0x0 viewport", () => {
    const g = G.buildGraph(tree());
    const t = G.fitTransform(g.nodes, 0, 0, 40);
    expect(t.scale).toBe(0.2);
    expect(Number.isFinite(t.tx)).toBe(true);
    expect(Number.isFinite(t.ty)).toBe(true);
  });
});

describe("caseLabel", () => {
  test("shows the id or NEW, and the ellipsised title when asked", () => {
    const g = G.buildGraph([
      {
        name: "A",
        count: 2,
        cases: [mapCase(7, "Short"), mapCase(null, "A very long title that keeps going well past forty characters")],
        children: [],
      },
    ]);
    const [, c1, c2] = g.nodes;
    expect(G.caseLabel(c1, false)).toBe("#7");
    expect(G.caseLabel(c2, false)).toBe("NEW");
    expect(G.caseLabel(c1, true)).toBe("#7  Short");
    const long = G.caseLabel(c2, true);
    expect(long.startsWith("NEW  A very long title")).toBe(true);
    expect(long.endsWith("…")).toBe(true);
    expect(long.length).toBeLessThanOrEqual("NEW  ".length + 40);
  });
});
