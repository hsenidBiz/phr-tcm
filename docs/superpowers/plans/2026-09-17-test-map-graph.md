# Test Map Graph Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redraw the Test map page as an Obsidian-style force-directed graph with zoom-dependent labels, and let `transform_cases` set a case's `area`.

**Architecture:** The webview keeps sending the same `MapNode` tree; Rust keeps writing one self-contained HTML file. Inside the page a pure helper script (`test-map-graph.js`: build graph, simulate, fold, label alpha, fit) is driven by a page script (`test-map.js`: canvas drawing, level of detail, hover/click/drag/zoom/pan, side panel) - the helper file has no DOM dependency so vitest can load it. Rust also writes a visually hidden list of the cases so screen readers and the Rust tests see them. The transform op is one more `String` op following `set_preconditions` exactly, plus one more filter key following `module_is`.

**Tech Stack:** Rust (tauri 2 crate `v2_lib`, tests under `src-tauri/tests/`), plain ES5 browser JS embedded with `include_str!`, Canvas 2D, vitest (jsdom) for the helper file.

**Spec:** `docs/superpowers/specs/2026-09-17-test-map-graph-design.md`

## Global Constraints

- Every Rust test is an integration test under `src-tauri/tests/` - never a `#[cfg(test)]` module in `src/`.
- No DELETE calls to Azure DevOps; nothing here talks to Azure DevOps at all.
- `src/bindings.ts` is generated - never hand-edit. This plan changes no command signature, so it must not change; if `cargo test` leaves it with a line-ending-only diff, `git checkout -- src/bindings.ts`.
- Never weaken `src/ui-consistency.test.ts`.
- Colours in the page come from the palette variables only (`--bg --surface --surface-2 --text --muted --faint --border --accent --success --danger --warning`); never a literal colour.
- The page must work as a `file://` in the temp directory with nothing else present: no network, no dependency.
- The page's JS is ES5 (`var`, `function`) like the other `src-tauri/web/*.js` files.
- Do not run `npx prettier`. One build or test command at a time on this machine.
- Commit with a Bash heredoc `git commit -F - <<'EOF' … EOF`, and end every message with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Exact constants (spec): area radius `clamp(10, 6 + 3*sqrt(count), 28)`, case radius 4; spring rest 40 (case→area) / 110 (area→area), strength 0.08; repulsion `900 / d²`, grid cell 120, skip pairs more than two cells apart; centre pull 0.01; damping 0.85; alpha decay 0.97, stop below 0.005; start rings 220 (top) / 90 (child), case jitter ±30; re-warm 0.5; LOD thresholds 0.6 and 1.2 with ±0.1 ramps; zoom range 0.2–4; fit padding 40, fit cap 1.5.

---

## File map

| File | Responsibility |
|---|---|
| `src-tauri/src/import_parser/mod.rs` | + `normalise_area(&str) -> String` next to `AREA_KEYS` |
| `src-tauri/src/transform.rs` | + `Op::SetArea`, `Filter.area_is`, parse/apply/describe/known_keys, `SUPPORTED_OPS` 25 |
| `src-tauri/src/mcp.rs` | tool description mentions `set_area` and `area_is` |
| `src-tauri/src/ai_bridge.rs` | the guide's `## area` section names the op |
| `src-tauri/tests/transform.rs` | set_area / area_is tests; `SUPPORTED_OPS` count 25 |
| `src-tauri/web/test-map-graph.js` | NEW - pure helpers on `window.testMap` |
| `src/lib/testMapGraph.test.ts` | NEW - vitest for the helper file |
| `src-tauri/src/test_map.rs` | shell: `<canvas id='graph'>`, hidden `#map-list`, embeds both scripts |
| `src-tauri/tests/test_map.rs` | shell assertions updated |
| `src-tauri/web/test-map.js` | REWRITE - canvas page script |
| `src-tauri/web/test-map.css` | REWRITE - graph page styles |

---

### Task 1: `set_area` op and `area_is` filter in `transform_cases`

**Files:**
- Modify: `src-tauri/src/import_parser/mod.rs` (after line 42, `AREA_KEYS`)
- Modify: `src-tauri/src/transform.rs` (`FILTER_KEYS` line 19; `Op` enum ~line 38; `SUPPORTED_OPS` line 92; `Filter` struct ~line 120; `Filter::matches` ~line 168; `parse_filter` ~lines 287-303; op parse ~line 367; `remove_cases` has_filter ~line 463; apply ~line 888; describe ~line 1130; `known_keys` ~line 1164)
- Modify: `src-tauri/src/mcp.rs` lines 106-125 (the `transform_ops_desc` string)
- Modify: `src-tauri/src/ai_bridge.rs` line 1484 (the `## area` paragraph)
- Test: `src-tauri/tests/transform.rs`

**Interfaces:**
- Consumes: `required_str(v, key, label)` (returns the string, errors when the key is absent - `""` is accepted and means clear); `TestCase.area: String`.
- Produces: `v2_lib::import_parser::normalise_area(raw: &str) -> String`; `Op::SetArea(String)`; `Filter { area_is: Option<String>, .. }`.

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/tests/transform.rs` (after `set_reviewer_notes_overwrites_and_requires_a_value`):

```rust
fn in_area(title: &str, area: &str) -> TestCase {
    TestCase { area: area.into(), ..case(title, vec![step("s")]) }
}

// ---- set_area / where.area_is -------------------------------------------

#[test]
fn set_area_overwrites_normalises_and_clears() {
    let cases = vec![in_area("A", "Old"), in_area("B", "")];
    let ops = parse_ops(&serde_json::json!([
        { "op": "set_area", "value": "Manage Events/Create /  / Validation " },
    ]))
    .unwrap();
    let (out, report) = apply(cases, &ops);
    assert_eq!(out[0].area, "Manage Events / Create / Validation");
    assert_eq!(out[1].area, "Manage Events / Create / Validation");
    assert!(report.applied[0].starts_with("Set area to 'Manage Events / Create / Validation'"), "{:?}", report.applied);

    let ops = parse_ops(&serde_json::json!([{ "op": "set_area", "value": "" }])).unwrap();
    let (out, report) = apply(out, &ops);
    assert_eq!(out[0].area, "");
    assert!(report.applied[0].starts_with("Cleared area"), "{:?}", report.applied);

    let err = parse_ops(&serde_json::json!([{ "op": "set_area" }])).unwrap_err();
    assert!(err.contains("value"), "{err}");
}

