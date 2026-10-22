# Manage Performance Cycles (PBI 138110): TCM Auto Run vs the Playwright repo, after TCM's new pipeline

Date: 2026-10-22. This is a read-only comparison. Nothing was run and nothing was changed except this file.
It follows the structure of `manage-cycle-tcm-vs-playwright-2026-10-20.md` (called "the 10-20 doc" below).
It focuses on (1) locators and (2) what each tool checks.

## 1. Summary

**What changed since 10-20.** Only the TCM side changed. All 37 Manage Cycle scripts were regenerated with the
2.2.3-beta.20-22 pipeline, audited (`mc3-audit.md`) and healed (`mc4-heal-brief.md`). This includes 141734, which is
now scripted against 11555 "Draft Review 2026". In the latest full run (`runs/run-1792645498636.json`) 36 of 37
passed. 141701 fails: `expected text "Duration" but saw "DURATION"`.

The Playwright side has not changed since 2026-10-20 15:17. `PC-manage-cycle/{specs,pages,data,test-cases}`,
the ten raw specs and the Manage Cycle ids in `suites/_generated/index.json` still cover the same 10 cases:
141698-141704, 141706, 141708 and 141720. There is no `test.fixme` or `test.skip`. The other 27 cases exist only in the
backup `D:\PHR-PLAYWRIGHT-AUTOMATION-backups\manage-cycle-2026-10-20` (unchanged), so section 4 compares against
that backup again.

**Case text used as the yardstick.** This is the live text from the bridge (`/test-cases?ids=...`, paged), read
2026-10-22:
- 141721-141724 **include** the "cards are fetched again ... refreshes in place" rewrite.
- 141741 (step 4) and 141749 (step 6) still say **"Evaluation Rules"**, so the Eval Rules rewrite has not been
  imported. 141736 step 8 already said "Eval Rules".

**Verdict counts, then and now**

| Basis | Date | Equivalent | TCM stronger | Playwright stronger | Different | Total |
|-------|------|-----------:|-------------:|--------------------:|----------:|------:|
| Current Playwright (10 cases in both) | 10-20 | 1 | 0 | 8 | 1 | 10 |
| Current Playwright (10 cases in both) | **10-22** | **5** | **2** | **1** | **2** | 10 |
| Backup Playwright (other 27, indicative) | 10-20 | 19 | 5 | 0 | 3 | 27 |
| Backup Playwright (other 27, indicative) | **10-22** | **7** | **20** | **0** | **0** | 27 |

**Headline findings**

1. **The gap is closed on the 10 shared cases.** The new pipeline added what Playwright had: every card checked
   with `expect_each`, patterns, `expect_order` for the dropdown, the Plus icon and a relational `expect_position`,
   glyph classes, `toHaveCount(0)`-style absence with a save-time positive control, and cards reached by
   `div[data-cycle-id="<id>"]`, not by loose name. Playwright is still stronger on one case (141708, a run-time
   positive control). Two cases differ (141701, 141706).
2. **TCM now goes further than Playwright on "matches the cycle".** 141700 and 141701 read the cycle's own Preview
   (method, participant count, Period) and compare the card with it. Playwright compares the card with hard-coded
   strings that are tagged `assumed`/`derived`.
3. **On the other 27, TCM is now stronger than the backup on 20 of them.** It checks the cards re-fetch
   (`expect_response`), every card's status, and that the filter label names the option chosen. It checks every tile
   and every input type for read-only. Footer checks are scoped, and buttons are counted to prove "only X". It checks
   the exact banner text plus the amber colour, every input disabled (not the first N), and the Draft "no banner".
   None of the 27 is weaker than the backup.
4. **141701 is red because of TCM's tool, not the case or the app.** TCM reads rendered text, and Playwright's
   `getByText('Duration', {exact:true})` matches DOM text. Playwright's check should therefore pass where TCM's fails,
   but it was not run here. This is backlog 37 ("text as written, not as drawn").
5. **What remains TCM-side is mostly vocabulary (backlog 36 and 37) plus provenance gaps.** These values were taken
   from the environment without a `because`: the participant "1", aria-pressed, checked states, amber RGB values,
   and Preview labels such as "Customer Focus". The save check does not compare patterns, attributes, styles,
   booleans, `expect_visible` text or the exempt 0 and 1, so these slip through.

