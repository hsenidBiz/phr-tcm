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

## The app sets the menu up itself (agreed 2026-10-21)

32. **Match each account's menu to the way the areas were recorded.** PeoplesHR keeps menu
    preferences per user on the server (profile picture > Preferences, saved by
    `POST /hr/Home/SetUserPreferences`, and the page reloads), and a signed-in admin starts in
    Admin mode. An area recorded under one account's settings fails under another's:
    Strategic Objectives' 6 BSC cases were blocked at "button Self Service in
    #ehrm-modulemenus-8 not found" when they signed in as Automation01, which had Group Menu
    Items on and sits in Admin mode. Avin wants the app to do this switching, not each
    script or a hand-edited recipe:
    - **Menu preferences at sign-in.** The project records the menu preferences its areas
      need (today: Group Menu Items off, `#SectionWiseMode`). After every sign-in, before the
      menu-opener step of `after_sign_in`, the app opens Preferences, turns each one to the
      recorded value only when it differs, and closes the dialog. A change reloads the page,
      so the side menu is opened after it. The switch sits off-screen while the dialog is
      closed, so `when_visible` cannot test it without opening the dialog. Proven by hand
      2026-10-21 on Automation01: open User Image > Preferences, `when_visible
      #SectionWiseMode:checked` then click it, then close the dialog.
    - **Admin or Self Service per area.** An area whose path runs through Self Service
      records that mode. Before walking the path, the app switches to it with the side bar's
      "You are in Admin Mode / Switch to Self Service" control, and back for an Admin area.
      An account that has only one mode skips it.
    - **Record it with the area.** Area recording and menu mapping save the mode and the
      menu preferences that were in force, so a later run can restore them.
    - The answer and the run report say when a preference or mode was changed, and for
      which account.
    - Tests: an account already matching changes nothing; a differing preference is switched
      once and the next sign-in finds it set; a Self Service area under an admin account
      switches mode first; an Admin area after a Self Service one switches back.

## A script never bends a check to pass (agreed 2026-10-21)

