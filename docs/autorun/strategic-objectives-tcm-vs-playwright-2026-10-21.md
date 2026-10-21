# Strategic Objectives (PBI 134623): TCM Auto Run vs the Playwright repo

Date: 2026-10-21. Read-only comparison. Nothing was changed in either repo, nothing was run, and no browser was opened.
The only file written is this one.
Focus: (1) locators, (2) what each tool checks to decide the expected outcome happened, (3) fitting to the environment.

## 1. Summary

**What exists today.** Both tools cover all 20 cases.

- **TCM:** 20 scripts, `case-<id>.json`, saved 08:42-08:53 today. All are `no_save: true` and `checked: true`, and none uses
  a shared component. The last runs were green on all 20. In run `1792553614589` the 14 admin scripts passed and the 6 BSC
  scripts were Blocked at the Self Service menu walk; the BSC re-run `1792555284684` passed all 6.
- **Playwright:** `suites/_generated/index.json` maps all 20 ids to raw specs (most still untracked in git).
  - **18 refactored specs.** 12 admin specs are in `suites/sl/admin/performance/strategic-objectives/` (page object
    `StrategicObjectivesPage`, `data/strategic-objectives.json`, test-case md). The 6 BSC specs landed in
    `suites/sl/self/performance/goal-cascading/` (page object `GoalCascadingPage`, `data/goal-cascading.json`).
  - **2 raw-only cases**, held back on purpose:
    - **147556:** the raw spec asserts the case's 4 levels, is red, and has "Not met" recorded in the md Preconditions.
    - **160128:** the raw spec is green, but the md records "Not met": the screen lists only one cycle.
  - **Run history.** The refactored SO runs at 10:09 and 11:04 were green. The refactored BSC runs at 10:48 and 10:52 had
    1-2 tests fail each. A fix to the shared home-page wait went in (`waitForHomeRender`, see `docs/app-quirks.md`), and
    the runs at 10:59 and 11:01 were green.

**Yardstick.** Case text came from the live bridge (`/test-cases?ids=...`). For all 9 cases with a pending rewrite
(147556, 147559, 160128 and the 6 BSC cases), the live text is the **old version**: it still has "exactly 4 DEF levels",
"Performance Management System" and the 4 seeded goal groups. Verdicts are against that live text. Section 4 notes
where `D:\PMS_Module\HRM-PMS-NET\.test-cases\strategic-objectives-rewrites.json` would change them.

**Verdict counts (against live text)**

| Basis | Equivalent | TCM stronger | Playwright stronger | Different | Total |
|-------|-----------:|-------------:|--------------------:|----------:|------:|
| All 20 cases | 5 | 1 | 11 | 3 | 20 |
| ...of which Playwright is raw-only (not in the SL suite) | 0 | 0 | 1 (147556) | 1 (160128) | 2 |

Raw check counts, for scale: Playwright has **305** `expect(` calls across the 20 specs and TCM has **179** checks
(`expect_*`, `wait_for` and `check_url`, cleanup steps excluded). Two things inflate the Playwright number. Every
Playwright spec opens with 5-7 landing checks (content root, cycle button, tablist, both tabs, heading, `aria-selected`).
And a loop counts once in the source but runs several times.

**How the prior TCM audit fares** (`so1-audit.md`, 19 scripts, plus 147556 added here as FITTED, which gives 20 scripts and
3 FITTED):

| Audit class | TCM count | Playwright fixes it | Playwright has the same gap | Notes |
|-------------|----------:|--------------------:|----------------------------:|-------|
| FITTED | 3 | 2 (147556 asserts 4; 160128 checks the "Select Cycle" header) | 1 (160207 count 2) | Playwright adds its own fitted counts, see section 4 |
| MISSING | 16 | 3 (147554 row sitemap icon, 147559 caption absence, 160205 "under Target") | 13 (tab icons, guidance icons, trash icons, menu steps, "others hide", cycle bar position) | |
| WEAKER | 17 | 11 (147557 option list, 147572 all rows, 147577 Info toast, 139161 bar scope and check mark, 5 x "Pending in that row", 160207 in-cycle date); 2 partly (160128 steps 4-5) | 4 (147558 and 147559 option enumeration, 160204 absence and narrowing) | |
| EXTRA | 6 | 2 (147546, 147559 do not count rows) | 4 (147547, 147558, 147572, 147577 also hard-code 6 or 5) | Playwright adds extras: 139161 option count 5, BSC card count 2 |
| PRECONDITION | 7 | 1 recorded in md (160128); 6 BSC only as `// Finding` comments | 6 BSC md files still list the 4 seeded groups | Playwright also recorded 147556 |

**Headline findings**

1. **147556 is the clearest difference in method.** The case says exactly 4 DEF levels; the environment has 6.
   - **TCM** asserts the environment: option count 6, order Level 1..6. It goes green, and nothing records the mismatch.
   - **Playwright** asserts the case: `toHaveText(['Level 1'..'Level 4'])` and `toHaveCount(4)`. The spec is red, with a
     `// FINDING` comment and a "Not met in QA Automation (Sri Lanka)" row in the test-case Preconditions. The pipeline
     then refused to refactor it.
2. **Playwright did not hold that line on the BSC cases.** The case's precondition seeds 4 goal groups; the app has 2.
   - All five Add Goal specs assert `addGoalButtons.toHaveCount(2)`, under a `// Finding:` comment. 160207 also asserts
     `noRecordsCells` = 2 and two "0 Goals · Min 1, Max 5" summaries.
   - The count is in the data as `perspectiveCardCount: 2`, tagged `assumed`. The six test-case md files were **not**
     updated with a "Not met" row.
   - So Playwright fitted 5 specs to the environment, while TCM fitted 1 (160207) and used `at_least 1` in the other
     four. The pipeline's "missing precondition" rule only fires when a run fails. A generator that writes what it sees
     produces a passing spec, so nothing classified these.
