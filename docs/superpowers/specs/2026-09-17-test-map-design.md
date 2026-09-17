# Test map - design

A browser page that shows a set of test cases as a tree of the areas they
test, so a reader can see which parts of a page or feature are covered and
open any case's steps without leaving the tree.

## The `area` field

- A new optional string on a test case in the JSON import format: `"area"`.
- It is a **path**: segments separated by `/`, spaces around the slash
  optional. `"Manage Events / Create / Validation"` and
  `"Manage Events/Create/Validation"` mean the same thing. Segments are
  trimmed; empty segments are dropped.
- Accepted spellings when reading a file: `area`, `section`, `group`
  (same alias rule as `comment` / `reviewer_notes`).
- **App-only**, like `comment`, `reviewer_notes`, `spec_order`,
  `tester_order` and `findings`: it round-trips through export, import and
  the draft merge, is never written to Azure DevOps, and is omitted from
  the JSON when empty. The existing `app_only_fields_never_reach_a_request_body`
  test guards the write path automatically.
- A transform's `insert_cases` op reads it through the same alias list.
- A watched-file edit to `area` shows in the change report as a field
  change named "Area".
- Not editable in the app's inline editor (edit the JSON); a later change.

## The tree

Built in the webview (`src/lib/testMap.ts`), one grouping implementation
for the app: cases with an `area` land under their path; cases without one
fall back to the existing title grouping (`groupIndices`) and land one
level deep under the group name, or under "Ungrouped". Both kinds merge
into one root: a title group named like an area segment joins it (names
compare case-insensitively, first spelling shown).

Children sort A-Z, case-insensitively, "Ungrouped" last. A node's count is
the number of cases in it and every node beneath it. Cases keep the order
they were given.

The webview sends the finished tree to Rust: `MapNode { name, count, cases,
children }` and `MapCase { id, title, steps, preconditions, tags,
automation_status }`.

## The page

- Rust command `view_test_map_html(nodes, subtitle, palette)` writes
  `test-map-<pid>.html` to the temp directory and opens it in the browser.
  Same page shell as the existing reports: the app's palette, the
  light/dark switch, `data-scheme`.
- The tree and its data are in the page: a `<script type="application/json"
  id="map-data">` block and a script that renders it, no network.
- Static: no revision poll. Opening it again rewrites the file.
- Each node shows its name and count, folds and unfolds by click; a case is
  a row under its node showing `#id` (or NEW) and the title.
- Zoom and pan: `+`, `-` and `Reset` buttons, Ctrl + mouse wheel to zoom,
  drag on empty space to pan. "Expand all" and "Collapse all".
- Clicking a case opens a side panel with the title, id, tags,
  preconditions and a numbered steps table (action, expected). Only one
  panel; clicking another case replaces it; a close button clears it.
- Everything is escaped; the JSON block escapes `</` so a title can never
  close the script.

## Where the button is

- Queue (Import File and Manual Entry): a **Test map** button beside
  "View in browser", enabled when the queue has cases. It maps the whole
  queue.
- View Test Cases: a **Test map** button beside "View All Test Cases in
  Browser". It maps the selection when there is one, otherwise everything
  the filter shows - the same scope rule as the browser view. Cases from
  Azure DevOps have no `area`, so this page groups by title.
- Icon: `IconTestMap` added to the shared vocabulary.

## The writing guide

The guide assistants read (`get_writing_guide`) gains an `## area`
section asking for an area path on every case, and the JSON export's
`instructions` string mentions the field, so files written by assistants
fill the map without anyone editing JSON.

## Deferred

- Colouring nodes by last run outcome from Run Tests.
- Editing `area` in the app's inline editor and bulk edit.
- Live refresh of an open map page.
