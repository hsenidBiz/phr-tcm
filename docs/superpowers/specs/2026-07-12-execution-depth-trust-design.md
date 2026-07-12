# Execution Depth & Trust — v2 iteration design

**Date:** 2026-07-12 · **Target:** Test Case Manager V2 (post-cutover, master)
**Features:** (A) run history & trends, (B) update diff-preview, (C) execution
report. Decisions below were made interactively with the owner.

## Goals

1. **A — Run history:** the app only knows each test point's *last* outcome.
   Surface the last 5 outcomes per case in Run Tests and the runner so flaky
   or repeatedly-failing cases are visible at a glance.
2. **B — Diff-preview:** the review gate does not show *what changes* when an
   import UPDATEs existing cases. Show per-field old→new, a step-level
   summary, the blank-skipped fields, and flag no-op updates — before submit.
3. **C — Execution report:** one click produces a shareable, self-contained
   HTML report of a PBI's (or any suite/folder's) execution state.

**Non-goals:** screenshots embedded in reports (kept out for size/speed; bugs
are linked by id); history in the Test Suites points tables; any DELETE or
data-mutating call — everything new is GET-only (C reuses the existing
open-temp-HTML pattern, which writes only a local temp file).

## Decisions (owner-confirmed)

- A surfaces in **Run Tests table + runner** (not Suites); **last 5 runs**.
- A fetches via the **recent-runs sweep** (GET `test/runs?planId=…` capped at
  ~15 recent runs, then each run's results with bounded concurrency), not the
  `results/query` POST — keeps the client's POSTs create-only in spirit and
  feeds C the same data shapes.
- B renders as **inline expandable rows** in the review queue (no extra
  confirm dialog); no-op badge visible without expanding.
- C is **per-PBI from Run Tests and per-suite/folder from Test Suites**;
  no embedded screenshots.

## Feature A — Run history & trends

### Rust (`ado_testplan.rs` + command in `lib.rs`)

- `run_history(org, project, plan_id) -> Vec<CaseHistory>` where
  `CaseHistory { test_case_id: i32, outcomes: Vec<RunOutcome> }` and
  `RunOutcome { outcome: String, completed_date: String, run_id: i32 }`.
- Implementation: GET the plan's runs (`_apis/test/runs?planId={id}`, most
  recent ~15 — server order or sorted by completed date desc), then GET each
  run's results concurrently (reuse the `SUITE_SCAN_CONCURRENCY` pattern).
  Aggregate per `testCase.id`: newest first, **cap 5 per case**. Skip results
  with no outcome / "unspecified" / in-progress. Runs that 403/404 are
  skipped like the suite scan; auth/rate-limit errors propagate.
- Wiremock tests: aggregation across runs, newest-first ordering, the 5-cap,
  unspecified skipped, and a no-DELETE-source guard already covers the file.

### Frontend

- Query `["run-history", org, project, planId]`, `staleTime` 5 min,
  `gcTime` 30 min, invalidated in RunPanel's submit `onSuccess` so a fresh
  run shows up immediately. (The runner window has its own QueryClient, so
  cross-window invalidation is out of scope — after a runner-submitted run,
  the main window's history refreshes on the next staleTime expiry or
  points refetch.)
- **Run Tests**: new "History" column between *Last outcome* and *This run*.
  Cell renders up to 5 dots, newest on the left, colored via the existing
  outcome tokens (success/danger/warning/faint); hollow ring = no data.
  Each dot gets `title="{outcome} · {date}"`. Missing history (case absent
  from the payload) renders an em dash like other empty cells.
- **Runner**: the same dot strip (smaller, no column chrome) beside the
  "Last: X" badge under the case title. Shares the same query key.
- A small pure helper (`lib/history.ts` or inline) maps outcome → token
  class; unit-test the mapping + cap/ordering assumptions via component test
  (dots rendered for mocked payload).

## Feature B — Update diff-preview in Review

### Pure logic (`v2/src/lib/caseDiff.ts`, unit-tested)

