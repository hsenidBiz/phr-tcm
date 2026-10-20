# Manage Performance Cycles (PBI 138110): TCM Auto Run vs the Playwright repo

Date: 2026-10-20. Read-only comparison; nothing was changed except this file.
Focus: (1) locators, (2) what each tool checks to decide the expected outcome happened.

## 1. Summary

**What exists today.** The TCM side has all 37 in-scope scripts (`case-141698` to `case-142514`). The Playwright
repo, after Avin moved the old suite out, currently has **only 5 of the 37 cases regenerated**: 141698, 141699,
141700, 141701 and 141702 (spec, page object, data and test-cases md under
`suites/sl/admin/performance/PC-manage-cycle`, raw specs in `suites/_generated/manage-performance-cycles-*.spec.ts`).
`suites/_generated/index.json` maps exactly those 5 ids. The new md lists only those 5 headings. So the like-for-like
comparison is 5 cases; for the other 32 the only Playwright counterpart is the superseded backup in
`D:\PHR-PLAYWRIGHT-AUTOMATION-backups\manage-cycle-2026-10-20`, compared in section 4 as an indicative secondary result.

**Verdict counts**

| Basis | Equivalent | TCM stronger | Playwright stronger | Different | Total |
|-------|-----------:|-------------:|--------------------:|----------:|------:|
| Current Playwright (5 cases present in both) | 0 | 0 | 4 | 1 | 5 |
| Superseded backup Playwright (the other 32, indicative) | 23 | 6 | 0 | 3 | 32 |
| Cases with no Playwright counterpart now | | | | | 32 |

**Headline findings**

1. On the 5 cases both tools cover, the new Playwright specs are stronger: they loop over every card, check the
   Plus icon and the top-right position, read subtitle and footer text in a pattern, and check two known cycles'
   exact subtitle and dates. TCM's versions are thinner (counts, first card only, `nth 0`).
2. The 5 new Playwright specs hard-code `FY2026 Annual Performance Review` and `FY2025 Annual Performance Review`
   plus expected subtitle/date strings that the data file marks `assumed`/`derived`. They declare no preconditions.
   TCM scripts for the same cycles declare `pms-performance-cycle` preconditions (10750, 10763); TCM 141701 uses a
   different cycle (FY2027 ... (Oct), 11171) and declares nothing.
3. The backup suite was a near line-for-line port of the TCM scripts (same locators, same `div[data-cycle-id]`
   CSS, same positional option picks). Where it differed, it was usually weaker: the "no publish switch" checks used a
   visible filter on a switch input that is 0x0 and never visible, so they pass whether the switch exists or not.
   TCM's runner treats a 0x0 switch as seen when its label is visible, so TCM's version of that check is real.
4. TCM gets the cookie banner and the `#ehrmLoader` spinner handled once, in the runner. Playwright handles the
   cookie banner by hand in the few actions that need it and has no loader wait in these pages.
5. The brief said Playwright also covers the 23 non-To-Be-Automated cases. Neither the current suite nor the backup
   contains them (the backup md has exactly the same 37 headings, all tagged To Be Automated). Nothing to list.

## 2. How each tool decides a step passed

| Aspect | TCM Auto Run | Playwright suite |
|--------|--------------|------------------|
| Locator model | `{role, name}` from the browser accessibility tree; name matches **loosely** (contains, ignoring case) unless `exact`; `{text}` deepest element; `{css}`; list = chain; `nth` zero-based. A locator for an action must match **exactly one** visible element. | Page-object `Locator`s, mostly `getByRole`; Playwright strict mode (more than one match throws) except where a `shown()` helper adds `.filter({visible:true}).first()`. |
| What is matched | Only what a person can see. A 0x0 switch input counts as seen when its label is visible. | Playwright visibility: a 0x0 / opacity-0 input is **never** visible. |
| Assertion retry | Every `expect_*` retries until true, 10 s default; `wait_for` takes explicit ms. | `expect` web-first retry (default 5 s unless configured; specs pass 30/20/15/8 s explicitly). |
| `expect_hidden` | "Gone counts as hidden": passes if nothing matches. | `toBeHidden` likewise passes for no match. |
| Disabled | `expect_disabled`: `disabled` property, or this element or an ancestor `aria-disabled="true"`, or disabled fieldset. | `toBeDisabled`; role-less `aria-disabled` divs are not recognised on the element itself (docs/app-quirks.md). |
| Text compare | `expect_text` equals, `expect_contains_text`; whitespace collapsed, case sensitive. | `toHaveText` / `toContainText` (optionally `useInnerText`), regex allowed. |
| Extra vocabulary | `expect_count`, `expect_attribute`, `check_url`, `expect_response`, `api_request`. No bounding box / geometry, no regex. | Anything: bounding boxes, regex, network `waitForResponse`. |
| Navigation to the page | Component "Fresh cycles list" only asserts the heading; the runner returns to the recorded area first. | `autoNavigate` fixture walks the recorded `performance/PC-manage-cycle` menu chain before the test body. |
| Steps 1 and 2 of every case | step 1: search textbox visible; step 2: heading visible. | step 1: `main#phr-content` visible (30 s) + heading; step 2: heading + search box + New Cycle visible. Equivalent. |
| Data set-up | Script `preconditions` (flow + stage + value + why) and `changes` / `needs_unchanged` for cases that flip state; `no_save` flag. | None declared; names are in `data/PC-manage-cycle.json` with provenance tags. Raw specs carry the names inline. |

