# Guided Tour: the user drives — Implementation Plan

**Goal:** Four changes to the shipped tour (1.22.0), from walking it in the real app:
1. Scroll a ringed area into view when it is off screen, and keep the ring on it.
2. Keep the sidebar expanded for the duration.
3. Stop auto-navigating: the tour asks the user to click the tab, then continues there.
4. Only the tab the tour is asking for is clickable — no skipping ahead.

**Spec change this makes.** `docs/superpowers/specs/2026-09-02-guided-tour-design.md` says the app
is *fully* restricted while the tour runs and the tour drives itself. Points 3 and 4 replace that
with a **selective** lock: everything stays dead except the single control the current stop is
asking for. The guiding principle, and the tie-breaker for anything this plan does not name:

> **The tour never moves you; it asks you to move.**

So any stop that changes *where you are* — a tab, or the crossing into Work Manager — waits for the
user. Stops that only ring something on the screen you are already on keep their Next button. The
Settings stop is unaffected: it spotlights the gear, it never opens it.

**Consequence — Next disappears on a "go there" step.** If Next still advanced, the prompt to click
would be decorative. Back stays, and still navigates for you: forward is taught, backward is
convenience.

**Why `inert` cannot stay as-is.** `inert` is inherited, so a hole cannot be punched through it for
one button. The sidebar (and the Work Manager pill) must sit OUTSIDE the inert region and gate
themselves per row.

**Tech stack:** React 19 + TypeScript, Vitest + Testing Library, Tauri 2 (`src/bindings.ts` is
generated — never edit it).

## Global Constraints

- Colours from theme tokens only, never raw hex or Tailwind default-palette colours; no text under
  10px. `v2/src/ui-consistency.test.ts` enforces both by scanning source.
- Tour copy stays plain language; the jargon, title-length (34) and body-length (160) gates in
  `v2/src/tour/tourScript.test.ts` bind any new or changed copy and must not be weakened.
- **Nothing the tour does may persist.** The sidebar override is in-memory only: the user's own
  collapsed/expanded setting must be exactly as they left it when the tour ends.
- Never import from `src/dev/`. Never hand-edit `v2/src/bindings.ts`.
- Verify with `npx tsc --noEmit` and `npx vitest run` from `v2/`. Run vitest in the FOREGROUND
  (~4.5 min, under the 10-minute cap) and never at the same time as cargo.
- Commit messages use `feat(v2): …` / `fix(v2): …` and end with
  `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`. Commit with a Bash heredoc
  (`git commit -F - <<'MSG' … MSG`), never a PowerShell `-m` flag. Confirm with `git log -1`.
- `master` is green at 631/631 with `tsc` clean as of `d18b4e3`.

---

## Task 1: The sidebar stays expanded, without touching the user's setting

**Files:** `v2/src/lib/sidebarState.ts`, `v2/src/components/Sidebar.tsx`, `v2/src/App.tsx`,
tests in `v2/src/lib/sidebarState.test.ts` (create if absent) and `v2/src/components/Sidebar.test.tsx`.

`Sidebar` holds `collapsed` in its own `useState` seeded from `localStorage`, and only *publishes*
changes to `lib/sidebarState.ts` — it never reads back. So the tour cannot currently influence it.

- Make `Sidebar` read its collapsed state from the shared store
  (`useSyncExternalStore(subscribeSidebar, sidebarCollapsedSnapshot)`), with the toggle writing
  storage and publishing exactly as it does now. This is the point of the store already existing.
- Add an in-memory tour override to `sidebarState.ts` — `setTourExpanded(on: boolean)` — consulted
  by `sidebarCollapsedSnapshot`, notifying subscribers on change. Same shape as
  `workingDir.ts`'s tour override: in memory only, never written to storage.
- `App.tsx` turns it on in `startTour` and off in `endTour` (and in the unmount backstop beside the
  other releases).

**Tests:** the override wins over a stored "collapsed" and notifies; clearing restores the stored
value; the storage key is never written while the override is on; `Sidebar` renders expanded under
the override with "collapsed" in storage.

---

## Task 2: The ringed area scrolls into view, and the ring follows it

**Files:** `v2/src/tour/UiTour.tsx`, `v2/src/tour/UiTour.test.tsx`.

A stop whose anchor is below the fold currently rings an area the user cannot see (reported from
the app at stop 13, "Tools an assistant may use").

- When an anchor resolves, scroll it into view before measuring — `scrollIntoView({ block: "center" })`.
- The spotlight is `position: fixed` and measured in viewport coordinates, so scrolling moves the
  target out from under it. Add a scroll listener that re-measures, alongside the existing resize
  one — `{ capture: true, passive: true }`, since the app scrolls in an inner container, not the
  window. Tear both down together.