3. **On the admin screen, Playwright's page object scopes every row by id** (`#dcl-dd-<n>`) and reads whole lists, which
   fixes most of TCM's WEAKER findings:
   - the exact option list in 147557;
   - every row enabled in 147572;
   - no assigned or delete buttons left in 147575 and 147577;
   - the toast's "Info" label in 147577;
   - "no caption option" in 147559.

   TCM reaches the same rows with `{role: button, name: "Select cascade level", nth: 0}`, which means "the first
   unassigned row" and shifts after every assignment.
4. **Both tools skip the icons the cases name**: tab sitemap and layer icons, guidance question mark and chevron, trash
   icons. All of them are in the DOM (`Index.cshtml` lines 66 and 74, `_CascadingLevels.cshtml` lines 20 and 23,
   `cascade-levels.js` line 365). Playwright's 147546 says they are "unnamed images, not asserted", yet its own 147554
   asserts `i.fa-sitemap` in every row. TCM says nothing.
5. **TCM is stronger in one case and in one habit.**
   - In **160204**, TCM checks the case's "between Metric and Target" with two `expect_position`s, and narrows the
     currency list against three named non-matches. Playwright has no placement check and one non-match.
   - On Goal Cascading, TCM's "visible elements only" rule ignores the hidden, pre-rendered cycle list. Playwright first
     hit a strict-mode violation on it and needed a quirk entry plus id scoping (`#dco-cycle-dates`).
6. **Neither suite carries a `test.fixme`/`test.skip`**, and neither checks the case's step-1 menu text.
   - TCM marks step 1 `unchecked`, with the reason that the run opens the area first.
   - Playwright's refactored specs drop navigation to the auto-fixture. Its raw specs walk the real `Talent > Performance &
     Evaluation` menu with `// Finding:` comments that the case's menu names differ (160128, 139161).

## 2. How each tool decides a step passed (what differs on these two screens)

The general rules are in section 2 of `manage-cycle-tcm-vs-playwright-2026-10-20.md` and are not repeated here.

| Aspect | TCM Auto Run | Playwright suite |
|--------|--------------|------------------|
| Reaching the screen | The runner walks the recorded area (`Performance & Evaluation / Admin / Strategic Objectives`, `... / Self Service / Goals / Goal Cascading`) before step 1; step 1 is `unchecked` (BSC, 160128) or starts with a tab click (admin). | `autoNavigate` walks `src/navigation.json` `performance/strategic-objectives` (admin, `Talent > Performance & Evaluation`, moduleNav `Strategic Objectives`) or `performance/goal-cascading` (self, 4-deep menu), both "verified 2026-10-21, user-assisted". Each spec then re-checks the landed screen. |
| Hidden duplicates | Only visible elements match, so the pre-rendered cycle option list next to the bar is ignored without help. | Strict mode: `getByText('01/01/2026 - 31/12/2026')` hit 2 elements; fixed by `#dco-cycle-dates` and `filter({visible:true})` (`docs/app-quirks.md`, "Goal Cascading pre-renders the cycle selector's option list"). |
| Whole-list text | `expect_order` (each text must be part of an item, others may sit between) or one `expect_text` per `nth`. | `toHaveText([...])` on a locator list: exact count, exact order, full text of each. |
| Absence | `expect_hidden` passes when nothing matches; `expect_count equals 0`. | `toHaveCount(0)`; `toBeHidden` likewise passes when nothing matches. |
| Position | `expect_position` (above, right_of, left_of). | `boundingBox()` arithmetic in the spec (147546 only). |
| Value of a field | `expect_text` on a textbox reads its value; `matches` regex. | `toHaveValue` exact. |
| Set-up state | `preconditions` (flow `pms-goal-cascading`, stage `cycle-open`, 11538 or 10750) and `needs_unchanged: ["OKR cascade levels saved"]` on the 14 admin scripts. | None in code. Data `_provenance` marks every cycle name `assumed`; test-case md Preconditions are the case's own text plus 2 "Not met" rows (147556, 160128). Page objects carry a "PROTECTED BASELINE" header. |

## 3. Per-case comparison

Cell shorthand: PW = Playwright; "row n" = `#dcl-dd-n` in the Set up panel; "SCL" = button "Select cascade level".

### 3.1 Admin: Strategic Objectives / Cascading Levels