#[test]
fn area_is_selects_one_area_case_insensitively_by_normalised_path() {
    let cases = vec![
        in_area("A", "Manage Events / Create"),
        in_area("B", "manage events/create"),
        in_area("C", "Manage Events / Edit"),
        in_area("D", ""),
    ];
    let ops = parse_ops(&serde_json::json!([
        { "op": "set_area", "value": "Events / Create", "where": { "area_is": "MANAGE EVENTS /CREATE" } },
    ]))
    .unwrap();
    let (out, report) = apply(cases, &ops);
    assert_eq!(out[0].area, "Events / Create");
    assert_eq!(out[1].area, "Events / Create");
    assert_eq!(out[2].area, "Manage Events / Edit", "another area is left alone");
    assert_eq!(out[3].area, "", "a case with no area is not in that area");
    assert!(report.applied[0].contains("modified 2 case(s)"), "{:?}", report.applied);

    // An empty area_is is the way to address the cases that have none.
    let ops = parse_ops(&serde_json::json!([
        { "op": "set_area", "value": "Unsorted", "where": { "area_is": "" } },
    ]))
    .unwrap();
    let (out, _) = apply(out, &ops);
    assert_eq!(out[3].area, "Unsorted");
    assert_eq!(out[0].area, "Events / Create");

    // remove_cases accepts area_is as its required filter.
    let ops = parse_ops(&serde_json::json!([
        { "op": "remove_cases", "where": { "area_is": "Manage Events / Edit" } },
    ]))
    .unwrap();
    let (out, _) = apply(out, &ops);
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|c| c.title != "C"));
}

#[test]
fn set_area_echoes_keys_it_does_not_read() {
    let (_, ignored) = parse_ops_full(&serde_json::json!([
        { "op": "set_area", "value": "X", "find": "y" },
    ]))
    .unwrap();
    assert!(ignored.iter().any(|s| s.contains("find")), "{ignored:?}");
}

#[test]
fn normalise_area_trims_segments_and_drops_empty_ones() {
    use v2_lib::import_parser::normalise_area;
    assert_eq!(normalise_area(" Manage Events/Create "), "Manage Events / Create");
    assert_eq!(normalise_area("A //  / B"), "A / B");
    assert_eq!(normalise_area("  "), "");
    assert_eq!(normalise_area("/"), "");
}
```

(`parse_ops_full` returns `(Vec<Operation>, Vec<String>)`; the second value is the ignored list.)

Then change the count in `supported_ops_is_the_parsers_whole_vocabulary`:

```rust
    assert_eq!(SUPPORTED_OPS.len(), 25);
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `src-tauri`): `cargo test --test transform`
Expected: compile error - `normalise_area` not found, and `set_area` unknown once that is stubbed.

- [ ] **Step 3: Add `normalise_area`**

In `src-tauri/src/import_parser/mod.rs`, directly after `pub(crate) const AREA_KEYS ...`:

```rust
/// "Manage Events/Create " -> "Manage Events / Create": segments trimmed,
/// empty ones dropped, one spelling of the separator. The Test map splits
/// on the same rule (`splitArea` in `src/lib/testMap.ts`), so two spellings
/// of one path land on one node.
pub fn normalise_area(raw: &str) -> String {
    raw.split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}
```

- [ ] **Step 4: Add the op and the filter**

In `src-tauri/src/transform.rs`:

`FILTER_KEYS` (line 19):
```rust
const FILTER_KEYS: [&str; 5] = ["title_contains", "has_tag", "module_is", "area_is", "at_index"];
```

`Op` enum, after `SetPreconditions(String),`:
```rust
    /// Overwrite the app-only `area` path (never sent to Azure DevOps);
    /// normalised, so "a/b" and "a / b" write the same thing. Empty clears.
    SetArea(String),
```

`SUPPORTED_OPS`: change `[&str; 24]` to `[&str; 25]` and insert `"set_area",` after `"set_preconditions",`.

`Filter` struct, after `pub module_is: Option<String>,`:
```rust
    /// Case-insensitive match of the normalised area path. `Some("")`
    /// selects the cases that have no area.
    pub area_is: Option<String>,
```

`Filter::matches`, after the `module_is` block:
```rust
        if let Some(a) = &self.area_is {
            let want = crate::import_parser::normalise_area(a).to_lowercase();
            if crate::import_parser::normalise_area(&c.area).to_lowercase() != want {
                return false;
            }
        }
```

`parse_filter`: the string-typed key loop becomes
```rust
    for key in ["title_contains", "has_tag", "module_is", "area_is"] {
```
and the `Ok(Filter { ... })` gains
```rust
        area_is: f["area_is"].as_str().map(str::to_string),
```

Op parse, after the `"set_preconditions"` arm:
```rust
            "set_area" => Op::SetArea(crate::import_parser::normalise_area(&required_str(v, "value", &label)?)),
```

`remove_cases` `has_filter`:
```rust
                let has_filter = f["title_contains"].as_str().is_some()
                    || f["has_tag"].as_str().is_some()
                    || f["module_is"].as_str().is_some()
                    || f["area_is"].as_str().is_some()
                    || f["at_index"].as_u64().is_some();
```

Apply, after `Op::SetPreconditions(v) => c.preconditions = v.clone(),`:
```rust
                        Op::SetArea(v) => c.area = v.clone(),
```

Describe, after the `SetPreconditions` line:
```rust
        Op::SetArea(v) if v.is_empty() => "Cleared area".to_string(),
        Op::SetArea(v) => format!("Set area to '{v}'"),
```

`known_keys`: add `| "set_area"` to the first arm's pattern (the `{value}` group):
```rust
        "set_tags" | "add_tags" | "remove_tags" | "set_module" | "set_automation_status"
        | "set_preconditions" | "set_area" | "set_reviewer_notes" | "set_findings" | "prefix_title" | "suffix_title"
        | "sort_by" | "group_by" => &["op", "where", "value"],
```

If the compiler reports any other `match op {` over `Op` that is now non-exhaustive (search `grep -n "Op::SetPreconditions" src-tauri/src/transform.rs`), add an `Op::SetArea` arm mirroring the `SetPreconditions` one there.

- [ ] **Step 5: Document the op where assistants read**