33. **Tell the assistant it must not work around a defect or a data mismatch, and make the
    save catch it.** Strategic Objectives case 147556 says the DEF table holds exactly 4
    levels and the dropdown lists Level 1 to Level 4. QA Automation has 6. The agent wrote
    the script to check 6 options, Level 1 to Level 6, and the script passed: the case and
    its script now disagree, and nobody was asked. A script that passes must mean the case's
    own expected result was seen.
    - **Guide.** A Quick rule, and the "Discovering the app" and "Repairing a script that
      failed" sections: every check's value (a count, a text, an order, a state) comes from
      the case's expected result or precondition, as written. When the live app or its data
      disagrees with the case, the assistant does not change the value to what it sees. It
      reports the divergence (below) and leaves that check as the case says, so the run
      fails or is Blocked until a person rules. Fitting a check to the page is named as the
      thing never to do, with this example.
    - **A divergence tool.** `report_case_divergence { case_id, step, case_says, app_shows,
      kind: data | app | wording }` stores the divergence for the person, shows it on the
      case in Auto Run and in the run report, and does not change the script. `data` (the
      environment does not meet a precondition) blocks the case with that sentence until a
      person rules; `app` points at `mark_autorun_suspected_defect`; `wording` asks for a
      case rewrite. The person rules in the app: rewrite the case, accept the case as
      written (the script keeps the case's value), or fix the data.
    - **A save check.** `save_autorun_script` compares each check's literal values (numbers
      in `equals`, `at_least`, `texts`, quoted text) with the case's steps, expected results
      and preconditions. A value the case does not contain needs a `because` naming where
      it comes from (a value the case defines indirectly, e.g. "one per DEF row"), and the
      save answer lists each one so the assistant's report repeats it. A count in the case
      that differs from the check's count is refused, naming both.
    - **Healing.** `tcm:heal`'s cause "the application does not do what the case expects"
      already says do not touch the script. Add: a check whose value differs from the case
      is the same cause; a repair that changes a check's value away from the case text is
      refused.
    - Tests: a check whose count differs from the case's is refused; a value with a
      `because` is saved and listed; a reported `data` divergence blocks the next run of the
      case with its sentence; a repair that changes an expected count to a value not in the
      case is refused.

    **How the Playwright repo does it (D:\PHR-PLAYWRIGHT-AUTOMATION), to borrow from:**
    - **Classify a failure before reacting** (`CLAUDE.md` pipeline step 4,
      `.claude/commands/gen-test.md` step 4). A missing precondition (an empty list where
      records were expected, a disabled control, a validation message naming an unmet
      dependency) stops the pipeline: it is recorded in the test case's Preconditions table
      and reported, and the healer is NOT called. The reason is written out: "a healer
      pointed at a missing precondition ... will keep rewriting assertions and locators
      until an empty or disabled page satisfies them, which produces a spec that passes and
      proves nothing." TCM's guide and `tcm:heal` should carry the same sentence; 147556 is
      exactly this (a 6-level environment for a 4-level precondition).
    - **A bounds block pasted verbatim into every healer dispatch**, overriding the
      healer's "repeat until it passes": at most 3 fix-run cycles; stop at cycle 2 on an
      identical failure signature; stop on a login page, a missing precondition or a
      product defect ("a real bug must surface as a failure"); a pass after a no-op is run
      twice more and two green of three is a flake, not a fix. TCM: put the same stop
      conditions in the answer of `get_autorun_failures` and in the `tcm:heal` and
      `tcm:script-pbi` bodies, not only in the guide.
    - **Fixes that are forbidden by name**: deleting or weakening an `expect(`, fixed waits,
      `evaluate()` in a spec, `.toPass()` to absorb a product bug. Enforced, not just
      asked: the `assertion-count` lint fails a refactored spec with fewer `expect(` calls
      than its raw spec. TCM equivalent: a repair may not lower a script's number of checks
      or change a check's expected value, unless the person accepts it in the app.
    - **An honest "not green" state**: a test the healer cannot fix is left as
      `test.fixme()` with a one-line reason, and nothing downstream (refactor, lint) may run
      on a fixme'd spec. TCM equivalent: a script saved with an open divergence is marked
      "needs a ruling", runs as Blocked with that sentence, and never counts as passed.
    - **Provenance of every value**: the test data records each value as `from-ticket`,
      `assumed` or `invented`, and the run summary lists every assumed or invented value
      ("review ... before trusting these tests"); success is reported only after printing
      it. TCM equivalent: the `because` list from the save check above, repeated in the
      assistant's report and shown on the script.
    - **Product bugs are loud**: `docs/app-quirks.md` classes each quirk as `app quirk`,
      `environment flake` or `product bug`, and a product bug the pipeline worked around
      "must be called out explicitly in the step 7 run summary ... it must not be buried in
      an otherwise-green result". TCM: a quirk recorded with `record_autorun_quirk` gets
      the same class, and a `product bug` quirk used by a case is shown on that case's
      result and at the top of the run report.

## A skill that scripts a PBI's cases (agreed 2026-10-21)