```
diffCase(queued: TestCase, current: TestCaseFull): CaseDiff
CaseDiff {
  fields: { name: string; old: string; new: string }[]   // changed only
  steps: {
    added: number; removed: number; changed: number;
    detail: { index: number; kind: "added"|"removed"|"changed" }[]
  }
  blankSkipped: string[]   // fields the update intentionally leaves alone
  noop: boolean            // nothing will change on the server
}
```

- Field set: title, tags (order-insensitive set compare on split tags),
  automation_status, module_value, preconditions.
- **Blank-skip mirror:** the Rust update rule never overwrites with blank —
  so a queued blank module/preconditions/tags lands in `blankSkipped`, not in
  `fields`. Title/status/steps always write, so they diff normally.
- Steps compare positionally (index i vs index i): different action/expected
  → changed; extra queued steps → added; extra current steps → removed.
- `noop` = no changed fields AND no step changes.
- Unit tests: each field kind, blank-skip cases, step add/remove/change,
  tag order-insensitivity, full no-op.

### Review UI (`QueueSection.tsx`)

- Entering review mode batch-fetches current values for all queued
  `update_id`s via the existing `testCasesByIds` (single call; query keyed on
  the id list; failures degrade to "diff unavailable" — never block submit).
- UPDATE rows gain, without any clicks: an amber **"no-op — nothing will
  change"** badge, or a muted summary chip like "2 fields · 3 steps change".
- A chevron expands the row: field table (`Title: "old" → "new"`), step
  summary line with per-step kinds, and the blank-skipped list ("left
  untouched: Module, Preconditions").
- Component test: mock two updates (one real diff, one no-op) and assert the
  badge, summary, and expanded old→new render.

## Feature C — Execution report

### Rust

- `view_execution_report(org, project, plan_id, suite_ids: Vec<i32>, title)`
  → renders HTML to a temp file and opens it (same mechanism as
  `view_queue_html`); returns `Result<(), String>`.
- Data: for each suite id, `get_test_points` (existing); aggregate outcomes.
  For failed points with a last run/result id, fetch the result detail
  (existing `get_result_detail`) with bounded concurrency to obtain the
  comment and, when the payload carries them, associated bug ids.
- HTML content (self-contained, dark-styled like the queue report):
  - headline: title, generated-at, total cases, **pass rate** over executed
    cases (unexecuted counted separately, not as failures);
  - an outcome bar (passed/failed/blocked/not-applicable/never-run counts);
  - a **flat case table sorted failures-first** (porting the title-grouping
    algorithm to Rust is deliberately avoided; grouping stays a UI concern);
  - failures section: case id/name, last comment, linked bug ids as
    `dev.azure.com` links when available.
- Wiremock test: aggregation counts + failures-first ordering via a testable
  inner `build_report_html(points, details) -> String` (assert on markers),
  temp-file writing tested like the existing HTML view.

### Triggers

- **Run Tests:** "Execution report" button beside *Open runner window* —
  passes the resolved plan id + the PBI's suite id, title = the PBI label.
- **Test Suites:** a "Report" chip on every suite and folder row (beside
  View/Edit); folders pass all descendant suite ids (same collection as the
  View action). Busy state shares the existing `busy` disable.

## Cross-cutting

- All new ADO calls are GET; the report writes a local temp file only.
- Tokens never cross IPC (unchanged); new commands take org/project like all
  others and go through `get_fresh_token`.
- Gates per feature: `cargo test` (wiremock + golden), `npx vitest run`,
  `npm run build`. One commit per feature (A, B, C), notes at release time.
- No release until the owner asks; expected next version: v1.5.0.

## Error handling summary

- History fetch failure → column shows dashes; no toast spam (single quiet
  error state in the header like other queries).
- Diff fetch failure → rows show "diff unavailable"; submit still allowed
  (the gate's purpose is information, not blocking).
- Report failures toast the message (same as View-in-browser today).

## Open items deliberately deferred

- Flaky-score/trend analytics beyond the 5 dots.
- History in Test Suites tables.
- Report screenshots; PDF export.
