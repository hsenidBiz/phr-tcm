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
  INDENT: number;
  DOT_R: number;
  ELBOW_R: number;
  buildGraph: (tree: unknown[]) => Graph;
  layoutTree: (graph: Graph) => { width: number; height: number };
  fold: (graph: Graph, area: Node, folded: boolean) => void;
  kidsOf: (area: Node) => Node[];
  elbowPath: (parent: Node, kid: Node) => string;
  trunkPath: (parent: Node) => string;
  linkPath: (parent: Node, kid: Node) => string;
  clampView: (
    tx: number,
    ty: number,
    contentW: number,
    contentH: number,
    w: number,
    h: number,
    pad: number,
  ) => { tx: number; ty: number };
  pathTo: (node: Node) => Node[];
  caseLabel: (node: Node) => string;
  caseTag: (node: Node) => string;
  areaLabel: (node: Node) => string;
  clipLabel: (text: string, room: number, measure: (s: string) => number) => string;
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

  test("a dot is the same size whatever the count: the count is written, not drawn", () => {
    // It was 7-16 by count; the row now says "Name · count" instead.
    const g = G.buildGraph([
      { name: "Huge", count: 1000, cases: [], children: [] },
      { name: "Tiny", count: 0, cases: [], children: [] },
    ]);
    expect(g.nodes.map((n) => n.r)).toEqual([G.DOT_R, G.DOT_R]);
    expect(G.DOT_R).toBe(4);
  });
});

