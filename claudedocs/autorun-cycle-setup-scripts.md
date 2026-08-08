# Auto Run scripts - Cycle Setup (PBI 135592)

52 scripts, 34 cases not scriptable, 86 Cycle Setup cases in total.

Import `autorun-cycle-setup-scripts.json` with Auto Run -> **Import scripts**.

## Preconditions

The app must be running at `https://localhost:7001` and you must sign in as
HR Admin in the browser window Auto Run opens, before running step 1. The
runner uses a fresh profile every time, so there is no saved session.

## Scripts

### 136448 - Cycle Setup - Admin can override evaluation method without clearing the configuration level selection
4 steps, 17 actions.

Precondition: signed in as HR Admin, app running on https://localhost:7001. Steps 1-2 deep-link to the wizard instead of walking the Performance Management menu; a human must confirm the menu path itself. The green border in the expected result is a colour assertion the runner cannot read - the script asserts the Basic card keeps class is-selected and its check-mark badge is no longer hidden instead. Basic = data-eval-method="90" (template ids are DB-generated).

### 136451 - Cycle Setup - Appraisal Guidelines section shows upload zone with drag-drop and browse options
4 steps, 17 actions.

Precondition: signed in as HR Admin. The 'a Configuration Level card is selected' precondition is set up inside step 4 by clicking the Standard card (data-eval-method="180"). The runner cannot drag-and-drop, so the script proves the dropzone and its drop/browse copy are rendered; actually dropping a file must be checked by hand. The max-size number in the meta line comes from the CycleSetupMaxFileBytes app setting, so only the fixed prefix is asserted.

### 136430 - Cycle Setup - Clicking Back in confirmation returns to cycle list without applying changes
4 steps, 21 actions.

Preconditions: signed in as HR Admin; at least one previous performance cycle must exist. Back is targeted positionally - the dialog footer appends the cancel button first and the confirm button second, and neither carries an id. The second half of the expected result ('no configuration has been copied', 'the Configuration Level section is unchanged') is an absence assertion the runner cannot make; a human must confirm no config card or method card lit up and no toast fired.

### 136429 - Cycle Setup - Clicking a cycle row shows inline confirmation message
4 steps, 20 actions.

Preconditions: signed in as HR Admin; at least one previous performance cycle must exist for the tenant, otherwise the list renders 'No previous cycles available to copy from.' and there is no row to click. The cycle-name portion of the message is data-dependent, so the fixed prefix and the fixed tail sentence are asserted separately. The case says the modal transitions to a confirmation view; the implementation opens a separate overlay appended to document.body - the script only asserts the confirmation message and the Confirm/Back labels are present, not that the list view went away (no negative assertion exists).

### 136431 - Cycle Setup - Confirming copy applies settings closes modal and shows success toast
4 steps, 21 actions.

Preconditions: signed in as HR Admin; at least one previous performance cycle must exist and it must have a source template, otherwise no Configuration Level card lights up. Confirm is targeted positionally (cancel is appended first, confirm second). The toast auto-hides after 5 s. The 'Configuration copied from' assertion is the case's expected wording; on a brand-new unsaved cycle the implementation instead emits 'Configuration from ... will be applied when the cycle is saved.', so this step is expected to surface that divergence. 'The modal closes' is an absence assertion and is not scripted. The card assertions only prove SOME card and method are selected - matching them to the source cycle needs its data, so confirm by hand.

### 136425 - Cycle Setup - Confirming template change re-applies new template and resets affected downstream data
5 steps, 17 actions.

Preconditions: signed in as HR Admin; a SAVED DRAFT cycle must exist that already has a Configuration Level applied and downstream evaluator/stage data configured, and its id must be substituted for REPLACE_WITH_DRAFT_CYCLE_ID in the step 2 URL - the script cannot run until that substitution is made. The draft must currently be on Basic or Standard, because step 4 clicks Advanced to force a different template; the confirmation only fires when switching to a different card on a draft whose saved data includes a source template. Step 1 opens the Manage list only to establish the entry point; step 2 deep-links to the draft because the wizard has no in-page resume control. 'Affected downstream step data (evaluators, stages) is reset' is server/other-step state - verify the Evaluators and Timeline steps by hand afterwards. 'The dialog closes' is an absence assertion and is not scripted. Confirm is targeted positionally; its visible label is 'Continue'.

### 136461 - Cycle Setup - Continue is disabled when configuration level is selected but required fields are empty
4 steps, 15 actions.

Precondition: signed in as HR Admin. Standard = data-eval-method="180". Start Date and End Date are never touched, which satisfies 'leave empty' - they could not be typed into anyway, the datepicker input is read-only. 'Visually disabled' (greyed out) cannot be read by the runner; the closest scriptable equivalent is the disabled attribute on #btn-continue-button, which the control sets whenever it is disabled. The shell only disables Previous on step 1, so this assertion is expected to fail if the gate is genuinely missing - which is what the case is for.

### 136460 - Cycle Setup - Continue is disabled when no configuration level is selected
4 steps, 19 actions.

Manual precondition: signed in as HR Admin. Evaluation Year is not touched - buildYearOptions preselects the current year on load. Dates are set by clicking calendar cells because the injected datepicker input is readOnly and registers no input/change listener, so fill cannot reach it; the popup's grid is only populated on open, hence the added wait for .phr-cal-day before clicking. First cell of the current month view and last cell of it guarantee End Date after Start Date (no min/max is configured on either picker). Description is not filled - it is optional and adds a failure point. Evaluation Method is deliberately left unselected too; the case is about the Configuration Level gate. The final assertion states the CASE's expected result, not the code's: cycle-setup.js never validates the Configuration Level (the string 'Please select a configuration level.' exists nowhere in the client) and updateFooterButtons only ever disables Previous on step 1, so #btn-continue-button will not carry a disabled attribute and this action is expected to time out. That timeout IS the finding. 'Visually disabled' is a colour judgement the runner cannot make.

### 136423 - Cycle Setup - Continue remains disabled after card selection when basic information fields are incomplete
4 steps, 15 actions.

Precondition: signed in as HR Admin on a fresh Create wizard so nothing is pre-populated. Advanced = data-eval-method="360". No field is filled; note the Cycle Name box is auto-populated with 'Annual Performance Cycle <year>' on a new cycle, so 'incomplete' here means the dates and method are unset, not that every field is blank. 'Remains disabled' is asserted as the disabled attribute on #btn-continue-button, since greying cannot be read. The shell only disables Previous on step 1, so expect this to fail if the gate is missing.

### 136463 - Cycle Setup - Continue with all required fields filled advances to Step 2
5 steps, 32 actions.

Preconditions: app running on https://localhost:7001 with its dev cert already accepted in the runner's browser profile; a live HR Admin session cookie from the legacy host (identity comes from forwarded cookies, there is no login form to drive); the perf_cycle_template seed rows present so a card with data-eval-method="180" exists. Case steps 1-3 are menu navigation (Performance Management -> cycle list -> Create); the runner deep-links to /PerformanceCycle?mode=Create instead, so step 2 asserts only the URL and that the shell rendered, not the list page. Dates are picked from the calendar (the datepicker input is readOnly and has no input/change listener, so fill cannot set it): start = today's cell, end = the first day of next month, which guarantees end > start in any run year. This script SAVES a real draft cycle to the database - delete the created draft after the run.

