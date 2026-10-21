# Auto Run and API Templates: improvements backlog

Agreed 2026-10-05, from the time lost writing and healing the Auto Run scripts
for PBI 135592 (Evaluators, Competencies, Participants). Each item says what
happened that motivates it.

## Auto Run

1. **Dismiss-if-present actions in scripts.** The sign-in recipe has
   `when_visible`; scripts do not. Cookie banners (137574), the "Another active
   session" prompt and the "Performance Method Changed" alert appear
   unpredictably, so a script either fails on them or cannot mention them.

   **Partly done.** Scripts have had `when_visible` for a while. Since
   2026-10-14 the cookie banner needs no script at all: the recipe's
   `close_when_shown` list names pop-ups the runner closes by itself before
   every click, fill, drag or upload, and the built-in recipe lists the
   banner (see `cookie-banner-recipe.md`). The other prompts are still
   guarded per script.

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

17. **Reuse a signed-in browser across template runs.** Every
    `run_api_template` / `prove_api_template` opens a fresh browser, replays
    the whole sign-in recipe (login page, password, home page, the
    "Another active session" bootbox, the menu) and loads the template's
    `antiforgery.page` before its requests - usually 10-30 s of setup for
    requests that take under a second. Building one draft cycle step by step
    (2026-10-09) meant 6-8 sign-ins. Fixtures already run their templates in
    one signed-in browser; give single runs the same: keep the browser and
    session open for a short idle window (per environment + account) so
    back-to-back runs skip the sign-in, re-reading the anti-forgery token
    only when the page differs, and dropping the session on an empty 400 or a
    sign-in page. PeoplesHR allows one session per user, so the held session
    must yield to a run or a person's browser on the same account.

## Discovery speed: validation refusals (agreed 2026-10-15)

While scripting PBI 135592 the assistants kept walking into the page's own
validation: a click "succeeds" (the button was clicked) while the page
refuses with a message, and the agent only finds out several calls later
when a wait times out or the next locator is missing - then reads the page,
works out the rule, and the next agent hits the same rule again.

18. **A validation refusal fails the discovery action.** After each click in
    `discover_autorun_action(s)` (and `try_autorun_action`), look for the
    page's refusal signals: field messages (`.invalid-feedback`,
    `.text-danger`, `[aria-invalid="true"]` with a message), an error toast or
    `alert-danger`, a SweetAlert/bootbox error, or a Save & Continue whose
    step indicator did not move. If one shows, answer the action as failed -
    "page refused: <message text>" - and stop the batch there, as a failed
    locator does. A replay or run step that ends on a refusal should say the
    same in its failure line instead of only the later timeout.

19. **Remember refusals per area.** Each refusal from 18 is stored with the
    area, the wizard step or page, the action that caused it and the message,
    deduplicated, and listed in the guide's quick live section ("known
    validation rules"). An agent then sees, before it starts, e.g. "Eval
    Rules: 'At least one assessment component must be enabled.' when Goals and
    Competencies are both off". Today this only happens when an agent records
    a quirk by hand, and quirks are capped at 40.

## Export to Playwright: hand over what TCM knows (agreed 2026-10-17)

Design: `docs/superpowers/specs/2026-10-17-pw-export-handoff-design.md`. The export adds
information in the slots the Playwright pipeline already reads; it never overrides the repo.

20. **Preconditions and setups (E1, E2).** Preconditions table rows from the script's
    preconditions, `needs_unchanged` marks and `needs_state` settings, ending "(from TCM)"; a
    re-export keeps the repo's own rows. Cases whose script has a setup are refused with a reason
    (they stay TCM-only). Shared-draft values (`{{fixture.<id>.<output>}}`) are substituted with
    the fixture's current output, with a row naming the draft and its fixture. Any other
    placeholder is refused, never written as literal text.
