# Phase 4 exit notes — Work Manager board (2026-07-11)

Branch: `feat/tauri-rewrite`. Verdict: **GO** for Phase 5 (cutover prep) —
but cutover itself needs the user's live validation first.

## Delivered

- **Board core** (`work_board.rs`) ported from v1 `work_item.py` +
  `mywork_screen` with tests for every documented rule:
  - Columns derive from state *categories* (Proposed / InProgress /
    Resolved+Completed), never hardcoded names; Removed hides the item.
  - A state literally named "Later" always lands in Done (v1's parked-work
    rule).
  - Unknown processes fall back to the v1 name heuristic.
  - Drops resolve the target state with the exact-column-name preference
    (Task/Bug dropped on In Progress becomes "In Progress", not "Active").
- **fetch_board** — WIQL `@Me` minus test artifact types, 200-chunk batch
  field fetch, per-type state discovery (unreadable types degrade to the
  heuristic), ChangedDate-DESC order preserved.
- **move_board_item** — resolves state and PATCHes `System.State`.
- **WorkBoard UI** — three columns, HTML5 drag-drop, optimistic move with
  rollback on error, state text/colour applied from the PATCH result; the
  header pill toggles Test Case Manager ↔ Work Manager (right side, like v1).
- Tests now: 55 Rust + 12 Vitest, all green.

## Deferred from v1's Work Manager (additive later)

- Detail editor (title/description/assignee/scheduling edits), comments
  with avatars, quick create, focus timer, team scoping (`get_team_field_values`),
  activity picklists. All are additive UI features over the same
  update_work_item_fields path already in place.
- **SQLite watermark cache**: deliberately NOT built (again). TanStack
  Query's in-memory cache is adequate at current board sizes; revisit only
  if cold-start fetch times actually hurt (`the tech-stack doc's original
  criterion`).

## The six phases: status after this session

| Phase | Status |
| --- | --- |
| 0 spike + release pipeline | DONE (10.2MB exe, Velopack pack, boot smoke) |
| 1 read-only browse | DONE (refresh, orgs, PBI search, linked cases) |
| 2 TC CRUD + import | DONE (golden ports, writes, queue UI; deferrals noted) |
| 3 runner | CORE DONE (suite ensure, points, runs; desktop UX deferred) |
| 4 Work Manager | CORE DONE (board + transitions; editor/comments deferred) |
| 5 cutover | NOT STARTED — blocked on live user validation (next note) |

## Phase 5 gate (needs the user)

Cutover means real users: publish pipeline for the V2 app id, settings
migration, WebView2 bootstrap decision, README/docs. None of that should
happen until the user has run `cd v2; npm run tauri dev` and validated the
live flow end-to-end (sign in → orgs → PBI search → create+link → run
outcomes → board moves). Everything up to here is wiremock/mockIPC-verified.
