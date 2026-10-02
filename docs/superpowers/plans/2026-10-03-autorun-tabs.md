# Auto Run tabs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the Auto Run screen into Test cases, Past runs and Setup tabs, with a readiness strip and a More menu, keeping every feature.

**Architecture:** This is a webview-only change. `src/screens/AutoRun/index.tsx` gains a tab state and renders one tab panel at a time. Two new focused components carry the new pieces:
- `ReadinessStrip.tsx`, a pure view fed by the readiness numbers the screen already loads;
- `useAutoRunReadiness.ts`, a hook that derives "essential missing" and "missing test files" from existing queries.

The Setup rows move unchanged into a `SetupTab` section. PastRuns moves to its own tab at full width. There are no Rust changes.

**Tech Stack:** React 19, TypeScript, TanStack Query, vitest + Testing Library (jsdom).

**Spec:** docs/superpowers/specs/2026-10-03-autorun-tabs-design.md

## Global Constraints

- Never weaken `src/ui-consistency.test.ts` or `src/a11y.test.tsx`. Use colour tokens only, and icons from `src/lib/actionIcons.ts` (named for what they do).
- Never hand-edit `src/bindings.ts`. This plan needs no Rust change; if one turns out to be needed, stop and report it.
- Auto Run is hidden: no changelog, help-site or README edits, and never mention a secret unlock.
- Keep every existing feature and accessible name unless the spec moves it. A test that depended on the old layout is updated to switch tabs first, never deleted.
- Run one test command at a time. Commit with a Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. Readiness data can load in any order (or fail). The opening tab must not flip after the first decision, and must not open on Setup because data is merely still loading.
2. A failed readiness query (site, accounts, test files) shows its existing error text in the strip or row. It must not count as "missing" in a way that hides the cases.
3. Keyboard: arrow keys move between tabs, and focus stays sensible when a tab's panel unmounts (for example, a dialog opened from Setup closing).
4. Review closing lands on Past runs even when the review opened from the run dialog while Test cases was showing.
5. The More menu must keep Clear scripts disabled when no case has a script, as it is today.

---

### Task 1: Tab shell, opening rule, Past runs and Setup tabs

**Files:**
- Create: `src/screens/AutoRun/useAutoRunReadiness.ts`
- Modify: `src/screens/AutoRun/index.tsx` (the root grid ~377, the Setup section ~398-575, `<PastRuns>` ~723, the review close ~873)
- Test: `src/screens/AutoRun.test.tsx` (the screen's existing test file)

**Interfaces:**
- Produces:
  - `useAutoRunReadiness(input: { siteUrl: string | null | undefined; signIn: "saved" | "builtin" | "none" | null; accountCount: number | null; areaCount: number | null; scripts: (CaseScript | null | undefined)[]; testFileNames: string[] | null }): { loaded: boolean; essentialMissing: boolean; missingTestFiles: string[] }`
  - `type AutoRunTab = "cases" | "runs" | "setup"`

- [ ] **Step 1: Write the failing tests.**
  - With no site address, the tab named `Setup` has `aria-selected="true"` once loaded. Its accessible name includes `needs attention`.
  - With a site address, a sign-in and one account, `Test cases` is selected.
  - While the readiness queries are pending, no tab switch happens after the first decision. Clicking `Past runs`, then letting a pending query resolve, leaves `Past runs` selected.
  - `Past runs` shows the existing PastRuns content (its filter row `Filter by result`).
  - Closing a review selects `Past runs`.
  - The Setup tab shows all five rows (the existing accessible names `Edit site address`, `Edit accounts`, `Edit areas`, `Manage test files`, and the sign-in buttons), plus the environment line naming the database as `<label>: <database> on <server>`.
  - Existing tests that click Setup or Past runs controls switch tabs first.
- [ ] **Step 2:** Run `npx vitest run src/screens/AutoRun.test.tsx`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - Follow the tab markup and keyboard handling used by `src/screens/ApiTemplates/index.tsx`. Use one `role="tablist"` labelled `Auto Run sections`.
  - The opening tab is decided once `loaded` is true, and stored in state. Only the person's clicks and the review-close change it afterwards.
  - The `/tcm:setup` note is one line of muted text on the Setup tab.
  - Drop the xl two-column grid.
- [ ] **Step 4:** Run `npx vitest run src/screens/AutoRun.test.tsx`, then `npx vitest run src/screens/AutoRun`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(v2): Auto Run splits into Test cases, Past runs and Setup tabs`.

### Task 2: Readiness strip and More menu

**Files:**
- Create: `src/screens/AutoRun/ReadinessStrip.tsx`
- Modify: `src/screens/AutoRun/index.tsx` (the header line ~381-396, the Test cases toolbar ~582-640)
- Modify: `src/lib/actionIcons.ts`, only if a needed icon is missing
- Test: `src/screens/AutoRun.test.tsx`

**Interfaces:**
- Consumes: `useAutoRunReadiness` and `AutoRunTab` (Task 1).
- Produces: `ReadinessStrip(props: { envName: string | null; siteHost: string | null; signIn: "saved" | "builtin" | "none" | null; accountCount: number | null; areaCount: number | null; testFileCount: number | null; missingTestFiles: string[]; onOpenSetup: () => void })`

- [ ] **Step 1: Write the failing tests.**
  - The strip shows the environment and host.
  - It shows `Built-in` when signIn is builtin, the account and area counts, and `1 test file missing` when a script's `upload` names a file that is not in the list.
  - `Open setup` selects the Setup tab.
  - The old header line is gone.
  - A `More` button opens a menu with `Import scripts` (and its description line) and `Clear scripts`.
  - Clear scripts is disabled when no case has a script, and still opens the existing confirm.
  - `Group by title` stays visible outside the menu.
- [ ] **Step 2:** Run `npx vitest run src/screens/AutoRun.test.tsx`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - The menu follows whatever menu or popover component the app already uses (grep `role="menu"` or a Menu component in `src/components`). Do not add a dependency.
  - Ticks and warnings use the success and warning tokens.
  - Missing test files come from `scripts[].steps[].actions` of kind `upload`, compared against the Test files list names.
- [ ] **Step 4:** Run `npx vitest run src/screens/AutoRun`, `npx tsc --noEmit`, `npm test`, and `cd src-tauri && cargo test --tests`, one at a time. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(v2): Auto Run cases tab opens with a readiness strip and keeps rare actions in More`.