21. **Safety facts (E3, E10).** A TCM-owned `**From TCM:**` block per case: look-only, what it
    appears to write/delete, protected records, publishes, cleanup relied on (described, no
    `afterEach`), known product defects. The "Finish in the repo" prompt carries the summary.
22. **Protected baseline (E4).** A project setting of `{kind, name}` entries, edited in Auto Run
    Setup, exported as `docs/tcm-protected-baseline.md`.
23. **Quirks and page behaviour (E5, E6).** Stored quirks relevant to the exported cases, and the
    recipe's `close_when_shown` / `wait_while_shown`, appended to the repo's `docs/app-quirks.md`
    in its own format, tagged `<!-- tcm:q<id> -->`. Validation rules are not exported.
24. **Run order notes (E7).** Per md file: what must run before what, what must run alone and last;
    `--chain` as a prose suggestion; `@destructive` / `@publishes` tags proposed to the repo owner.
25. **Markers (E8, E9).** `**User:**` provenance `assumed` instead of `from-tcm`; `**Uses:**` from a
    TCM component → common-component mapping.
26. **Export warnings (E11).** Before writing: unnamed deletes, publishes, state conflicts, not run
    twice in a row, quirks attached.

## Replays without re-signing in (agreed 2026-10-18)

27. **Stop releasing the browser between replays.** Assistants call `release_autorun_browser`
    and then `replay_autorun_to_step`, which opens a fresh browser and replays the whole sign-in
    (login, password, the "Another active session" bootbox, the menu) - 10-30 s every time,
    many times per healing session. Causes and fixes:
    - **`no_save` is sticky per browser**: once a no_save replay/discovery runs, every later save
      in that browser is blocked until release, so the rules tell agents to release between a
      no_save replay and a saving one. Make `no_save` a per-call setting that resets with the call.
    - **Replays always start fresh**: if the Auto Run browser is already signed in as the same
      account and environment, a replay should return to the case's area through its menu path
      (as `return_to_area` does) instead of opening a new browser and signing in; start fresh only
      when the session has dropped, the account differs, or the page is broken. (Same idea as
      item 17 for template runs.)
    - **A cheap reset**: `reset_autorun_browser` - go home, clear the save guard and any open
      dialog, keep the session - for when an assistant wants a clean start without a sign-in.
    - **Discovery and replay share one browser**, so moving from exploring to replaying does not
      sign in again.
    - **Guide**: release only when the browser is broken (frozen, wrong account, session gone),
      never as a routine step between replays.

## Assistants can change a script's preconditions (agreed 2026-10-18)

28. **Let `save_autorun_script` edit preconditions.** *Built in 2.2.3-beta.17.* A repair (`edits`) can change steps but not a
    script's preconditions, so when the data model changed (Playwright leaves Definition Wizard
    profile assignments empty; scripts now assign a temporary Step 4 profile themselves) the
    precondition "A profile has a competency assigned" blocked 135057, 135621 and 135581 before
    step 1, and only a person could remove it in the script editor. Add a precondition edit to
    repairs (add / remove / change one precondition, with a one-line reason), shown in the save
    answer and in the script's history like other repairs. Guardrails: a removed precondition must
    be replaced by steps that create that state (or the reason must say why it is no longer
    needed); the repair cap counts it; a person can still see and revert it in the editor.

## Full request and response capture (agreed 2026-10-18)