## 3. Per-case comparison: the 5 cases present in both

Cell shorthand: `nth` = positional; "role+name" = accessibility locator; PW = Playwright.

### 3.1 Table

| Case | TCM locator(s) | Playwright locator(s) | TCM checks | Playwright checks | Verdict |
|------|----------------|-----------------------|------------|-------------------|---------|
| 141698 New Cycle button with Plus icon top-right | role textbox "Search cycles..."; role heading "Manage Performance Cycles"; role button "New Cycle" (loose name). | `main#phr-content` scope; `getByRole('button',{name:'New Cycle'})` (loose); `button.locator('i.fa-plus')`; `boundingBox()` of button and of `main#phr-content`; inherited `manageHeading`/`searchCycles`/`newCycleButton` (all `shown()` = first visible). | Heading visible; New Cycle button visible. Nothing about the Plus icon or the position. | Button visible; icon **attached** (not visible); right edge within 40 px of content right edge; left edge in the right half of the content; top within 300 px of content top. Also content root visible (30 s), heading, search box visible. | **Playwright stronger.** Case asks for icon and top-right; only PW checks them. PW risks: `toBeAttached` proves a class exists, not that the icon renders; the 40 px / 300 px thresholds are guesses (false fail if the header has more than 40 px right padding). |
| 141699 Cycle card shows cycle name | role textbox, role heading (as above); CSS `div[data-cycle-id]` (card container) and `div[data-cycle-id] h3`. | `main#phr-content` > `getByRole('button',{name:/^Open /})` for cards (`po.cards`); `card.getByRole('heading',{level:3})`; `toHaveAccessibleName`; then `searchCycle(name)` and `getByRole('heading',{level:3,name,exact:true})`. | `expect_count` cards = 10 and `h3` = 10 (page size). Never reads a heading's text. Passes with empty or wrong names. | Loop over every card on the page (count > 0 only): h3 visible, text not empty, card accessible name equals `Open <h3 text>`. Then searches FY2026 and FY2025 (`Showing 1-1 of 1`) and asserts the exact heading. | **Playwright stronger.** Ties the visible name to the card and checks two known names. Loses TCM's "exactly 10 cards" check, which the case does not ask for. PW relies on both cycles existing (no precondition). |
| 141700 Card subtitle shows method and participant count | CSS `div[data-cycle-id] p` (count) and `{css:..., nth:0}` for text. | `card.locator('p')` on each card; `po.card(name)` exact "Open <name>" then `locator('p')`. | `p` count = 10; **first card only** contains "Evaluation · " and " participants". No year check, no number check, no method check, no per-cycle check. | Every card on page: `toHaveText(/^(?!.*\b(19\|20)\d{2}\b).+ · \d+ participants?$/)` (format, number present, no 4-digit year). FY2026 and FY2025: exact text `180° Evaluation · 1 participants`. | **Playwright stronger.** Over-strict edges: exact "1 participants" locks in the app's missing singular; the year lookahead would reject "2000 participants"; expected subtitle is marked `assumed`. |
| 141701 Card footer shows Duration and period dates | CSS `div[data-cycle-id]` chained to `{text:"Duration"}` (count 10; first card visible); search "FY2027 Annual Performance Review (Oct)"; `Open <name>` button chained to text "Duration" and to text "Oct 01, 2026 - Sep 30, 2027". | `po.cardText(card,'Duration')` exact; `card.getByText(/^[A-Z][a-z]{2} \d{2}, \d{4} - [A-Z][a-z]{2} \d{2}, \d{4}$/)`; `po.card(name)` exact + `getByText(dates,{exact:true})`. | Duration label on all 10 cards (count) and on one card by name; that one card contains the literal period text (the follow-up `contains " - "` is redundant once the text matched). Dates of other cards are not read. Hidden dependency: cycle 11171 is never declared as a precondition on this case. | Duration label **and** date-shaped period on every card on the page (hyphen included in the regex); FY2026 and FY2025 exact period strings. | **Playwright stronger.** Neither independently reads Start/End Date from the cycle, so "dates match" is only checked against hard-coded strings. PW's `Jan 01 - Dec 31` strings are `assumed`/`derived`; TCM's Oct dates were confirmed live. |
| 141702 Draft cycle card shows 'Draft' label | role textbox "Search cycles..." filled with `AUTOTEST Annual Performance Review FY2026`; role button "2 page"; `Open AUTOTEST Annual Performance Review FY2026` chained to `{text:"Draft", exact}`. | `cards.filter({has: page.getByText('Draft',{exact:true})}).first()`; `getByText('Draft',{exact:true})` inside it. No search. | Searches the fixture draft; waits for the pager to shrink (`expect_hidden` "2 page" acts as the wait); that card shows exact "Draft". | First card on the **current page** that contains an exact "Draft"; then that label visible. | **Different.** Same assertion on different targets. PW is looser and can false-fail if page 1 has no Draft card; TCM is deterministic but depends on a unique fixture draft and a loose-name match that must hit exactly one card. |