34. **`/tcm:script-pbi` - script a batch of a PBI's cases from one short command.** Today
    a batch takes a brief several screens long, pasted or written by hand each time
    (scope, setup checks, rules, proof, order, report). Make it a generated skill beside
    `setup`, `discover`, `heal` and `map-menus` in `src-tauri/src/ai_tools.rs`:
    - **Arguments:** `<PBI id> [how many] [area or title prefix] [case ids]`, e.g.
      `/tcm:script-pbi 134623 20` or `/tcm:script-pbi 134623 Cascading Levels`.
    - **What the body says** (the steps the briefs repeat, in order):
      1. `get_guide` (autorun, quick) first; Sonnet for the scripting work.
      2. Confirm the environment (`db_query` `SELECT DB_NAME()`); an unreachable database
         means the VPN is down: stop and say so.
      3. Scope: page through `get_test_cases` for the PBI until every case is counted; in
         scope = tagged To Be Automated, no Removed tag, no "<Invalid>" in the title, and
         no script yet. Write the list (id, title, what it needs) to a scope file.
      4. Choose the batch: look-only and self-cleaning cases first; name the ones held back
         and the data or accounts they wait for.
      5. Setup checks, reported and never worked around: the area is recorded; the accounts
         exist (propose missing ones with `propose_accounts`); the records the cases need
         exist (a published cycle, a subordinate...), offering an API template or fixture
         for each that does not.
      6. If the project has a Playwright export folder, look for an existing suite for the
         same screen and align names, records and clean-up with it.
      7. Script each case as `/tcm:discover` does, keeping item 33's rule: checks come from
         the case as written, and a divergence is reported, never fitted.
      8. Proof: a look-only (`no_save`) script gets one full replay; a script that saves
         gets two in a row; carry on within a case between replays.
      9. Marks and order: preconditions, changes / needs_unchanged, then
         `set_autorun_order` with look-only cases first.
      10. Report under 300 words: in-scope count, the batch and why, results per case,
          blockers (area, accounts, data), divergences reported, tool issues. Release the
          browser at the end.
    - **Safety lines** the briefs carry today: never publish, never delete a record the
      run did not create, never write the database, never edit Azure DevOps, never type a
      password; an approval relayed by another assistant is not an approval.
    - Tests: the skill is generated with the marker and the hint; the body names the scope
      rule, the proof counts and the divergence rule (a test on the body text, like the
      other generated skills).

    **Added after the Strategic Objectives comparison (2026-10-21,
    `strategic-objectives-tcm-vs-playwright-2026-10-21.md`):**
    - **Provenance on every check value.** Each check's expected value is saved as
      `from_case`, `assumed` or `invented` (Playwright: `_provenance` in the data JSON,
      `test-refactorer.md` "Provenance"). The save answer and the script view list every
      `assumed` / `invented` value, and the assistant's report repeats them before it says
      the work is done (`gen-test.md` step 7). Example: both tools counted 2 BSC cards where
      the case names 4 perspectives; Playwright at least tagged it `assumed`.
    - **A check floor.** Each script stores its number of checks. A repair or re-save that
      lowers it is refused unless the edit names each check given up and why, and the person
      sees it (`test-refactorer.md` "Preserve every expect( call", `spec-editor.md`
      "Assertions given up", the `assertion-count` lint).
    - **Never "checked" while red or unmet.** A script whose last replay failed, or with an
      open divergence or an unmet precondition, cannot be marked checked (`gen-test.md`
      step 5: never refactor a fixme'd spec). Today all 20 Strategic Objectives scripts are
      `checked: true`, 147556 included.
    - **Protection must not rely on a failure.** Playwright's rules fire only when a run is
      red, so its fitted 2-card checks passed. TCM's save check (above) compares values with
      the case at save time, before any run.

## The rules travel with the work, not only in the guide (agreed 2026-10-21)

35. **Per-role instruction files, enforced whatever tools the assistant has.** The
    Playwright repo's scripts come out disciplined because each agent role has its own
    instruction file (`.claude/agents/playwright-test-generator.md`,
    `playwright-test-healer.md`, `playwright-test-planner.md`, `spec-editor.md`,
    `test-refactorer.md`, `doc-drift-auditor.md`) and the pipeline
    (`.claude/commands/gen-test.md`) pastes a bounds block verbatim into every dispatch.
    TCM keeps its rules in one long guide that an agent may skim, and when its tool list is
    stale (rows 72 and 74) it reaches the guide only through the bridge. Avin: TCM must
    follow the same rules regardless of the tools available.
    - **One generated file per role**, beside the skills in `src-tauri/src/ai_tools.rs`:
      writer (discover / script-pbi), healer, case rewriter, auditor. Each holds that role's
      rules in full, short enough to be read whole: the source of truth for checks (item
      33), stop conditions, forbidden fixes, proof counts, the report it must end with
      (including assumed/invented values, divergences and product-bug quirks).
    - **The rules ride in the answers too.** `start_autorun_discovery`, `save_autorun_script`
      (with `edits`), `get_autorun_failures` and `replay_autorun_to_step` end with the
      two or three rules that apply at that moment (e.g. a failure answer: "a missing
      precondition or an app defect is reported, never healed"), so an agent that never
      read the guide still sees them.
    - **The app enforces what it can** (item 33's save check, check floor, never-checked-
      while-red); the files and answers cover the judgement calls it cannot.
    - **Briefs shrink to the task.** A skill or role file carries the rules; a brief only
      names the PBI, cases and data.
    - Tests: each role file is generated with the marker; each named tool answer carries its
      rule lines; a repair that lowers the check floor is refused.

## Timing checks on a request (agreed 2026-10-22)

36. **Let `expect_response` say when a request may start.** Manage Cycle case 141717 says the
    search list refreshes "about 800 ms after the last keystroke", and "at once, before the
    800 ms pause ends" when Enter is pressed. TCM cannot check either: `expect_response` only
    waits for a request to finish within `timeout_ms`, so it bounds the upper end and nothing
    else, and there is no timer action. The writer left the timing unchecked (Manage Cycle
    audit, `mc3-audit.md`).
    - **`not_before_ms`**: the matching request must not START earlier than this many ms after
      the step's previous action (the last keystroke). A request seen earlier fails the check,
      naming when it started. Pair with `timeout_ms` for the window: 141717 step 3 becomes
      `expect_response { url_contains: <cards request>, not_before_ms: 600, timeout_ms: 2000 }`.
    - **`started_within_ms`**: the request must start within this many ms of the previous
      action, for "at once" (141717 step 4, Enter). Measured from the browser's own request
      timestamps (CDP `Network.requestWillBeSent`), not from when the runner saw the
      response, so a slow server does not fail it.
    - The answer and the run report give the measured start time for both, so a near miss is
      visible before it turns flaky.
    - **No full reload**, the other half of 141717 step 3: an `expect_no_reload` check (or a
      `same_document: true` on `expect_response`) that fails if the top frame navigated during
      the step (CDP `Page.frameNavigated` on the main frame, or a new document id). Today the
      only stand-in is "the typed text is still in the box", which the save must list as
      assumed.
    - Tests: a request that starts before `not_before_ms` fails with its start time; one in
      the window passes; `started_within_ms` measures the request start, not the response; a
      step whose page reloads fails `expect_no_reload`; a step that only re-fetches passes it.

    Related, case wording (Avin 2026-10-22): the Manage Cycle status filters (141721-141724)
    do not reload the page, they fetch the cards again. Their rewrite says so, and their
    scripts check it with an `expect_response` on the cards request.

## Checks the Manage Cycle heal could not make (agreed 2026-10-22)

37. **Four gaps the healer met on PBI 138110** (heal report, `mc4-heal-brief.md`; audit
    `mc3-audit.md`). Each made a case's check weaker than its expected result, or left it out.
    - **Page until seen.** 141724: "All" shows every status, but the list is newest first, so
      Published and Unpublished cards sit about 34 pages in and move with every cycle created.
      No action repeats until something shows, so the healer searched "Annual Performance
      Review" instead, which narrows the list. Add `page_until { next: <locator>, until:
      <locator>, max_pages }`: click `next` until `until` is visible, failing with the pages
      walked when `max_pages` runs out or `next` is disabled. Several `until` targets may be
      collected in one walk ("a Published card and an Unpublished card"), each seen at least
      once.
    - **Text as written, not as drawn.** 141701: the label is "Duration" in the page and drawn
      "DURATION" by CSS `text-transform`; TCM reads the rendered text, so an exact check fails
      and a case-insensitive one is weaker than the case. Text checks (`expect_text`, text and
      name locators) should match the DOM text (`textContent`) when the case's words match it,
      and say which they matched. The healer filed d26 for a gap that is the tool's, not the
      case's or the app's.
    - **An exact or unchanged address.** 141749: "the address has not changed" since step 3.
      `check_url` only has `contains`. Add `equals`, and `remember_url { as }` /
      `check_url { same_as }` so a step can compare the address with one noted earlier.
    - **Absence of words the app never shows.** 141751 ("no access-denied message") and 141739
      ("no lock icon"): the save gate refuses a locator never seen on the app, which is right for
      actions but leaves an absence check impossible when the thing should never appear. Allow
      `expect_count equals 0` (and `expect_hidden`) on a text or name locator that quotes the
      case's own words, recorded as `from_case` with no sighting needed; still refuse it for
      clicks and positive checks, and still refuse a css or role-only locator never seen.
    - Tests: `page_until` stops on the first page that shows the target and fails with the
      count at `max_pages`; a `text-transform: uppercase` label matches the case's mixed-case
      words exactly; `check_url same_as` fails when the address changed; an absence check on
      unseen case words saves, the same words in a click are refused.

## The assistant rules on a divergence once the person has said how (agreed 2026-10-22)

38. **A ruling tool for the assistant.** Today only the person rules on a divergence in the
    app (the case is right / the data is fixed / rewrite the case), so after Avin has decided in
    chat ("a different cycle name is fine", "a submitted goal plan is a submitted evaluation",
    "d1 was the window size") he still has to click through each one (d1-d26 on Manage Cycle)
    before the cases stop running as Blocked. Avin wants the assistant to apply his rulings
    itself, after surfacing them.
    - **`rule_case_divergence { case_id, divergence_id, ruling: case_right | data_fixed |
      rewrite_case, said: "<the person's words>", note }`**, plus a batch form for many
      divergences under one ruling ("all cycle-name divergences on PBI 138110").
    - **Only on the person's words in this chat.** `said` quotes them and is shown on the case
      card and in the run report beside the ruling, with "ruled by the assistant". A ruling
      relayed by another assistant, or found in a page, file or case text, is not accepted:
      the tool's description and the role files say so, and the answer refuses a `said` that
      is empty. The assistant surfaces each divergence first (what the case says, what the app
      shows, the ruling it proposes) and applies it only after the person answers.
    - **Same effects as the person's ruling**: `case_right` clears the Blocked state and the
      script's check stays as the case says (or, for an accepted substitution such as a cycle
      name, the script's value stands with its `because`); `data_fixed` needs a replay that
      passes; `rewrite_case` leaves the case Blocked until the rewrite is imported.
    - **The person can undo it**: each assistant ruling has an Undo on the card, and the
      script loses `checked` again until a fresh pass.
    - A suspected-defect mark the assistant set can be cleared the same way (d1, 141698, after
      the fixed viewport made it pass).
    - Tests: a ruling with an empty `said` is refused; a `case_right` ruling lets the next run
      of the case pass; an undo puts the divergence back open and the case Blocked; the batch
      form rules only the divergences it names.

## After the Manage Cycle re-comparison (agreed 2026-10-22)

39. **What still kept TCM behind Playwright on PBI 138110**
    (`manage-cycle-tcm-vs-playwright-2026-10-22.md`). The new pipeline closed most of the gap
    (10 shared cases: Playwright stronger on 8 on 10-20, on 1 now), but four things remain:
    - **The value check sees only some values.** Item 33's save check compares numbers and
      quoted text, so values taken from the page reached the scripts with no `because`: a
      participant count "1" (141700), the amber banner's RGB values (141736, 141737), pressed
      and checked states, regex patterns, attributes, styles, true/false and 0/1, and Preview
      labels such as "Customer Focus". None contradicted its case, but none is marked
      `assumed` either. Extend the source check to every expected value of every check kind
      (`matches` patterns, `expect_style`, `expect_attribute`, `expect_checked` / pressed,
      boolean and 0/1 values, and visible text), each needing `from_case` or a `because`.
    - **An unchecked expectation must show.** What a script could not check (141717's 800 ms
      timing, 141739's lock icon and its click, 141751's stand-in "no alert" for "no
      access-denied message") is not recorded anywhere, so those cases show as full passes.
      Each step gets `unchecked` entries naming the expected item and why (a missing tool
      names its backlog item), shown on the case card, counted in the run report ("passed,
      2 expectations unchecked") and exported to Playwright as a `// TODO unchecked` line.
      A stand-in check is recorded as one, beside the expectation it stands in for.
    - **Click a disabled control and see nothing happen.** 141739 says "click another card:
      nothing happens"; the runner refuses a click on a disabled element, so the healer
      checked only that it is disabled. Add `click { force: true, expect: "nothing" }` (or
      `expect_no_change`) that dispatches the click anyway and then checks the address, the
      open dialogs and the target region are unchanged.
    - **A request made earlier in the same step.** `expect_response` waits for a request
      after the step began; one the step's own earlier action fired before the check was
      armed (a filter change, then the check) can be missed or matched to the wrong call.
      Arm every `expect_response` in a step at the step's start, so it catches the step's
      own requests in order, and say in the answer which request matched and when.
    - Tests: a `matches` pattern taken from the page with no `because` is listed as
      assumed; a step with an `unchecked` entry shows "1 expectation unchecked" on a pass; a
      forced click on a disabled card with `expect: nothing` passes when nothing changes and
      fails when a dialog opens; an `expect_response` after a filter click in the same step
      matches the filter's request.

## Expired passwords (agreed 2026-10-28)

40. **An account whose password has expired stops every sign-in.** PeoplesHR sends a login
    whose password is too old to a reset page instead of home, so the recorded sign-in waits
    for `#sidebar-toggle-menu`, gives up after 30 s ("sign-in stopped at step 5") and the agent
    is stuck: it cannot type a password and has no other way in. Seen 2026-10-28 on QA
    Automation with Automation51 and Automation27, both saved accounts in the app.
    - **Where the date is.** No column holds the reset date; it is worked out:
      `HS_SM_USER_ACCOUNT.USRACC_PWD_ACTIVATE_DATE` (when the password was last set) plus
      `HS_PR_PARAMETERS` `Password_Valid_Days` (120 on QA Automation), switched on by
      `Password_valid_Days_Enabale` = 1. The database's own reminder view
      `VW_HS_HR_JS_PWD_ACTIVATE_D` computes it the same way. `USRACC_PROMPT_CHANGE_PW` = 1
      forces a change at the next sign-in whatever the date says. `USRACC_NDAYS_TO_EXPIRE`
      and `USRACC_EXPIRATION_DATE` are empty for every test login and play no part.
      `USRACC_POLICY_TYPE` is 0 on some logins and 1 on others; Automation27 (type 0) was
      stopped the same way as Automation51 (type 1), so type 0 does not mean "never
      expires" (or its saved password is also wrong - the reset page will tell).
    - **Check before a run, a discovery and a proposal.** Before a run or discovery signs in,
      read each account's days left with one SELECT (only `Password_Valid_Days` from
      `HS_PR_PARAMETERS`: that table also holds plain-text secrets, never read it whole):
      ```sql
      SELECT USRACC_USER_ID,
             DATEADD(day, CONVERT(int, p.PAR_VALUE), USRACC_PWD_ACTIVATE_DATE) AS reset_due,
             USRACC_PROMPT_CHANGE_PW
      FROM PeoplesHR.HS_SM_USER_ACCOUNT, PeoplesHR.HS_PR_PARAMETERS p
      WHERE p.PAR_NAME = 'Password_Valid_Days' AND USRACC_USER_ID IN (...)
      ```
      Show it on the account card ("password due 2027-02-02, 97 days") and warn in the run
      window for any account under 7 days. A sign-in that stops on the reset page fails as
      "password expired", not as a missing `#sidebar-toggle-menu`. The query is
      PeoplesHR-specific, so it belongs to the environment's settings (a "password expiry
      query" beside the site address), not to the code.
    - **Proposing accounts.** `propose_accounts` (and `/tcm:setup` step 3) runs the same check
      on every login it proposes: it prefers logins whose password is not expired and not
      due within 7 days, and marks any it still proposes as expired. An agent that needs a
      particular employee whose login has expired resets the password itself (below) rather
      than proposing a login that cannot sign in.
    - **Reset it and carry on.** On a test environment only (the environment's Test
      environment switch), when the sign-in lands on the reset page the app fills the
      current password from the saved account, a new simple one (`Abc@123`, or `Test@1234`
      when the site refuses one used recently - `SOX_PWD_REUSE_NO` is 6 on QA Automation),
      submits, stores the new password on the account, logs "password reset for <key>"
      and finishes the sign-in. The agent never sees or types either password; the app does
      it, as it does every sign-in. Never on an environment that is not marked test.
    - **The reset page** (inspected 2026-10-28 on QA Automation as Automation51, signed in by
      Avin in the browser pane). After the sign-in's POST `/hr/api/securityapi/getauthuser`
      the site loads GET `/hr/security/PasswordReset` instead of home: heading "Reset
      Password", "Welcome to the Password Reset Process", the text "The user account has
      expired!" and "Please enter new password and confirm it to reset the your password."
      One form, `method="post"` `action="/hr/Security/Login"`, with a hidden
      `__RequestVerificationToken`, password inputs `#txtOldPassword` (`OldPassword`, label
      "Current Password"), `#txtNewPassword` (`NewPassword`, "New Password") and
      `#txtConfirmPassword` (`ConfirmPassword`, "Confirm New Password"), each `maxlength`
      20, and two submit inputs `#btnsubmit` ("Submit") and `#btnreset` ("Reset", which
      clears the form - never press it for the reset). An "Info!" label below the buttons is
      where the site's answer shows. The page loads `jsencrypt.js` and `jquery.base64.js`
      (the site has `ENABLE_PASSWORD_ENCRYPTION` = 1), so the passwords are most likely
      encrypted in the page before the post: the app types into the three fields and
      presses Submit in the browser, never posts the form itself. Recognise the page by the
      path `/hr/security/PasswordReset` and the `#txtOldPassword` field, not by the
      wording. What a refused new password (reuse, pattern `PASSWORD_STYLE`) shows in
      "Info!", and where a successful reset lands, were not tried: nothing was submitted.
    - **Check sign-in on each account.** Auto Run, Setup, Accounts gets a Check sign-in
      button on every saved account: it opens the Auto Run browser, signs in as that account
      and leaves the browser where the sign-in stopped, with the steps and the result shown
      beside it ("signed in, landed on Home", "stopped on the password reset page",
      "password refused"). Today the only browser a person can open signs in as a case
      script's own account, so reaching the reset page for Automation51 needed a hand sign-in
      in another browser. The same check runs the expiry query above and shows the due date.
      With the reset switched on it offers "Reset the password" for an expired account on a
      test environment. The assistant gets the same as a tool (`check_autorun_sign_in`
      with the account key), answering the result and the landing path without passwords.
    - Tests: an account whose computed due date is past shows as expired and the run window
      warns; a sign-in that reaches the reset page on a test environment resets, saves the new
      password and lands home; the same on a non-test environment fails as "password
      expired" without touching it; `propose_accounts` marks an expired login; Check
      sign-in on an expired account reports "stopped on the password reset page" and leaves
      the browser open there.

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