### 136426 - Cycle Setup - Copy from Previous button is visible in the Configuration Level section header
4 steps, 11 actions.

Preconditions: app on https://localhost:7001 with the dev cert accepted and a live HR Admin session cookie. No previous cycle is needed - the button renders whenever CanCopyFromPrevious is true, which Create mode always is. Case steps 1-3 are collapsed into a direct navigation to /PerformanceCycle?mode=Create. Partly covered: 'in the top-right area of the section header' is a layout claim the runner cannot read; the script proves only that the button exists inside the Configuration Level section box and carries the label text.

### 146308 - Cycle Setup - Copy from Previous button is visible when editing a draft cycle
4 steps, 12 actions.

Manual preconditions, both load-bearing: (1) at least one DRAFT cycle exists whose Cycle Setup step is NOT yet complete — in Edit mode ResolveResumeStepKey lands on the first incomplete step and the Edit URL carries no stepKey, and completion is written only by Save & Continue, so a draft left by a plain Save (e.g. the one case 136468 creates) resumes on Step 1 while a fully continued draft lands on Step 2 and step 3 fails its aria-current wait. (2) That draft must be the newest — the script clicks the first .phr-mc-card[data-is-draft="true"], and the list is ordered created_at_utc DESC, ten per page.

CHANGED from the earlier draft: the check_text "Cycle Setup" was removed. It also matches the step-nav tile label, so it would have passed even if the wizard had landed on Step 2; the aria-current wait_for is the real assertion. The claim that clicking Edit can leave localhost was also dropped — with no tenant context AppRootUrl resolves to "/", so BuildHostUrl produces a relative /PerformanceCycle?...&mode=Edit&mvc=1&digest=... URL. (Note the path keeps its capitals, which is why the check_url asserts mode=Edit and not a lowercase path.)