`src-tauri/src/mcp.rs`, in `transform_ops_desc`: change
`set_tags/add_tags/remove_tags/set_module/set_automation_status/set_preconditions/\`
to
`set_tags/add_tags/remove_tags/set_module/set_automation_status/set_preconditions/set_area/\`
and change
`` `where` with title_contains/has_tag/module_is/at_index - at_index (zero-based position \``
to
`` `where` with title_contains/has_tag/module_is/area_is/at_index - area_is matches the normalised \``
`` area path case-insensitively (\"\" selects cases with no area); at_index (zero-based position \``

`src-tauri/src/ai_bridge.rs` line 1484: after `This is the app's own grouping path, not the work item's Area Path.` insert ` Set or move it in bulk with `transform_cases` (`set_area`, and `where.area_is` to pick an area).` keeping the line inside the same string literal (it ends with `Never sent to Azure DevOps.\n\n\`).

- [ ] **Step 6: Run the Rust suites that touch this**

Run: `cargo test --test transform` then `cargo test --test tcm_mcp` then `cargo test --test ai_bridge`
Expected: all pass (the tcm_mcp test asserts every `SUPPORTED_OPS` name is in the description - `set_area` now is).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/import_parser/mod.rs src-tauri/src/transform.rs src-tauri/src/mcp.rs src-tauri/src/ai_bridge.rs src-tauri/tests/transform.rs
git commit -F - <<'EOF'
feat(v2): transform_cases sets an area path with set_area and selects one with where.area_is

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 2: Graph helpers (`test-map-graph.js`) with vitest

**Files:**
- Create: `src-tauri/web/test-map-graph.js`
- Test: `src/lib/testMapGraph.test.ts`

**Interfaces:**
- Consumes: the `MapNode` JSON shape `{name, count, cases: [{id, title, steps, preconditions, tags, automation_status}], children}`.
- Produces (on `window.testMap`):
  - `buildGraph(tree) -> {nodes, edges}`. Node fields: `kind: 'area'|'case'`, `i` (index in `nodes`), `x, y, vx, vy`, `r`, `hidden`, `pinned`; area nodes also `name, count, parent, children[], cases[], folded`; case nodes also `data` (the MapCase), `area`, `ci` (0-based position among ALL case nodes in build order - the `data-i` the hidden list uses). Edge: `{a, b, len}`.
  - `step(nodes, edges, alpha)`.
  - `fold(graph, areaNode, folded)`.
  - `labelAlpha(kind, scale, reduced)`; kind is `'case-id'` (threshold 0.6) or `'case-title'` (threshold 1.2); returns 0..1.
  - `fitTransform(nodes, w, h, pad) -> {scale, tx, ty}`.
  - `caseLabel(node, withTitle)` -> `'#123'` / `'NEW'`, or `'#123  Title…'` ellipsised at 40 chars.

- [ ] **Step 1: Write the failing test**

Create `src/lib/testMapGraph.test.ts`:

```ts
/**
 * The Test map's graph helpers live in a plain browser script embedded in
 * the generated page (src-tauri/web/test-map-graph.js). They have no DOM
 * dependency, so this loads the file as-is and exercises them here.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
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
  const src = readFileSync(resolve(__dirname, "../../src-tauri/web/test-map-graph.js"), "utf8");
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npx vitest run src/lib/testMapGraph.test.ts`
Expected: FAIL - `ENOENT ... test-map-graph.js`.

- [ ] **Step 3: Write the helper file**

Create `src-tauri/web/test-map-graph.js`:

```js
/* The Test map's graph: build it from the area tree, lay it out with a
   force simulation, fold a subtree, decide label opacity from the zoom,
   fit it to a viewport. No DOM in here - the page script (test-map.js)
   draws, and the vitest file under src/lib loads this file as-is. Plain
   ES5 script: the page is a file in the temp directory with nothing else
   present. */
(function (root) {
  var CASE_R = 4;
  var SPRING = 0.08, CASE_LEN = 40, AREA_LEN = 110;
  var REPEL = 900, CELL = 120;
  var CENTRE = 0.01, DAMP = 0.85;
  var RING_TOP = 220, RING_CHILD = 90, JITTER = 30;
  var ID_AT = 0.6, TITLE_AT = 1.2, RAMP = 0.1;
  var TITLE_MAX = 40;

  function areaRadius(count) {
    return Math.max(10, Math.min(28, 6 + 3 * Math.sqrt(count)));
  }

  // A seeded 0..1 hash, so the same tree always starts in the same place.
  function hash(n) {
    var x = Math.sin(n * 12.9898 + 78.233) * 43758.5453;
    return x - Math.floor(x);
  }

  function buildGraph(tree) {
    var nodes = [], edges = [], caseCount = 0;
    function add(node) { node.i = nodes.length; nodes.push(node); return node; }
    function addArea(n, parent, index, siblings) {
      var angle = (index / Math.max(1, siblings)) * Math.PI * 2;
      var ring = parent ? RING_CHILD : RING_TOP;
      var cx = parent ? parent.x : 0, cy = parent ? parent.y : 0;
      var area = add({
        kind: 'area', name: n.name, count: n.count, r: areaRadius(n.count),
        x: cx + Math.cos(angle) * ring, y: cy + Math.sin(angle) * ring, vx: 0, vy: 0,
        parent: parent, children: [], cases: [], folded: false, hidden: false, pinned: false
      });
      if (parent) {
        parent.children.push(area);
        edges.push({ a: parent, b: area, len: AREA_LEN });
      }
      (n.cases || []).forEach(function (c) {
        var seed = area.i * 31 + caseCount;
        var cn = add({
          kind: 'case', data: c, ci: caseCount++, r: CASE_R, area: area,
          x: area.x + (hash(seed) - 0.5) * 2 * JITTER,
          y: area.y + (hash(seed + 0.5) - 0.5) * 2 * JITTER,
          vx: 0, vy: 0, hidden: false, pinned: false
        });
        area.cases.push(cn);
        edges.push({ a: area, b: cn, len: CASE_LEN });
      });
      (n.children || []).forEach(function (ch, i, all) { addArea(ch, area, i, all.length); });
    }
    (tree || []).forEach(function (n, i, all) { addArea(n, null, i, all.length); });
    return { nodes: nodes, edges: edges };
  }

  function step(nodes, edges, alpha) {
    var live = [], i, j, n, m;
    for (i = 0; i < nodes.length; i++) if (!nodes[i].hidden) live.push(nodes[i]);

    // Repulsion, bucketed: only nodes within two cells push each other.
    var grid = {};
    for (i = 0; i < live.length; i++) {
      n = live[i];
      var key = Math.floor(n.x / CELL) + ',' + Math.floor(n.y / CELL);
      (grid[key] || (grid[key] = [])).push(n);
    }
    for (i = 0; i < live.length; i++) {
      n = live[i];
      var gx = Math.floor(n.x / CELL), gy = Math.floor(n.y / CELL);
      for (var dx = -2; dx <= 2; dx++) {
        for (var dy = -2; dy <= 2; dy++) {
          var cell = grid[(gx + dx) + ',' + (gy + dy)];
          if (!cell) continue;
          for (j = 0; j < cell.length; j++) {
            m = cell[j];
            if (m === n) continue;
            var ex = n.x - m.x, ey = n.y - m.y;
            var d2 = ex * ex + ey * ey;
            if (d2 < 1) {
              // Coincident: nudge along a seeded direction, never divide by zero.
              var a = hash(n.i * 7 + m.i) * Math.PI * 2;
              ex = Math.cos(a); ey = Math.sin(a); d2 = 1;
            }
            var d = Math.sqrt(d2);
            var f = (REPEL / d2) * alpha;
            n.vx += (ex / d) * f;
            n.vy += (ey / d) * f;
          }
        }
      }
    }

    // Springs along the edges.
    for (i = 0; i < edges.length; i++) {
      var e = edges[i];
      if (e.a.hidden || e.b.hidden) continue;
      var sx = e.b.x - e.a.x, sy = e.b.y - e.a.y;
      var len = Math.sqrt(sx * sx + sy * sy) || 1;
      var pull = (len - e.len) * SPRING * alpha;
      var fx = (sx / len) * pull, fy = (sy / len) * pull;
      e.a.vx += fx; e.a.vy += fy;
      e.b.vx -= fx; e.b.vy -= fy;
    }

    // Centre pull, damping, move.
    for (i = 0; i < live.length; i++) {
      n = live[i];
      if (n.pinned) { n.vx = 0; n.vy = 0; continue; }
      n.vx -= n.x * CENTRE * alpha;
      n.vy -= n.y * CENTRE * alpha;
      n.vx *= DAMP;
      n.vy *= DAMP;
      n.x += n.vx;
      n.y += n.vy;
    }
  }

  // Hidden follows the ancestors: a node is hidden when any area above it
  // is folded. Recomputed from the top of the changed subtree, so a
  // folded child stays folded when its parent opens again.
  function refresh(area) {
    var off = area.hidden || area.folded;
    var i;
    for (i = 0; i < area.cases.length; i++) area.cases[i].hidden = off;
    for (i = 0; i < area.children.length; i++) {
      area.children[i].hidden = off;
      refresh(area.children[i]);
    }
  }

  function fold(graph, area, folded) {
    area.folded = !!folded;
    refresh(area);
  }

  function labelAlpha(kind, scale, reduced) {
    var at = kind === 'case-title' ? TITLE_AT : ID_AT;
    if (reduced) return scale >= at ? 1 : 0;
    var t = (scale - (at - RAMP)) / (2 * RAMP);
    return Math.max(0, Math.min(1, t));
  }

  function fitTransform(nodes, w, h, pad) {
    var minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity, any = false;
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      if (n.hidden) continue;
      any = true;
      minX = Math.min(minX, n.x - n.r); maxX = Math.max(maxX, n.x + n.r);
      minY = Math.min(minY, n.y - n.r); maxY = Math.max(maxY, n.y + n.r);
    }
    if (!any) return { scale: 1, tx: w / 2, ty: h / 2 };
    var bw = Math.max(1, maxX - minX), bh = Math.max(1, maxY - minY);
    var scale = Math.min(1.5, (w - 2 * pad) / bw, (h - 2 * pad) / bh);
    return {
      scale: scale,
      tx: (w - bw * scale) / 2 - minX * scale,
      ty: (h - bh * scale) / 2 - minY * scale
    };
  }

  function caseLabel(node, withTitle) {
    var id = node.data.id != null ? '#' + node.data.id : 'NEW';
    if (!withTitle) return id;
    var title = node.data.title || '';
    if (title.length > TITLE_MAX) title = title.slice(0, TITLE_MAX - 1) + '…';
    return id + '  ' + title;
  }

  root.testMap = {
    buildGraph: buildGraph,
    step: step,
    fold: fold,
    labelAlpha: labelAlpha,
    fitTransform: fitTransform,
    caseLabel: caseLabel
  };
})(typeof window !== 'undefined' ? window : this);
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npx vitest run src/lib/testMapGraph.test.ts`
Expected: PASS, all tests. If `fitTransform`'s single-node case fails on the centre assertion, check the arithmetic: with one node of radius r, `bw = 2r`, `scale = 1.5`, `tx = (w - 2r*1.5)/2 - (x - r)*1.5`, so `x*1.5 + tx = w/2`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/web/test-map-graph.js src/lib/testMapGraph.test.ts
git commit -F - <<'EOF'
feat(v2): Test map graph helpers - build, simulate, fold, label alpha, fit

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 3: Page shell - canvas, hidden case list, both scripts

**Files:**
- Modify: `src-tauri/src/test_map.rs` (the constants and `export_test_map_html`)
- Test: `src-tauri/tests/test_map.rs`

**Interfaces:**
- Consumes: `MAP_JS`, plus the new `src-tauri/web/test-map-graph.js` from Task 2.
- Produces: the page markup Task 4's script drives - `<canvas id='graph' role='img' aria-label='Test map: A areas, C test cases'>` inside `#viewport`; `<div id='map-list' class='sr-only'>` holding, per area in depth-first order, `<h3>` then `<ol>` of `<li><button type='button' class='case' data-i='N'>#id title</button></li>` (cases before children, `N` counting every case in that walk from 0); `<p id='map-empty' class='empty'>` when there are no cases.

- [ ] **Step 1: Update the Rust test**

In `src-tauri/tests/test_map.rs`, in `the_page_carries_the_tree_as_json_and_the_script_that_draws_it`, replace the chrome block

```rust
    for id in ["map-expand", "map-collapse", "map-in", "map-out", "map-reset", "map-zoom", "viewport", "canvas", "detail"] {
        assert!(html.contains(&format!("id='{id}'")), "missing #{id}: {html}");
    }
    assert!(html.contains("getElementById('map-data')"), "the script reads the data block");
```

with

```rust
    for id in ["map-expand", "map-collapse", "map-in", "map-out", "map-reset", "map-zoom", "viewport", "graph", "detail", "map-list"] {
        assert!(html.contains(&format!("id='{id}'")), "missing #{id}: {html}");
    }
    assert!(html.contains("getElementById('map-data')"), "the script reads the data block");
    assert!(html.contains("root.testMap = {"), "the graph helpers are embedded");
    assert!(
        html.find("root.testMap = {").unwrap() < html.find("getElementById('map-data')").unwrap(),
        "the helpers load before the page script uses them"
    );
    // The canvas says what it is; the hidden list carries every case for
    // screen readers and the keyboard, numbered the way the script numbers
    // its case nodes (an area's cases before its children).
    assert!(html.contains("<canvas id='graph' role='img' aria-label='Test map: 2 areas, 3 test cases'>"), "{html}");
    let list = html
        .split("<div id='map-list' class='sr-only'>")
        .nth(1)
        .and_then(|rest| rest.split("<aside id='detail'").next())
        .expect("the hidden list");
    assert!(list.contains("<h3>Manage Events (3)</h3>"), "{list}");
    assert!(list.contains("<h3>Create (2)</h3>"), "{list}");
    assert!(list.contains("<button type='button' class='case' data-i='0'>#81310 Page navigation</button>"), "{list}");
    assert!(list.contains("<button type='button' class='case' data-i='1'>#81314 Validation &amp; limits</button>"), "{list}");
    assert!(list.contains("<button type='button' class='case' data-i='2'>NEW Fill details&lt;/script&gt;&lt;b&gt;x&lt;/b&gt;</button>"), "{list}");
    assert!(!list.contains("<b>x</b>"), "titles are escaped in the list: {list}");
```

And add a test:

```rust
#[test]
fn an_empty_map_says_so_and_still_has_the_chrome() {
    let path = tmp_path("map-empty.html");
    export_test_map_html(&[], &path, "", &PagePalette::default()).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("<p id='map-empty' class='empty'>No test cases to map.</p>"), "{html}");
    assert!(html.contains("aria-label='Test map: 0 areas, 0 test cases'"), "{html}");
    assert!(html.contains("<div id='map-list' class='sr-only'></div>"), "{html}");
}
```

(`esc` - `crate::import_parser::esc`, already imported in `test_map.rs` - escapes `&`, `<`, `>` and `"`.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test test_map`
Expected: FAIL on `missing #graph`.

- [ ] **Step 3: Write the shell**

In `src-tauri/src/test_map.rs`:

Add a constant after `MAP_JS`:
```rust
const GRAPH_JS: &str = include_str!("../web/test-map-graph.js");
```

Add two helpers above `export_test_map_html`:
```rust
/// Areas and cases in the whole tree, for the canvas's accessible name.
fn totals(nodes: &[MapNode]) -> (usize, usize) {
    nodes.iter().fold((0, 0), |(a, c), n| {
        let (ca, cc) = totals(&n.children);
        (a + 1 + ca, c + n.cases.len() + cc)
    })
}

/// The hidden list: every area as a heading, every case as a button. The
/// button's `data-i` is the case's position in this walk - cases before
/// children - which is the order `buildGraph` numbers its case nodes, so
/// the script finds a button's node by index.
fn list_html(nodes: &[MapNode], next: &mut usize, out: &mut String) {
    for n in nodes {
        out.push_str(&format!("<h3>{} ({})</h3>", esc(&n.name), n.count));
        if !n.cases.is_empty() {
            out.push_str("<ol>");
            for c in &n.cases {
                let id = c.id.map(|i| format!("#{i}")).unwrap_or_else(|| "NEW".into());
                out.push_str(&format!(
                    "<li><button type='button' class='case' data-i='{}'>{id} {}</button></li>",
                    *next,
                    esc(&c.title)
                ));
                *next += 1;
            }
            out.push_str("</ol>");
        }
        list_html(&n.children, next, out);
    }
}
```

In `export_test_map_html`, before `let html = format!(`:
```rust
    let (areas, cases) = totals(nodes);
    let mut list = String::new();
    list_html(nodes, &mut 0, &mut list);
    let empty = if cases == 0 { "<p id='map-empty' class='empty'>No test cases to map.</p>" } else { "" };
```

Replace the `.layout` block in the format string
```
         <div class='layout'>\
         <div id='viewport' class='viewport'><div id='canvas' class='canvas'></div></div>\
         <aside id='detail' class='detail' aria-label='Test case' hidden></aside>\
         </div>\
         <script type='application/json' id='map-data'>{data}</script>\
         <script>{js}</script>\
```
with
```
         <div class='layout'>\
         <div id='viewport' class='viewport'>\
         <canvas id='graph' role='img' aria-label='Test map: {areas} areas, {cases} test cases'></canvas>\
         {empty}</div>\
         <div id='map-list' class='sr-only'>{list}</div>\
         <aside id='detail' class='detail' aria-label='Test case' hidden></aside>\
         </div>\
         <script type='application/json' id='map-data'>{data}</script>\
         <script>{graph_js}</script>\
         <script>{js}</script>\
```
and add to the format arguments:
```rust
        areas = areas,
        cases = cases,
        empty = empty,
        list = list,
        graph_js = GRAPH_JS,
```

Update the module doc comment's first paragraph to: `//! The Test map: a set of cases drawn as a graph of the areas they test, in the browser - areas and cases as nodes, membership as edges, laid out by a force simulation. The webview builds the tree (one grouping implementation, in `src/lib/testMap.ts`); this side only writes the page.` (keep the wrapping style of the file.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test test_map`
Expected: PASS (3 tests). The old tree script is still embedded at this point; that is fine - Task 4 replaces it.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/test_map.rs src-tauri/tests/test_map.rs
git commit -F - <<'EOF'
feat(v2): Test map page shell - canvas, accessible case list, graph helpers embedded

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 4: The page script and styles

**Files:**
- Rewrite: `src-tauri/web/test-map.js`
- Rewrite: `src-tauri/web/test-map.css`
- Test: `src-tauri/tests/test_map.rs` (already passes; re-run), plus a manual walk described in Step 4.

**Interfaces:**
- Consumes: `window.testMap` (Task 2) and the shell ids from Task 3: `graph`, `viewport`, `detail`, `map-list`, `map-zoom`, `map-in`, `map-out`, `map-reset`, `map-expand`, `map-collapse`, `map-data`, `map-empty`.
- Produces: nothing other tasks use.

- [ ] **Step 1: Write the page script**

Replace `src-tauri/web/test-map.js` entirely:

```js
/* Draws the Test map from the #map-data JSON block as a graph: areas and
   cases as nodes on a canvas, laid out by the simulation in
   test-map-graph.js (window.testMap). Zoom decides how much each node
   says - zoomed out, the area names; zoomed in, the cases - and clicking
   a case opens the side panel. Plain script, no dependencies: the page is
   a file in the temp directory and must work with nothing else present. */
(function () {
  var dataEl = document.getElementById('map-data');
  if (!dataEl || !window.testMap) return;
  var G = window.testMap;
  var data = JSON.parse(dataEl.textContent || '[]');
  var canvas = document.getElementById('graph');
  var viewport = document.getElementById('viewport');
  var detail = document.getElementById('detail');
  var zoomLabel = document.getElementById('map-zoom');
  var ctx = canvas.getContext ? canvas.getContext('2d') : null;
  var reduced = !!(window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches);

  var graph = G.buildGraph(data);
  var nodes = graph.nodes, edges = graph.edges;
  var caseNodes = [];
  var areaNodes = [];
  nodes.forEach(function (n) { (n.kind === 'case' ? caseNodes : areaNodes).push(n); });

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  // ---- Colours: the palette variables, re-read when the theme switches.
  var colours = {};
  function readColours() {
    var cs = getComputedStyle(document.documentElement);
    ['text', 'muted', 'faint', 'border', 'accent', 'surface', 'bg'].forEach(function (k) {
      colours[k] = cs.getPropertyValue('--' + k).trim();
    });
  }
  readColours();
  if (window.MutationObserver) {
    new MutationObserver(function () { readColours(); draw(); })
      .observe(document.documentElement, { attributes: true, attributeFilter: ['data-scheme'] });
  }

  // ---- The view: graph units -> screen px.
  var scale = 1, tx = 0, ty = 0, width = 0, height = 0;
  function toScreen(x, y) { return { x: x * scale + tx, y: y * scale + ty }; }
  function toGraph(sx, sy) { return { x: (sx - tx) / scale, y: (sy - ty) / scale }; }
  function setZoom(next, cx, cy) {
    next = Math.min(4, Math.max(0.2, next));
    // Zoom about the pointer (or the viewport's centre), so the spot under
    // the cursor stays put.
    var px = cx == null ? width / 2 : cx, py = cy == null ? height / 2 : cy;
    tx = px - (px - tx) * (next / scale);
    ty = py - (py - ty) * (next / scale);
    scale = next;
    draw();
  }
  function fit() {
    var t = G.fitTransform(nodes, width, height, 40);
    scale = t.scale; tx = t.tx; ty = t.ty;
    draw();
  }
  function resize() {
    var r = viewport.getBoundingClientRect();
    width = r.width; height = r.height;
    var dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width = width + 'px';
    canvas.style.height = height + 'px';
    draw();
  }

  // ---- The simulation: runs while warm, cools to a stop.
  var alpha = 0, running = false, settledOnce = false;
  function warm(a) {
    alpha = Math.max(alpha, a);
    if (!running) { running = true; requestAnimationFrame(tick); }
  }
  function tick() {
    if (alpha < 0.005) {
      running = false; alpha = 0;
      if (!settledOnce) { settledOnce = true; fit(); } else draw();
      return;
    }
    G.step(nodes, edges, alpha);
    alpha *= 0.97;
    draw();
    requestAnimationFrame(tick);
  }

  // ---- Drawing.
  var hover = null, activeNode = null;
  var FONT = 'ui-sans-serif, system-ui, "Segoe UI", sans-serif';

  function neighbourhood(n) {
    var keep = {};
    keep[n.i] = true;
    edges.forEach(function (e) {
      if (e.a === n) keep[e.b.i] = true;
      if (e.b === n) keep[e.a.i] = true;
    });
    return keep;
  }
  function screenRadius(n) {
    if (n.kind === 'area') return n.r * scale;
    return scale < 0.6 ? 2 : n.r * scale;
  }

  function draw() {
    if (!ctx) return;
    var dpr = window.devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    var focus = hover ? neighbourhood(hover) : null;
    var idA = G.labelAlpha('case-id', scale, reduced);
    var titleA = G.labelAlpha('case-title', scale, reduced);

    ctx.lineWidth = 1;
    ctx.strokeStyle = colours.border;
    edges.forEach(function (e) {
      if (e.a.hidden || e.b.hidden) return;
      var dim = focus && !(focus[e.a.i] && focus[e.b.i]);
      ctx.globalAlpha = dim ? 0.25 : 1;
      var a = toScreen(e.a.x, e.a.y), b = toScreen(e.b.x, e.b.y);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
    });

    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';
    nodes.forEach(function (n) {
      if (n.hidden) return;
      var p = toScreen(n.x, n.y);
      var r = screenRadius(n);
      var base = focus && !focus[n.i] ? 0.25 : 1;
      ctx.globalAlpha = base;
      ctx.fillStyle = n.kind === 'area' ? colours.accent : colours.muted;
      ctx.beginPath(); ctx.arc(p.x, p.y, r, 0, Math.PI * 2); ctx.fill();
      if (n === activeNode) {
        ctx.strokeStyle = colours.accent; ctx.lineWidth = 2;
        ctx.beginPath(); ctx.arc(p.x, p.y, r + 3, 0, Math.PI * 2); ctx.stroke();
        ctx.lineWidth = 1; ctx.strokeStyle = colours.border;
      }

      ctx.fillStyle = colours.text;
      if (n.kind === 'area') {
        // Area names keep a fixed screen size: they are the map's titles.
        ctx.font = '600 13px ' + FONT;
        ctx.fillText(n.name + (n.folded ? ' (' + n.count + ')' : ''), p.x, p.y + r + 4);
        return;
      }
      // Case labels scale with the graph and cross-fade from id to title.
      var size = Math.max(9, Math.min(22, 11 * scale));
      ctx.font = size + 'px ' + FONT;
      if (n === hover) {
        ctx.fillText(G.caseLabel(n, true), p.x, p.y + r + 3);
        return;
      }
      if (titleA > 0) {
        ctx.globalAlpha = base * titleA;
        ctx.fillText(G.caseLabel(n, true), p.x, p.y + r + 3);
      }
      if (idA > 0 && titleA < 1) {
        ctx.globalAlpha = base * idA * (1 - titleA);
        ctx.fillText(G.caseLabel(n, false), p.x, p.y + r + 3);
      }
    });
    ctx.globalAlpha = 1;
    zoomLabel.textContent = Math.round(scale * 100) + '%';
  }

  // ---- Hit testing: the nearest visible node within its radius + 4px.
  function nodeAt(sx, sy) {
    var best = null, bestD = Infinity;
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      if (n.hidden) continue;
      var p = toScreen(n.x, n.y);
      var d = Math.sqrt((p.x - sx) * (p.x - sx) + (p.y - sy) * (p.y - sy));
      if (d <= screenRadius(n) + 4 && d < bestD) { best = n; bestD = d; }
    }
    return best;
  }
  function local(e) {
    var r = viewport.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  // ---- The side panel.
  var listButtons = document.querySelectorAll('#map-list button.case');
  function showCase(node) {
    var c = node.data;
    activeNode = node;
    for (var i = 0; i < listButtons.length; i++) {
      listButtons[i].classList.toggle('active', Number(listButtons[i].getAttribute('data-i')) === node.ci);
    }
    detail.innerHTML = '';
    var close = el('button', 'close', 'Close');
    close.type = 'button';
    close.setAttribute('aria-label', 'Close the test case');
    close.addEventListener('click', hideCase);
    detail.appendChild(close);
    detail.appendChild(el('h2', null, (c.id != null ? '#' + c.id + '  ' : '') + c.title));
    var meta = [];
    if (c.tags) meta.push('Tags: ' + c.tags);
    if (c.automation_status) meta.push(c.automation_status);
    detail.appendChild(el('p', 'meta', meta.join(' · ')));
    if (c.preconditions) {
      detail.appendChild(el('h3', null, 'Preconditions'));
      detail.appendChild(el('p', 'pre', c.preconditions));
    }
    if (!c.steps || c.steps.length === 0) {
      detail.appendChild(el('p', 'meta', 'No steps.'));
    } else {
      var table = el('table');
      var head = el('tr');
      ['#', 'Action', 'Expected'].forEach(function (h) { head.appendChild(el('th', null, h)); });
      table.appendChild(head);
      c.steps.forEach(function (s, i) {
        var tr = el('tr');
        tr.appendChild(el('td', 'n', String(i + 1)));
        tr.appendChild(el('td', null, s.action));
        tr.appendChild(el('td', 'exp', s.expected));
        table.appendChild(tr);
      });
      detail.appendChild(table);
    }
    detail.hidden = false;
    // The panel changes the viewport's width.
    resize();
  }
  function hideCase() {
    activeNode = null;
    for (var i = 0; i < listButtons.length; i++) listButtons[i].classList.remove('active');
    detail.hidden = true;
    detail.innerHTML = '';
    resize();
  }
  for (var b = 0; b < listButtons.length; b++) {
    listButtons[b].addEventListener('click', function () {
      var node = caseNodes[Number(this.getAttribute('data-i'))];
      if (node) showCase(node);
    });
  }

  // ---- Fold.
  function setFolded(area, folded) {
    G.fold(graph, area, folded);
    if (activeNode && activeNode.hidden) hideCase();
    warm(0.5);
  }
  function foldAll(folded) {
    areaNodes.forEach(function (a) { G.fold(graph, a, folded); });
    if (activeNode && activeNode.hidden) hideCase();
    warm(0.5);
  }

  // ---- Pointer: hover, click, drag a node, pan the graph.
  var drag = null; // { node } or { panX, panY }
  var moved = false;
  function setCursor() {
    viewport.classList.toggle('dragging', !!(drag && !drag.node));
    viewport.classList.toggle('over-node', !!(hover || (drag && drag.node)));
  }
  viewport.addEventListener('mousemove', function (e) {
    var p = local(e);
    if (drag) {
      if (!moved && (Math.abs(e.clientX - drag.startX) > 4 || Math.abs(e.clientY - drag.startY) > 4)) moved = true;
      if (drag.node) {
        var g = toGraph(p.x, p.y);
        drag.node.x = g.x; drag.node.y = g.y;
        drag.node.vx = 0; drag.node.vy = 0;
        warm(0.3);
      } else {
        tx = p.x - drag.panX; ty = p.y - drag.panY;
        draw();
      }
      return;
    }
    var h = nodeAt(p.x, p.y);
    if (h !== hover) { hover = h; setCursor(); draw(); }
  });
  viewport.addEventListener('mouseleave', function () {
    if (!drag && hover) { hover = null; setCursor(); draw(); }
  });
  viewport.addEventListener('mousedown', function (e) {
    if (e.button !== 0) return;
    moved = false; // every press starts clean
    var p = local(e);
    var n = nodeAt(p.x, p.y);
    if (n) {
      n.pinned = true;
      drag = { node: n, startX: e.clientX, startY: e.clientY };
    } else {
      drag = { panX: p.x - tx, panY: p.y - ty, startX: e.clientX, startY: e.clientY };
    }
    setCursor();
    e.preventDefault();
  });
  window.addEventListener('mouseup', function () {
    if (!drag) return;
    var n = drag.node;
    drag = null;
    if (n) {
      n.pinned = false;
      if (moved) warm(0.5);
      else if (n.kind === 'case') showCase(n);
      else setFolded(n, !n.folded);
    }
    setCursor();
    draw();
  });
  viewport.addEventListener('wheel', function (e) {
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      var p = local(e);
      setZoom(scale * (e.deltaY < 0 ? 1.1 : 1 / 1.1), p.x, p.y);
      return;
    }
    // Plain wheel pans; Ctrl/Cmd + wheel zooms about the cursor.
    tx -= e.deltaX; ty -= e.deltaY;
    draw();
  }, { passive: false });

  // ---- Chrome.
  document.getElementById('map-in').addEventListener('click', function () { setZoom(scale * 1.2); });
  document.getElementById('map-out').addEventListener('click', function () { setZoom(scale / 1.2); });
  document.getElementById('map-reset').addEventListener('click', fit);
  document.getElementById('map-expand').addEventListener('click', function () { foldAll(false); });
  document.getElementById('map-collapse').addEventListener('click', function () { foldAll(true); });
  window.addEventListener('resize', resize);

  // ---- Go.
  resize();
  if (nodes.length) {
    fit();
    warm(1);
  }
})();
```

- [ ] **Step 2: Write the styles**

Replace `src-tauri/web/test-map.css` entirely:

```css
/* Test map page. Colours are the palette variables the app emits
   (--bg, --surface, --surface-2, --text, --muted, --faint, --border,
   --accent, --success, --danger, --warning), so the page follows the
   app's theme and its own light/dark switch. The graph itself is drawn on
   the canvas by test-map.js from the same variables. */
* { box-sizing: border-box; }
html, body { height: 100%; margin: 0; }
body {
  font-family: ui-sans-serif, system-ui, "Segoe UI", sans-serif;
  font-size: 14px;
  color: var(--text);
  background: var(--bg);
  display: flex;
  flex-direction: column;
}
.bar {
  display: flex;
  align-items: baseline;
  gap: 16px;
  padding: 10px 16px;
  padding-right: 100px;
  border-bottom: 1px solid var(--border);
  background: var(--surface);
  flex: 0 0 auto;
}
.bar h1 { margin: 0; font-size: 16px; }
.subtitle { margin: 0; color: var(--muted); }
.tools { margin-left: auto; display: flex; align-items: center; gap: 6px; }
.tools button {
  font: inherit;
  color: var(--text);
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 4px 10px;
  cursor: pointer;
}
.tools button:hover { border-color: var(--accent); color: var(--accent); }
.tools .sep { width: 1px; height: 18px; background: var(--border); margin: 0 4px; }
#map-zoom { min-width: 3.5em; text-align: right; color: var(--muted); font-variant-numeric: tabular-nums; }

.layout { flex: 1 1 auto; display: flex; min-height: 0; }
.viewport {
  flex: 1 1 auto;
  overflow: hidden;
  position: relative;
  cursor: grab;
  user-select: none;
}
.viewport.dragging { cursor: grabbing; }
.viewport.over-node { cursor: pointer; }
#graph { display: block; position: absolute; left: 0; top: 0; }
.empty { position: absolute; left: 16px; top: 16px; margin: 0; color: var(--muted); }

/* The accessible list: every case as a real button, off-screen for sighted
   users but in the tab order and the accessibility tree. */
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
  clip-path: inset(50%);
  white-space: nowrap;
}
.sr-only button:focus-visible { outline: 2px solid var(--accent); }

/* The side panel: one case's steps. */
.detail {
  flex: 0 0 400px;
  max-width: 50vw;
  overflow: auto;
  border-left: 1px solid var(--border);
  background: var(--surface);
  padding: 14px 16px;
}
.detail h2 { margin: 0 0 4px; font-size: 15px; }
.detail h3 { margin: 10px 0 4px; font-size: 13px; color: var(--muted); font-weight: 600; }
.detail .meta { color: var(--muted); font-size: 12px; margin: 0 0 10px; }
.detail .close {
  float: right;
  font: inherit;
  color: var(--muted);
  background: none;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 2px 8px;
  cursor: pointer;
}
.detail .close:hover { color: var(--accent); border-color: var(--accent); }
.detail .pre { white-space: pre-wrap; margin: 0 0 10px; }
.detail table { width: 100%; border-collapse: collapse; font-size: 13px; }
.detail th { text-align: left; color: var(--faint); font-weight: 500; padding: 4px 6px; }
.detail td { vertical-align: top; padding: 5px 6px; border-top: 1px solid var(--border); white-space: pre-wrap; }
.detail td.n { color: var(--faint); width: 2em; font-variant-numeric: tabular-nums; }
.detail td.exp { color: var(--muted); }
@media print { .tools, .detail { display: none; } }
```

- [ ] **Step 3: Run the gates that embed the page**

Run: `cargo test --test test_map` (the scripts are `include_str!`, so this recompiles them in)
Expected: PASS.
Run: `npx vitest run src/lib/testMapGraph.test.ts`
Expected: PASS (unchanged file, sanity).

- [ ] **Step 4: Generate a page and walk it**

Write a scratch Rust example is not needed - the existing Rust test writes a page: after `cargo test --test test_map` the file is at `%TEMP%\tcm-v2-test-map-tests\<pid>-map.html`. Find the newest one:

```powershell
Get-ChildItem "$env:TEMP\tcm-v2-test-map-tests\*-map.html" | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
```

Open it in the in-app browser (`preview_start` with a `file:///` URL, or serve the directory with `python -m http.server` from that folder and open `http://localhost:8000/<name>`). Check, and note each result in the task report:
1. Three area nodes and four case nodes appear, settle within ~3 s, then the view fits them.
2. At the fitted zoom (likely ≥ 1.2 for this small tree) case labels show `#id  title`; press `−` until below 60 %: case nodes become dots, labels vanish, area names stay at the same size.
3. Hover a case: it, its area and the edge stay full colour, the rest dim; its full label shows.
4. Click a case: the side panel opens with its steps; the node is ringed; Close clears it.
5. Click "Manage Events": its cases and "Create" disappear, the label reads `Manage Events (3)`; click again to restore. Collapse all / Expand all.
6. Drag a node: it follows and the rest re-settle; drag empty space: pans; Ctrl+wheel: zooms about the cursor; Reset: fits.
7. The light/dark switch recolours the graph immediately.
8. Tab from the page start reaches the hidden case buttons; Enter opens the panel.

If the tree is too small to see LOD clearly, temporarily bump the counts by editing the test's `tree()` in a scratch copy - never commit that.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/web/test-map.js src-tauri/web/test-map.css
git commit -F - <<'EOF'
feat(v2): Test map draws an Obsidian-style graph - zoom decides labels, fold, hover, drag

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

## Self-review notes

- Spec coverage: graph model, layout constants, LOD table and ramps, hover/click/fold/drag/cursor, page shell, hidden list (Rust), `role='img'`, helpers on `window.testMap`, theme recolour, `set_area` + `area_is` + docs, Rust/vitest tests, manual walk - each maps to Task 1–4 above. "Reduced motion makes the ramp a step" is in `labelAlpha(kind, scale, reduced)`; the page passes its media-query result.
- Type consistency: node field names (`i`, `ci`, `hidden`, `pinned`, `folded`, `data`, `cases`, `children`) are the same in Task 2's file, Task 2's test and Task 4's script; the `data-i` numbering in Task 3 (cases before children, depth-first) matches `buildGraph`'s `ci`.
- The final version bump and changelog entry are done by the controller at release time, not in a task.