- Do not scroll for a stop with no anchor (the opening and closing cards).

**Tests:** `scrollIntoView` is called for an anchored stop and not for an unanchored one; a scroll
event re-measures the ring; both listeners are removed on unmount and on step change.

jsdom does not implement `scrollIntoView` or layout — `src/test-setup.ts` already stubs
`scrollIntoView`. Assert it was CALLED (spy on the prototype); do not assert positions.

---

## Task 3: The tour asks, the user navigates, and only that one control answers

**Files:** `v2/src/tour/tourScript.ts`, `v2/src/tour/UiTour.tsx`, `v2/src/App.tsx`,
`v2/src/components/Sidebar.tsx`, `v2/src/components/ContextBar.tsx`, plus their tests.

This is the behaviour change. Three parts, all needed together.

**a. The script knows which stops are a move.** A stop is a "go there" stop when its effective
destination (the carry-forward already computed in `UiTour`) differs from the previous stop's. That
is derived — do NOT add a hand-maintained field that can drift from `where`. Export a helper from
`tourScript.ts` so the overlay and App agree on one answer, and unit-test it against the real
`TOUR_STEPS`: exactly the stops that begin Import, Update, View, Run, Test Suites, AI Bridge and the
Work Manager board are moves; the rest are not.

**b. The overlay asks instead of advancing.** On a "go there" stop:
- `UiTour` does NOT call `onNavigate`. It shows the destination's name and asks for the click.
- **Next is hidden.** Back stays and still navigates.
- When the app arrives at that destination, the overlay advances to that stop's content by itself.
  App already knows where it is; pass the current location into `UiTour` so it can compare.
- The card copy must obey the existing gates. Say what to click, by the label the rail actually
  shows — read `CASE_ITEMS` in `Sidebar.tsx` for it, do not invent one.

**c. Only the awaited control is live.** The shell stays `inert`, but the sidebar and the context
bar's Work Manager pill move OUTSIDE the inert region, and gate themselves:
- While the tour runs, every rail row and the Work Manager pill is disabled EXCEPT the one the
  current stop is waiting for. A disabled row must not respond to a click or a keyboard press.
- Nothing else in the context bar becomes live: the org/project pickers, the PBI picker and the
  Settings gear stay locked.
- The tour's own keyboard guard stays as it is: Ctrl+1..8 and Ctrl+K remain dead throughout, so the
  shortcut cannot skip the sequence the click enforces.

**Tests:**
- From the Manual Entry stop, the Import File row is enabled and Update Test Cases is not; clicking
  Update does nothing and the tour does not advance.
- Clicking Import File advances the tour to the Import stop and the app is on Import File.
- Next is absent on a "go there" stop and present on the others.
- Back from a "go there" stop still returns to the previous stop and its tab.
- Ctrl+2 still does nothing while the tour runs.

---

## Task 4: Walk it, then ship 1.23.0

1.22.0 is already published, so this is 1.23.0. Manual walk first — this plan exists because the
last one shipped a navigation bug that 627 tests did not catch:

- The sidebar is expanded for the whole tour, and back to the user's own setting afterwards.
- Every ringed area is on screen when its card appears, including the AI Bridge stops that sit
  below the fold.
- The tour will not move on until the named tab is clicked, and no other tab responds.
- Back still works across the whole route.
- Skip at any point returns the app exactly as it was.

Then: changelog entry, bump `tauri.conf.json` and `Cargo.toml` (a test fails the gate if they
drift), full gates, push source, `npm run tauri build`, `scripts/pack.ps1 -Version 1.23.0`,
`vpk upload github … --publish --releaseName v1.23.0 --tag v1.23.0`. Check `gh release list` for a
free version first — the other machine publishes too.

---

## Carry-forward into Task 3 (found while doing Task 1)

Task 1's override keeps the sidebar expanded, but the sidebar's own collapse **toggle** is still a
live button. Today that is harmless: the whole shell is `inert`, so nothing in the rail can be
clicked. Task 3 moves the sidebar OUT of the inert region so the awaited tab can be clicked — which
makes that toggle reachable again. Clicking it mid-tour writes the user's real stored preference,
so the tour would change a setting it promised not to touch.

Task 3 must therefore disable the collapse toggle for the tour's duration, alongside the rail rows
it is already gating. It is the same rule stated once: while the tour runs, the ONLY live control
is the one the current stop is waiting for.