### 3.2 Notes behind the table

- **Locator strategy.** TCM stays on role+name and chains (`Open <name>` button, then text/switch inside) and falls back
  to `div[data-cycle-id]` for list-wide counts (no role exists for the card container). Playwright uses
  `getByRole('button', {name: /^Open /})` as the card, so "a card" is a different DOM node (role=button) from TCM's
  (`div[data-cycle-id]`); heading and subtitle are reached by `h3`/`p` in both. Playwright scopes everything to
  `main#phr-content` (the page has two nested `main`); TCM does not scope and relies on "visible only".
  Both match card names with the `Open ` prefix: TCM loosely, Playwright exactly in `card()`.
- **Fragility.** TCM: positional `nth 0` (141700), count 10 (needs 10+ cycles), loose name `Open AUTOTEST Annual
  Performance Review FY2026` (fails on a second match; the components describe "the newest" of several such
  drafts). Playwright: geometry thresholds (141698), `.first()` in `firstCardWithLabel`, a seven-class-deep page object
  chain (`ManageCyclesPage` extends `EvaluationRulesPage` extends `TimelinePage` ...) so a change in an unrelated
  page object can break this one.
- **Waits as checks.** TCM waits for the pager to shrink (`expect_hidden` "2 page") and for `Showing 1-1 of 1` plus
  the Open button. Playwright waits for the debounced search's own network response and then for a card heading with the name
  to appear and every other card heading to go (`searchCycle` in `evaluation-rules.page.ts`), which is the more
  robust form (the quirk "a late debounced search paints the list over the wizard" is documented there).
- **Data.** TCM (141698-141700, 141702): whichever cards are on page 1, plus the shared draft by name; 141701:
  FY2027 (Oct) [11171]; preconditions declared for the Published/Unpublished cards in 141703+ only.
  Playwright: FY2026 [published] and FY2025 [unpublished] by name in four of five specs; page-1 cards otherwise.
  Both are look-only, no `Save`, no toggle, no restore needed. TCM marks all five `no_save: true`.

## 4. The other 32 cases (TCM only now)

Compared with the superseded backup suite (its page object and spec were read) so the lessons carry over. "Old PW" below
means the backup, not anything that runs today. Verdict = TCM script vs old PW.

