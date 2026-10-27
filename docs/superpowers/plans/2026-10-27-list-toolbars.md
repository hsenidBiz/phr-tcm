# List toolbars Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every case and list screen uses one header line (title or count, refresh, page actions on the right) and one toolbar line (a wide search, then filters), as the spec describes.

**Architecture:** Three shared presentational pieces in `src/components/ui/` (`PageHeader`, `ListToolbar`, `ToolbarSwitch`), then each screen swaps its hand-built rows for them. Behaviour (queries, handlers, stored preferences) does not change.

**Tech Stack:** React 19 + TypeScript, Tailwind tokens, vitest + Testing Library.

**Spec:** `docs/superpowers/specs/2026-10-27-list-toolbars-design.md`

## Global Constraints

- Tokens only, icons only from `src/lib/actionIcons.ts`, `src/ui-consistency.test.ts` passes untouched, never weaken a test or a docs guard.
- Accessible names unchanged where the visible text is unchanged. Group by title / This sprint become `role="switch"` with the same names. The View Test Cases browser button becomes "Open in browser" ("Open N in browser" with a selection).
- Stored preference keys unchanged.
- No em or en dashes in copy. Never name hidden features. CRLF sources: Edit/Write tools, never sed.
- No release. Work stays on `feat/list-toolbars`.

## Review Focus

1. A narrow window (about 900px wide app content): the toolbar wraps, the search shrinks first, no control is cut off or overlaps (class contract: `flex flex-wrap`, search `flex-1 min-w-64`, filters `shrink-0`).
2. A selection on Update / View Test Cases: the selection bar still appears and Export JSON / Open in browser still switch to their "N" wording.
3. Run Tests with no points yet: the header shows no Set execution order (as today) and the info tooltip is still reachable by keyboard.
4. Work Manager's stored filters (`tcm-v2-this-sprint`, swimlanes, open lanes) still load and save through the switch chips.
5. How To Use: any control that locates a changed role or name is updated so `npx vitest run docs-site` passes after `npm run docs:build`.

---

### Task 1: Shared header and toolbar, used by Update and View Test Cases

**Files:** Create `src/components/ui/page-header.tsx`, `src/components/ui/list-toolbar.tsx` (exports `ListToolbar` and `ToolbarSwitch`), their tests beside them (`*.test.tsx`, the pattern `search-input.test.tsx` uses). Modify `src/screens/ExistingCases/index.tsx`, `src/screens/ViewCases/index.tsx` and their tests.

**Interfaces (produced):**
- `PageHeader(props: { title: ReactNode; titleAs?: "h2" | "h3"; onRefresh?: () => void; refreshing?: boolean; refreshLabel?: string; info?: string; aside?: ReactNode; actions?: ReactNode; children?: ReactNode })` - one `flex flex-wrap items-center gap-2` line: heading, refresh icon button (`aria-label={refreshLabel ?? "Refresh"}`, spins while `refreshing`; screens that need Shift-click or their own handler pass their own button in `aside` instead), info icon button whose tooltip and `aria-label` carry `info` (use the app's `tooltip.tsx`), `aside` (small notes), then `actions` pushed right (`ml-auto flex gap-2`). `children` renders as the sub line below.
- `ListToolbar(props: { search: ReactNode; children?: ReactNode })` - `flex flex-wrap items-center gap-2`; the search slot wrapper is `flex-1 min-w-64`; children are `shrink-0`.
- `ToolbarSwitch(props: { checked: boolean; onCheckedChange: (v: boolean) => void; label: string; title?: string })` - a 40px (`h-10`) bordered chip (`rounded-md border border-border px-3 text-sm`) holding the existing `Switch` (`ariaLabel={label}`) and the visible label.
- Screens pass `SearchInput` with `className="w-full"` into `search`.

- [ ] Tests first for the three pieces (structure classes above, refresh label and spin class, info button name and tooltip text, actions rendered in the right group, ToolbarSwitch toggles and has role switch named by `label`). Fail, implement, pass.
- [ ] Update Test Cases and View Test Cases use them as the spec says (View: Export JSON and Open in browser as `actions`). Update their tests (checkbox -> switch for Group by title; the browser button's new name). Run `npx vitest run src/components/ui src/screens/ExistingCases.test.tsx src/screens/ViewCases.test.tsx src/screens/ExistingCases src/screens/ViewCases src/ui-consistency.test.ts` and `npx tsc --noEmit`.
- [ ] Commit `feat(v2): one header and toolbar for the case lists, with a wide search`.

### Task 2: Run Tests, Search Suites and Suite Management

**Files:** `src/screens/RunPanel/index.tsx`, `src/screens/Suites.tsx`, `src/screens/ManageCases/SuiteCases.tsx` (and the screen that frames it), their tests.

- [ ] Run Tests: header "Run tests for #N" with `info` = the "Outcomes are recorded through the runner window..." sentence (removed from the body), actions = Set execution order (only when points exist, as today, keeping its disabled/title logic) + Execution report; sub line = Plan "..." with its own refresh button in the sub line (Shift-click re-detect kept); toolbar = search + All outcomes + the controls already on that row.
- [ ] Search Suites: header "Test plans" + its refresh (disabled rule kept) + the "Collecting test cases" note as `aside`; toolbar = search.
- [ ] Suite Management: header + whole-suite buttons as actions; toolbar = its search (if any) + Group by title as `ToolbarSwitch`.
- [ ] Update tests; run `npx vitest run src/screens/RunPanel src/screens/RunPanel.test.tsx src/screens/Suites.test.tsx src/screens/ManageCases src/ui-consistency.test.ts`, `npx tsc --noEmit`. Commit `feat(v2): Run Tests, Search Suites and Suite Management use the shared header and toolbar`.

### Task 3: Work Manager, the Auto Run case list, and How To Use

**Files:** `src/screens/WorkBoard.tsx`, `src/screens/AutoRun/index.tsx` (~850), `src/screens/AutoRun/CaseSearch.tsx`, tests; `docs-site/src/content/*.ts` locators; `src-tauri/help/` (built).

- [ ] Work Manager: header = title + refresh (+ "Loading pull requests" as `aside`); toolbar = search wide, then assignee, PBI picker, type, This sprint / Swimlanes / Open lanes only as `ToolbarSwitch` (titles kept, "(N done hidden)" kept beside Open lanes only), all 40px.
- [ ] Auto Run case list: search wide + Group by title chip; page actions on the header line.
- [ ] How To Use: Grep `docs-site/src/content` for locators naming the changed roles/names (Group by title checkbox, This sprint, View All Test Cases in Browser, the Run Tests sentence) and update them and their prose; `npm run docs:build`; `npx vitest run docs-site`. If a guard fails only because a box must be recaptured, stop and report it (do not fake boxes).
- [ ] Run `npx vitest run src/screens/WorkBoard.test.tsx src/screens/AutoRun src/screens/AutoRun.test.tsx src/ui-consistency.test.ts`, `npx tsc --noEmit`. Commit `feat(v2): Work Manager and the Auto Run case list use the shared header and toolbar`.
