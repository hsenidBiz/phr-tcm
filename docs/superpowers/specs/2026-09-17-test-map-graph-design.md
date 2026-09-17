# Test map as a graph - design

The Test map page (see `2026-09-17-test-map-design.md`) drawn on a canvas
as a tree that grows left to right: areas and cases as nodes, membership
as curved limbs, every case on its own row, with labels that depend on the
zoom so a zoomed-out map reads as the structure's titles and a zoomed-in
one as the cases. (A first version used a force-directed graph in the
Obsidian style; it was replaced because labels piled up unreadably once a
set passed a few hundred cases.) The data contract, the buttons in the app
and the side panel are unchanged.

Alongside it, `transform_cases` gains a `set_area` op and an `area_is`
filter so an assistant can set or move areas without editing JSON.

## Graph model

Built from the `MapNode` tree the webview sends (`src/lib/testMap.ts`);
`export_test_map_html(nodes, path, subtitle, palette)` keeps its signature
and its `#map-data` JSON block.

- **Area node** - one per `MapNode`. Radius `clamp(7, 5 + 1.5*sqrt(count), 16)`
  px in graph units. An edge to its parent area; top-level areas have no
  parent and are laid out as separate trees, stacked.
- **Case node** - one per `MapCase`, radius 4. An edge to the area that
  holds it.
- Colours from the page palette variables: area nodes `--accent`, case
  nodes `--muted`, edges `--border`, labels `--text`; read with
  `getComputedStyle(document.documentElement)` and re-read when the theme
  switch changes `data-scheme`, so the graph recolours live.

## Layout

A tidy tree in `test-map-graph.js` (`layoutTree(graph)`), no dependency:

- Depth is the column: an area at depth `d` sits at `x = d * 220`; its
  cases one column further right.
- Every visible case takes the next row (`ROW = 22` units), in file order,
  an area's cases before its child areas. A folded area, or an empty one,
  takes one row itself.
- An area sits at the vertical midpoint of the first and last row beneath
  it.
- Top-level areas are separate trees stacked top to bottom with a 40-unit
  gap.
- Hidden nodes (inside a folded area) are skipped and keep their last
  position. The layout is recomputed after every fold or unfold, and is
  deterministic: the same file opens looking the same.
- `layoutTree` returns `{width, height}` in graph units.

## Level of detail

Decided in the draw loop from `scale` (graph units → screen px):

| scale | case nodes | case label | area label |
|---|---|---|---|
| `< 0.5` | dots (screen radius 2) | none | name, fixed 13 px screen size |
| `0.5 – 0.8` | scaled | `#id` (nothing for a new case) | name |
| `≥ 0.8` | scaled | `#id  title`, the whole title | name |

Every case has its own row, so labels never collide; the thresholds exist
only because text below about 9 px is unreadable. Case labels are drawn to
the right of the dot, on the row; area names above the node, from its
left edge.

Each label's opacity ramps linearly over ±0.1 of its threshold
(`labelAlpha(kind, scale)` returns 0–1); under
`prefers-reduced-motion: reduce` the ramp is a step. Area labels are drawn
at constant screen size at every zoom; case labels scale with the graph.
Zoom range 0.2–4. Ctrl/⌘ + wheel zooms about the cursor; plain wheel and a
drag on empty space pan; `+` and `−` zoom by 1.2 about the viewport
centre; Reset and the first view call
`fitWidthTransform(nodes, width, 40, 320)`, which fits the tree's width
plus 320 units of label room, never above 100 %, starting at the top left
- a tall tree is read by scrolling, not shrunk to fit. `fitTransform`
(fit everything, centred, cap 1.5) remains as a helper.

## Interaction

- Hover: the node under the pointer and its whole ancestor chain
  (`pathTo(node)`: every area from the top-level one down to it, and the
  limbs between them) draw at full colour, everything else at 25 %
  opacity; the hovered node's full label draws regardless of LOD. While a
  case is hovered, a short accent-coloured pulse travels along the chain's
  limbs from the top-level area to the case, one pass every 900 ms,
  repeating until the pointer leaves; not under `prefers-reduced-motion`.
- Click a case node: `showCase` opens the existing side panel; the node is
  ringed while the panel is open. The panel's meta line leads with the
  case's status: "New – not yet in Azure DevOps", or "In Azure DevOps as
  #id".
- Click an area node: folds its subtree - its cases and descendant areas
  leave the simulation and its label gains ` (n)` with the folded count;
  click again to unfold. "Expand all" / "Collapse all" fold or unfold every
  area. Folded state is `node.folded`, not remembered across opens.
- Drag anywhere pans (nodes have fixed places in the tree, so there is no
  node drag). The 4 px `moved` guard stays: a press that moved never also
  clicks. A case is hit by its whole row, from the dot to the end of its
  label; an area by its circle.
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
or the title alone for a new case), where `N` is the case's position in a depth-first walk
that visits an area's cases before its children - the same order
`buildGraph` numbers case nodes, so the script wires each button to its
node by index. Keyboard users tab through it; the Rust test asserts on it.
The canvas carries `role='img'` and an `aria-label` of
"Test map: N areas, M test cases".

The simulation and LOD helpers live in their own file,
`src-tauri/web/test-map-graph.js`, which attaches them to
`window.testMap`: `buildGraph(tree)` (returns `{nodes, edges}`),
`layoutTree(graph)`, `fold(graph, areaNode, folded)`,
`labelAlpha(kind, scale, reduced)`, `fitTransform(nodes, w, h, pad)`,
`fitWidthTransform(nodes, w, pad, labelSpace)`, `caseLabel(node,
withTitle)` and the constants `ROW` and `LEVEL`.
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
  area and case and one edge per membership; `layoutTree` gives every
  visible case its own row in file order, puts areas at the midpoint of
  their rows, stacks top-level trees without overlap, collapses a folded
  area to one row, and is deterministic; `fold` removes a subtree and
  `fold(..., false)` restores it; `labelAlpha` is 0 / ramping / 1 across
  each threshold; `fitTransform` keeps every node inside the viewport;
  `fitWidthTransform` fits the width at most 100 % and starts top-left.
- Manual walk of a generated page in the in-app browser: fold, hover,
  zoom LOD, pan, theme switch, side panel.

## Deferred

- Colouring nodes by last run outcome from Run Tests.
- Editing `area` in the app's inline editor and bulk edit.
- Live refresh of an open map page.
- Remembering folded state or the view transform between opens.
