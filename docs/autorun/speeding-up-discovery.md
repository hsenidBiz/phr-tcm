# Auto Run: speeding up discovery and script writing

Notes from writing the PBI 135592 (Performance Cycle wizard) scripts on
2026-10-09/10. Where the time goes, and what would cut it - app changes
first, then changes to how assistants work. Companion to `agent-pitfalls.md`
(mistakes), `agent-pitfalls-2.md` (run/browser problems) and
`backlog-2026-10.md`.

## Where the time goes

- **Round trips.** Every `discover_autorun_action` does one action, waits for
  the page to settle and returns the whole page as text, which the agent then
  reads before the next action. A 60-case page is several hundred of these.
- **Refused saves.** About 1 in 3 saves was refused on the first two pages
  (Cycle Setup scripts 13 ok / 9 refused; Evaluation Rules, first hour, 1 / 4
  scripts and 1 / 4 components). Each refusal means going back to the browser,
  seeing the locator, and saving again. Causes: `agent-pitfalls.md`, "Script
  and component saves refused".
- **One discovery browser.** Only one exists, so a whole PBI goes through one
  agent, page after page.
- **Sign-ins.** Every template run signs in afresh (10-30 s each).
- **Start-up.** Every agent reads the full guide (~1,500 lines) first.

## App changes (biggest gain first)

1. **Let reading a screen count as seeing it.** Make `get_autorun_page` (or a
   new "record this screen" action) mark every element it lists as seen,
   including an open dialog's contents. An agent could then visit each wizard
   step once and write all of that page's scripts from the record, carrying
   out live only steps where the order of actions matters. Today most live
   clicking exists only to get locators past the sighting check.
2. **Fix the sighting check** (`agent-pitfalls.md`): accept `{{fixture.*}}`,
   `{{setup.*}}`, `{{prefix}}`, `{{now:...}}` as wildcards; match a selector
   with `:checked` / `:not()` on its base; normalise dashes, spacing and case
   in names; exempt Test-file names/sizes and picked dates like typed text.
3. **Report every problem in one try.** A save now stops at the first unseen
   locator, so three problems cost three saves. Add a dry-run (or make the
   refusal list all of them), each with the closest seen locator ("did you
   mean ...").
4. **Batch actions.** `discover_autorun_actions` taking a list and answering
   only the final page (or a diff, or a page scoped to one container). Routine
   walks - open draft, click the Timeline tile, expand Notifications - become
   one call.
5. **More than one discovery browser,** one per account (e.g. evelyn and
   evelyn1), so one agent per page can work in parallel - roughly 2-3x faster
   for a PBI like 135592 (PeoplesHR allows one session per user, hence one
   browser per account).
6. **Open a record directly by id** in discovery and in scripts (a deep link
   or a built-in "open cycle" action), instead of search + pagination + Edit.
   Removes the "newest draft with this name" fragility that broke the shared
   draft when another case's setup made a newer one.
7. **Reuse a signed-in session for template and fixture runs**
   (`backlog-2026-10.md` item 17).

If only two are done: 1 and 2 - together most scripts become writable from one
look per screen with almost no refused saves.

## How assistants work (no app change)

8. **Map first, write second.** Walk each step once (all its dialogs), save
   components for the shared parts, then compose every script of the page from
   that, reusing locators already seen.
9. **Let the first unattended run be the check.** Cycle Setup's first run
   passed 43 of 49 and the 5 failures took one short heal - cheaper than
   walking every case end to end while writing it.
10. **A short guide for repeat work.** A one-page cheat sheet (the rules and the
    pitfalls) instead of the full guide each time.