## 2. How each tool decides a step passed (changes since 10-20)

The 10-20 doc's table still holds for Playwright. These TCM rows changed:

| Aspect | TCM Auto Run (new scripts) | Playwright suite |
|--------|----------------------------|------------------|
| Card locator | `div[data-cycle-id="10750"]` (the cycle id) chained to the child, after "Find cycle card by exact name" (fill, wait `Showing 1-1 of 1`, wait `Open <name>`). Page-wide lists use `div[data-cycle-id]`. | `main#phr-content` > `getByRole('button',{name:'Open <name>',exact:true})`; list = `/^Open /`. |
| "Each card" | `expect_each` with `min: 1`, `child`, and `matches` / `equals` / `contains` / `not_empty`; the list must hold still for 750 ms. | `for` loop over `cards.nth(i)`. |
| Order | `expect_order` (texts contained, case-sensitive) plus `expect_count` 4. | `options.nth(i)` `toHaveAccessibleName(/^X \(\d+\)$/)`. |
| Position | `expect_position relation=in_top_right relative_to main#phr-content` (the element's centre in that quarter; no pixels). | `boundingBox()` with 40 px / 300 px thresholds. |
| Style | `expect_style` on computed `color` / `background-color`. | None in the current suite. |
| Network | `expect_response {method, url_contains, status}` within the step. | `searchCycle()` waits for the debounced search's own response. |
| Absence | `expect_count equals 0`. The save refuses a locator only ever seen matching nothing (a save-time positive control); there is no run-time control. | `toHaveCount(0)`, plus a run-time positive control in 141708 only. |
| Text read | Rendered text (CSS `text-transform` applies). | DOM text for `getByText`/`toHaveText` unless `useInnerText`. |
| Provenance | Step `because` entries for values the case does not hold. Patterns, attributes, styles, `expect_visible` text, booleans and 0/1 are not compared, so no `because` is demanded for them. | `_provenance` per data value (`from-ticket` / `assumed` / `derived` / `invented`). Regexes inside the page object carry none. |
| Preconditions | Declared per script (10750, 10763, 11171, 10316, 11555); `changes` on toggle cases; a `cleanup: true` step runs after a failure too. | None declared. |

## 3. Per-case comparison: the 10 cases present in both

### 3.1 Table

| Case | TCM locator(s) and checks (new) | Playwright locator(s) and checks (unchanged since 10-20) | Verdict 10-22 (10-20) |
|------|--------------------------------|-----------------------------------------------------------|------------------------|
| 141698 New Cycle + Plus icon top-right | button "New Cycle" visible; chain > `i.fa-plus` **visible**; `expect_position in_top_right` of `main#phr-content`. | Button visible; `i.fa-plus` **attached**; bounding box: right edge within 40 px, left edge in the right half, top within 300 px. | **TCM stronger**, narrowly (PW stronger). The icon is checked as rendered, not just present; the position check has no guessed pixel thresholds. PW's "flush right" is stricter than the case needs. |
| 141699 Card shows cycle name | `expect_each` page-1 card headings `not_empty`. Search "Annual Performance Review"; cards 10750, 10763 and 11171 heading `equals` the cycle name; each result heading contains the search text. `because` lists the three names (Avin's ruling). | Every card: h3 visible, not empty, card accessible name == `Open <h3>`. FY2026 and FY2025 exact heading after search. | **Equivalent** (PW stronger). PW ties each heading to its own card name; TCM checks one more named cycle. Neither shows that an arbitrary card's heading is *its* cycle's name. TCM uses 11171 with no precondition declared. |
| 141700 Subtitle method + count | `expect_each` page-1 `p` matches `^\d+° Evaluation · \d+ participants$`. View FY2026 and FY2025: Preview item nth 2 `equals "180° Evaluation"`, nth 4 `equals "1"`. Then search; both cards `^180° Evaluation · 1 participants$`; every result card matches the pattern. | Every card `^(?!.*\b(19\|20)\d{2}\b).+ · \d+ participants?$`; both named cards exact `180° Evaluation · 1 participants` (`assumed`). | **TCM stronger** (PW stronger). Only TCM checks "the method matches the cycle's method and the number matches the participant count", against the cycle's Preview. PW checks the no-year rule explicitly; TCM's anchored pattern excludes a year implicitly. TCM risk: positional `[class*=phr-pv-item-value]` nth 2 and nth 4. Both lock in "1 participants". The "180° Evaluation" and "1" values have no `because` ("1" is exempt from the save check; patterns are not compared). |
| 141701 Footer Duration + dates | `expect_each` page-1 `span.phr-mc-duration-value` matches the date pattern. Preview Period of FY2025 and FY2026 matches the dates. Search; both cards' value `equals` the dates; every card's `span.phr-mc-duration-label` `equals "Duration"` (**fails: rendered "DURATION"**). `because` for the 2026 dates. Preconditions 10750 and 10763. | Every card: `getByText('Duration',{exact:true})` visible and a period-pattern text visible. Both named cards' exact dates (`derived`). | **Different** (PW stronger). TCM checks more: dates against the cycle's own Preview, via a scoped span rather than any text. But its label check fails with the app right (tool gap, backlog 37). PW's DOM-text match gets the label right. Neither declares the case's Draft precondition. |
| 141702 Draft label | `expect_count` page-1 `div[data-cycle-id]` > text "Draft" exact `at_least 1`. | First card on page 1 with exact "Draft"; label visible. | **Equivalent** (Different). Both now take "any Draft card" from page 1, which the case allows ("any Draft"). Both rely on page 1 holding a Draft (newest first, per the 141724 ruling). |
| 141703 Published label | Find FY2026; `div[data-cycle-id="10750"]` > "Published" exact `equals`. Precondition 10750. | Search (response-aware); exact `Open <name>` card; "Published" visible; "Unpublished" count 0. | **Equivalent** (PW stronger). TCM now scopes by id, so the card is exact. PW's extra negative is redundant beside an exact "Published". PW waits for the search response; TCM waits for `Showing 1-1 of 1` and the Open button. |
| 141704 Unpublished label | Same with FY2025 / 10763 / "Unpublished". | Same pattern. | **Equivalent** (Equivalent) |
| 141706 Action icons by status | Steps 3-5 split as the case is. 10750: View > `i.fa-eye` and Edit > `i.fa-edit` **visible**, both texts `""`. 10763: eye and pencil visible. Draft 10316 (declared): Edit > `i.fa-edit` and Delete > `i.fa-trash-can` visible; `i.fa-eye` count 0 in the card. | Exact-name buttons visible; glyph classes **attached**; all six buttons `toHaveText('')`. Delete absent on the Published and Unpublished cards; View absent on the Draft card. Draft = first Draft on page 1. | **Different** (PW stronger). TCM: glyphs rendered, a deterministic declared draft, steps split as the case is. PW: proves "**two** buttons" (no Delete) on Published and Unpublished, and "no text label" on all three cards; TCM checks the text only on the Published card and never checks that Delete is absent there. |
| 141708 No toggle on Draft | Find draft 10316; "Draft" exact; `role=switch` count 0 in the card. The positive control is save-time only (10750 declared for it). | First Draft card on page 1; `getByRole('switch')` and `input[type=checkbox], [role=switch], label.phr-switch-item` count 0, no visibility filter; run-time positive control: count 1 on the FY2026 card. | **Playwright stronger**, narrowly (PW stronger). Its absence proof re-checks at run time and covers a wider control set. TCM targets a named draft, which is closer to the case's named "Draft Review 2026", but uses AUTOTEST 10316, not the ruled 11555 (see 6.1). |
| 141720 Status dropdown options | Click `button[aria-controls="mc-status-filter-panel"]`; `role=option` count 4; `expect_order` All, Draft, Published, Unpublished. | "Status:" label; button `/^All \(\d+\)$/`; `aria-expanded` true; 4 options in order, each `/^X \(\d+\)$/`; Escape; `aria-expanded` false. | **Equivalent** (PW stronger). Count and order, which are all the case states, are now in both, and case-sensitive `expect_order` keeps "Published" apart from "Unpublished". PW's count suffix and open/close state are extras the case does not state. |

### 3.2 Notes

- **Locators.** TCM moved from loose `Open <name>` chains to `div[data-cycle-id="<id>"]`. This is more robust:
  ids do not collide the way "AUTOTEST Annual Performance Review FY2026" could. It is tied to the record, so the
  precondition is what makes it portable. PW keeps exact role+name (`Open <name>`, exact), which is portable across
  environments but depends on unique names. Both reach the subtitle by `p`. Footer spans are reached by class in TCM
  (`span.phr-mc-duration-*`, which is more precise) and by text in PW. TCM's dropdown opener is a CSS
  `aria-controls`; PW's is a name regex with a count.
- **Positional.** TCM still has `nth` in the Preview reads (141700, 141701) and for the option picks in
  141721-141724. PW has `.first()` in `firstCardWithLabel` (141702, 141706, 141708).
- **Waits.** TCM components still wait for `Showing 1-1 of 1` plus the Open button, not for the search response
  (10-20 recommendation 9 is still open). Only 141717 uses `expect_response` for search.

## 4. The other 27 cases (TCM vs the backup Playwright)

"Old PW" means the 10-20 backup (unchanged). The backup's checks are as described in section 4 of the 10-20 doc;
this table states what TCM does now.

| Case | TCM now (locators and checks) | Old PW | Verdict 10-22 (10-20) |
|------|-------------------------------|--------|------------------------|
| 141709 Toggle on Published | id-scoped `role=switch` visible; `aria-checked="true"`; "Published" exact. | `toBeAttached` + label. | **TCM stronger** (TCM stronger) |
| 141710 Toggle on Unpublished | Same, `aria-checked="false"`, "Unpublished". | Same as 141709. | **TCM stronger** (TCM stronger) |
| 141711 Toggle Published OFF | `aria-checked` true + label. Click the switch; `role=dialog` count 0; toast container contains the case's exact sentence; `aria-checked` false; "Unpublished". The `cleanup` step flips it back only if it reads Unpublished. | Same checks; restore in the test body. | **Equivalent** on checks (Equivalent). TCM is safer: its restore runs after a failure too (6.3). |
| 141712 Toggle Unpublished ON | Mirror image (10763). | Mirror image. | **Equivalent** (Equivalent) |
| 141714 New Cycle opens wizard | Click New Cycle; heading "Create Performance Cycle"; `span.phr-progress-title` contains "Step 1" and "Cycle Setup" (the 9-step total was dropped from the locator). | Heading; progressbar contains "Cycle Setup". | **TCM stronger**, narrowly (Equivalent). "Step 1" is now read. TCM does not exit the wizard; nothing is saved. |
| 141717 Search, 800 ms | Fill "Annual"; `expect_response GET Search=Annual 200`; every card heading matches "Annual". Clear, full name, Enter; response `Search=FY2026`; `Showing 1-1 of 1`; every heading `^FY2026 Annual Performance Review$`. | CSS `[aria-label*="annual" i]` matching / non-matching; "2 page" hidden. No network. | **TCM stronger** (Equivalent). The re-fetch partly shows "refreshes in place". The 800 ms pause, "at once on Enter" and "no full reload" are unchecked in both (backlog 36). TCM records no `unchecked` reason on the step, and the step-4 response could come from the debounced fill rather than from Enter (mc4 issue 6). |
| 141721 Draft filter | Opener CSS; option nth 1; `expect_response StatusFilter=Draft`; filter label `^Draft`; every card has a visible exact "Draft" (`min 1`); "Published" and "Unpublished" exact count 0 page-wide. | `chooseStatusAt(1)`; Draft count = 10; Draft cards = 10. | **TCM stronger** (Equivalent). Checks the case's new "fetched again" wording; every card, not a fixed 10; the label proves which option was chosen. |
| 141722 Published filter | Option nth 2; response `StatusFilter=Active`; label `^Published`; every card "Published"; Draft and Unpublished absent. Preconditions 10750 and 10763. | `chooseStatusAt(2)`; labels hidden; search + named card. | **TCM stronger** (Equivalent) |
| 141723 Unpublished filter | Option nth 3; response `StatusFilter=Inactive`; mirror checks. | `chooseStatusAt(3)`. | **TCM stronger** (Equivalent) |
| 141724 All filter | Step 3 as 141721. Step 4: option nth 0; response `PageNumber=1`; label `^All`; a Draft card on page 1. Then search "Annual Performance Review" (ruled): Draft, Published and Unpublished cards each `at_least 1`. | Draft + search, named cards hidden; All, named cards visible. | **TCM stronger** (Equivalent). Weak point: `url_contains "PageNumber=1"` does not show the request was made "without a status filter"; a `StatusFilter=Draft` request also contains it. |
| 141727 Pager | Cards = 10 (the case says 10); `#mc-page-count` `^Showing 1-10 of \d+$`; Previous disabled; "1 page" `aria-current=page`; "2 page" and Next visible. Click Next; response `PageNumber=2`; `^Showing 11-\d+ of \d+$`; "2 page" current. | Same, plus Next enabled; cookie banner closed by hand. | **TCM stronger**, narrowly (Equivalent). The re-fetch shows "refreshes in place". Neither ties the step-4 total to step 3. TCM's "2 page" is a loose name; the app-quirks file says exact. |
| 141729 View mode | View component (progressbar "Step 9 of 9 — Preview"); heading; title contains "Step 9" and "Preview"; **all 8** step tiles disabled; textbox, checkbox, switch, combobox and spinbutton each count 0 in `main#phr-content`. | One disabled tile. | **TCM stronger** (Equivalent) |
| 141731 View footer | `div.phr-cc-nav-footer` > Close (exact) visible; footer buttons = 1; Previous, Exit, Save and Save & Continue count 0 **in the footer**. | Page-wide visible/hidden. | **TCM stronger** (Equivalent). The count of 1 proves "only Close", even with a renamed button. |
| 141733 View preview cards | 7 toggles: `aria-expanded` true for Cycle Setup, false for the rest. No button named "Edit…" in main, plus 7 exact names. Expand 6, all true; labels visible (Goals, Goal Planning, Self, Group By, Total Employees, Customer Focus); every `[class*=phr-pv-item-value]` not empty; no input roles; no "Ready to Publish" heading or "Cycle Status" text. | Same toggles; form controls hidden; vacuous visible-switch filter. | **TCM stronger** (TCM stronger). Still extra: toggle names carry "Ready" and a fixed 7-card list. "Customer Focus" and the other labels are 10750's data, with no `because`. |
| 141734 Edit Draft | Draft **11555 "Draft Review 2026"** (ruling, declared): Edit; heading; progressbar "Step 2 of 9 — Eval Rules"; heading "Evaluation Rules"; **alert count 0**. Cycle Setup: alert count 0; year, dates, Basic, Standard, 90° and 180° enabled; Copy from Previous. Advanced and 360° skipped (ruling). | AUTOTEST Draft First Incomplete 2027; enabled set; no banner check. | **TCM stronger** (Equivalent). The case's two "no warning banner" checks are now in. The progressbar name carries the 9-step total. |
| 141735 Edit Published, no submissions | Preview, alert count 0; Timeline: alert count 0, Auto-fill dates and Auto-balance enabled; Cycle Setup: alert visible, text matches `...integrity\.$`; year, dates, 3 config and 3 method cards disabled. | Same set. | **Equivalent** (Equivalent) |
| 141736 Submitted evaluations | evelyn1; 11171 (goal plan = evaluation, ruling, recorded in `precondition_repairs`). Preview alert count 0; 5 tiles, each: title, alert **exact** text, `expect_style` color `rgb(217, 119, 6)` and background `rgba(245, 158, 11, 0.08)`. | Shared general account; `toContainText`. | **TCM stronger** (Different). Exact text on every step plus "amber". The accounts still differ. The RGB values are from the environment and have no `because`. |
| 141737 Banner text | Preview alert count 0; Cycle Setup alert `equals` the sentence + amber style. | `toHaveText` (innerText). | **TCM stronger** (Equivalent). Amber is checked. |
| 141738 Editable fields | Fill Name and Description with AUTOTEST text (invented, typed); `expect_text equals`; "Choose files" (exact) enabled. Exits without saving. | `toHaveValue`; drop zone by `/Drop your files here/`. | **Equivalent** (Equivalent). The 10-20 locale problem ("No file chosen" in the name) is gone. |
| 141739 Locked fields | Year opener (CSS `aria-controls`), dates, 6 cards disabled; Standard and 180° `aria-pressed=true`. "No lock icon" and "click a card" not checked (mc4 issues 3 and 5). | Same; also `aria-pressed=false` on the other four. Same omissions. | **Equivalent** (Equivalent). The pressed states are 11171's data with no `because`, and the script records nothing to say why the lock icon and the click are left out. |
| 141741 Eval Rules locked | Tile "Eval Rules" (`because`; the live case still says "Evaluation Rules"); alert; 4 `#er-*-card` disabled + their checkboxes; checked true/true/true/false. | `aria-disabled` on the divs; `toBeDisabled` on the inputs; goals and comps checked. | **Equivalent** (Equivalent). TCM reads all four states (from 11171, no `because`). TCM's checkbox "disabled" can still be satisfied by the aria-disabled card around it. |
| 141744 Evaluators locked | spinbutton and slider nth 0-1 disabled; **`main input:not(:disabled)` count 0** (and at least 1 disabled); 3 toggles disabled; 20%/80% in the distribution bar (`because`). | First two of each. | **TCM stronger** (Equivalent). Covers "all" and "saved values stay". |
| 141747 Goal Groups locked | Distribute Evenly; checkboxes 0-2 and Edit row 0-1 disabled; enabled `input` and `button` count 0 in the table; every row's name not empty and weight `^\d+$`. | First-n, unfiltered `nth`. | **TCM stronger** (Different). "Saved values stay" is checked as shape, not compared with stored values. |
| 141748 Restricted Preview | "Step 9 of 9" + "Preview"; no "Ready to Publish" heading or "Cycle Status" text; footer: Previous and Exit visible, buttons = 2 (`because`), Save and Save & Continue count 0. | Same list; vacuous visible-switch check. | **TCM stronger** (TCM stronger) |
| 141749 Banner per step | `check_url contains "/hr/home/index"` at step 3 and step 7; banner per tile as the case says; "Eval Rules" (`because`). | `toHaveURL(/\/hr\/home\/index/)` (`addressPath` tagged `invented`). | **Equivalent** (Different). Neither compares the address with the one noted. The accounts still differ (evelyn1 vs general). |
| 141751 HR admin access | Heading; at least 1 card (15 s); `expect_hidden role=alert` as the stand-in for "no access-denied message". | Next page visible; card count = 10 (`invented`). | **TCM stronger** (Equivalent). The stand-in is weak: a denial page without `role=alert` passes (backlog 37, absence of unseen words). |
| 142514 Republish | Edit FY2025 (Preview); no "Ready to Publish" or "Cycle Status"; Previous and Exit visible, Save and Save & Continue count 0, **page-wide**; Exit + Continue; switch click; toast; `aria-checked` true; "Published"; `cleanup` restores. | Same flow; vacuous switch check. | **TCM stronger** (TCM stronger). It did not get 141748's footer scoping and button count, so "Previous and Exit only" is not proven. |

## 5. Cross-cutting patterns

### 5.1 Locators
- TCM: role+name first; `exact` on tiles and footer buttons; card by `data-cycle-id`; CSS where no role exists
  (`span.phr-progress-title`, `div.phr-cc-nav-footer`, `#mc-page-count`, `#er-*-card`, `[data-phr-toast-container]`,
  `[class*=phr-pv-item-value]`). An action still needs exactly one match, so duplicates fail loudly.
- Playwright: unchanged. Strict page-object members scoped to `main#phr-content`, exact `Open <name>`, and a deep
  inheritance chain.
- Robustness: TCM's id scoping is the most precise and depends on declared records. PW's exact names are portable
  when names are unique. TCM's remaining fragile spots are `nth` in the Preview (141700, 141701), option `nth` 0-3
  (141721-141724), and `Ready` in the toggle names (141733).

### 5.2 Assertion style
- TCM now reads content: `expect_each` with patterns on every card, exact strings for known records, `expect_order`,
  state attributes (`aria-checked`, `aria-current`, `aria-expanded`, `aria-pressed`), computed style, network
  responses, and counts of 0 or N to prove "only".
- Absence: TCM uses `expect_count equals 0` with a save-time positive control. PW uses `toHaveCount(0)`, with a
  run-time control only in 141708.
- Exact vs partial: TCM uses exact text for labels, the banner (141736, 141737), headings and toast sentences
  (contains, within the toast container). The 141735 banner is a suffix regex, as the case says "ends with".

### 5.3 Request and network checks
| Case | TCM | Playwright |
|------|-----|------------|
| 141717 search | `Search=Annual`, `Search=FY2026` responses | backup: none |
| 141721-141723 status filter re-fetch | `StatusFilter=Draft / Active / Inactive` | backup: none |
| 141724 All re-fetch | `PageNumber=1` (does not show the filter was dropped) | backup: none |
| 141727 next page | `PageNumber=2` | backup: none |
| Card searches in components | none (waits for `Showing 1-1 of 1`) | current: `searchCycle()` waits for the response |

## 6. Fitting, data and safety

### 6.1 Fitting beyond Avin's rulings
**TCM.**
- **141708** stands the AUTOTEST draft 10316 in for the case's "Draft Review 2026". The ruled cycle for that name is
  11555, which now exists and is used by 141734. 141708 predates it (saved 03:01, 11555 first used 04:48). Its
  precondition `why` does say it "stands for the case's draft".
- **141701** is *not* fitted any more: the label is exact "Duration" and the run fails honestly (d26).
- **141720-141724** no longer demand the count suffix. **141714** dropped the 9-step total.
- **Values taken from the environment, with no `because`.** None contradicts the case, but the provenance is not
  recorded:
  - 141700: "180° Evaluation" and "1" (and the `1 participants` pattern);
  - 141733: "Customer Focus", "Goal Planning", "Total Employees" and similar labels;
  - 141736 and 141737: amber as two RGB values;
  - 141739: Standard and 180° pressed;
  - 141741: the four checked states.

  They escape the save check because patterns, attributes, styles, `expect_visible` text, booleans and 0/1 are not
  compared (guide, `save_autorun_script`). With a `because`: 141699 names, 141701 2026 dates, 141744 20%/80%,
  141748 "2", and 141741/141749 "Eval Rules".
- **Cycle-name substitutions** are recorded as preconditions (id + name). Only 141699 (a `because`) and 141708
  (precondition `why`) say which of the case's names they replace. The other 19 do not say "stands for 'Annual Review
  2025'".
- **Stand-ins and omissions are not recorded in the scripts.** These are in `last_repair` or `mc4-tcm-issues.md`
  only: the 141751 alert stand-in, the 141739 lock icon and click, and the 141717 timing. The scripts carry no
  `gives_up` entries and no step `unchecked` for them, so the run report shows these cases as fully passed.

**Playwright (current).** Substituted names are tagged `assumed`, dates and `Showing 1-1 of 1` `derived`, and the
subtitle `assumed`. That is correct. Not recorded:
- the 141702/141706/141708 "first Draft on page 1" stand-in for "Draft Review 2026" appears only in a comment;
- the count-suffix regex in 141720 lives in the page object, not the data, so it has no provenance;
- 141698's pixel thresholds are untagged.

The **backup** tagged several substitutions `from-ticket`, which is wrong: FY2026 in `publishedCard`, FY2027 in
`submittedEdit`. That provenance must not be carried forward.

### 6.2 Accounts and preconditions
- TCM: evelyn1 for all 37. It declares 10750 / 10763 (publish), 11171 (publish, goal plan submitted), 10316 and 11555
  (cycle-setup). Undeclared:
  - 141699's use of 11171;
  - the "Draft not past Preview" precondition in 141700 and 141701;
  - a Draft on page 1 (141702, 141724);
  - more than 10 cycles (141727).
- Playwright: `AutomationSL.performance-management.general`; no preconditions declared in the current suite or in
  the backup.

### 6.3 What each changes, and clean-up
- TCM: only 141711 (10750), 141712 and 142514 (10763) flip publish state. Each declares `changes`, runs last in the
  saved order, and ends with a `cleanup: true` step. That step runs after a failed step too; it returns to the area,
  finds the card, and flips only if the label shows the changed state (`when_visible`). 141738 types into Name and
  Description and leaves through "Exit wizard without saving". View and Edit cases end with a Close or Exit cleanup
  step. Every other script is `no_save`. 141714 leaves the Create wizard open without saving.
- Playwright: the current 10 are look-only. The backup's toggle tests restored in the test body (no `finally` or
  `afterEach`), so a failure strands the cycle. TCM's guarded cleanup closes the risk the 10-20 doc raised for both
  tools.

## 7. What still keeps TCM behind (or short of the case)

| Gap | Cases | Backlog |
|-----|-------|---------|
| Rendered text instead of DOM text: CSS `text-transform` breaks exact labels (the only red case) | 141701 | **37** (text as written) |
| No timing window on a request and no "no full reload" check | 141717 (also "refreshes in place" in 141721-141727) | **36** |
| No exact or remembered address | 141749 | **37** (`check_url equals`, `same_as`) |
| Absence of words the app never shows | 141751 (alert stand-in), 141739 (lock icon) | **37** |
| No paging until a card is seen | 141724 (a search stands in, ruled) | **37** (`page_until`) |
| A disabled control cannot be clicked to show "does not respond" | 141739 step 6 | not in 33-38 (mc4 issue 5) |
| `expect_response` matches requests made earlier in the same step, and it cannot assert a parameter is absent | 141717 step 4 (Enter vs debounce), 141724 ("without a status filter") | not in 33-38 (mc4 issue 6) |
| Provenance gaps: patterns, attributes, styles, `expect_visible` text, booleans and 0/1 escape the `because` demand | 141700, 141733, 141736, 141737, 141739, 141741 | extends **33/34** (provenance) |
| Gave-ups not recorded in the script, so the run shows a full pass | 141717, 141739, 141751 | extends **33/35** ("assertions given up") |
| Divergences ruled in chat still need clicking through in the app | d1-d26 | **38** |
| Absence has a save-time positive control only, so it is not re-checked at run time | 141706 (eye), 141708 (switch), 141731/141748 footer | none (PW 141708 pattern) |
| Smaller script items | 141706 (Delete absent and empty text on the Unpublished and Draft cards); 142514 (footer scope and count 2); 141708 (use 11555); components (wait for the search response); 141727 ("2 page" exact) | heal follow-ups |

**Did the new pipeline close the gap?** Yes. On the 10 shared cases TCM went from 0 stronger and 8 weaker to 2
stronger and 1 weaker. On the 27 backup-only cases it went from 5 stronger to 20 stronger, with none weaker. The
items that keep TCM short of the case are now mostly missing vocabulary (36, 37) and bookkeeping (provenance and
gave-ups), not weak checks.

## 8. Evidence and limits
- TCM scripts: `%APPDATA%\com.avinalwis.testcasemanager.v2\autorun\scripts\case-<id>.json` (37, saved 2026-10-22
  02:52-04:48 UTC). Components: `projects\peopleshr__hrm-b9614466-components.json`. Last run:
  `runs\run-1792645498636.json`. Save-check and positive-control rules: `src-tauri/src/autorun/guide.rs`.
- Playwright: `suites/sl/admin/performance/PC-manage-cycle/*` and `suites/_generated/index.json` (unchanged since
  10-20), `docs/app-quirks.md` (Manage Cycle entries: pager exact names, late debounced search, cookie banner, footer
  Duration span, 0x0 switch). Backup at `D:\PHR-PLAYWRIGHT-AUTOMATION-backups\manage-cycle-2026-10-20` (unchanged).
- Case text: live bridge read on 2026-10-22, saved in the session scratchpad as `cmp4-cases.json`.
- Audit and heal: scratchpad `mc3-audit.md`, `mc4-heal-brief.md`, `mc4-tcm-issues.md`.
- Nothing was run. Whether Playwright's `getByText('Duration')` passes against the CSS-uppercased label comes from
  reading the code: Playwright matches DOM text. It was not tried.
