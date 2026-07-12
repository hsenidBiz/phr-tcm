# Iteration UX — v1.4.0 (2026-07-12)

Work-Manager-heavy round from live testing.

- **Board drag-drop actually works in the packed app**: Tauri's default
  window drag-drop handler swallowed WebView2 drag events, silently
  disabling HTML5 drag-to-column; `dragDropEnabled: false` (no file
  drops exist) - drag a card to In Progress/Done moves it.
- **Descriptions keep ADO formatting**: WorkItemDetail gains
  `description_html`; the drawer converts it to markdown via turndown on
  load, so DevOps bold/lists/links round-trip through Write/Preview/save.
- **Work Manager**: refresh button (spins while fetching); searchable
  team scope combobox; "Open in Azure DevOps" from the drawer header;
  checkbox multi-select type filter (new MultiSelect primitive, v1
  CheckableComboBox parity) persisted via tcm-v2-type-filter.
- **Run Tests**: floating bottom-right selection pill (count / Run N in
  runner / clear) stays on screen while the points table scrolls.
- **Shell**: browser context menu suppressed everywhere except editable
  fields (native cut/copy/paste kept).

---

# Iteration UX — v1.3.1 (2026-07-12)

First post-cutover patch (developed on master).

- **First-run UI tour**: spotlight walkthrough over the context bar, PBI
  picker, every sidebar tab (incl. Import as the upload point), Work
  Manager and Settings; auto-runs once after first sign-in
  (`tcm-v2-tour-done`); replayable from Settings > Interface tour.
- **Fixes**: suite-search folders collapsible mid-search (separate
  search-mode collapse set); background prefetch survives React Query's
  5-min gc (gcTime 60/30 min on plans-suites + points, prefetch AND
  screen queries); recent-PBI titles word-wrap; context bar wraps at
  narrow widths (icon-only Work Manager below lg, email hidden below xl,
  min-w PBI picker); empty-state flask bubbles emit from the mouth.

---

# Iteration UX — v1.3.0 (2026-07-12)

Third same-day UX round from live testing. Branch stays
`feat/tauri-rewrite` — NO merge, NO migration.

## What shipped in 1.3.0

- **Searchable pickers**: Module is a searchable combobox (custom entry
  kept for unmapped fields); Tags are a searchable multi-select with
  removable chips (project-tag suggestions, Enter adds new, Backspace
  pops) — in Manual Entry, the case editor and Bulk edit.
- **Module data correctness**: picklists fetched with
  `$expand=allowedValues` (custom picklist fields omit values without
  it); when a field truly has no picklist, `field_values_in_use` falls
  back to the distinct values on the project's recent Test Cases (WIQL,
  unsafe-ref guarded); auto-pick ranks exact "Module" > prefix > contains.
- **Background prefetch**: Test Suites' plan scan warms on org/project
  pick; on PBI pick the suite resolves via the new READ-ONLY
  `find_pbi_suite` (never creates) and its points prefetch - both screens
  render instantly on navigation.
- **Grouping upgrades**: headers are centered "Title (N)" flanked by
  full-width separator lines; clicking a title selects the whole group;
  a chevron collapses/expands each group (Edit + Run Tests).
- **Controls polish**: hover states on every Input/Textarea/Select;
  shadcn-style Checkbox everywhere; new-picker backgrounds matched to
  the standard inputs.
- **Sign-in flair**: animated flask logo (draw-on, glow, comet orbits,
  bubbles from the mouth), accent-tinted, reduced-motion aware.

---

# Iteration UX — v1.2.0 (2026-07-12)

Follow-up UX round after 1.1.0, driven by live testing feedback. Branch
stays `feat/tauri-rewrite` — NO merge to master, NO v1-user migration.

## What shipped in 1.2.0

- **Suite performance/correctness**: suite-detection cache short-circuits
  in the queryFn (no re-resolve on tab switches or remount); plan scans
  run 8-concurrent instead of serially (~8x faster on big projects); the
  tauri-specta event registry is now mounted, so `SuiteScanProgress` /
  `SubmitProgress` emits reach the UI instead of panicking a tokio worker.
- **Smart grouping (v1 parity)**: `groupIndices` ported from
  `app/utils/grouping.py` (golden tests too) — "Group by title" on Edit
  Test Cases and Run Tests, delimiter- then word-prefix folders.
- **Test Suites**: multi-level folders collapsed by default; search box
  (prunes + auto-expands matches); larger View/Edit-cases/Run buttons.
- **Full theme system**: `data-theme` swaps the entire palette — Light,
  Slate, Midnight, Graphite, Ocean, **OLED** (true-black); "Theme
  default" accent option; theme cards + accent swatches in Settings; the
  context-bar dark/light toggle removed (themes live in Settings).
- **Run Tests**: shift+click range selection; outcomes capitalized for
  display ("passed" -> "Passed").
- **Work Manager**: "Hide Done" toggle (2-column board, more drawer
  room); drawer slides in from the right; drawer is drag-resizable from
  its left edge (persisted); markdown description editor (Write/Preview,
  saved as HTML).
- **Calendar**: shadcn-style react-day-picker calendar+DateField replaces
  the native date inputs (themes correctly).
- **Polish**: native-dark `color-scheme` for form controls; wide PBI chip
  in the context bar; recent PBIs as a labeled vertical list; ambient
  flask animation fills the pick-a-PBI empty states; violet splash
  (flask/bar/background); Import card spans full width; single Work
  Manager scrollbar; queue "Remove all"; trailing "..." removed app-wide.

Deviations/limits: markdown descriptions round-trip through ADO's stored
HTML (reopening shows flattened text, not the original markdown source);
region capture uses the ms-screenclip overlay + clipboard poll.

---

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
