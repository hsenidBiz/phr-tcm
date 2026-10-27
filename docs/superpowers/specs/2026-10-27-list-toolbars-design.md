# One header and toolbar layout for the case and list screens

Owner, 2026-10-27, with screenshots of Update Test Cases, View Test Cases and Run Tests: "I'm not too happy about the small search bar as well as how the buttons are somewhat scattered everywhere." A mockup was shown in chat and accepted; then: "Apply it to all those screens as well, I will view it in the dev app before releasing."

## Intent

Every list screen reads the same way: what you are looking at and what you can do with all of it on one line, then how to narrow the list on the next. The search is wide enough to type in comfortably.

## The layout (decided)

1. **Header line.** Left: the screen's count or title (e.g. "630 test cases", "Run tests for #135592", "Test plans") with its refresh button right after it; an optional info icon whose tooltip carries a long explanation. Right (pushed to the end): every action that acts on the whole page. Nothing else on the line.
2. **Optional sub line** under the header for context that is not an action (Run Tests' plan name).
3. **Toolbar line.** The search box first, taking all free width (`flex-1`, a sensible minimum such as `min-w-64`, 40px tall, already the `SearchInput` height); then that screen's filters right beside it, each 40px tall so the line is level. On/off filters are a **switch in a bordered 40px chip** with its label (not a checkbox). The line wraps on a narrow window, the search shrinking before anything is hidden.
4. **Selection bars stay where they are** (the existing `ActionDock` bars for selected cases). Not part of this change.

## Screens and what moves where

- **Update Test Cases** (`src/screens/ExistingCases/index.tsx`): header = count + refresh. Toolbar = search + Group by title (switch chip).
- **View Test Cases** (`src/screens/ViewCases/index.tsx`): header = count + refresh; right: Export JSON (keeps its "Export N JSON" selection wording) and the browser button, relabelled **Open in browser** (keeps its selection wording pattern, e.g. "Open N in browser"). Toolbar = search + Group by title.
- **Run Tests** (`src/screens/RunPanel/index.tsx`): header = "Run tests for #N" + info icon (tooltip: today's "Outcomes are recorded through the runner window..." sentence, which leaves the page body); right: Set execution order (shown when there are points, as today) and Execution report. Sub line = Plan "..." + its refresh (Shift-click behaviour kept). Toolbar = search + All outcomes select + whatever sits there today.
- **Search Suites** (`src/screens/Suites.tsx`): header = "Test plans" + refresh, the "Collecting test cases" note after it. Toolbar = search.
- **Suite Management** (`src/screens/ManageCases/SuiteCases.tsx` and its screen): same pattern; its Group by title becomes the switch chip; its whole-suite buttons go to the header's right.
- **Work Manager** (`src/screens/WorkBoard.tsx`): header = title + refresh (+ the first-load "Loading pull requests" note); toolbar = search first and wide, then its filters (assignee, PBI picker, type, This sprint, Swimlanes, Open lanes only) as 40px controls; This sprint becomes a switch chip like the others.
- **Auto Run case list** (`src/screens/AutoRun/index.tsx` ~850 and `CaseSearch.tsx`): toolbar = search wide + its Group by title switch chip; page actions on the header line.

## Constraints

- Shared pieces, not seven hand copies: a `PageHeader` and a `ListToolbar` (plus the switch chip) in `src/components/ui/`, used by every screen above.
- Accessible names stay the same wherever the visible text stays the same (tests and How To Use locate by them). Group by title moves from checkbox to switch role; the browser button's name changes. Update tests and `docs-site` locators accordingly; screenshots are retaken in the final screenshot run, not now.
- Tokens only; icons from `src/lib/actionIcons.ts`; `src/ui-consistency.test.ts` untouched; no em or en dashes in copy.
- Stored preferences keep their keys (`tcm-v2-group-cases`, `tcm-v2-this-sprint`, ...).
- No release: the owner checks it in the dev app first. Work stays on branch `feat/list-toolbars`.

## Approaches

Shared header and toolbar components (chosen) vs. editing each screen's markup by hand. Hand edits drift apart again; the shared pieces are what makes "the same everywhere" hold.

## Left to the builder

Exact spacing; the info icon (an existing icon from the vocabulary, or add one there); how each screen's existing extra notes (scan progress, warnings) sit under the toolbar.
