# Formatting In Test Case Text Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** test case text in TCM is Markdown (steps: bold, italic, line breaks, lists as "• " lines, code fences as plain lines; prerequisites: full formatting), written to Azure DevOps as the HTML each field supports and read back as Markdown, shown formatted in the app, and written meaningfully by the assistant.

**Architecture:** one Rust module converts both ways (`case_markdown.rs`); `steps_xml.rs` and the prerequisites write/read paths use it; saves skip unchanged text by a rule that treats old whitespace-collapsed copies as unchanged; the webview renders case text through one `CaseText` component on the existing `renderMarkdown` sanitiser; the writing guide and `validate_cases` steer the assistant.

**Tech Stack:** Rust (`pulldown-cmark` already a dependency), React 19 + TS (`marked` + DOMPurify already), vitest, Rust integration tests.

**Spec:** `docs/superpowers/specs/2026-10-18-case-markdown-design.md`

## Global Constraints

- Steps XML only through `src-tauri/src/steps_xml.rs` (step ids start at 2); saves stay merge-only.
- A case that renders as plain text today renders and saves the same until someone edits it (anti-goal).
- No Markdown symbol shows literally in Azure DevOps (`**`, `- `, ```` ``` ````) (anti-goal).
- Never render HTML from Azure DevOps or an import unsanitised.
- No new dependency; no Azure DevOps field converted to Markdown storage.
- Step HTML out: `<B>`, `<I>`, `<BR/>` only (Avin: steps offer bold, italic, underline). Prerequisites HTML out: `<p>`, `<br>`, `<strong>`, `<em>`, `<ul>`/`<ol>`/`<li>`, `<code>`, `<pre><code>`.
- Unchanged rule, verbatim: text counts as unchanged when it equals the current value **or** equals the current value with its whitespace collapsed.
- Formatting warnings only on new or changed text; warnings, never refusals.
- UI: colours from tokens, icons from `src/lib/actionIcons.ts`, the UI consistency gate never weakened.
- Rust tests only in `src-tauri/tests/suite/`; one test run at a time.

## Review Focus

1. A step `Run the following query. SELECT COUNT(*) FROM t WHERE a * b > 1` with no fence → no italics anywhere (in ADO, in the app), and it round-trips unchanged. Test in Task 1.
2. A queue draft saved before this change (one-line step) over a case whose ADO step now has line breaks → the step is not rewritten. Test in Task 2.
3. Editing only a title of a case whose prerequisites hold ADO formatting → the prerequisites field is not in the update. Test in Task 3.
4. Prerequisites containing `<script>`-like text or an `onerror` attribute from ADO → rendered as text, never as an element. Test in Task 5.
5. A Bug filed from a failed run whose step holds `**Save**` → the Bug shows bold, no asterisks. Test in Task 6.

---

### Task 1: The converter

**Files:**
- Create: `src-tauri/src/case_markdown.rs` (+ `pub mod case_markdown;` in `src-tauri/src/lib.rs`)
- Test: `src-tauri/tests/suite/case_markdown.rs` (+ mod line)

**Interfaces:**
- Produces:
  - `pub fn step_md_to_html(md: &str) -> String` - escaped text; `**x**`→`<B>x</B>`, `*x*`/`_x_`→`<I>x</I>`; newlines → `<BR/>`; `- a`/`* a` → `• a` line, `1. a` → `1. a` line; a fenced block → its lines as escaped plain lines; inline code → its text; anything else (headings, tables, links, images, quotes, raw HTML) → its plain text. CommonMark rules (`pulldown-cmark`) decide what is emphasis.
  - `pub fn prereq_md_to_html(md: &str) -> String` - per the spec table; raw HTML events dropped (as `markdown.rs` does); headings/tables/links/images → plain text.
  - `pub fn step_html_to_md(html: &str) -> String` and `pub fn prereq_html_to_md(html: &str) -> String` - the reverse; tags outside the table → their text; `<U>` → plain; `<BR/>`, `<P>`, `<DIV>` → line breaks; `<li>` → `- `; `<pre>` → a fence; plain text escaped only where it would otherwise read as Markdown (a `*` that would open emphasis, a leading `#`, `- ` at a line start that was not a list item) so it reads back exactly.
  - `pub fn unchanged(new_md: &str, current_md: &str) -> bool` - the Global Constraints rule.

- [ ] **Step 1: failing tests:** a table-driven round trip for each row of the spec table, both fields; `count_star_is_never_italic` (Review Focus 1, step and prerequisites); `a_fence_in_a_step_becomes_plain_lines`; `markdown_symbols_never_reach_ado` (no `**`, `` ` ``, `#`, leading `- ` in any step HTML); `ado_html_with_b_strong_div_p_reads_back` (`<DIV><B>x</B></DIV>`, `<P><STRONG>x</STRONG></P>`); `plain_text_round_trips_exactly` for 20 real step texts from `.test-cases/*.json` (read at test time, skipped if absent); `unchanged_treats_a_collapsed_copy_as_unchanged`; `raw_html_is_dropped_on_the_way_in`.
- [ ] **Step 2:** `cd src-tauri && timeout 1800 cargo test --test suite -- case_markdown::` → FAIL; **Step 3:** implement; **Step 4:** PASS; commit `feat(v2): case text converter - Markdown in TCM, HTML in Azure DevOps`.

### Task 2: Steps

**Files:**
- Modify: `src-tauri/src/steps_xml.rs` (`step_text` ~64 / `step_node` ~69 write `step_md_to_html`; `parse_steps_xml` ~601 reads with `step_html_to_md`, no `collapse_ws`; `merge_steps_xml` unchanged), `src-tauri/src/ado/endpoints.rs` (`steps_patch` ~1672 compares with `case_markdown::unchanged`)
- Modify: every caller that relied on collapsed step text - find them (`parse_steps_xml` and `collapse_ws` have 5 callers outside `steps_xml.rs`; also the duplicate-title warning, the seen check, Auto Run step texts, the Playwright export's `cell()`); where a comparison needs one line, collapse there.
- Test: `src-tauri/tests/suite/steps_xml.rs`, `ado_endpoints.rs` (or the module that tests `steps_patch`), plus any caller's module that changes.

- [ ] **Step 1: failing tests:** `a_step_keeps_its_line_breaks_when_read`; `a_formatted_step_is_written_as_b_i_br`; `an_old_one_line_draft_does_not_rewrite_a_step_with_line_breaks` (Review Focus 2); `a_real_edit_adding_line_breaks_is_saved`; the existing steps tests unchanged in meaning.
- [ ] **Step 2:** FAIL; **Step 3:** implement; **Step 4:** `steps_xml:: ` plus the whole suite (`timeout 3000 cargo test --tests`) PASS - this change touches many readers; commit `feat(v2): steps keep their formatting and line breaks`.

### Task 3: Prerequisites

**Files:**
- Modify: `src-tauri/src/ado/endpoints.rs` (`create_test_case_doc` ~442/481-485, `update_test_case_doc` ~664/704-712), `src-tauri/src/steps_xml.rs` (`html_to_text` ~729 callers for the prerequisites field use `prereq_html_to_md`), the update paths that build `update_test_case_doc` (single, batch, queue) so they have the field's current value
- Test: the endpoints module tests, `import_parser` tests if touched

Rules: write `prereq_md_to_html`; an update omits the prerequisites field when `case_markdown::unchanged(new, current)` (current read from the work item fetched for the update - the same fetch that gives the steps' original XML; add it where a path lacks it); when the work item reports the field's `multilineFieldsFormat` as `Markdown`, write and read the Markdown as-is.

- [ ] **Step 1: failing tests:** `prerequisites_are_written_as_html_lists_and_code`; `a_title_only_edit_leaves_the_prerequisites_out` (Review Focus 3); `an_old_bulleted_value_reads_back_and_saves_unchanged` (`<div>• a<br>• b</div>`); `a_markdown_field_is_written_as_markdown`; `a_blank_policy_still_applies`.
- [ ] **Step 2:** FAIL; **Step 3:** implement; **Step 4:** module tests and `timeout 3000 cargo test --tests` PASS; commit `feat(v2): prerequisites keep full formatting both ways`.

### Task 4: The assistant writes it well

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (the writing guide ~9302-9500 gains a short "Formatting" section; `validate_cases` ~8527-8553 gains warnings), `src-tauri/src/steps_xml.rs` (`contains_html_markup` stays)
- Test: the writing-guide and validate tests (find them: `tests/suite/*writing*` or `ai_bridge`)

Guide meaning: formatting only where it helps; bold for the one value or control a step turns on; a list for several prerequisites; a fenced code block for any SQL or code (in a step its lines stay plain lines in Azure DevOps, the lead-in sentence before it - "Run the following query."); never headings, tables, links or whole-sentence emphasis; at most a few bold words in a step.
Warnings (new or changed text only, compared with the case's current text when updating): a heading, table, link or image in a step or prerequisites; more than 3 bold/italic spans in one step; a step mostly bold; an SQL-looking line (`SELECT|EXEC|UPDATE|INSERT|DELETE` followed later by `FROM|WHERE|SET|VALUES`) outside a fence.

- [ ] **Step 1: failing tests:** one per warning; `an_unchanged_old_case_raises_no_formatting_warning`; the guide names the fence rule and "not excessively".
- [ ] **Step 2:** FAIL; **Step 3:** implement; **Step 4:** PASS; commit `feat(v2): the assistant formats test cases sparingly`.

### Task 5: Showing and editing it in the app

**Files:**
- Create: `src/components/CaseText.tsx` (`<CaseText text={md} kind="step" | "prereq" />` - renders `renderMarkdown(md)` in a container using the app's prose tokens; a step's single line breaks render as breaks), `src/components/CaseText.test.tsx`
- Modify: `src/screens/ViewCases/CaseDetail.tsx`, `src/components/CaseStepsTable.tsx`, `src/screens/RunPanel/CasePreview.tsx`, `src/screens/RunnerWindow.tsx`, the queue's case view, Auto Run's case text views (find where step text is shown with `whitespace-pre-wrap`); editors `src/components/StepsEditor.tsx` (~125-135), `src/components/QueueCaseEditor.tsx` (~86), `src/screens/ExistingCases/CaseEditor.tsx` - multi-line `Textarea` keeping the Markdown, with a Preview toggle (`IconPreview` or the nearest existing icon in `actionIcons.ts`; add one there if none fits).
- Test: `CaseText.test.tsx`, the touched screens' tests, `src/ui-consistency.test.ts`

- [ ] **Step 1: failing tests:** `CaseText` renders `**Save**` bold and a fence as `<pre><code>`; `CaseText` never renders `<img onerror>` or `<script>` as elements (Review Focus 4); `count_star` text renders without italics; editors keep typed Markdown and Preview shows it formatted.
- [ ] **Step 2:** `npx vitest run src/components/CaseText.test.tsx` → FAIL; **Step 3:** implement; **Step 4:** `npm test` PASS and `npx tsc --noEmit`; commit `feat(v2): test case text shows its formatting`.

### Task 6: The review page, Bugs and help

**Files:**
- Modify: `src-tauri/src/import_parser/html.rs` (steps ~569-572 and prerequisites ~490-497 rendered through `markdown::to_html` - sanitised, raw HTML dropped), `src-tauri/web/cases-page.js` (~219 reads step text back - keep reading the Markdown source, e.g. from a `data-md` attribute, not `textContent`), `src-tauri/src/commands/bugs.rs` (~48 repro steps through `prereq_md_to_html`), `docs-site/src/content/*` (one sentence where editing test cases is described) then `npm run docs:build`
- Test: `tests/suite/import_parser.rs` (or the review-page tests), the bugs tests, `src/lib/casesPage.test.ts`, docs guards

- [ ] **Step 1: failing tests:** the review page shows a step's bold and a fence as code, and its live update still reads the Markdown back; a Bug's repro steps show `<strong>Save</strong>` not `**Save**` (Review Focus 5).
- [ ] **Step 2:** FAIL; **Step 3:** implement; **Step 4:** module tests, `npm test` PASS; commit `feat(v2): review page and Bugs show case formatting`.

### Task 7: Gate

- [ ] `cd src-tauri && cargo test --tests`, then `npm test`, then `npm run build` - one at a time, all green.
- [ ] Owner hand checks: create a case with a fenced query step and bulleted prerequisites, open it in Azure DevOps (bold/lines in steps, lists/code in prerequisites); edit a title only and confirm in the work item history that the prerequisites were not rewritten; an old Calculation Engine case opens unchanged.