29. **Keep the page's requests and responses whole during Auto Run, for the assistant.** *Built in 2.2.3-beta.14.* Today
    TCM keeps each request's method, path and status, and reads a body only for a refused request
    or an `expect_response`. Keep, for the page's own XHR/fetch requests (same site, not polls),
    the request body and the response body (capped), the status and the headers that matter
    (content type, not cookies or authorization), with secrets redacted as `shown_body` does
    (passwords, tokens, session values). A tool lets the assistant list them and read one in
    full. Purpose (Avin): give the assistant more to go on, and let it build **API templates**
    from the real requests it sees while discovering a new area, instead of guessing their
    shape. Not exported; not a separate stealth browser (rejected 2026-10-18: it would bypass
    TCM's save guard, seen check and session rules).

    **Scope widened (Avin, 2026-10-18): TCM's own versions of stealth-browser-mcp's 13 network and
    cookie tools**, to make the assistant more capable at edge cases: list, request details,
    response details, response content, search, export and import of captured network data, read
    and set capture filters, modify request headers, get / set / clear cookies. Guards:
    - header changes go through the save guard's own Fetch interceptor, the guard deciding first
      (a blocked save stays blocked); they last for the call or discovery that set them and every
      answer names them;
    - `set_cookie` never touches the sign-in session's cookies (no switching who is signed in);
    - `clear_cookies` is a real sign-out: the session records it, and the next replay signs in;
    - import loads a capture for reading and searching only, never replayed into the browser;
    - cookie values of session/auth cookies and secret body fields are never shown.

## Element inspection (agreed 2026-10-18)

30. **Read-only element inspection for the assistant** *Built in 2.2.3-beta.14.*, from stealth-browser-mcp's "Element
    Extraction" group, TCM's own versions on its CDP driver, for diagnosing a step and checking
    edge cases (Avin: make the assistant more capable):
    - **computed styles** of an element (CSS `getComputedStyle` / `CSS.getComputedStyleForNode`),
      e.g. to confirm a field turned red, a button looks disabled, text is hidden; and a script
      check `expect_style {property, equals}` built on it;
    - **event listeners** on an element and on the element covering it
      (`DOMDebugger.getEventListeners`), for "why did my click do nothing";
    - **animations and transitions** running on an element (computed transition/animation, the
      Web Animations list), to explain "still moving" failures and the wait they need;
    - **one element's structure**: attributes, children, text and which frame it is in, beside the
      page read TCM already has.
    Not taken: assets, related CSS/JS files and complete element cloning - they serve copying a
    design, not testing, and would pull large files into the assistant's context.

## Carry a replay on across cases that share their first steps (agreed 2026-10-21)

31. **A shared-steps checkpoint between cases.** Agents prove scripts by replaying them, and
    cases on one screen often open with the same steps (Strategic Objectives' Goal Cascading
    cases: open the page, select "Common Automation", open Add Goal). A replay already carries
    on within one case ("Carried on from step K"), but `carry_from` in
    `src-tauri/src/autorun/replay_to.rs` requires the same case id, so case B replays steps
    1-3 that case A has just run. Change:
    - **Match on the steps, not the case.** Case B may carry on from where case A's replay
      stands before step K+1 when both have the same account, area, preconditions, setup and
      save mode (`no_save` or not), and steps 1 to K are identical in every action
      (`run_fingerprint` of the prefix), with the browser still on that page and the stand
      fresh. The answer says "Carried on from step K of case A (the same first K steps)".
    - **Only a prefix that saves nothing.** A stand whose steps 1 to K made a write cannot be
      shared: case B would start from data case A changed.
    - **Back to the checkpoint.** After case B's own steps run, the page is no longer where
      step K left it. Keep the checkpoint for case C only when B ended back on it: the same
      address and nothing B opened still open (its last step is a clean-up that closes the
      dialog it opened, as the look-only cases do). Otherwise C replays the prefix from the
      start.
    - **The proof stays whole.** This only saves an assistant's replays while writing and
      healing. A person's run still plays every case from its sign-in, so every case keeps
      its end-to-end proof; the guide says so.
    - Tests: two cases sharing 3 steps carry on; a differing action, account, area, save mode
      or precondition starts from the beginning; a prefix that wrote data is never shared; a
      case that ends off the checkpoint loses it for the next case.

    Already changed without the app (2026-10-21): the agent briefs prove a look-only
    (`no_save`) script with one full replay instead of two; a script that saves keeps two in a
    row for repeatability.

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
