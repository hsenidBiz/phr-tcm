# Test map as a graph - design

The Test map page (see `2026-09-17-test-map-design.md`) drawn the way
Obsidian draws its graph view: areas and cases as nodes, membership as
edges, laid out by a force simulation, with labels that depend on the zoom
so a zoomed-out map reads as the structure's titles and a zoomed-in one as
the cases. The graph replaces the tree; the data contract, the buttons in
the app and the side panel are unchanged.

Alongside it, `transform_cases` gains a `set_area` op and an `area_is`
filter so an assistant can set or move areas without editing JSON.

## Graph model

Built from the `MapNode` tree the webview sends (`src/lib/testMap.ts`);
`export_test_map_html(nodes, path, subtitle, palette)` keeps its signature
and its `#map-data` JSON block.

- **Area node** - one per `MapNode`. Radius `clamp(10, 6 + 3*sqrt(count), 28)`
  px in graph units. An edge to its parent area; top-level areas have no
  parent and float as separate components.
- **Case node** - one per `MapCase`, radius 4. An edge to the area that
  holds it.
- Colours from the page palette variables: area nodes `--accent`, case
  nodes `--muted`, edges `--border`, labels `--text`; read with
  `getComputedStyle(document.documentElement)` and re-read when the theme
  switch changes `data-scheme`, so the graph recolours live.

## Layout

A force simulation in `test-map.js`, no dependency:

- Spring along every edge: rest length 40 for case→area, 110 for
  area→area, strength 0.08.
- Repulsion between every pair of nodes, `k / d²` with `k = 900`, applied
  through a uniform grid bucket (cell 120) so 500 nodes stays cheap; pairs
  more than two cells apart are skipped.
- A weak pull toward the origin, strength 0.01.
- Velocity damping 0.85 per step; `alpha` starts at 1, multiplies by 0.97
  per step and the loop stops below 0.005 (about 2 s at 60 fps).
- Deterministic start: top-level areas on a ring of radius 220 by index,
  child areas on a ring of radius 90 around their parent, cases jittered
  ±30 around their area by a seeded hash of their index. The same file
  opens looking the same.
- Re-warm to `alpha = 0.5` after a fold, unfold or node drag.

`step(nodes, edges, alpha)` mutates positions and returns nothing; it
depends on nothing but its inputs so it can be tested off the page.

## Level of detail

Decided in the draw loop from `scale` (graph units → screen px):

| scale | case nodes | case label | area label |
|---|---|---|---|
| `< 0.6` | dots (screen radius 2) | none | name, fixed 13 px screen size |
| `0.6 – 1.2` | scaled | `#id` (or NEW) | name |
| `≥ 1.2` | scaled | `#id  title`, ellipsised at 40 chars | name |

Each label's opacity ramps linearly over ±0.1 of its threshold
(`labelAlpha(kind, scale)` returns 0–1); under
`prefers-reduced-motion: reduce` the ramp is a step. Area labels are drawn
at constant screen size at every zoom; case labels scale with the graph.
Zoom range 0.2–4. Ctrl/⌘ + wheel zooms about the cursor; plain wheel and a
drag on empty space pan; `+` and `−` zoom by 1.2 about the viewport
centre; Reset calls `fitTransform(nodes, width, height)`, which returns
`{scale, tx, ty}` fitting every visible node with 40 px padding, capped at
scale 1.5.

## Interaction

- Hover: the node under the pointer (distance ≤ radius + 4 px screen) and
  its neighbours draw at full colour, everything else at 25 % opacity; the
  hovered node's full label draws regardless of LOD.
- Click a case node: `showCase` opens the existing side panel; the node is
  ringed while the panel is open.
- Click an area node: folds its subtree - its cases and descendant areas
  leave the simulation and its label gains ` (n)` with the folded count;
  click again to unfold. "Expand all" / "Collapse all" fold or unfold every
  area. Folded state is `node.folded`, not remembered across opens.
- Drag a node: it follows the pointer and is pinned until release. Drag on
  empty space pans. The 4 px `moved` guard from the tree stays: a press
  that moved never also clicks.
- Cursor: `grab` on empty space, `grabbing` while panning, `pointer` over a
  node.

## Page shell

Header unchanged except the tools: Expand all, Collapse all, separator,
`−`, `+`, Reset, zoom %. The viewport holds `<canvas id='graph'>` sized
to the viewport on load and on resize (device-pixel-ratio aware). The
side panel `<aside id='detail'>` is as before.

## Accessibility and testability

Canvas is invisible to screen readers and to jsdom, so Rust also writes a
visually hidden `<div id='map-list'>` into the page: for each area a
heading (`<h3>` name and count) followed by an `<ol>` of
`<button type='button' class='case' data-i='N'>` entries (`#id title`,
or `NEW title`), where `N` is the case's position in a depth-first walk
that visits an area's cases before its children - the same order
`buildGraph` numbers case nodes, so the script wires each button to its
node by index. Keyboard users tab through it; the Rust test asserts on it.
The canvas carries `role='img'` and an `aria-label` of
"Test map: N areas, M test cases".

The simulation and LOD helpers live in their own file,
`src-tauri/web/test-map-graph.js`, which attaches them to
`window.testMap`: `buildGraph(tree)` (returns `{nodes, edges}`),
`step(nodes, edges, alpha)`, `fold(graph, areaNode, folded)`,
`labelAlpha(kind, scale, reduced)` and `fitTransform(nodes, w, h, pad)`.
The page embeds it before `test-map.js`. A vitest file under `src/` loads
the helper file and exercises it.

## `transform_cases`

- `Op::SetArea(String)`: `{ "op": "set_area", "value": "Manage Events / Create" }`
  overwrites `area` on every matched case. The value is normalised the way
  the import parser normalises a path (segments trimmed, empty ones
  dropped, joined with ` / `); an empty value clears the field. Report line
  `Set area to '<value>'` (or `Cleared area`). `SUPPORTED_OPS` gains
  `set_area` (25 entries); `known_keys` lists it with the `{value}` group;
  the MCP tool description and `ai_tools.rs` mention it beside
  `set_preconditions`.
- `Filter.area_is: Option<String>`: exact match after the same
  normalisation, case-insensitive; a case with no area matches only an
  empty `area_is`. Documented beside `module_is`.

## Testing

- Rust `tests/test_map.rs`: the shell has `<canvas id='graph'>` and
  `id='map-list'`; every case's `#id title` appears in the list; a title
  containing `</script>` still cannot close the JSON block.
- Rust `tests/transform.rs`: `set_area` sets and clears, requires a value,
  normalises spacing; `area_is` matches case-insensitively and by
  normalised path, and excludes cases in other areas; an unknown key on
  `set_area` lands in `ignored`.
- Vitest `src/lib/testMapGraph.test.ts`: `buildGraph` makes one node per
  area and case and one edge per membership; `step` lowers total kinetic
  energy over 200 steps from the deterministic start; `fold` removes a
  subtree's nodes and edges and `fold(..., false)` restores them;
  `labelAlpha` is 0 / ramping / 1 across each threshold; `fitTransform`
  keeps every node inside the viewport with the padding.
- Manual walk of a generated page in the in-app browser: fold, hover,
  zoom LOD, drag, theme switch, side panel.

## Deferred

- Colouring nodes by last run outcome from Run Tests.
- Editing `area` in the app's inline editor and bulk edit.
- Live refresh of an open map page.
- Remembering folded state or the view transform between opens.