Step 4 asserts the button exists (host #btn-copy-from-previous renders its child as #btn-copy-from-previous-button) and reads "Copy from Previous". Its placement at the top right of the Configuration Level section is a visual property the runner cannot read — confirm by eye.

### 136427 - Cycle Setup - Copy from Previous modal opens with correct title and subtitle
4 steps, 14 actions.

Preconditions: app on https://localhost:7001 with the dev cert accepted and a live HR Admin session cookie. Works with any number of previous cycles - the modal is only shown after the ?handler=PreviousCycles GET resolves, hence the 15 s wait. Case steps 1-3 are collapsed into a direct navigation to /PerformanceCycle?mode=Create. Caveat: the modal markup is server-rendered and in the DOM while hidden, so both check_text lines would pass without the modal opening; the '#phr-cc-copy-modal.is-open' wait is what actually proves it opened.

### 136479 - Cycle Setup - Copy from Previous modal opens without crashing when no previous cycles exist
4 steps, 15 actions.

Preconditions: app on https://localhost:7001 with the dev cert accepted, a live HR Admin session cookie, and a tenant with ZERO previous performance cycles - that data state is the point of the case and must be arranged before the run. It conflicts with cases 136426/136427 if those are run in the same sitting. Case steps 1-3 are collapsed into a direct navigation to /PerformanceCycle?mode=Create. Partly covered: 'without a JavaScript error' cannot be asserted (no console access), so the script instead re-checks that the step-1 nav tile is still current after the modal opens. The empty-state wording is marked TBC in the case; the check_text uses the string the build actually renders, so change it if the agreed wording differs. Note the .phr-cc-prev-cycle-empty class is also on the server-rendered 'Loading previous cycles…' placeholder - the check_text is what distinguishes them.

### 136433 - Cycle Setup - Copy from Previous modal shows empty state when no previous cycles exist
4 steps, 13 actions.

Effectively a duplicate of 136479 - run only one of the two. Preconditions: app on https://localhost:7001 with the dev cert accepted, a live HR Admin session cookie, and a tenant with ZERO previous performance cycles; conflicts with 136426/136427 in the same run. Case steps 1-3 are collapsed into a direct navigation to /PerformanceCycle?mode=Create. The case does not state the empty-state wording, so the check_text quotes the string the build renders; adjust once the wording is agreed.

### 146314 - Cycle Setup - Copying from a previous cycle copies each stage's email notification settings
7 steps, 36 actions.

Manual preconditions: signed in as HR Admin; a DRAFT cycle whose Cycle Setup is already saved, and a source cycle whose FIRST timeline stage has notifications enabled with Employees ticked under Send To (the two assertions in step 7 are pinned to that). Steps 3-6 are the same corrected navigation as case 146313: the step tile plus its 'Leave this step?' confirm to reach Cycle Setup, and #cycle-step-content[aria-busy="false"] before each Continue so the click is not dropped while the shell is still loading. The notifications badge is an id-less <phr-button data-action="toggle-notif"> inside the cloned stage card, so its injected child is addressed as ... [data-action="toggle-notif"] button and the host itself gains .is-open. PHR.Switch and PHR.Checkbox render real <input type=checkbox> children and set .checked as a property, so input:checked matches - that is the one part of 'the settings were copied' the runner can actually assert. If the configured stage is not the first, re-point the selectors at #tl-stage-list [data-stage-id="<id>"]. The remaining Send To boxes and the notification triggers are checkbox states the tester still cross-checks against the source by eye.

### 146313 - Cycle Setup - Copying from a previous cycle copies timeline stages but leaves stage dates blank
8 steps, 34 actions.

Manual preconditions: signed in as HR Admin; a DRAFT cycle whose Cycle Setup is already saved (name, year, start/end date, method - Save & Continue needs all of them) and a source cycle with a fully configured timeline. Step 3 reaches Cycle Setup through the step tile plus its 'Leave this step?' confirm, because Edit resumes at the first incomplete step, not Step 1. Every step landing now waits on #cycle-step-content[aria-busy="false"] as well as the nav tile: renderShellState sets aria-current at the START of navigateTo, while the content is still a skeleton and isLoadingStep is true - a Continue click in that window is silently dropped by the shell's own guard, which is what would have hung the original chain. The footer's forward button is 'Save & Continue' (#btn-continue-button). Step 8's literal is what validateForContinue/getDateValidationError emit, routed through PmsShowAlertError into #phrApiClientErrorToast (auto-hides after 5 s, so read it promptly). Run step 8 before touching any date. validateForSave runs first, so a copy that produced a malformed stage set can surface a different message (e.g. 'Annual Performance Cycle stage is required.') - that is a different failure, not this one. Step 7 only proves stage rows rendered; stage order, weights, allocations and the emptiness of the date fields are input values the runner cannot read back.

### 146320 - Cycle Setup - Copying from a previous cycle reveals a step that was disabled on the target cycle
5 steps, 27 actions.

Manual preconditions: signed in as HR Admin; a DRAFT cycle with Competencies disabled and its Cycle Setup already saved (Copy from Previous renders only while cycle_status is draft; disabling Competencies means Evaluation Rules was completed, so the wizard resumes past Step 1); a source cycle with Competencies enabled available to copy from. Step 3 no longer asserts that Edit lands on Cycle Setup - ResolveInitialStepKey resumes at the first INCOMPLETE step, so it will not. It clicks the cycle_setup step tile instead; every step-nav move raises PHR.Confirm 'Leave this step?' whose confirm button is the .phr-btn-primary in .phr-modal-footer (confirm dialogs are removed from the DOM on close, so the selector can only match the live one). If the draft happens to resume ON Cycle Setup the tile click is a no-op and the confirm never appears - skip those three actions. text=Confirm was replaced throughout: the footer's forward button reads 'Save & Continue' and would be an ambiguous text match. Step 4 clicks the FIRST previous-cycle row (rows carry no id and no data-* attributes); use text=<source cycle name> to pin a specific source. The copy POSTs only because the draft already has a cycle id; on success it clears PHRCycleDataCache and calls PHRCycleShell.refreshSteps(), which re-renders #phr-shell-step-nav - that re-render is what step 5 waits for. Step 3's 'no Competencies step is listed' is an absence the runner cannot assert; the tester notes the step list by eye before step 4.

### 136477 - Cycle Setup - Cycle Name at exactly 200 characters is accepted
4 steps, 27 actions.

Preconditions: app on https://localhost:7001 with the dev cert accepted, a live HR Admin session cookie, and the perf_cycle_template seed rows present. Case steps 1-3 are collapsed into a direct navigation to /PerformanceCycle?mode=Create; the case's 'a Configuration Level card is selected' precondition is satisfied by the card click at the start of step 4, which also auto-selects the 180 method that Continue requires. Evaluation Year is left at its default (the current year is preselected on load), which satisfies the required-field gate. The value is the digits 0-9 repeated 20 times = exactly 200 characters; the DB column is NVARCHAR(200) and there is no server-side length rule, so 200 is the boundary. Partly covered: 'no validation error for Cycle Name' is an absence the runner cannot assert, so reaching Step 2 is the proxy. This script SAVES a real draft cycle - delete it after the run.

### 136436 - Cycle Setup - Cycle Name is required and shows error when cleared and Continue is clicked
4 steps, 19 actions.

Preconditions: app on https://localhost:7001 with the dev cert accepted, a live HR Admin session cookie, and the perf_cycle_template seed rows present. Case steps 1-3 are collapsed into a direct navigation to /PerformanceCycle?mode=Create; the 'a Configuration Level card is selected' precondition is met by the card click at the start of step 4. The field is pre-filled on load, so the fill with an empty value is what clears it. Divergence to record at run time: the case expects 'Cycle name is required.' but the markup's data-required-message has NO trailing period and is rendered as raw text, so the check_text asserts the string the build actually shows - flag the punctuation gap rather than treating it as a pass/fail on wording. Because the dates are deliberately left empty, a 'Start date is required.' error toast also appears alongside the inline name error; that is expected and does not affect the assertions. 'The wizard does not advance' is an absence, so it is checked positively via the still-current cycle_setup nav tile.

### 136465 - Cycle Setup - Cycle Name with only whitespace is treated as blank and shows required error
4 steps, 13 actions.

Precondition: a dev instance running on https://localhost:7001 with the remote-app auth host reachable; the Configuration Level card the case asks for is selected inside step 3. Cycle Name is pre-filled by the page with 'Annual Performance Cycle <year>' (cycle-setup.js:195-200), so the step-4 fill replaces a real value with three spaces. The assertion omits the trailing period on purpose: the control renders data-required-message verbatim and the markup value is 'Cycle name is required' with no period, while the case's expected text has one — raise that wording gap as a defect rather than letting punctuation fail the run. A second, unrelated 'Start date is required.' toast also appears on this Continue because the dates are untouched; it does not affect the inline assertion.

### 136440 - Cycle Setup - Description accepts up to 500 characters with character countdown
5 steps, 13 actions.

Precondition: dev instance on https://localhost:7001. The two fill values are exactly 500 and 501 characters. Step 5 is reliable regardless of how the runner delivers text: the control truncates any value longer than maxLength inside setValueInternal and writes the truncated value back to the textarea (phr.textarea.js:485-490), so the counter reads '500 / 500' whether the extra character is blocked by the maxlength attribute or by the control. The case says 'countdown' but the product renders a count-up 'used / limit' counter — '500 / 500' is the only readable indicator; flag the wording if the case expects 'characters remaining'.

### 136439 - Cycle Setup - Description is optional and blank value does not block Continue
4 steps, 18 actions.

Precondition: dev instance on https://localhost:7001, signed in as a user who can create a cycle — Save & Continue POSTs and CREATES A REAL DRAFT CYCLE, so clean it up after the run. The Standard card in step 3 also auto-selects the 180 method, so the method rule does not block Continue. Evaluation Year keeps the page default (current year) and Description is deliberately never touched. Dates are set without hardcoding a calendar month: Start uses the calendar footer's Today button (the footer's first button), End uses the Next month arrow then the first cell of that month, which is always after today. The datepicker input is read-only and has no input/change listener, so a fill cannot set a date. Partial coverage: 'no validation error is shown for Description' cannot be asserted directly (no negative assertion) — it is inferred from the wizard reaching Step 2. Step navigation is an AJAX swap that does not change the URL, hence the assertion on the step-nav tile's aria-current.

### 136444 - Cycle Setup - End Date before Start Date shows date range validation error
4 steps, 19 actions.

Precondition: dev instance on https://localhost:7001. Configuration Level selected in step 3; Cycle Name (pre-filled by the page) and Evaluation Year (current year) keep their defaults, so the date range is the only failing rule. Start is the 1st of next month (Next month arrow, then the first cell of that month) and End is today via the calendar footer's Today button — End is therefore always earlier than Start with no hardcoded dates. Neither datepicker has a min/max, so a past End date is selectable. The message is a toast that auto-hides after 5 seconds, so the check_text must run immediately after the wait_for. 'The wizard does not advance' is asserted positively via the Step 1 nav tile still carrying aria-current.

### 136443 - Cycle Setup - End Date equal to Start Date shows date range validation error
4 steps, 18 actions.

Precondition: dev instance on https://localhost:7001. Configuration Level selected in step 3; Cycle Name and Evaluation Year keep their page defaults. Both dates are set to today with the calendar footer's Today button (the footer's first button, present because data-show-today="true"), so Start and End are identical without hardcoding a month. The error is a 5-second auto-hiding toast — the check_text must follow the wait_for promptly. 'The wizard does not advance' is asserted via the Step 1 nav tile still carrying aria-current.

### 136442 - Cycle Setup - End Date is required and shows error when left empty
4 steps, 15 actions.

Precondition: dev instance on https://localhost:7001. Configuration Level selected in step 3; Cycle Name and Evaluation Year keep their page defaults, so the missing End Date is the first failing rule in the validator's order (year, start, end, range, method). Start Date is set with the calendar footer's Today button; End Date is never touched. The message is a 5-second auto-hiding toast — read it immediately after the wait_for. 'The wizard does not advance' is asserted via the Step 1 nav tile still carrying aria-current.

### 136478 - Cycle Setup - Evaluation Year boundary values are selectable
6 steps, 23 actions.

Manual preconditions: signed in as HR Admin; app at https://localhost:7001. Steps 1-3 reach Step 1 as in case 136464; step 3 also carries the case's precondition of a selected Configuration Level card.

Evaluation Year is a custom listbox, not a native select, so there is nothing to fill — click the toggle, then an option div. buildYearOptions() emits exactly 11 entries (currentYear-5 .. currentYear+5) followed by a hidden "No results found" node, so :nth-child(1) is the minimum and :nth-child(11) the maximum in any calendar year.

CHANGED from the earlier draft: the year literals "2021"/"2031" were dropped. They were only correct in 2026 and would silently rot. Selection is now asserted year-agnostically through updateSingleDisplay, which puts class is-selected (and aria-selected="true") on the matching option. Each assertion re-opens the panel first so the option is visible when it is read; open() does not re-render the list, so is-selected survives. Step 5 relies on the panel being left open by step 4, and step 6 closes it.

"without error" is a negative assertion the runner cannot make — confirm by eye that no red message appears in #cc-eval-year-error.

### 136437 - Cycle Setup - Evaluation Year defaults to current year and shows correct year range
5 steps, 13 actions.

Precondition: the dev app running on https://localhost:7001 with the tenant remote-app host reachable. Signing in is not required to reach or read this screen (no [Authorize] gate, no login redirect); it is only required for saving, which this case does not do. Case steps 1-2 (menu -> cycle list) are collapsed into one direct navigation; step 2 only re-checks the address bar. Steps 4-5 hardcode 2026 and the 2021/2031 endpoints (buildYearOptions is currentYear-5..currentYear+5) — re-stamp every January. The .has-value class on step 4 proves the select actually holds a selection rather than showing the caption, but check_text "2026" is still page-wide, so a tester should confirm by eye that it is the year dropdown reading 2026. Step 5 asserts only that the two endpoints exist; it cannot assert the count is exactly 11 nor that out-of-range years are absent.

### 136447 - Cycle Setup - Evaluation method is auto-selected when a configuration level card is chosen
6 steps, 15 actions.

Precondition: dev instance on https://localhost:7001, with the perf_cycle_template rows seeded — if a card selector is missing, the templates are absent in this environment. This case needs a brand-new cycle with nothing selected, so no card is pre-clicked. The config cards carry no id and are addressed by data-eval-method (Basic=90, Standard=180, Advanced=360); the method cards by data-method. 'Selected' is asserted through aria-pressed, which selectTemplate/selectMethod set on every card, rather than by reading colour or styling. 'Replacing the previous selection' is covered by waiting for the previously selected method card to carry aria-pressed="false" — a positive assertion of the deselected state. No confirmation dialog appears on these switches because that prompt only fires on a resumed draft.

### 136449 - Cycle Setup - Evaluation method is mandatory and shows error when bypassed
4 steps, 18 actions.

Precondition: dev instance on https://localhost:7001. WEAKER THAN THE CASE AS WRITTEN, deliberately: the case selects a Configuration Level and then clears the evaluation method with a form-manipulation tool. The runner has no DOM/devtools action and there is no UI gesture that deselects a method card, so the script reaches the same submit state from the other side — it fills Cycle Name, keeps the default Evaluation Year, sets a valid Start/End range (Today, then the 1st of next month) and never selects a Configuration Level or method. That covers 'method is mandatory' but NOT the 'cleared after having been selected' tamper path, which needs a manual devtools run. Leaving the Configuration Level unselected raises no earlier error because that field is not validated on this step. The message is a 5-second auto-hiding toast.

### 136428 - Cycle Setup - Modal lists previous cycles with name year badge and evaluation method badge
4 steps, 13 actions.

Precondition: at least one existing performance cycle for the tenant, otherwise renderPrevCycleList writes the "No previous cycles available to copy from." empty state and step 4 fails. The modal is only shown after ?handler=PreviousCycles resolves, hence the 10 s wait and the .is-open guard (the overlay is display:none until PHR.Modal adds is-open, so an unguarded selector could match a hidden node). Weaker than the case asks: the script proves each row carries a name element and a meta element, but the specific year and method depend on seeded data and must be read by the tester. The year/method are FontAwesome icon + text nodes inside p.phr-cc-prev-cycle-meta, not phr-badge elements — "badge" is spec wording, not a DOM fact.

### 136473 - Cycle Setup - Navigating back from Step 2 to Step 1 shows previously saved values
5 steps, 30 actions.

Preconditions: signed in — SaveCycleSetupCoreAsync throws UnauthorizedAccessException when ClaimsHelper.UserId is empty, so Save & Continue fails outright for an anonymous session; and a Standard (180) config template must exist in perf_cycle_template. The run creates a real draft named "Auto Run Cycle 136473" that someone must clean up. Dates: #cc-start-date-input is readOnly with no input/change listener, so fill can never set it — the dates are set by clicking day cells. The popup opens on the current month, so 2026-08-10 / 2026-08-20 only exist while the machine date is in August 2026; re-stamp them to the current month before running. Step 5's Previous click always raises the "Go to previous step?" PHR.Confirm — the script now dismisses it via the last button in the open .phr-modal footer (a text=Continue selector would be unsafe because the footer's "Save & Continue" also contains that word). Partly covered: only the Configuration Level card, the Evaluation Method card and the year text are assertable on return; Cycle Name, Start Date and End Date live in input values, which check_text cannot read — a tester must eyeball those three.

### 136412 - Cycle Setup - Page title and subtitle are visible on load
4 steps, 13 actions.

Precondition: the dev app running on https://localhost:7001. No sign-in needed — this case only reads the page. Case steps 1-2 (menu -> list page) are collapsed into the direct navigation; step 2 only re-checks the address bar. Step 4 asserts the strings are present as page text; it cannot judge visual prominence or placement.

### 136467 - Cycle Setup - Previous button is visible but disabled on Step 1
4 steps, 11 actions.

Precondition: the dev app running on https://localhost:7001; Step 1 of a new cycle. No sign-in needed for this read-only check. The footer host is an empty <phr-button> until the shell's init runs, hence the wait on the injected #btn-previous-button. "Visually muted" cannot be checked — the runner reads no colour or opacity; the script substitutes the real disabled state (Button.applyEnabledState sets button.disabled and toggles class is-disabled, phr.button.js:356-365). "Cannot be clicked" is inferred from the disabled attribute rather than attempted, since a click on a disabled button is a silent no-op the runner could not tell from a pass. check_text "Previous" is page-wide and is also satisfied by the "Copy from Previous" button, so it only proves the label text exists.

### 136480 - Cycle Setup - Resuming a draft cycle pre-populates all previously saved fields
4 steps, 12 actions.

Preconditions: a saved DRAFT cycle that already has Cycle Name, Evaluation Year, Start/End Date, Description, a Configuration Level and an Evaluation Method stored. Substitute the real id for cycleId=1 in step 1 — the placeholder lands on an empty form otherwise. &stepKey=cycle_setup is required: without it ResolveInitialStepKey resumes on the first incomplete step, not Step 1. Case steps 2-3 (list page, then opening the draft) are not driven — the script deep-links to the resume URL, so those steps only re-check the address bar and the Edit-mode heading. Partly covered: only the selected Configuration Level card, the selected Evaluation Method card and a non-empty year selection are assertable; Cycle Name, Start Date, End Date and Description are input values that check_text cannot read, so a tester must confirm those four by eye.

### 136472 - Cycle Setup - Returning to a cycle after Save and Exit resumes with previously saved values
5 steps, 36 actions.

Preconditions: signed in (SaveCycleSetupCoreAsync throws UnauthorizedAccessException on an empty ClaimsHelper.UserId, so an anonymous session cannot save), and a Standard (180) config template seeded. The run leaves a real draft named "Auto Run Draft 136472" that must be cleaned up. Three divergences the tester must accept. (1) There is no 'Save & Exit' button — the footer is Previous | Exit | Save | Save & Continue — so step 4 clicks Save, waits for the success toast, then Exit and confirms the dialog (confirm is the LAST button in .phr-modal-footer; text=Continue would also match "Save & Continue"). (2) Start/End dates are now filled even though the case does not name them: handleSaveClick runs the same non-inline validators as Continue, so a Save with empty dates raises "Start date is required." and never persists. Those hardcoded 2026-08-10 / 2026-08-20 day cells only exist while the machine date is in August 2026 — re-stamp them. (3) Step 5 cannot open the draft by clicking its list row: the card click handler bails unless the cycle is published (manage-cycles.js canPreviewCard), so the script uses the row's Edit action button, matched by its data-aria-label. That Edit URL is built through BuildHostUrl and is app-relative only while no tenant AppRootUrl is resolved; if the environment supplies one, the click leaves localhost and the script fails there. check_url uses lowercase performancecycle/manage because RouteOptions.LowercaseUrls is true and the exit URL comes from Url.Page. Partly covered on return: only the config card, the method card and the year text are assertable; Cycle Name and Description are input values check_text cannot read.

### 136476 - Cycle Setup - SQL injection string in Description is stored and displayed safely
5 steps, 28 actions.

Manual preconditions: signed in as HR Admin; app at https://localhost:7001. Step 3 carries the case's stated precondition (a Configuration Level card selected) and additionally fills Cycle Name, Evaluation Year and both dates — Save runs the same required-field gate as Continue, so without them step 5 would only raise a validation toast.

Step 4 cannot read the field back (fill writes, nothing reads), so acceptance is asserted through the textarea's own counter: the payload is exactly 39 characters and span.phr-textarea-counter renders `length + " / " + maxLength`, so it must read "39 / 500". Escaping or truncation on entry would change that count. This is a proxy, not the case's own wording.

Step 5 is only half driven. The "resume it and view the Description" half is MANUAL: the runner cannot read an input's value and the URL does not change on save, so it never learns the server-assigned cycle id. To finish by hand, open Manage Performance Cycles, edit this draft with &stepKey=cycle_setup, and confirm the Description shows the literal "'; DROP TABLE perf_performance_cycle;--" with no database error and the cycle list still populated.

### 136468 - Cycle Setup - Save with complete fields saves without validation and stays on Step 1
4 steps, 29 actions.

Manual preconditions: signed in as HR Admin; app at https://localhost:7001. Steps 1-3 reach Step 1 exactly as in case 136464 — see that case's notes for why the Manage page is the entry and why the create URL is navigated rather than clicked.

The success toast is waited for as a completion barrier only: without it the "still on Step 1" assertions would pass before the POST even lands, since Step 1 is already current. Its text is not asserted because the case marks the toast TBC; for reference the implementation shows "Cycle setup saved successfully." and it auto-hides after 5 s.

Two things remain manual: (1) "the step is not marked complete" is a negative assertion the runner cannot make — after Save, confirm the Step 1 tile still shows the number 1 and not a check-mark (the code only writes completion on Save & Continue, so this should hold). (2) DIVERGENCE: the title says Save persists "without validation", but handleSaveClick runs the identical gate as Continue (inline Cycle Name validate plus the first-error toast for year/dates/method). With complete fields both paths pass, so this script cannot distinguish them — a case with fields left blank would.

### 136469 - Cycle Setup - Save without configuration level selected saves without template validation
4 steps, 24 actions.

Preconditions: app on https://localhost:7001, browser session already signed in as HR Admin. This script WRITES a real cycle row (Save with no cycleId creates the cycle) - clean up afterwards.

PARTIAL COVERAGE: 'no Please select a configuration level error' is a negative assertion; the script instead proves the save completed (success toast) and the wizard stayed on Cycle Setup. No Configuration Level card is ever clicked, which is what the case requires.

Fixes applied to the supplied script: (1) the start/end date waits were on '.phr-cal-grid', which is created by Datepicker.render() at mount and is present-but-empty whether or not the popup is open (phr.datepicker.js:602) - replaced with '.phr-datepicker-wrap.is-open', the class open() actually adds (phr.datepicker.js:526). (2) '.phr-cal-today-btn' is NOT unique - render() creates two buttons with that exact class, 'Today' and 'Clear' (phr.datepicker.js:604-605) - replaced with the unambiguous '.phr-cal-day.is-today' cell (renderDays defaults viewYear to today when unset, so the cell always exists on first open). (3) '.phr-cal-nav ~ .phr-cal-nav' replaced with the aria-labelled next button (phr.datepicker.js:630). (4) Step 3's 'Step 1 of 9' check was dropped: the step count is 9 only when both goal-groups and competencies are enabled (BuildDefaultStepKeys, Index.cshtml.cs:362-379) and it is not this case's expected result - the active step-nav tile is asserted instead.

Cycle Name is pre-filled with 'Annual Performance Cycle <year>' in Create mode (cycle-setup.js:190-196); the fill overwrites it. Evaluation Year is not set because the select self-selects the current year at bind (cycle-setup.js:202-211).

### 136420 - Cycle Setup - Selecting Advanced card highlights it and auto-selects 360 degree evaluation method
4 steps, 12 actions.

Preconditions: signed in as HR Admin; fresh Create wizard.

PARTIAL COVERAGE: 'green border' is a colour assertion and out of scope - the script asserts the DOM expression of selection instead (is-selected + aria-pressed="true" on the card, and the check badge losing cs-hidden, per selectTemplate in cycle-setup.js:593-606).

The Advanced card is addressed by data-eval-method="360" because template ids are DB-seeded. Step 3's 'Step 1 of 9' check was removed - configuration-dependent and not this case's expected result. Note 'Multi-rater feedback' (the method card's sub-label) would also have matched a looser text check; the assertion uses the exact card name '360° Evaluation' from _CycleSetup.cshtml:214.

### 136417 - Cycle Setup - Selecting Basic card highlights it and auto-selects 90 degree evaluation method
4 steps, 12 actions.

Preconditions: signed in as HR Admin; fresh Create wizard.

PARTIAL COVERAGE: 'green border' is a colour assertion the runner cannot make - is-selected / aria-pressed plus the un-hidden check badge are asserted instead.

Basic is addressed by data-eval-method="90" because template ids are DB-seeded. Step 3's 'Step 1 of 9' check was removed as configuration-dependent and not this case's expected result.

### 136419 - Cycle Setup - Selecting Standard card highlights it and auto-selects 180 degree evaluation method
4 steps, 12 actions.

Preconditions: signed in as HR Admin; fresh Create wizard.

PARTIAL COVERAGE: 'green border' is a colour assertion the runner cannot make - is-selected / aria-pressed plus the un-hidden check badge are asserted instead.

Standard is addressed by data-eval-method="180" because template ids are DB-seeded. Step 3's 'Step 1 of 9' check was removed as configuration-dependent and not this case's expected result.

### 136422 - Cycle Setup - Selecting a card does not navigate away or show a toast
4 steps, 12 actions.

Preconditions: signed in as HR Admin; fresh Create wizard (no draft).

PARTIAL COVERAGE: 'no toast notification appears' is a negative assertion the runner cannot make - the script only proves the wizard did not navigate away (URL unchanged, Step 1 headings still rendered). A tester must eyeball that no toast pops.

The Standard card is addressed by data-eval-method="180" because the card ids are DB-seeded (perf_cycle_template) and not fixed in markup. Step 3's 'Step 1 of 9' check was removed - the 9-step count is configuration-dependent (BuildDefaultStepKeys) and is not this case's expected result.

### 136424 - Cycle Setup - Selecting a different template after downstream data exists shows a warning dialog
5 steps, 12 actions.

Preconditions: signed in as HR Admin AND a saved DRAFT cycle must exist whose Configuration Level is Standard with downstream evaluator/stage data configured. REPLACE cycleId=1 in step 1 with that draft's real id - the id is not knowable from source.

The warning only fires when state.isResumedDraft is true, which is set only when the hydration payload carries sourceTemplateId (cycle-setup.js:566-570). Step 4 therefore waits for the Standard card to come back highlighted before step 5 clicks Advanced. If the draft's level is not Standard, re-point step 4's wait and step 5's click (90=Basic, 180=Standard, 360=Advanced).

Fixes applied: (1) the supplied script navigated to ?mode=Create in step 1 and only reached the Edit URL in step 4, so its first three steps asserted a wizard the case is not about - the Edit URL is now the step-1 navigation. (2) 'text=Change Configuration Level?' replaced with the real confirm-dialog selector: PHR.Confirm appends div.phr-modal (cssClass phr-modal-danger) with h2.phr-modal-title to document.body (phr.confirm.js:69-133). (3) The 'Step 1 of 9' check was removed - in Edit the step list is per-cycle.

### 136441 - Cycle Setup - Start Date is required and shows error when left empty
4 steps, 16 actions.

Preconditions: signed in as HR Admin; fresh Create wizard.

Step 4 selects the Standard card and fills Cycle Name first: Cycle Name validates ahead of the date checks and Evaluation Year self-selects the current year, so the first failing message is the start-date one (validateNonInlineFields, cycle-setup.js:428-441). The forward button is labelled 'Save & Continue' and its clickable element is #btn-continue-button (host #btn-continue + '-button', phr.button.js:270-272).

PARTIAL COVERAGE: the case expects an INLINE error but the implementation delivers this message as a toast (phr.datepicker.js has no validation support at all). check_text only proves the string is somewhere on the page, so a tester must confirm placement. The toast auto-hides after 5000 ms, so the check must run straight after the wait_for.

'Wizard does not advance' is asserted only as the Cycle Setup heading still being present - step navigation is an AJAX swap and never changes the URL (Index.cshtml.cs:259-270). Step 3's 'Step 1 of 9' check was removed as configuration-dependent.

### 136464 - Cycle Setup - Step 1 tile shows completed state after successful Continue
5 steps, 28 actions.

Manual preconditions: signed in as HR Admin; app running at https://localhost:7001; the three seeded configuration templates exist (the script needs the Advanced card, data-eval-method="360").

Deliberate deviations: (a) Steps 1-2 use Manage Performance Cycles as the module entry — dev has no host main-menu panel. (b) Step 3 navigates straight to the create URL instead of clicking the list page's button: that button is labelled "New Cycle", and its click runs through manage-cycles.js navigateTo, which hands the URL to window.top.loadModulePageUrl when the page is hosted. (In dev AppRootUrl resolves to "/", so BuildHostUrl yields a relative URL and nothing leaves localhost — the earlier claim that clicking it leaves localhost is wrong.) (c) The footer forward button reads "Save & Continue"; #btn-continue-button is its injected child (phr-button renders host.id + "-button").

Eval Year option 6 of 11 is the current year (options are currentYear-5..currentYear+5), so the position selector is year-agnostic. Neither datepicker is given a min/max, so no day cell is disabled; the 1st and 28th of the displayed month always give end > start. Step 5 asserts the tile's circle span (.phr-dw-step-nav-circle.is-done, which swaps the number for a check-mark svg) — renderShellState never puts is-done on the button element itself.

### 136413 - Cycle Setup - Step progress bar shows Step 1 of 9 at 11 percent on first load
4 steps, 12 actions.

Preconditions: signed in as HR Admin, fresh Create (no draft resumed). The 9-step count requires BOTH the goal-groups and competencies steps to be enabled for the tenant - BuildDefaultStepKeys adds them conditionally (Index.cshtml.cs:362-379). If either is off the wizard shows 8 or 7 steps and the percentage changes, so this case must be run on an environment with both on.

PARTIAL COVERAGE: the literal 'Step 1 of 9 - 11%' is never one contiguous text run - the count lives in span.phr-progress-title and the percentage in span.phr-progress-pct (phr.progressbar.js:339-391) - so it is asserted as two checks plus the track's aria-valuenow="11" (set from the rounded percent, phr.progressbar.js:410-426). 'No step tiles are marked complete' is a negative assertion and cannot be scripted; the script asserts only that the Cycle Setup tile is the active one. A tester must eyeball that no tile shows a checkmark.

### 136421 - Cycle Setup - Switching template card removes previous highlight and only one card is active
4 steps, 16 actions.

Manual precondition: signed in as HR Admin. Step 1 substitutes a direct navigate for the main-menu click - the side menu belongs to the legacy host app. Step 3's list-page button is labelled 'New Cycle'; its URL is BuildHostUrl-wrapped but AppRootUrl is '/' with no tenant context, so the click stays on localhost and only adds mvc=1&digest. Added waits the original lacked: #cycle-step-content[aria-busy="false"] is the only signal that the shell's opaque hydration skeleton (an absolutely-positioned overlay over the step content) has lifted - clicking a card before that hits the overlay. Cards are keyed by data-eval-method (Basic=90, Standard=180, Advanced=360) because template ids are DB seed data. Card selection is a delegated document listener using closest('.phr-cc-config-card'), so clicking the name paragraph selects the card, and in Create mode isResumedDraft is false so no 'Change Configuration Level?' confirm fires. selectTemplate sets aria-pressed on EVERY card, and applyConfigTemplate then auto-selects the matching method card - the added 360 method assertion is a second positive witness that the switch took. 'Only one card has the green border and checkmark' is a colour/absence judgement the runner cannot make.

### 136415 - Cycle Setup - Three configuration level cards are displayed in order
9 steps, 39 actions.

Preconditions: signed in as HR Admin; fresh Create wizard; the three perf_cycle_template rows must be seeded - the card names, descriptions and feature bullets all come from that data, not from markup (scripts/Schema-SQLServer.sql:1006-1015).

FEATURE-BULLET LITERALS CORRECTED. The supplied script asserted 'Manager-assessment only (90°)' and 'Calibration Support' (twice). Neither string exists anywhere in the app: the seeded rows read 'Self-assessment only (90°)' for Basic, 'Moderation support' for Standard, and 'Calibration & Moderation' for Advanced, and the spec doc does not list bullets at all, so those literals were invented rather than quoted. The assertions now use the strings the page actually renders. If the test case genuinely specifies the other wording, that is a data/spec discrepancy for a human to raise - do not let the script fail on an invented literal.

PARTIAL COVERAGE: (a) left-to-right ORDER of the three cards cannot be asserted - the script only proves all three exist; a tester must confirm the order (it is driven by perf_cycle_template.sort_order). (b) Steps 6-8 must expand each card first because ul.phr-cc-config-features ships with class cs-features-hidden, and only one card can be expanded at a time (toggleConfigExpand, cycle-setup.js:617-634) - hence one expand per step. The clickable element is the injected button (data-css-class -> class phr-cc-config-toggle-btn on the child <button>, phr.button.js:270-302), not the phr-button host. (c) Step 9 checks the placeholder via an attribute selector because a placeholder is not page text; note the field is ALSO pre-filled with the same string as a real value in Create mode (cycle-setup.js:190-196), so the placeholder is never actually visible - a tester must confirm which of the two the case means. The year 2026 is hardcoded and must be updated if run in a different calendar year.

### 136446 - Cycle Setup - Three evaluation method cards are displayed
4 steps, 19 actions.

Preconditions: WebUI running at https://localhost:7001 with the dev certificate already trusted in the browser profile, and the remote-app host (RemoteAppUri) reachable so the HR Admin identity is forwarded. The page has no [Authorize] attribute and no login redirect, so an anonymous profile still renders Step 1.

Step mapping: the case's steps 1-2 (open the module from the menu, then the Performance Cycle list) cannot be driven — the Manage page's Create button URL is rewritten by BuildHostUrl to the tenant AppRootUrl with mvc=1 and a digest, which leaves localhost. Step 1 navigates straight to the Create-mode wizard and step 2 only re-asserts the address bar.

Deliberate failing assertion: step 4's last check uses the case's expected wording 'Multi-role feedback'. _CycleSetup.cshtml:215 renders 'Multi-rater feedback', so this check will fail until the case wording or the markup is corrected. It is left as the case states it and must not be changed to match the code.

The degree signs are U+00B0 and the dash in 'Step 1 of 9 — Cycle Setup' is U+2014.

### 136416 - Cycle Setup - View details toggle expands and collapses the feature list on each card
5 steps, 23 actions.

Preconditions: WebUI running at https://localhost:7001 with the dev certificate trusted, the remote-app host reachable so the HR Admin identity is forwarded, and the three perf_cycle_template seed rows present (Schema-SQLServer.sql:1005-1014) so the Configuration Level cards render.

Step mapping: the case's steps 1-2 cannot be driven through the menu or the Manage list (its Create button is rewritten to the tenant host URL), so step 1 navigates directly to the Create-mode wizard and step 2 only re-asserts the URL.

The Basic card is targeted by data-eval-method="90" because the cards carry no id and data-template-id is a DB identity value. If the seed is changed so Basic no longer maps to '90', substitute that card's data-template-id.

Partial coverage: step 5's 'label reverts to View details' is asserted page-wide, and the two collapsed cards always show that text, so the check_text alone proves nothing. The collapse is really proven by the card-scoped aria-expanded="false" and aria-hidden="true" waits, which only exist after a toggle has run.

The case title says 'each card' but its steps exercise only the Basic card; re-run against data-eval-method="180" and "360" if the title's full scope is wanted.

### 140891 - Cycle setup - Help icon available
5 steps, 9 actions.

Manual precondition: the signed-in user is in a security group eligible for PMS and cycle setup.

Steps 1-3 map the host main-menu path onto what exists in dev: there is no host menu panel at localhost:7001, so the script goes straight to the Performance Cycle module page and asserts it rendered. The header partial only emits the help link when Model.HelpLink is set, which the Manage page's OnGetAsync always does (settings value, else the "https://help.peopleshr.com/" default).

Step 4 flags a wording DIVERGENCE for the tester to judge: the case expects a help ICON, but _Header.cshtml renders a plain text link (p.phr-help-link > a) reading "Help". The script asserts what exists; whether an icon is required is a design call.

Step 5 clicks the link and asserts nothing — the anchor carries target="_blank", so the page opens in a NEW TAB that check_url cannot see. Confirm by eye that the new tab lands on the PMS performance cycle help page, and close it before running further scripts so the runner is not left on the wrong tab.

## Not scriptable

These need something the six actions cannot do. Run them by hand.

- **147769** Create cycle - Groups of fields are in a uniform sizes & alignments  
  Pure visual comparison - the case asks whether field groups are the same size and aligned across every step of the wizard. Size, spacing and alignment are outside the six actions, and the case spans all nine steps rather than Cycle Setup alone.
- **136432** Cycle Setup - Basic information fields are not overwritten when copying from a previous cycle  
  The expected result is that Cycle Name, Evaluation Year, Start Date, End Date and Description keep the values entered before the copy, which requires reading control values - the runner has no read-value action and check_text cannot see input, textarea or datepicker values.
- **136474** Cycle Setup - Continue button is visually distinct when disabled versus enabled  
  The whole case is a colour/opacity comparison the runner cannot read, and proving the enabled state requires asserting the absence of the disabled attribute plus setting Start and End Date, which can only be done through calendar day clicks.
- **136462** Cycle Setup - Continue with no template via form bypass returns configuration level error  
  The case requires editing or clearing a hidden field via developer tools before submitting, and the runner has no action that executes JavaScript or manipulates a hidden input, so the bypass cannot be set up at all.
- **147764** Cycle Setup - Copy Previous button copies all relevant tabs properly  
  Steps 5 through 13 are all 'cross check the copied field data' observations across later wizard steps, which needs reading current input values back - the runner's fill writes but nothing reads.
- **146310** Cycle Setup - Copy from Previous button is hidden on a published cycle  
  Same absence-only expected result as 146309. Worse here: a published cycle has every step complete, so Edit resumes on Preview and the Cycle Setup partial never renders at all unless the wizard is driven back to Step 1, and even then the assertion the case wants cannot be made.
- **146309** Cycle Setup - Copy from Previous button is hidden on an unpublished cycle  
  The entire expected result is an absence - no Copy from Previous button in the Configuration Level header - and the runner has no negative assertion. The submitted script's step 5 only anchored on the 'Configuration Level' heading, which is present whether or not the button renders, so it asserted nothing about the case and pretended an absence check was scripted.
- **146311** Cycle Setup - Copy from Previous button shows a spinner while the copy is running  
  Step 5 requires asserting the spinner has cleared, and the runner has no negative assertion; the spinner itself is a transient state that can finish before any wait_for observes it.
- **146319** Cycle Setup - Copying from a previous cycle applies the source cycle's Cycle Configurations toggles  
  The expected result is that every Cycle Configurations toggle matches the source, and which keys were moved off default is source-data-dependent, so there is no fixed positive assertion to make. The submitted script only proved #cfg-list rendered cards. It also carries the same seven-hop Continue chain as 146318, which cannot be driven end to end without manual input at intermediate steps.
- **146312** Cycle Setup - Copying from a previous cycle applies the source cycle's Evaluation Rules  
  Step 6 reads the toggle states, Goal/Competency Marks, Rating Method and Calculation Method values on Step 2 and compares them to the source cycle, and reading an input's current VALUE is a hard limit.
- **146316** Cycle Setup - Copying from a previous cycle applies the source cycle's competency profiles and weights  
  The verification reads area and competency weight input values on Step 5 and compares them to the source cycle, and reading an input's current VALUE is a hard limit.
- **146315** Cycle Setup - Copying from a previous cycle applies the source cycle's evaluator roles and weights  
  The verification reads the Self/Manager/Reviewer weight input values on Step 4 and compares them to another cycle's values, and reading an input's current VALUE is a hard limit.
- **146318** Cycle Setup - Copying from a previous cycle copies goal groups with their names limits weights and order  
  The expected result is that group names, Min/Max Goals, weights and row order match the source cycle - all input values the runner cannot read back (fill writes, nothing reads). The submitted script's only assertion was that #gg-groups-tbody had rows, which is true of any cycle. Reaching the step also needs a six-hop Save & Continue chain through Evaluation Rules, Timeline, Evaluators, Competencies and Participants, each with its own unmet validation, and the direct-navigate shortcut in the notes needs a cycle id the runner cannot obtain.
- **146317** Cycle Setup - Copying from a previous cycle copies participants with their manager and reviewer assignments  
  The verification compares participant, manager and reviewer names in the Participants grid against another cycle's data, which the runner cannot read from the source cycle or assert without knowing those names in advance.
- **136435** Cycle Setup - Cycle Name accepts up to 200 characters with character countdown  
  Neither expected result is observable: the runner cannot read the input's value to confirm 200 characters were kept, rejecting the 201st is an absence assertion, and the Cycle Name textbox renders no counter element at all (its footer hint is used only for the validation message).
- **136434** Cycle Setup - Cycle Name is pre-filled with default value on page load  
  The default name is set with control.setValue(), which writes only the input's value property (phr.textbox.js:478-482) and never a value attribute, so no CSS selector can match it and check_text cannot read input values - there is no action that reads a field's contents.
- **136466** Cycle Setup - Description exceeding 500 characters shows character limit error  
  The over-limit state is unreachable: the textarea control truncates any value longer than maxLength inside setValueInternal and writes the truncated text back to the DOM (phr.textarea.js:485-490) on top of the input's own maxlength=500 cap, so neither typing nor filling can exceed 500, the case's clipboard/browser-manipulation path is outside the runner's six actions, and the expected message 'Description must not exceed 500 characters.' exists nowhere in the client code.
- **136438** Cycle Setup - Evaluation Year is required and shows error when deselected  
  Step 4 needs the Evaluation Year cleared, but phr-select is a custom listbox with no clear affordance and no text input, so fill cannot reach it and clicking an option only re-selects — the required-year state cannot be produced with the six available actions.
- **136450** Cycle Setup - Evaluation method cards are read-only when cycle is active  
  Proving the method cards are non-interactive needs either a style/disabled read or a negative assertion that the selection did not change after a click, and the runner has neither; it also needs the hardcoded cycleId of a pre-existing active cycle to build the ?mode=Edit&restricted=true URL.
- **146603** Cycle Setup - Multiple files can be uploaded  
  Steps 9 and 10 require setting #cc-file-upload-input to multiple files and to an oversized file. phr-fileupload reads input.files on the change event, and a file input's files cannot be set by script - the runner has no upload or drag-and-drop action.
- **136414** Cycle Setup - No configuration level is pre-selected and Continue is disabled on load  
  Step 4's expected result is an absence assertion (no configuration card shows a highlight or checkmark) and the runner has no negative assertion; the remaining half is a visual judgement of the Continue button, which cannot be read either.
- **136458** Cycle Setup - Removing the attached file restores the upload zone  
  Step 4 must first attach a real PDF through the file input, which the runner cannot do, and the step 5 assertion that the drop zone is restored is an absence check.
- **136470** Cycle Setup - Save and Exit redirects to Manage Performance Cycle page with draft status  
  The control the case exercises does not exist, so there is nothing to script. The wizard footer holds exactly four hosts (Index.cshtml:56-63) rendering #btn-previous-button "Previous", #btn-exit-button "Exit", #btn-save-button "Save" and #btn-continue-button "Save & Continue"; Exit leaves WITHOUT saving after a confirm reading "Any unsaved changes on this step will be lost if you exit the wizard. Do you want to continue?". A server handler OnPostSaveAndExitAsync exists in Index.CycleSetup.cshtml.cs:90 but no client code ever calls it (grep for SaveAndExit across the wwwroot JS returns nothing for PerformanceCycle). The earlier draft scripted a wait_for on text=Save & Exit that was designed to time out; a script engineered to fail is not a test. Two further blockers even if the button existed: the draft assertion .phr-mc-card[data-is-draft="true"] matches any draft row and cannot identify the cycle just created, and the exit navigation lands on the server-built /PerformanceCycle/Manage URL whose capitals break a lowercase check_url. Reword the case as Save then Exit (two clicks plus the confirm's "Continue") or raise the missing control as a defect.
- **136471** Cycle Setup - Save and Exit with incomplete fields saves partial data and redirects  
  The case's expected result cannot be produced: Save runs the same required-field gate as Continue (cycle-setup.js handleSaveClick -> validateNonInlineFields), so with Start/End Date empty nothing is persisted and no redirect happens, leaving every later assertion unreachable — and the row's Draft status is row-scoped state the runner cannot assert with a page-wide check_text.
- **146321** Cycle Setup - Saving after a Copy from Previous does not reset the copied timeline  
  Step 5's expected result is that no timeline-reset message is displayed, which is a negative assertion, and step 6 reads the copied stage weight values, which is a hard limit.
- **136457** Cycle Setup - Uploading a double-extension file is rejected with invalid format error  
  Hard limit: the runner has no upload action and a file input's value cannot be set with fill, so malicious.exe.pdf can never reach #cc-file-upload-input or the dropzone.
- **136455** Cycle Setup - Uploading a file exceeding 10 MB shows size limit error  
  Hard limit: setting a file input and drag-and-drop are both outside the six actions, so an oversized file cannot be attached.
- **136459** Cycle Setup - Uploading a second file replaces the first attached file  
  Hard limit twice: no upload action exists to attach either file, and the expected result requires asserting file_a.pdf is absent, which the runner cannot do.
- **136453** Cycle Setup - Uploading a valid DOC file succeeds and shows filename with remove button  
  Hard limit: no upload action exists, so guidelines.doc cannot be selected through the file chooser or the dropzone.
- **136454** Cycle Setup - Uploading a valid DOCX file succeeds and shows filename with remove button  
  Hard limit: no upload action exists, so guidelines.docx cannot be selected through the file chooser or the dropzone.
- **136452** Cycle Setup - Uploading a valid PDF file succeeds and shows filename with remove button  
  Hard limit: no upload action exists, and the expected result also requires asserting the upload zone is replaced, an absence assertion.
- **136456** Cycle Setup - Uploading an unsupported file type shows format error  
  The case turns on attempting to upload an .xlsx or .png file, and the runner has no action that can set a file input.
- **136445** Cycle Setup - Valid End Date after Start Date clears the date validation error  
  Hard limit: the decisive expected result is that the 'End date must be after start date' error disappears, and there is no negative assertion — compounded by that error being a 5-second auto-hiding toast that vanishes whether or not the fix worked.
- **140890** Cycle setup - Check the Cycle setup menu item  
  Every step exercises the legacy host application's slide-out main menu, which is not served by this repo's pages at https://localhost:7001 and has no verified selectors in the source.
