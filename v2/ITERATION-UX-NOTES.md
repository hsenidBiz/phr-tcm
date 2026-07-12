# Iteration UX — v1.1.0 (2026-07-12)

The ~30-item UX-conveniences batch (user list, 2026-07-12). Plan of record:
`claudedocs/plans/2026-07-12-v2-ux-conveniences.md`. Six commits (A–F) +
this release commit; branch stays `feat/tauri-rewrite` — NO merge to
master, NO v1-user migration yet.

## What shipped

### App shell (batch B)
- Frameless windows: custom TitleBar (v1 chrome parity — flask mark,
  dynamic title, min/max/close with danger-hover close) on the main and
  runner windows; `decorations: false`.
- Startup splash in `index.html` (inline CSS, flask draw-on animation)
  removes the white flash; main window is `visible: false` until React
  mounts and calls `show()`.
- v1 logo adopted: window/taskbar icons regenerated from
  `resources/icon.png`; `flask.svg` inlined as the in-app mark.
- Collapsible sidebar (56px icon rail, persisted), themed scrollbars,
  and 5 accent presets (green default, blue, violet, amber, rose) via
  `data-accent` — picker in Settings.
- Post-OAuth browser page restyled (dark gradient card, auto-close).

### Manual Entry / Import (batch C)
- Shared StepsEditor grid (two boxes per step, reorder/remove/add) —
  same editor in Manual Entry, case editor and (fields only) bulk edit.
- Module is a dropdown fed by the org's picklist
  (`test_case_field_values`); free entry only when no picklist exists.
- Two-column Manual Entry layout — fields left, steps right, no dead
  space.
- JSON is the only import/export format in the UI (xlsx parser stays in
  Rust for compatibility); "View in browser" renders the HTML report to
  a temp file and opens it directly — no save dialog.

### Edit Test Cases (batch D)
- Checkboxes removed: click selects, ctrl/cmd+click toggles, shift+click
  range-selects; chevron/double-click expands the inline editor.
- Selection toolbar: Bulk edit / Export JSON / Clear.
- BulkEditDialog: per-field "leave unchanged"; serial updates with
  "Updating X of Y"; each update round-trips the case's own steps so
  nothing is clobbered.
- Smart grouping by module (persisted toggle) + Refresh button.

### Run Tests / Runner (batch E)
- Suite detection cached (staleTime Infinity + localStorage seed
  `tcm-v2-suite:{org}/{pbi}`); explicit re-detect button.
- "Scanning test plans X of Y..." progress (SuiteScanProgress event)
  instead of skeletons.
- Point rows tinted by last outcome; "Not Applicable" label everywhere
  (ADO value unchanged); Rust strips "unspecified" → never-run.
- Row selection + "Run N in runner" (RunnerSession.caseIds).
- Runner: Pin toggle (always-on-top, default on), auto-loaded uploaded
  screenshots, per-case "Last: <outcome>" badge, Snip capture
  (ms-screenclip overlay + 60s clipboard poll), "Attach file..." for any
  file, named attachment chips with remove.

### Test Suites (batch F)
- Multi-level folder tree from `parent_id` (collapsible folders,
  requirement suites as leaves, per-depth indent).
- Session-cached plan scan + Refresh + scan progress line.
- "View" on any suite or folder (descendant cases → HTML report).
- Folder "Edit cases" hands descendant case ids to Edit Test Cases
  (App `caseSelection` route, "Back to PBI cases" banner).
- Recent-PBI chips in all four pick-a-PBI empty states.

## Deviations from the request
- "Capture like Snipping Tool": implemented via the `ms-screenclip:` URI
  + clipboard polling (no in-app region overlay). If the overlay is
  unavailable the toast directs to manual snip + Paste.
- Multi-suite RUN from a folder is out of scope (runs stay
  requirement-suite-scoped); folders support View + Edit instead.
- Bulk edit intentionally never touches titles/steps.

## Gates at release
- cargo test: 67 passed (all integration targets), 0 failed.
- vitest: 43 passed (15 files).
- `npm run build` clean; e2e CDP smoke against the release exe passed.