| Case | TCM locator(s) | Playwright locator(s) | TCM checks | Playwright checks | Verdict |
|------|----------------|-----------------------|------------|-------------------|---------|
| 147546 Two pill tabs, cycle bar above | button "FY2027 Company OKRs"; text "01/10/2026 - 30/09/2027"; tab x2; tabpanel; css `.phr-dcl-level-label`; buttons Edit/Reset/Save (loose, page-wide); table "Strategic Objectives"; button "Add Objective". | `cycleButton(name)` (loose, page-wide); `tablist > tab`; tabpanel > `getByText(level,{exact})`; `rowSelects.first()`; Edit/Reset/Save `exact` inside the panel; `boundingBox()`. | Cycle button and dates; both tabs; button **above each tab** (`expect_position`); 6 level labels (EXTRA); Edit/Reset/Save; objectives panel, table, Add Objective. | Cycle button; both tabs; **tab count 2**; bar `y` < Set up tab `y`; after click `aria-selected`, "Group" and "Department" rows (assumed), first SCL, Edit/Reset/Save; objectives `aria-selected`, table, Add Objective. | **Equivalent.** PW adds the tab count and selection state; TCM adds dates and position against both tabs. Neither checks the sitemap or layer icon; PW's reason ("unnamed images") does not hold. |
| 147547 Set up is the default tab | tabs by name with `aria-selected`; tabpanel; `.phr-dcl-level-label`. | same tabs; `setupPanel`; `setupPanelText(level)` x6; `rowSelects`. | `aria-selected` true/false; panel visible; 6 labels (EXTRA). | `aria-selected` true/false; panel; each of 6 level names visible (assumed); 6 SCL. | **Equivalent.** Both hard-code 6 where the case gives no count. PW also names the rows. |
| 147551 Section header | tabpanel > text exact (x2). | `setupPanel.getByText(text,{exact})` (x2). | Both texts `equals` (exact). | Both texts visible (exact match). | **Equivalent.** Neither checks "heading" as a role. |
| 147552 Guidance collapsed, toggles | button "How to set up Cascading levels"; tabpanel > `listitem` (count, `nth 0`). | `guidanceToggle` (panel-scoped); `page.getByText(bullet)` **page-wide**. | Step 1: 0 list items in the panel; steps 2/3: `at_least 1`, first visible / 0 again. | `aria-expanded` not true / true / not true; bullet 1 hidden / visible / hidden; bullet 2 visible. | **Different.** TCM's collapse check covers **every** bullet; PW checks only bullet 1, page-wide, but reads the expanded state and two bullet texts. Both miss the question mark icon and chevron. |
| 147553 All five guidelines | tabpanel > `listitem` count and `nth 0..4`. | `setupPanel.getByRole('listitem')`. | Count 0, then count 5 and each `nth` `equals` the case text. | Count 0 and `aria-expanded`, then `toHaveText(array)` (exact, ordered, count implied). | **Equivalent** |
| 147554 One row per DEF level in order | `.phr-dcl-level-label` (count, `expect_order`); SCL page-wide. | `.phr-dcl-level-row`; `.phr-dcl-level-label`; `.phr-dcl-level-label i.fa-sitemap`; SCL inside the rows. | 6 labels; order Group..Sub Department (`expect_order`, part-of match); 6 SCL. | 6 rows; labels `toHaveText` exact list; **6 sitemap icons**; 6 row-scoped SCL. | **PW stronger** (the case's per-row sitemap icon; exact label text). Both rightly treat the environment's 6 as the case's "known set" (DB order confirmed in the audit). |
| 147556 Caption with Level 1..N | SCL `nth 0` (visible, click); SCL count; `role=option` page-wide (count, order). | Raw only: row 1 > SCL; `getByRole('button',{name,expanded:true})`; row 1 `listbox > option`. | SCL visible; **6** SCL; **6 options, Level 1..6 in order**. | Caption button expanded; options `toHaveText(['Level 1'..'Level 4'])` and `toHaveCount(4)` (the case) → **red** by design. | **PW stronger.** TCM is **FITTED** to the environment's 6 DEF rows; PW keeps the case and records "Not met" in the md. PW has no test in the SL suite for it until the precondition or the case changes. |
| 147557 Only Level 1 selectable | SCL `nth 0`; `option "Level N" exact` page-wide; button "Level 1" page-wide; button "Delete cascading level for Group". | row 1 SCL; `rowOptions(1)`; `rowOption(1,level)`; `rowLevelButton(1,'Level 1')`; `deleteLevelButton('Group')`. | Level 1 enabled; 2..6 disabled (named); Level 1 shown; delete for Group. No option count. | Options **exactly** Level 1..6 (proves "to Level N" and the count); Level 1 enabled, rest disabled; row 1 shows Level 1; delete for Group. | **PW stronger** (list-exact, row-scoped). Both miss the trash icon. PW's comment claims "with a trash icon", but nothing checks it. |
| 147558 Only Level 2 selectable lower down | SCL `nth 0` twice (after assignment `nth 0` is the next unassigned row); options page-wide; delete for Sector. | rows 1 and 2 by id; `rowSelects` count; `rowOption(2,level)`; `deleteLevelButton('Sector')`. | Counts 6 → 5; Level 1 shown; Level 2 enabled, 1/3..6 disabled (named); Level 2 shown; delete for Sector. | 5 SCL left and row 2 still SCL; Level 2 enabled, rest disabled (named); row 2 shows Level 2; delete for Sector. | **Equivalent.** PW targets the row explicitly; TCM's `nth 0` lands on the right row only because assigned rows stop being "Select cascade level". Neither proves "and higher" with a count. |
| 147559 Assigned row: select disabled, delete | SCL `nth 0`; button "Level 1" exact; options page-wide; Escape. | row 1 by id; `rowOption(1,level)`; **`rowCaptionOption(1)`**. | Level 1 shown; delete for Group; Levels 1..6 disabled; after Escape Level 1 still shown. | Same, plus `rowCaptionOption(1).toHaveCount(0)`: the list has no caption to choose, so the row stays on Level 1. | **PW stronger.** It turns the case's impossible action ("choose the caption") into a stated, checked fact, which is exactly the pending rewrite's wording. TCM silently swaps the action for Escape. Both miss the trash icon; neither clicks a disabled option. |
| 147572 Edit mode, Edit disabled | button Edit; SCL `nth 0`; SCL count; Reset/Save. | `rowSelects` + `.nth(i)` loop; Edit/Reset/Save exact in the panel. | Edit visible, disabled; **first** SCL enabled; 6 SCL (EXTRA); Reset/Save enabled. | 6 SCL, **each** enabled; Edit visible, disabled; Reset/Save enabled. | **PW stronger** ("the rows are editable" checked on every row). |
| 147575 All rows unassigned | `.phr-dcl-level-label`; SCL count; Edit; SCL `nth 0`. | `rowSelects` loop; `assignedLevelButtons` (`/^Level \d$/`); `deleteLevelButtons` (`/^Delete cascading level for/`). | 6 labels and 6 SCL (every row); Edit disabled; first SCL enabled. | Edit disabled; 6 SCL each enabled; **0** assigned buttons; **0** delete buttons. | **PW stronger** (minor). TCM's 6 = 6 already proves "every row"; PW adds explicit absence and edit mode on every row. |
| 147577 Reset Yes clears, Info toast | SCL `nth 0` twice; dialog "Reset Cascading Levels?" > text / No / Yes; `{text}` toast page-wide. | rows 1 and 2 by id; `resetDialog(title)` + heading + `getByText(message)`; `toast(message)` = `role=alert` filtered `hasText`, `toastLabel(...,'Info')`. | Level 1/2 shown; Reset enabled; dialog; message **`equals`**; No/Yes; after Yes 6 SCL, "Level 1" hidden; toast text (substring). | Level 1/2 in their rows; Reset enabled; dialog, heading, message (substring), No/Yes; after Yes dialog hidden, 6 SCL, **0** `Level N` buttons, toast visible with **"Info"** label. | **PW stronger** (Info kind, every assignment gone, dialog closed). TCM's message check is exact where PW's is substring. |

### 3.2 Define Company Objectives

| Case | TCM locator(s) | Playwright locator(s) | TCM checks | Playwright checks | Verdict |
|------|----------------|-----------------------|------------|-------------------|---------|
| 160128 "Search cycles" filters | button "FY2027 Company OKRs"; heading; listbox "Select Evaluation Cycle"; searchbox "Search cycles"; option by full name "FY2027 Company OKRs 01/10/2026 - 30/09/2027 Active". | Raw only. Real menu walk (`#sidebar-toggle-menu`, `#phr-sidebar-menu-list` text, `toPass` re-click guard); listbox "Select Evaluation Cycle" > `getByText('Select Cycle',{exact})`; searchbox **inside** the listbox; every `option` in the listbox. | Step 1 `unchecked`; heading and cycle; list + search box + the option; "Company" keeps it, **"zzzz" hides it**; cleared → visible. | Heading; list, **"Select Cycle" header** (the case's quote), search box; types "2026" (the case's example), **every** remaining option contains it; cleared → count equals the starting count; Escape closes, cycle unchanged. | **Different.** PW checks the quoted header and the whole list; TCM is the only one that proves the filter hides anything. With one cycle, neither can show "the other cycles hide". PW recorded the unmet precondition; TCM did not. |

### 3.3 Self Service: Goal Cascading (BSC)

| Case | TCM locator(s) | Playwright locator(s) | TCM checks | Playwright checks | Verdict |
|------|----------------|-----------------------|------------|-------------------|---------|
| 139161 Nearest-end in-progress cycle auto-selected | headings; button "FY2026 Annual Performance Review"; text dates and "In Progress" page-wide; css `a.phr-dropdown-item` (`expect_each`, `.is-active` count); link by full name. | `#dco-cycle-dates` inside `main#phr-content`; `visibleStatus` (`getByText` exact + visible filter); `cycleOption` link by regex name; `li:has(a.phr-dropdown-item)` with `.phr-eval-item-name/-dates/-status-text`; `.phr-eval-item-check i.fa-check` visible-filtered. | Both headings; cycle name; dates and "In Progress" anywhere visible; every option matches "`dd/dd/dddd - dd/dd/dddd <words>`" (min 2); 10750 option has class `is-active`; exactly 1 active. | Dates **in the bar** (exact); exactly 1 visible "In Progress"; selected option visible, `is-active`, **check mark visible**; **5 options** (assumed); each option: name non-empty, date-range regex, status in {In Progress, Upcoming, Completed, Closed} (assumed); exactly 1 visible check mark. | **PW stronger** (the visible check mark the case names, bar-scoped dates, field-by-field options). PW adds an environment count (5). Neither checks "at the top" or computes "nearest end date"; both hard-code FY2026 (TCM by precondition 10750, PW by an "Observed live" comment). |
| 160202 Goal Title stops at 300 | `cell "Common Automation" exact > checkbox` (click); `cell "Pending"` page-wide; text "1 Selected"; button "Add Goal" (`at_least`, `nth 0`); dialog "Add Goal"; textbox "Goal Title"; text "300 / 300". | `row /Common Automation/` filtered by `cell "Pending" exact`, click the row, `row.getByRole('checkbox')`; `addGoalButtons` (count, `.first()`); dialog-scoped textbox and counter. | A Pending cell somewhere; 1 Selected; at least 1 Add Goal; dialog; counter `equals`; value regex pins exactly 300 characters. | Pending **in that row**; checkbox checked; **2** Add Goal buttons (fitted); dialog; value `toHaveValue('A'x300)`; counter exact; Cancel; dialog hidden; row unticked. | **PW stronger** on step 3, but the 2 is **fitted** (the case seeds 4 groups). The limit checks are equivalent. |
| 160203 Description stops at 5000 | as 160202, textbox "Briefly describe the intent and expected outcome", "5000 / 5000". | as 160202. | as 160202 for 5000. | as 160202 for 5000. | **PW stronger**, same caveat. |
| 160204 Currency field, base currency | as 160202; `dialog > button "LKR"`; button "Select metric", `option "Currency"`; `expect_position` LKR relative to `button "Currency" exact` (the Metric button after selection) and `spinbutton "Target"`; searchbox "Search"; options USD/LKR/EUR/GBP. | `metricButton('Select metric')`; `currencyButton('LKR')` in the dialog; `dialogOption(...)`; `currencySearch` (dialog searchbox). | Step 4: no LKR button (weaker than "no Currency field"); step 5: LKR shown, **right of Metric and left of Target**; step 6: search box, "us" → USD shown, **LKR, EUR, GBP hidden**. | Step 3 as 160202; step 4 "Select metric" shown and **0** LKR buttons; step 5 LKR shown; step 6 search box, "US" → USD shown, **AFN** count 0. | **TCM stronger.** Only TCM checks the case's "between Metric and Target", and it tests more non-matches. Both prove "no Currency field" only as "no LKR button". |
| 160205 Yes/No replaces Target | `option "Yes/No"`; `spinbutton "Target"`; `radio "Yes"`/`"No"` page-wide. | `targetSpinbutton` (dialog); `radiogroup "Target"` > `radio`. | Target box hidden; Yes and No shown; Yes checked, No not. | Metric shows Yes/No; Target box **count 0**; radios **inside the "Target" group** (= "under Target"), **exactly 2**; Yes checked, No not. | **PW stronger** (placement through the group and the count of two). |
| 160207 Reset clears every field | as 160202; textboxes/spinbuttons by name; `radio "Optional"`/`"Mandatory"`; button "Today"; `heading "Add Goal"` page-wide; `cell "No records available"`. | dialog-scoped fields; `getByText('Optional',{exact})` clicked (the input is hidden); `button "2026-10-15"` (day buttons are named by ISO date); `dialogHeading` (level 2, in dialog); `noRecordsCells`; `cardSummary('0 Goals · Min 1, Max 5')`. | Values `equals`; date `^\d{4}-\d{2}-\d{2}$` ("Today"); after Reset all blank **including Target**, Mandatory on, Optional off, heading; after Cancel **2** "No records available" (fitted). | Values exact, including **date 2026-10-15 inside the cycle**; after Reset blank (no Target check), Mandatory on, Optional off, dialog open, heading in the dialog; after Cancel **2** empty cells and **2** "0 Goals" summaries (fitted, assumed). | **Different.** PW meets "a date inside the cycle"; TCM checks Target blank. Both check "no goal added" after Cancel rather than after Reset, and both are **fitted** to 2 cards. |

### 3.4 Notes behind the tables

- **Row targeting.** The cascading-level rows are role-less divs. Playwright resolves them by the app's own ids
  (`#dcl-dd-1` is the top row, "verified live on 2026-10-21") and classes (`.phr-dcl-level-row`, `.phr-dcl-level-label`),
  all inside the page object. TCM reaches a row only through its button's caption plus `nth`. That is unambiguous while
  exactly one dropdown is open, but it says "first unassigned row", not "row 2". A case that assigns a lower row first
  would point TCM's `nth 0` at the wrong row.
- **Option lists.** TCM matches options page-wide (`role=option "Level 2" exact`). That works because only one list is open
  and only visible options count. Playwright scopes to `row n > listbox > option`. Both are sound today; Playwright's
  survives a second open list.
- **Test ids.** Neither tool has one; the app ships no `data-testid` (stated in the page object).
- **CSS use.** TCM uses `.phr-dcl-level-label`, `a.phr-dropdown-item` and `.is-active`. Playwright uses
  `.phr-dcl-level-row/-label`, `#dcl-dd-n`, `#dco-cycle-dates`, `li:has(a.phr-dropdown-item)`, `.phr-eval-item-*`,
  `i.fa-sitemap` and `i.fa-check`, all in page objects; the linter bans raw CSS in specs.
- **Strictness.** TCM's loose names (`button "Edit"`, `"Save"` page-wide) would trip on a second visible "Edit". The runner
  then fails loudly ("matches N"), never silently. Playwright uses `exact` and the panel as scope for the panel buttons,
  but its `cycleButton(name)` and `metricButton(label)` are loose and page-wide. `addGoalButtons.first()` picks the first
  card silently, which is what the case asks for here.
- **Timing.** TCM's runner waits for `#ehrmLoader` before every action. Playwright had to promote `waitForHomeRender` into
  `src/components/app-shell.ts` after the self-side home (`widgets/home/index?mvc=1`, 7.8 s) painted over Goal Cascading
  and cost 1-2 of 6 tests per run (the quirk entry "A slow home landing page ..." update). That is the flake protocol
  working: it was diagnosed and promoted, not wrapped in `toPass`.

## 4. Fitting, preconditions, provenance (question 3)

### 4.1 The four named hot spots

| Hot spot | Case says (live) | Environment | TCM | Playwright | Pending rewrite |
|----------|------------------|-------------|-----|------------|-----------------|
| 147556 DEF levels | "exactly 4 DEF levels"; lists Level 1-4 | 6 rows (Group..Sub Department) | **Fitted**: options = 6, Level 1..6. Precondition not declared; green. | **Held to the case**: Level 1-4 and count 4, red. `// FINDING` comment; md Preconditions row 2 "Not met ... Needs a 4-level hierarchy, or the case updated". Not refactored. | "one option per DEF level row ... Level 1 to Level N". TCM's 6 then matches, but only as a literal; the right check is "options = rows". PW needs a `/gen-test --sync`. |
| BSC perspectives | precondition seeds Finance, Customer, Internal Process, Learning & Growth (4) | 2 (Customer Focus, Operational Excellence) | 160207 **fitted** (`No records available` = 2). 160202-160205 use `at_least 1` (not fitted, but weaker). Nothing declared. | **Fitted in 5 specs**: Add Goal count 2 (160202-160205, 160207), plus 160207's 2 empty cells and 2 "0 Goals" summaries. `// Finding:` comment in each spec; `perspectiveCardCount: 2` and `emptyCardSummary` tagged `assumed`. **The md Preconditions were not updated.** | "The cycle has goal groups (perspectives) set up". The conflict goes away, but a count of 2 is still an environment value the case never states. |
| 160128 one cycle | "Multiple evaluation cycles ... in_progress, completed, upcoming, and drafted" | 1 non-draft OKR cycle (11538) | Declares only 11538. Checks a name match and a "zzzz" non-match. Audit FITTED for using the listbox's aria-label instead of the "Select Cycle" header. Unmet precondition not recorded. | md Preconditions row 2 "Not met ... Searching '2026' therefore hides nothing". The spec checks the header and that every remaining option contains "2026", which passes on one cycle. Held back from refactoring. | Only the menu path changes; the multi-cycle precondition stays, so both remain unable to prove "others hide". |
| 160207 | "Pick a date inside the cycle"; "No goal is added to the perspective card" | cycle 10750 ends 31/12/2026 | "Today" + shape regex: in the cycle today, outside it from 2027-01-01. Card count 2 fitted. | `2026-10-15` **invented** and inside the cycle, value checked exactly. But the day button is clicked by name: if the picker opens on the current month, it is not on screen from November 2026 (not verified). Card count 2 fitted. | No change. |

### 4.2 Other environment values written in as if they were the case's

| Value | TCM | Playwright (provenance tag) |
|-------|-----|------------------------------|
| 6 hierarchy rows / Level 1..6 | 10 scripts (counts 6 and 5, option lists; 9 in the audit plus 147556); declared nowhere | 8 refactored specs; `levelCount` `derived`, `levelOptions` `derived`, `levelNames` `assumed` |
| Cycle "FY2027 Company OKRs" / "FY2026 Annual Performance Review" | declared preconditions 11538 / 10750 | `cycleName` `assumed` in every case block |
| 139161 cycle option count 5, status vocabulary | not checked | `cycleOptionCount: 5` and `cycleStatuses` `assumed` |
| Subordinate "Common Automation", Pending | literal | `subordinate` `assumed` |
| Base currency LKR, USD, AFN | literal (LKR confirmed as base in the audit) | `baseCurrency`/`otherCurrency`/`nonMatchingCurrency` `assumed` |
| Level names Group / Sector / Department | literal | `topLevelName` etc. `assumed` |

Playwright's provenance vocabulary (`from-ticket`, `invented`, `assumed`, `derived`, `manual`) has no value for "observed
live in this environment". Everything the generator read off the screen is tagged `assumed` (meaning "picked one option
the UI offered") or `derived`. So an environment count and a genuine guess look the same in the run summary.

### 4.3 What Playwright recorded, and where

- **"Not met" preconditions:** two, both in test-case md Preconditions tables, dated "found 2026-10-21 (pipeline step 4)":
  - `test-cases/cascading-levels-unassigned-row-shows-the-select-caption.md` (147556);
  - `test-cases/define-company-objectives-search-cycles-box-filters-the.md` (160128).
- **`// Finding:` comments only**, with no md row:
  - the 5 BSC Add Goal specs (raw and refactored);
  - `_generated/define-company-objectives-...spec.ts` (menu names);
  - the raw 139161 spec (menu note).
- **`test.fixme` / `test.skip`:** none in any of the 20 specs.
- **App quirks added for these screens:** "Goal Cascading pre-renders the cycle selector's option list next to the cycle
  bar" (established 2026-10-21, healing 139161, `app quirk`, not yet promoted), and the 2026-10-21 update to "A slow home
  landing page paints over ..." (self home request, promoted).
- **Small drift:** the md for both raw-only cases says "One `test()` in `specs/<name>.spec.ts`", and neither file exists.

## 5. Data and safety (question 4)

| Concern | TCM | Playwright |
|---------|-----|------------|
| Saves | Nothing. `no_save: true` on all 20. Assignments, typing and metric choices stay in the page. | Nothing. Page-object header "PROTECTED BASELINE": no method presses Save, deletes a cascading level, picks a cycle or adds an objective; Save, delete and Add Objective are exposed only to assert on. |
| Shared state relied on | `perf_evaluation_cascade_level` empty for 11538 (declared as `needs_unchanged: ["OKR cascade levels saved"]` on the 14 admin scripts). Cycles 11538 (the only non-draft OKR cycle) and 10750 (in progress, ends 2026-12-31), declared as `preconditions`. | The same two cycles and the same empty cascade config, by name in data (`assumed`). Declared nowhere in code; a saved config or a closed cycle shows up as a failed assertion. |
| Clean-up | A trailing `cleanup: true` step per script: Escape, or `when_visible` then Cancel / No. It runs after the last step and also after a failed step. The ticked subordinate is not unticked (it is local and lost on navigation). | In the test body only: Escape or Cancel, then the subordinate is unticked and `not.toBeChecked`. No `afterEach`/`finally`, so a failure part-way leaves the dialog open (harmless; nothing is saved). |
| Accounts | Admin `evelyn1` (Evelyn1); BSC `gabriel` (Gabriel). | Admin `AutomationSL.performance-management.general` (Automation01); BSC `AutomationSL.performance.manager.bsc` (**Gabriel**). |
| Clash | Both suites sign in as **Gabriel** for the BSC cases. PeoplesHR allows one session per user, so running both at once logs one out mid-run. | |
| Time-dependence | 10750 stops being "In Progress" after 2026-12-31: 139161's selected cycle and every BSC precondition change. 160207 "Today" leaves the cycle on 2027-01-01. | Same cycle dependence. 160207's fixed `2026-10-15` stays inside the cycle, but clicking that day by name may fail from November 2026. |

## 6. Cross-cutting patterns

### 6.1 Locator philosophy on these screens
- **TCM:** role+name first, CSS only for role-less containers (`.phr-dcl-level-label`, `a.phr-dropdown-item`). Rows
  are reached by caption + `nth`, options and toasts page-wide. Relies on visible-only matching (which handles the hidden
  cycle list for free) and exactly-one-match for actions.
- **Playwright:** a page object per screen with every locator scoped to the panel, dialog, row id or group. CSS and ids
  are allowed there as a last resort and are commented with the date they were verified. Strict mode by default; `.first()` only
  for "the first card". Whole lists are compared with `toHaveText([...])` and `toHaveCount`.
- **More robust:** Playwright for the cascading rows, dialogs and radios (scoping makes the target explicit and
  survives a second match). TCM for the Goal Cascading cycle bar (visible-only matching needs no quirk entry). Both
  break on a renamed caption ("Select cascade level") equally.

### 6.2 Where a tool would PASS with the app wrong
TCM
1. 147556 passes with 6 options when the case requires 4 (fitted).
2. 147557, 147558, 147559: an extra enabled option, or a seventh level, passes (the options are named, never counted).
3. 147572, 147575: only the first row is proven editable.
4. 147577: a Warning or Error toast with the same words passes; a leftover "Level 2" passes.
5. 147559: the "choose the caption" half is replaced by Escape with no note.
6. Icons: 147546, 147552, 147557, 147559 pass with no sitemap, layer, question mark, chevron or trash icon (147554 too).
7. 139161: a visible check mark on the wrong option passes, because only the class is read.
8. 160202-160207: "Pending" may belong to another row; one card with Add Goal passes for "each card".
9. 160204: a Currency field with any value other than LKR passes "no Currency field".

Playwright
1. 147546, 147552, 147557, 147559: same icon gaps (147557's comment says it checks the trash icon; it does not).
2. 147552: collapse is proven for bullet 1 only, and page-wide.
3. 147558, 147559: options are named, not counted, as in TCM.
4. 160204: no placement check; only one non-matching currency; the absence check is as weak as TCM's.
5. 160207: Target is not checked after Reset.
6. 160128 (raw): passes on one cycle without showing anything hide.

### 6.3 Where a tool would FAIL with the app right
TCM
- The 10 scripts with the hard-coded 6 (or 5) fail if a DEF row is added, although 147546/147547/147572 give no count.
- 160207 fails the "inside the cycle" intent from 2027 without noticing (it still passes, which is worse).

Playwright
- The same hard-coded 6s (`levelCount`, `levelOptions`) in 8 specs.
- BSC: Add Goal count 2 fails as soon as the cycle gets the case's 4 groups, i.e. when the environment is fixed to match
  the case.
- 139161: exactly 5 options, and the status list, fail when a cycle is added or a new status label appears.
- 160207: the `2026-10-15` day button may not be on screen once the current month moves on.
- 147556: red today by design.

## 7. Recommendations

### 7.1 Playwright practices that would most improve TCM (question 6)

TCM already has some of the parts:
- the expected-result floor (`src-tauri/src/autorun/floor.rs`: every case step checked or `unchecked` with a reason);
- the seen-locator save check (`seen_check.rs`);
- `mark_autorun_suspected_defect`;
- precondition gating ("precondition not met: ...");
- the guide rules in `guide.rs`: "What a check asserts comes from the case's expected result" and "Live data. Do not copy
  a number ... For a count use `at_least` or `at_most` ..., not a hard-coded total".

The hard-coded 6s and 2s show those guide rules are text, not enforced. The Playwright practices below are ranked by how
much they would have changed this PBI.

1. **Hold to the case; record an unmet precondition instead of re-targeting the check.**
   *Where:* `.claude/commands/gen-test.md`, step 4 "Verify raw & heal": "Classify a failure before deciding what to do
   with it ... If it's a missing precondition, stop — do not delegate to the healer ... record the discovered
   precondition in the test case's Preconditions table". Bounds-block item 4 adds "do not rewrite assertions until an
   empty page satisfies them". Evidence: 147556's raw spec and md row 2.
   *For TCM:* a script-level "precondition not met (environment)" record. The run would report Blocked with that sentence,
   and the script would keep the case's value (4). Plus a save-time refusal of an `equals` count or option list the case
   does not state, unless it carries a reason.
2. **Provenance on every value the tool supplies, and a run summary of it.**
   *Where:* `.claude/agents/test-refactorer.md` "Provenance" (five values, stripped before tests see them); `CLAUDE.md`
   "Record provenance for every value the pipeline supplies itself" and "Test data provenance"; `gen-test.md` step 7
   "Run summary" ("the one place the pipeline surfaces its own guesses; report success only after printing it").
   *For TCM:* tag each literal in a script as from-case, environment (observed live, a value Playwright lacks and
   should have) or invented. Print the non-case ones after `save_autorun_script`. That would have shown 6, 5, 2,
   FY2026, Common Automation and LKR in this PBI at save time.
3. **A check-count floor that a repair cannot lower without declaring it.**
   *Where:* `test-refactorer.md` "Preserve every `expect(` call" (linter `assertion-count` against the raw spec, hard
   error); `.claude/agents/spec-editor.md` "What you may never do" and "Report a `## Assertions given up` section";
   `gen-test.md` step 4S "Never re-baseline an assertion floor".
   *For TCM:* `floor.rs` guarantees every step is checked, not that a repair keeps the checks it had. Record the checks
   per step at first save, and refuse a repair that drops one unless it names it in a "given up" list the person sees.
4. **A bounded heal with stop conditions and forbidden fixes, given to the healer verbatim.**
   *Where:* `gen-test.md` step 4, the "Bounds for this dispatch" block:
   - at most 3 fix-run cycles;
   - stop on a repeated failure signature;
   - stop on a login page, a missing precondition or a product defect ("a real bug must surface as a failure");
   - the flake protocol: "Two green out of three is a flake, not a fix";
   - "Fixes you may not make": no deleting or weakening an `expect(`, no `waitForTimeout`/`networkidle`/`evaluate`, no
     new `toPass`;
   - stop with `test.fixme()` and a one-line reason.

   Also `CLAUDE.md` "Retrying a genuinely flaky step": "**A real defect must surface as a failing test and be reported to
   a human.**"
   *For TCM:* put the same block in the `tcm:heal` skill and the repair path. The repair notes on 139161 and 160204
   (`last_repair`) are the kind of change it would gate.
5. **Never promote a red or unmet case.**
   *Where:* `gen-test.md` step 5: "Never refactor a spec that still contains `test.fixme(` or `test.skip(` ... a skipped
   test satisfies both the run in step 6 and the `assertion-count` rule while proving nothing", and "If any ticket's raw
   spec is fixme'd, do not refactor any of them". This is why 147556 and 160128 stayed raw-only.
   *For TCM:* do not set `checked: true` on a script whose case precondition is unmet, or whose checks were fitted. In
   this PBI all 20 scripts are `checked: true`, including 147556.

Also worth taking:

6. **Quirk records with a class, and product bugs called out loudly.** `gen-test.md` step 4.5 and the `docs/app-quirks.md`
   entry format: Symptom in a failing run / Cause / Established live / Permanent fix: not yet promoted / Class: `app
   quirk` | `environment flake` | `product bug`. "a defect the pipeline worked around is the single most important thing
   a human needs to see". TCM's `record_app_quirk` has no class or promotion state.
7. **Separate the writer from the runner.** `spec-editor.md` "You cannot run tests, and that is deliberate": an agent
   that can run tests iterates toward green, "and ... the cheapest route to green is deleting the step QA just asked
   for". For TCM, a case-rewrite edit to a script should be applied without a run, then verified by a separate step.
8. **Ask for the expected outcome before exploring.** `gen-test.md` step 1 (`--describe`): "The expected outcome, asked
   before any exploration. Asking afterwards would only be asking the human to confirm whatever the app happened to do."
   Also step 3's generator rule: "stop and report the blocked step verbatim. Do not guess a locator".
9. **Scope every locator to its container in one place.** The two page objects (`strategic-objectives.page.ts`,
   `goal-cascading.page.ts`) show the gain: `#dcl-dd-n`, the dialog, `radiogroup "Target"`. In TCM, a saved component or
   area-level "row n" locator would replace `SCL nth 0`.

**Which rules keep Playwright's agents from working around defects, and where they leak.**
- **The rules:**
  - the healer bounds block (stop conditions, forbidden fixes, `fixme` on stop);
  - the missing-precondition stop before any heal;
  - the `assertion-count` floor and the "Assertions given up" report;
  - "never refactor a fixme'd spec";
  - the `toPass` four conditions;
  - spec-editor having no Bash;
  - the doc-drift auditor's report-only stance ("You report; you never edit").
- **Where they leak:**
  - The vendor healer's own text says "Fixing assertions and expected values" and "do the most reasonable thing possible
    to pass the test". It is contained only because every dispatch must paste the bounds block (`CLAUDE.md` "What is
    bounded, and what isn't").
  - The missing-precondition rule is failure-triggered. The BSC specs were written to the environment's 2 cards during
    live generation, passed, and so were never classified. The finding stayed a code comment, not a Preconditions row.
  - Comments can overclaim (147557 "with a trash icon") or excuse a gap (147546 "unnamed images"), and no rule checks a
    comment against an assertion.

### 7.2 Playwright suite
1. Add "Not met" Preconditions rows to the six BSC md files (4 groups vs 2). Then, as with 147556, either assert the
   case's 4 (red until the environment matches) or wait for the pending rewrite and `/gen-test --sync`.
2. Assert the tab icons (`i.fa-sitemap`, `i.fa-layer-group`), the guidance `fa-circle-question` and `fa-chevron-right`,
   and the trash icon (`fa-trash-can`) beside the delete buttons in 147546, 147552, 147557 and 147559. Fix the 147557 comment.
3. 147552: count list items in the panel (0 collapsed) instead of one page-wide bullet text.
4. 160204: add the Metric < Currency < Target placement (bounding boxes, as 147546 does) and more than one non-match; prove
   "no Currency field" by the field's label, not by "no LKR button".
5. 160207: check Target is blank after Reset; pick the in-cycle date through the picker's month navigation, or derive it
   from the open month.
6. 160128: add a search that matches nothing, so the spec proves something hides.
7. Add a provenance value for "observed live", so environment counts are not reported as `assumed`.
8. Fix the "One `test()` in `specs/...`" line in the two raw-only md files.

### 7.3 TCM scripts (beyond the audit's list)
1. 147556: revert to the case's 4 and mark it blocked by the unmet precondition, or wait for the rewrite and check
   "options = rows" (`expect_count role=option` against the `.phr-dcl-level-label` count, not a literal 6).
2. 147557 / 147558 / 147559: add `expect_count role=option` to prove "and higher".
3. 147572: check every row's dropdown enabled (or count disabled ones = 0); 147575: add `expect_count` of
   `Delete cascading level for` = 0.
4. 147577: check the Info kind and `expect_count` of any `Level \d` button = 0.
5. 147559: check that the list has no "Select cascade level" option, or mark that half `unchecked` with the reason.
6. 139161: check the visible check mark (`.phr-eval-item-check i.fa-check` inside the active option), and scope dates
   to `#dco-cycle-dates`.
7. 160202-160207: chain "Pending" to the Common Automation row; 160205: scope the radios to the `radiogroup "Target"`.
8. 160128: also check the "Select Cycle" header (`p.phr-eval-menu-header`), and record the one-cycle precondition as unmet.

## 8. Evidence and limits

- TCM scripts are in `%APPDATA%\com.avinalwis.testcasemanager.v2\autorun\scripts\case-<id>.json`. All 20 were read in full;
  none uses `use_component`. Runs: `autorun\runs\run-1792553614589.json` and `run-1792555284684.json`. Account usernames
  come from `autorun\accounts\env-ce1b48bb.json` (no passwords read).
- Playwright files read:
  - `suites/sl/admin/performance/strategic-objectives/{pages,data,specs,test-cases}`;
  - `suites/sl/self/performance/goal-cascading/{pages,data,specs,test-cases}`;
  - the raw specs in `suites/_generated/` for 147556, 160128, 139161 and 160207;
  - `suites/_generated/index.json`, `src/navigation.json` (two entries), `src/users/users.json` (two keys);
  - `docs/app-quirks.md` (uncommitted additions);
  - `CLAUDE.md` ("Non-negotiable rules", "What is bounded", "Retrying a genuinely flaky step");
  - `.claude/commands/gen-test.md` (steps 1, 3, 4, 4.5, 5, 7);
  - `.claude/agents/*.md` (all six).
  - Pass and fail outcomes are from `reports/2026-10-2*.json`.
- Case text came from the live bridge, saved under the scratchpad's `so3/live-*.json`. The pending rewrites came from
  `D:\PMS_Module\HRM-PMS-NET\.test-cases\strategic-objectives-rewrites.json` (not yet imported; live text is the old version).
- The prior audit (`so1-audit.md`) was reused. Its script-side findings were re-confirmed against the current scripts,
  and its icon claims against `src/PeoplesHR.PMS.WebUI/Pages/CompanyObjective/Index.cshtml`, `_CascadingLevels.cshtml`
  and `wwwroot/VIewModels/CompanyObjective/cascade-levels.js`.
- The date-picker risk in Playwright 160207 is inferred from the locator, not tried.
- No test was run. Verdicts come from reading code.