| Case | TCM locator(s) and checks | Old PW locator(s) and checks | Verdict |
|------|---------------------------|------------------------------|---------|
| 141703 Published label | Find card by exact name (fill, `Showing 1-1 of 1`, Open button), card chain to text "Published" exact. Precondition 10750. | `searchSingleCycle` then `card.getByText('Published',{exact})`. | Equivalent |
| 141704 Unpublished label | Same with FY2025 and "Unpublished" (10763). | Same pattern. | Equivalent |
| 141706 Action icons | role button `View <name>` / `Edit <name>` / `Delete <name>`; `expect_text equals ""` (icon only); absent icon via chain to `css button[aria-label^="Delete "]` / `View `. Published, Unpublished, draft `AUTOTEST Draft First Incomplete 2027`. | `viewButton`/`editButton`/`deleteButton` (substring role+name, `shown()`); `toHaveText('')`; `cardDeleteIcon`/`cardViewIcon` CSS. | Equivalent. Neither checks the glyph (eye, pencil, trash), only name and empty text. |
| 141708 Draft has no toggle | Card chain to "Draft" exact; `expect_hidden` chain `role=switch` inside the card. | `cardVisibleSwitch` = `.filter({visible:true})` on the switch, `toBeHidden`. | **TCM stronger.** Old PW check is vacuous: the switch input is 0x0 so it is never visible, so the check passes even when a switch exists. |
| 141709 Toggle on Published | Card chain to "Published" and `expect_visible` role=switch (label counted as seen). | `cardSwitch(...).toBeAttached()` plus label. | **TCM stronger** (visible vs present in DOM). |
| 141710 Toggle on Unpublished | Same with "Unpublished". | Same as 141709. | **TCM stronger** |
| 141711 Toggle Published OFF | `expect_checked` true, label Published; click role=switch in card; `expect_hidden role=dialog`; `wait_for` toast text 8 s; `expect_checked` false; label Unpublished; click again; toast; checked true; label Published. `changes`/`needs_unchanged: "FY2026 publish state"`. | `cardSwitch` `toBeChecked`; click `label.phr-switch-item`; `anyDialog` hidden; `waitForToast` 8 s; `not.toBeChecked` and `not aria-checked=mixed`; restore with same toast waits. `@publishes` tag. | Equivalent. See pattern 5.4 for restore behaviour. |
| 141712 Toggle Unpublished ON | Mirror image (FY2025 / 10763). | Mirror image. | Equivalent |
| 141714 New Cycle opens wizard | click button "New Cycle"; heading "Create Performance Cycle"; progressbar name contains "Cycle Setup"; component "Exit wizard without saving". | `clickNewCycle`; `createHeading`; `cycleSetupProgress` (progressbar "Cycle Setup", substring); `exitWizard`. | Equivalent. Neither reads "Step 1" explicitly. |
| 141717 Search filters | Fill "Annual"; first card with `[aria-label^="Open "][aria-label*="annual" i]` visible and `expect_count` of non-matching = 0; fill full name, `press_key Enter`; "2 page" hidden; card visible; non-matching count 0. | `cardsMatching`/`cardsNotMatching` same CSS; `keyboard.press('Enter')`; "2 page" hidden. | Equivalent. Neither checks "no page reload" or the 800 ms pause or that Enter beats the pause. |
| 141720 Status dropdown options | click button "All (" (loose); `expect_count role=option = 4`; options "All (", "Draft (", "Published (" nth 0, "Unpublished (" nth 0. | `openStatusFilter`; `toHaveCount(4)`; `statusOption()` = substring + `.first()`. | Equivalent. Both: "Published (" is a substring of "Unpublished (", so the Published check is satisfied by the Unpublished option alone; only the count of 4 and the other options guard it. |
| 141721 Draft filter | click "All (", click option "Draft ("; button "Draft (" visible; cards = 10; cards chained to exact "Draft" = 10. | Same, with `pageSize` 10. | Equivalent. Both over-strict: needs 10 or more Draft cycles on the environment. |
| 141722 Published filter | click "All (", option **nth 2**; `wait_for` text "Published" exact; `expect_hidden` "Draft" / "Unpublished" text; search "Annual Performance Review"; wait for FY2026 card; FY2025 and AUTOTEST cards hidden. Preconditions 10750, 10763. | `chooseStatusAt(2)` (comment explains substring problem); same hidden checks. | Equivalent. Positional option pick in both; nothing proves the dropdown now reads "Published (". |
| 141723 Unpublished filter | option nth 3; mirror checks. | `chooseStatusAt(3)`. | Equivalent |
| 141724 All filter | Draft (nth 1) + search; Open AUTOTEST visible, FY2025 and FY2026 hidden; then switch to All (nth 0); FY2026 wait; FY2025 and AUTOTEST visible. | Same order and indexes. | Equivalent |
| 141727 Pager | cards = 10; text "Showing 1-10 of"; Previous page disabled; "1 page" (exact) `aria-current=page`; Next page enabled; click Next page; "2 page" `aria-current`; "Showing 11-20 of". | Same; `nextPage()` closes the cookie banner first when `#btnCookieClose` shows. | Equivalent. TCM relies on the runner's automatic banner close. |
| 141729 View opens View mode | "View cycle by exact name" component (search, Open card, click `View <name>`, heading "View Performance Cycle", progressbar "Step 9 of 9 — Preview", Close visible); then heading, progressbar, heading "Preview"; `expect_disabled` button "Cycle Setup" exact; Close component (Close, "Exit wizard?" dialog, Continue). | `viewToPreview`; same headings; `stepTile('cycleSetup')` `toBeDisabled`; `closeView`. | Equivalent. "Read-only" is inferred from a disabled step tile only. |
| 141731 View footer | Close (exact) visible; Previous, Exit, Save, Save & Continue (all exact) hidden. | Same five checks. | Equivalent. All four hidden checks pass vacuously if a button is renamed. |
| 141733 View preview cards | Seven `Toggle <X> section <X> Ready` buttons: `aria-expanded` true then false; expand six; all true; `expect_hidden` CSS `main main input, main main select, main main textarea`, button "Edit", texts "Ready to Publish" / "Cycle Status", `role=switch`. | `sectionToggle(...)` with the same names; `toHaveAttribute('aria-expanded')`; `visibleFormControl`, `anyEditButton`, `anyVisibleSwitch` hidden. | **TCM stronger** (only because Old PW's switch check is vacuous; the rest matches). Neither checks that the cards show saved values as text. |
| 141734 Edit Draft | fill `AUTOTEST Draft First Incomplete 2027`; "2 page" hidden; click `Edit <name>`; heading "Edit Performance Cycle"; progressbar "Eval Rules"; heading "Evaluation Rules"; click tile "Cycle Setup" (exact); wait "Copy from Previous"; enabled: year "2026", Start Date *, End Date *, Standard Configuration card, 180° card, Copy from Previous. | Same elements and enabled checks. | Equivalent. Neither checks "no warning banner", which the case states. |
| 141735 Edit Published, no submissions | Edit-published component (Preview step 9, button "Edit Cycle Setup"); `expect_hidden role=alert`; Timeline tile, progress "Step 3 of 9 — Timeline", Auto-fill dates / Auto-balance enabled, alert hidden; Cycle Setup tile; alert visible and contains "Some settings are locked ..."; year/dates/3 config cards/3 method cards disabled. | `editToPreview`; `banner` hidden/visible (`shown(getByRole('alert'))`), `toContainText`; same disabled set. | Equivalent |
| 141736 Edit with submitted evaluations | Signs in as **evelyn1**; FY2027 Annual Performance Review (Oct) [11171] (+ submitted goal plan 546); five tile clicks each with progressbar wait, alert visible, alert contains the full banner text. | Runs as the shared general account; same cycle, same tile order, `toContainText`. | **Different** (account). If the cycle is org-scoped, one of them reads another tenant's data; checks themselves match. |
| 141737 Banner text | `expect_text role=alert equals` the full sentence "This cycle has submitted evaluations. Some settings are locked ..." on FY2026 (which per 141735 may have no submissions). | `toHaveText(bannerText)` with `useInnerText`. | Equivalent. Worth a ruling: the case text says "submitted evaluations" but 141735 only asserts the suffix. |
| 141738 Editable fields | fill `Cycle Name *`, `Description`; `expect_text equals` the typed values; `expect_enabled` file-drop button whose name is the whole native string "Choose files: No file chosen Drop your files here ... up to 10 MB". | `fill`; `toHaveValue`; file-drop matched by regex `/Drop your files here/`. | Equivalent in checks. TCM's locator embeds the browser's own "No file chosen" text, so it breaks on another browser locale. |
| 141739 Locked fields | Year "2026" exact, Start/End disabled; six config/method cards disabled; `aria-pressed` true on Standard and 180°, false on the other four. | Same. | Equivalent |
| 141741 Eval Rules locked | `expect_disabled css #er-goals-card, #er-comps-card, #er-custom-goals-card, #er-okr-card` and their checkboxes; goals and comps checked. | `toHaveAttribute('aria-disabled','true')` on the four role-less divs; `toBeDisabled` on `#...-card input[type=checkbox]` (first, unfiltered); `toBeChecked`. | Equivalent. TCM's disabled rule also accepts an aria-disabled ancestor, so its checkbox check can pass on an enabled input inside a disabled card. |
| 141744 Evaluators locked | Buttons "Toggle Self/Manager/Reviewer evaluator" disabled; `spinbutton` nth 0,1 and `slider` nth 0,1 disabled. | Same elements via visible-filtered `.nth`. | Equivalent. Both only check the first two of each input type, not "all". |
| 141747 Goal Groups locked | button "Distribute Evenly" disabled; `role=checkbox` nth 0,1,2; `Edit row` nth 0,1 disabled. | `getByRole('checkbox')` with **no visible filter** `.nth(0..2)`; `Edit row` visible-filtered. | **Different.** The two tools' `nth` positions land on different checkboxes: TCM among visible ones, old PW among every checkbox on the page including 0x0 inputs. |
| 141748 Restricted Preview | Previous and Exit exact visible; Ready to Publish, Cycle Status, switch, Save, Save & Continue hidden. | Same list; the switch check is `shown(getByRole('switch')).toBeHidden()`. | **TCM stronger** (old PW switch check vacuous). |
| 141749 Banner per step | Account evelyn1; Preview alert hidden; `check_url contains "/hr/home/index"` at start and end; alert visible on Cycle Configurations, Goal Groups, Evaluators, Timeline, Eval Rules; hidden on Participants, Competencies; progressbar wait before each. | Same sequence; `toHaveURL(/\/hr\/home\/index/)` at start and end. | **Different** (account). Neither compares the end address with the noted start address, only both against a constant path. |
| 141751 HR admin can access | Search box, heading; button "Next page" visible; cards = 10. | Same. | Equivalent. Neither asserts the absence of an access-denied message. |
| 142514 Republish | Find FY2025 card; click `Edit FY2025 ...`; wizard Preview; Previous/Exit visible, Ready to Publish / Cycle Status / switch / Save / Save & Continue hidden; Exit component; click card switch; toast published; checked; label Published; click again; toast unpublished; unchecked; label Unpublished. | Same flow; Edit via `editButton`; footer checks include the vacuous `anyVisibleSwitch` hidden. | **TCM stronger** (switch absence is a real check in TCM only). |

## 5. Cross-cutting patterns

### 5.1 Locator philosophy
- TCM: role+name first, chained to scope (`Open <name>` button then text/switch), CSS only when no role exists
  (`div[data-cycle-id]`, `#er-goals-card`, `[aria-label^="Open "]`). Names are loose by default, `exact` added only when
  a near miss exists. The runner needs one match, so duplicates are loud (a "matches N" failure), never masked.
- Playwright: page objects. `shown()` = `filter({visible:true}).first()` on most members, which silently picks the first
  of several matches; the new `ManageCyclesPage` members are plain locators (strict), which is better.
  The class sits at the bottom of a deep inheritance chain; the new specs call inherited members
  (`manageHeading`, `searchCycles`, `newCycleButton`, `searchCycle`).
- Positional: TCM uses `nth` for the status options (2, 3, 1, 0), `Published (`/`Unpublished (` and the spinbutton /
  slider / checkbox / Edit-row checks. The old PW used the same indexes. The new 5 specs use `.nth(i)` only as a loop
  index over a full list and `.first()` once (Draft card).
- Different elements for the same thing: the card (TCM `div[data-cycle-id]`, PW role=button "Open ..."); a switch
  (TCM role=switch seen via its label, PW input `getByRole('switch')` without visibility, clicked via
  `label.phr-switch-item`); the locked cards on Evaluation Rules (both CSS ids, different disabled semantics).

### 5.2 Assertion style
- TCM prefers presence/absence and counts; it never uses regex or geometry. Exact-text checks are the toast, the banner
  and the `Showing` count. Many "each card" expectations are reduced to a count.
- Playwright reads content: the new specs assert shapes by regex on every card and exact strings for two named
  cycles. Setup waits are explicit and long (30/20/15 s) and use network-aware waits for the debounced search.

### 5.3 Cookie banner, loader, "Exit wizard?" dialog
| Concern | TCM | Playwright |
|---------|-----|------------|
| Cookie banner (`#cookieBanner`, close `#btnCookieClose`) | Runner closes it before every click/fill/drag/upload via the sign-in recipe's `close_when_shown` (never Accept All). Scripts carry no guard. | Hand-rolled in specific actions (`nextPage`, `selectRatingMethod`), a 1-2 s `waitFor` then close. `performLogin` clicks **Accept All** on the login page. The 5 new specs never click anything the banner could cover. |
| `#ehrmLoader` spinner | Runner waits (up to 90 s) before every click/fill/wait_for. No script needs a long first wait. | No loader wait found in `src/` or in the manage-cycle page; relies on web-first timeouts (e.g. `content` visible 30 s). |
| "Exit wizard?" dialog | Components "Exit wizard without saving" (Exit, wait dialog, Continue exact, wait heading) and "Close View mode without saving". Dialog located by `role=dialog name`, button Continue exact. | `exitWizard()` / `closeView()` / `confirmExitDialog()` in the backup page object, same steps. Not used by the 5 look-only specs. |
| Wizard exit on Competencies | Closes the designation chip footer first (`.phr-select-chip-footer`). | Same logic. |

### 5.4 Publish toggle
- Both click the label, not the 0x0 input (TCM's runner redirects a click on the input to the label; Playwright's page object
  clicks `label.phr-switch-item`).
- State read: TCM `expect_checked` (live state) plus the Published/Unpublished label; Playwright `toBeChecked` / `not.toBeChecked`
  on the unfiltered input plus the label, plus a never-fires `not.toHaveAttribute('aria-checked','mixed')`.
- Restore: both toggle back **in the test body** with a second toast wait; neither uses `finally`, so a failure after the
  first toggle leaves the shared cycle in the wrong state and poisons the next run's opening assertion. TCM at least
  declares `changes` / `needs_unchanged` on the script so the runner and precondition engine know the case moves that state;
  Playwright only tags the tests `@publishes`.
- Absence of a switch: only TCM checks it correctly (see 5.5).

### 5.5 Where a tool would PASS with the app wrong (weak checks)
TCM
1. 141698: passes if New Cycle is anywhere on the page, with or without a Plus icon.
2. 141699: passes with blank or wrong card names (counts only).
3. 141700: only the first card's subtitle; year not checked; number not checked.
4. 141701: dates of every card but one are unread; label presence only.
5. 141733: "read-only" means "no input/select/textarea"; the saved values are never read.
6. 141734: the case's "no warning banner" is not asserted.
7. 141744 / 141747: only two of each input type / three checkboxes / two Edit-row buttons.
8. 141749: the address is compared with a constant substring, not with the noted address; `check_url` looks once.
9. 141720: Published option is satisfied by Unpublished; 141722 / 141723 / 141724 pick the option by index and never confirm
   the dropdown label.
10. `expect_hidden` passes for a missing element. A renamed Previous/Exit/Save button leaves 141731 / 141748 / 142514 green.

Playwright (new)
1. 141698: icon only `toBeAttached`.
2. 141699: the card's name is only checked against its own heading, plus two named cycles; no count of expected cycles.
3. 141701: dates of the two named cycles are hard-coded; "dates match Start/End" never compared with the cycle's own fields.
4. All five: no precondition declared, so a missing FY2025/FY2026 cycle shows up as a failed assertion, not a clear set-up error.

Playwright (backup, same class of bug can come back in regeneration)
1. "No switch" checks via `.filter({visible:true})` (141708, 141733, 141748, 142514) can never fail.
2. 141709 / 141710 used `toBeAttached`.

### 5.6 Where a tool would FAIL with the app right (over-strict)
TCM
- 141721 needs 10 or more Draft cycles; 141727, 141751, 141699, 141700 need 10 or more cycles of any kind (hard-coded 10).
- 141701 depends on cycle 11171 existing (declared nowhere on that script).
- Loose `Open AUTOTEST Annual Performance Review FY2026` (141702, 141708, 141717) fails if a second fixture draft with
  a longer name exists.
- 141738 file-drop button name embeds browser text ("No file chosen").
- 141736 / 141749 run as `evelyn1`, which differs from the Playwright account; both are right if the data is visible to that user.

Playwright (new)
- 141698: geometry thresholds (40 px flush, 300 px) are guesses.
- 141700: exact "180° Evaluation · 1 participants" for both named cycles (assumed) and the year lookahead rejects "2000 participants".
- 141701: `Jan 01, 2026 - Dec 31, 2026` and `Jan 01, 2025 - Dec 31, 2025` marked `assumed`/`derived`.
- 141702: needs a Draft card on page 1 of the unsearched list; with 10 or more newer non-draft cycles it fails.
- `headerButton('New Cycle')` is a substring match inside `main#phr-content`; a cycle named "New Cycle ..." would trip strict mode.

## 6. Recommendations

### TCM Auto Run
1. 141698: add `expect_visible` for the icon inside the button (chain `{role:button,name:New Cycle}` > `{css:"i.fa-plus"}`).
   Top-right position cannot be expressed in the action vocabulary; note it as unchecked (`unchecked: "position"`) or
   ask for a bounding-box/position check.
2. 141699 / 141700 / 141701: replace bare counts with content. Cheap options: `expect_count css "div[data-cycle-id] h3:empty"`
   equals 0; read the two known cycles' subtitle (`180° Evaluation · 1 participants`) and period via the card chain
   (the FY2026 / FY2025 cards already exist in the other scripts); confirm those dates once live.
3. 141701: change the cycle to FY2026 or FY2025 (declared, 10750/10763) or add the 11171 precondition.
4. 141734: add `expect_hidden role=alert` after the wizard opens, as the case says.
5. 141744 / 141747 / 141741: replace the first-n positional disabled checks with a count that proves none are enabled
   (`expect_count` of `input:not([disabled])` inside the step container equals 0) where possible.
6. 141749: store the address after step 3 is not possible; accept as unchecked or compare against the full URL, not a substring.
7. 141738: replace the long file-drop name with the stable fragment ("Drop your files here"), as Playwright does.
8. Keep an eye on `check_url` "once" semantics and on loose names that can match a second cycle; use `exact` on every `Open <name>`.
9. Borrow Playwright's wait for the debounced search's own response (the `get_autorun_network_wait` tool) in the
   "Find cycle card" family, so a stale `Showing 1-1 of 1` cannot satisfy the wait.

### Playwright suite
1. Declare preconditions or fixtures for FY2026/FY2025 (cycles 10750 and 10763 in TCM) so a missing cycle reports as
   set-up, not as a failed assertion; confirm the assumed subtitle and dates against the live cards.
2. 141702: search the named fixture draft (as TCM does) or assert the category across the whole filtered list
   (Status filter = Draft) rather than "first Draft on page 1".
3. 141698: keep the Plus icon and position checks but assert the icon `toBeVisible`, and compute the thresholds from the
   header's own padding; drop the hard 40 px.
4. Never assert a switch's absence through a visible filter (0x0 input). Use `toHaveCount(0)` on `card.getByRole('switch')`
   and `toBeVisible` on `label.phr-switch-item` for presence. Port TCM's "visible via label" stance when the other 32 are
   regenerated, and do not carry `cardVisibleSwitch` / `anyVisibleSwitch` forward.
5. Move the cookie-banner close and the `#ehrmLoader` wait into the shared base page or the `page` fixture instead of
   per-action copies, so every new spec gets them. The manage-cycle page object does not call any of it today.
6. Add the case-level checks TCM has and the new specs lack when they are generated: `Showing 1-10 of` / disabled
   Previous / `aria-current` (141727), `expect_count` 4 options (141720), the full banner text equality (141737),
   `aria-pressed` states (141739).
7. Restore shared state in `test.afterEach` or a `finally` (publish toggles), not only in the test body; a failed first
   toggle currently strands the cycle.
8. When the remaining 32 cases are generated, compare each to the TCM script on this table: any case where the
   generated version has fewer checks than the TCM one (section 4 verdict "TCM stronger") should be raised, not shipped.
9. The data file's `_provenance` marks FY2026/FY2025 names, dates and subtitle as `assumed`/`derived`; promote them to
   `verified` once checked live.

## 7. Evidence and limits

- TCM scripts and components read from `%APPDATA%\com.avinalwis.testcasemanager.v2\autorun\` (37 scripts present; the
  components file expands "Fresh cycles list", "Find cycle card by exact name", "Edit published cycle by exact name",
  "View cycle by exact name", "Close View mode without saving", "Exit wizard without saving").
- Matching rules and the runner's cookie/loader handling taken from `src-tauri/src/autorun/guide.rs`.
- Playwright new files: `PC-manage-cycle/{specs,pages,data,test-cases}` and `suites/_generated/manage-performance-cycles-*.spec.ts`,
  plus inherited members in `PC-timeline/pages/timeline.page.ts` and `PC-evaluation-rules/pages/evaluation-rules.page.ts`.
  The backup was read for section 4 only.
- No test was run. Verdicts are from reading code; whether an assertion would pass or fail on the live app was not tried.
- Console output of this session showed mojibake for the middle dot and degree sign (a PowerShell code-page issue); the
  files themselves are correct UTF-8 (confirmed with Grep).