describe("layoutTree", () => {
  test("the outline is 24px rows: an area, its cases, then its child areas, indented by depth", () => {
    const g = G.buildGraph(tree());
    const size = G.layoutTree(g);
    expect(G.ROW).toBe(24);
    expect(G.INDENT).toBe(24);
    // Document order: the area, its own cases, then its child areas.
    const byRow = [...visible(g)].sort((a, b) => a.y - b.y);
    expect(byRow.map(label)).toEqual([
      "Manage Events",
      "Page navigation",
      "Create",
      "Validation & limits",
      "Fill details",
      "Reports",
      "Export",
    ]);
    const [manage, nav, create, limits, fill, reports, exportCase] = byRow;
    // Rows one ROW apart inside a tree, the dot centred in its row.
    expect(nav.y - manage.y).toBe(G.ROW);
    expect(create.y - nav.y).toBe(G.ROW);
    expect(manage.y).toBe(G.ROW / 2);
    expect(fill.y - limits.y).toBe(G.ROW);
    // Depth sets the indent; a case is one level in from its area.
    expect([manage.x, nav.x, create.x, limits.x, fill.x]).toEqual([
      0,
      G.INDENT,
      G.INDENT,
      2 * G.INDENT,
      2 * G.INDENT,
    ]);
    expect([reports.x, exportCase.x]).toEqual([0, G.INDENT]);
    expect(size.width).toBe(2 * G.INDENT);
    // A small gap between the two top-level trees, on the 4px grid.
    expect(reports.y - fill.y).toBe(G.ROW + 8);
    expect(size.height).toBe(exportCase.y + G.ROW / 2);
    for (const n of g.nodes) expect(n.y % 4).toBe(0);
  });

  test("a folded area is one row and the layout closes the gap", () => {
    const g = G.buildGraph(tree());
    const manage = g.nodes[0];
    const before = G.layoutTree(g).height;
    G.fold(g, manage, true);
    const after = G.layoutTree(g).height;
    // Four rows left the outline: its case, Create, and Create's two cases.
    expect(after).toBe(before - 4 * G.ROW);
    expect(visible(g).map(label)).toEqual(["Manage Events", "Reports", "Export"]);
    const reports = g.nodes.find((n) => n.name === "Reports")!;
    expect(reports.y - manage.y).toBe(G.ROW + 8);
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

describe("guide lines", () => {
  test("the trunk runs from under the parent's dot to where the last kid's elbow begins", () => {
    const g = G.buildGraph(tree());
    G.layoutTree(g);
    const manage = g.nodes[0];
    const create = g.nodes.find((n) => n.name === "Create")!;
    expect(G.kidsOf(manage).map(label)).toEqual(["Page navigation", "Create"]);
    expect(G.trunkPath(manage)).toBe(`M0 ${manage.y + G.DOT_R + 4} L0 ${create.y - G.ELBOW_R}`);
    // A folded area has no trunk.
    G.fold(g, manage, true);
    expect(G.trunkPath(manage)).toBe("");
  });

  test("each guide turns with an 8px elbow and stops short of the dot it reaches", () => {
    const g = G.buildGraph(tree());
    G.layoutTree(g);
    const manage = g.nodes[0];
    const nav = g.nodes[1];
    // Down the trunk to 8px above the row, a quarter turn of radius 8 onto
    // the row, level to the dot's edge.
    expect(G.elbowPath(manage, nav)).toBe(
      `M0 ${nav.y - 8} Q0 ${nav.y} 8 ${nav.y} L${nav.x - G.DOT_R - 4} ${nav.y}`,
    );
    expect(G.ELBOW_R).toBe(8);
    // One move, one quarter turn, one level line: orthogonal, no diagonals.
    expect(G.elbowPath(manage, nav).match(/[MLQ]/g)!.join("")).toBe("MQL");
  });

  test("a link is the trunk down to one kid and its elbow, what a hover lights", () => {
    const g = G.buildGraph(tree());
    G.layoutTree(g);
    const manage = g.nodes[0];
    const create = g.nodes.find((n) => n.name === "Create")!;
    expect(G.linkPath(manage, create)).toBe(
      `M0 ${manage.y + G.DOT_R + 4} L0 ${create.y - 8} Q0 ${create.y} 8 ${create.y} L${create.x - G.DOT_R - 4} ${create.y}`,
    );
    const limits = g.nodes.find((n) => n.kind === "case" && n.data!.title.startsWith("Validation"))!;
    expect(G.linkPath(create, limits).startsWith(`M${create.x} `)).toBe(true);
  });
});

describe("clampView", () => {
  test("content bigger than the view can pan only until its far edge arrives, and never past the pad", () => {
    // 1000x2000 of content in a 400x300 view with a 16 pad.
    expect(G.clampView(100, 100, 1000, 2000, 400, 300, 16)).toEqual({ tx: 16, ty: 16 });
    expect(G.clampView(-9999, -9999, 1000, 2000, 400, 300, 16)).toEqual({
      tx: 400 - 1000 - 16,
      ty: 300 - 2000 - 16,
    });
    expect(G.clampView(-50, -60, 1000, 2000, 400, 300, 16)).toEqual({ tx: -50, ty: -60 });
  });

  test("content smaller than the view sits at the pad", () => {
    expect(G.clampView(-40, 90, 200, 100, 800, 600, 16)).toEqual({ tx: 16, ty: 16 });
  });
});

describe("rows", () => {
  test("an area row writes its name and its count, descendants included", () => {
    const g = G.buildGraph(tree());
    const [manage, create, reports] = g.nodes.filter((n) => n.kind === "area");
    expect(G.areaLabel(manage)).toBe("Manage Events · 3");
    expect(G.areaLabel(create)).toBe("Create · 2");
    expect(G.areaLabel(reports)).toBe("Reports · 1");
  });

  test("a case row is #id Title, or the title alone when it is new, with a NEW or UPDATE tag", () => {
    const long = "A very long title that keeps going well past seventy characters and is shown whole";
    const g = G.buildGraph([
      { name: "A", count: 2, cases: [mapCase(7, "Short"), mapCase(null, long)], children: [] },
    ]);
    const [, c1, c2] = g.nodes;
    expect(G.caseLabel(c1)).toBe("#7 Short");
    expect(G.caseTag(c1)).toBe("UPDATE");
    // The label is the whole title; the page cuts it to the room it has.
    expect(G.caseLabel(c2)).toBe(long);
    expect(G.caseTag(c2)).toBe("NEW");
  });

  test("a title is cut to the room with an ellipsis, or to nothing if not even a letter fits", () => {
    const measure = (s: string) => s.length * 7;
    expect(G.clipLabel("Reports", 100, measure)).toBe("Reports");
    const cut = G.clipLabel("Manage Events and Registrations", 70, measure);
    expect(cut).toBe("Manage Ev…");
    expect(measure(cut)).toBeLessThanOrEqual(70);
    expect(G.clipLabel("Reports", 10, measure)).toBe("");
    expect(G.clipLabel("Reports", 0, measure)).toBe("");
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
