# Auto Run and API Templates: improvements backlog

Agreed 2026-10-05, from the time lost writing and healing the Auto Run scripts
for PBI 135592 (Evaluators, Competencies, Participants). Each item says what
happened that motivates it.

## Auto Run

1. **Dismiss-if-present actions in scripts.** The sign-in recipe has
   `when_visible`; scripts do not. Cookie banners (137574), the "Another active
   session" prompt and the "Performance Method Changed" alert appear
   unpredictably, so a script either fails on them or cannot mention them.

2. **A read-only guard for scripts on shared drafts.** Most Competencies and
   Participants scripts use shared drafts that must never be saved. A script
   flag that fails the case the moment the page posts a save handler would stop
   one bad script from quietly corrupting the draft for every later case.

3. **Preconditions checked before step 1.** A script names the record it needs
   and the state it must be in (for example "Competencies assigned on FY2027
   Finance Performance Review"); the run checks it with the API flows' database
   checks and reports Blocked with the reason, instead of failing halfway
   through. Several scripts were written before their drafts existed.

4. **Retry the module path once on the first-case stall.** The first case of a
   run often stalls with the PMS submenu never loading ("New Cycle" not found;
   137515 again on 2026-10-04). Reload the start page and walk the path once
   more before Blocked. The page log then shows what was pending.

5. **Download checks.** `expect_download`: a file arrived with this name, and for
   .xlsx/.csv, these headers or this cell. Unblocks 137540 (template columns)
   and lets 137537 check the error log's messages, not only the response.

6. **Replay a case up to step N while healing.** Healing needs the supervised
   browser at the failing step; today the person walks there by hand. Replaying
   the case's own steps up to the failing one would let healing start at once.

7. **Re-run failed only, and retry transient failures.** A case that fails on
   a server refusal (empty 400s were common) is retried once and labelled
   transient rather than failed.

## API Templates

8. **Fixtures.** A saved, named, ordered list of template runs with values
   passed between them (seed tree, assign, save weights, continue...), plus
   "rebuild" for when a shared draft is damaged. The Competencies and
   Participants drafts were built by hand-written scripts; a fixture makes them
   reproducible on any machine.

9. **Script setup that runs a fixture.** Scripts that need their own draft spend
   about two minutes per run clicking through Copy from Previous and leave a
   draft behind each time. A declared setup that runs a fixture through the
   signed-in browser does it in seconds. It writes data, so each script's setup
   is shown to and approved by the person.

10. **Clean up test-made drafts.** By name prefix and age, through the proven
    delete handler, and only for cycles the tests created. Several dozen FY2027
    Procurement / Legal / Research / Workforce drafts exist now.

11. **Never run templates and Auto Run as the same account at once.** PeoplesHR
    allows one session per user; overlapping sign-ins cause empty 400s and
    dropped sessions. A lock that waits, or a warning.

## Next batch: new script capabilities (agreed 2026-10-06, after part 5)

12. **New tabs and popups.** A link or button that opens a new tab
    (reports, print previews, help pages) cannot be followed today. Add
    "expect a new tab" (with its address), "switch to tab" and "close tab",
    so a script can check the new tab and come back.

13. **Browser dialogs.** Every `alert`/`confirm` is accepted automatically
    today (`browser/cdp.rs`, `Page.javascriptDialogOpening`), so a script
    cannot check an "Are you sure?" message or test its Cancel path. Add
    "expect a dialog" with its text, then accept or dismiss it. A dialog no
    step expects is still accepted, as now.

14. **Table and grid checks.** Most screens are grids, and scripts can only
    count matches or look for text anywhere on the page. Add three checks:
    - a row whose column X is A and column Y is B;
    - a table sorted by a column;
    - a table with N rows after a filter.

15. **Page errors as a check.** JavaScript errors and failing requests are
    already captured in the page log (`browser/page_log.rs`) but never affect
    the result. Add a script option that fails or flags a case when the page
    throws, or a request returns a 5xx, during a step.

16. **PDF downloads.** Extend `expect_download` so it can check that a PDF
    contains a piece of text, for generated forms and reports, reusing the
    download pipeline from part 3.

## Smaller

- `save_autorun_script` through MCP drops `edits`, so repairs had to go through
  the bridge's `/autorun-script` route.
- `list_api_templates` returned about 210K characters; it needs a filter or paging.
- Iframe follow-ups deferred from the review of the iframe locators
  (`docs/superpowers/plans/2026-10-03-autorun-iframe-locators.md`): `frame_step` can
  build invalid CSS for an unusual iframe id or title; "the frame ... cannot reach"
  is not its own failure class in `autorun::patterns`; the click point is not
  re-centred after clipping through enclosing frames; the unreachable-frame
  sentence is recorded even when another frame at that step was entered; the
  frame check adds a protocol round trip per non-final match (unmeasured);
  snapshot frames deeper than 3 print with no note; `check_text` reads only the
  top document but the guide says "anywhere on the page".
- The cookie banner: see `cookie-banner-recipe.md` (a recipe change, no code).
