# Work Manager depth 2 — design

**Date:** 2026-07-18 · **Status:** approved direction (this doc), features land incrementally
**Area:** v2 Work Manager (`work_board/` Rust core, `WorkBoard.tsx` + drawer/modal UI)

## Goal

Make the Work Manager answer three questions it currently can't:
*what's the code status of my items* (PRs), *what belongs to this PBI*
(scope), and *what needs attention* (staleness / sprint focus) — without
leaving the board.

## Hard constraints (unchanged)

- **GET-only additions.** Every feature here is read-only against ADO; the
  client's no-DELETE invariant and the existing write surface are untouched.
- Token stays in Rust; new endpoints ride the existing `AdoClient` transport.
- All failures degrade soft (empty panel + muted notice), like suite
  detection and avatars do today.

## Features, in build order

### 1. Scope by PBI (small) — FIRST
The board's scope selector grows a third mode: **Me / Team / PBI**.
Choosing a PBI fetches everything parented under it plus the PBI itself:
`([System.Parent] = <id> OR [System.Id] = <id>)` in the existing WIQL,
replacing the @Me/team clause. Uses the same PbiPicker as everywhere else.

- Rust: `fetch_board` gains `pbi_id: Option<i32>`; team and pbi are
  mutually exclusive (pbi wins if both arrive).
- Caveat surfaced in UI: parent links only catch properly-parented items;
  the board header shows "N items under #id" so an unmaintained hierarchy
  is visible rather than silently sparse.
- Tests: wiremock assertion on the generated WIQL for the pbi mode.

### 2. Riders (nearly free, land with #1 or opportunistically)
- **Stale-item highlight:** cards untouched for 7+ days (from the already
  fetched `changed_date`) get a subtle warning-tinted left edge and a
  "stale 12d" title tooltip. Pure frontend.
- **Current-sprint filter:** a "This sprint" toggle adding
  `[System.IterationPath] = @CurrentIteration` to the WIQL (server-side
  macro, team-scoped: applies in Team mode; hidden in Me/PBI modes where
  the macro has no team context unless a team is also known). Column
  headers show item counts.

### 3. PR badges on board cards (medium)
ADO links PRs to work items as `ArtifactLink` relations
(`vstfs:///Git/PullRequestId/...`). New GET: for the visible board items,
resolve linked PR ids and their status via
`_apis/git/pullrequests/{id}` (batched, best-effort, cached per board
fetch). Cards with linked PRs show a chip: **PR ●** (active),
**PR ✓** (completed), **PR ✕** (abandoned). Failures = no chip.

### 4. Pull Requests panel (medium-large)
A tab/rail inside Work Manager with three groups, top to bottom by
actionability:
1. **Awaiting your review** — active PRs where you are a reviewer and your
   vote is 0 (`searchCriteria.reviewerId=me`, filter vote==0).
2. **Mine** — `searchCriteria.creatorId=me&status=active`, each with
   reviewer vote pips (approved green / waiting grey / rejected red) and
   merge-conflict flag.
3. **Active on \<repo\>** — repo picker (repos via `_apis/git/repositories`,
   choice persisted per project), all active PRs.
Rows open the PR in the browser (opener plugin). No PR writes of any kind
— voting/completing stays in ADO.

### 5. Parked (revisit after 3–4 ship)
- Swimlanes by parent PBI/Feature (visual twin of #1 — build if PBI scope
  gets real use).
- Mentions inbox (@you in comments) — needs per-item comment polling;
  cost/benefit unclear.
- Quick filter chips (type/assignee/tag, client-side) — anytime, low
  effort; grab as a rider when touching WorkBoard.

## API surface added (all GET)

| Endpoint | Used by |
|---|---|
| `wit/wiql` with `System.Parent` clause | #1 (existing endpoint, new clause) |
| `git/repositories` | #4 repo picker |
| `git/pullrequests?searchCriteria.*` | #4 lists |
| `git/pullrequests/{id}` (+ reviewers) | #3 badges, #4 detail |

## Testing

Each Rust addition gets wiremock coverage (WIQL shape for #1, PR list /
detail parsing and vote mapping for #3/#4). Frontend: vitest for scope
switching, badge rendering from mocked relations, and PR group ordering.
Demo-data mode grows fake PRs so the whole area is testable offline.
