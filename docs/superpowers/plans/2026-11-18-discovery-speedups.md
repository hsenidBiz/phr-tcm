# Discovery speed-ups (page changes, fill_form) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `discover_autorun_actions` answer prints only what changed on the page since that reader's last page, and a discovery-only `fill_form` step fills a form from label -> value pairs and answers the concrete actions it ran.

**Architecture:** Page changes are worked out where the page is rendered (`browser/snapshot.rs::render_read`), under a task-local scope set by the route (the same pattern `page_compact` uses for the side-menu collapse), so the text printed and the lines filed as seen come out of one place and the existing filing path is untouched. `fill_form` travels through the discovery batch as a marked stand-in action (the same pattern `screenshot` uses), is resolved field by field against the live page just before it runs, and each field's concrete action runs through the existing single-action path, so recording, budgets, the no-save guard, `heard` and explore keeps apply to each field unchanged.

**Tech Stack:** Rust (Tauri 2 backend), CDP driver (`browser::cdp::Driver`), serde_json with `preserve_order`. Tests: one integration binary, `src-tauri/tests/suite/`.

**Spec:** `docs/superpowers/specs/2026-11-18-discovery-speedups-design.md` (approved by the owner 18 November 2026). Read it before your task; it is the authority where this plan is silent.

## Global Constraints

- Every Rust test is an integration test in `src-tauri/tests/suite/<module>.rs` with a `mod <module>;` line in `tests/suite/main.rs`. Never a `#[cfg(test)]` module in `src/`, never a new file directly under `tests/`.
- A test touching process-wide state (the page memory statics, env vars) takes its lock from `tests/suite/serial.rs` (`serial::page_menu()` is the existing one for page memory; reuse it).
- Run one module: `cd src-tauri && cargo test --test suite <module>::`. Never set `CARGO_TARGET_DIR` or `--target-dir`. One build/test command at a time.
- No change to scripts, runs, replays, or the Playwright export. `fill_form` is never saved in a script or component.
- `get_autorun_page`, `start_autorun_discovery`, `replay_autorun_to_step` and `try_autorun_action` keep the page they answer today (their page is remembered).
- What is filed as seen never shrinks: the whole page is still read; a changes answer additionally files every `+` line it printed.
- Exact copy (verbatim):
  - `page: unchanged since your last answer`
  - `page: changes since your last answer ("page": "whole" for the page)`
- Page line cap: `PAGE_LINES_DEFAULT` (120).
- `fill_form` fields: at least 1, at most 30. Form-control roles: `textbox, searchbox, spinbutton, combobox, listbox, checkbox, radio, switch`.
- No user-facing text names a URL, a host or a path; never log a `fill` value.
- `src/bindings.ts` is generated; regenerate with `cargo test --test bindings` only if a command signature changes (none should).
- Never name hidden features in code comments, docs or commits.
- Commits: `git commit -F - <<'EOF' … EOF` from Bash, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Never stage `.test-cases/`.

## Review Focus

1. **`"page": false` then an action.** The reader never saw the left-out page, so the memory must stay at the last page it did see; the next answer diffs against that. (Test in Task 2.)
2. **A failed answer (status not 200) or a page that could not be read** remembers nothing and prints no diff. (Task 2.)
3. **A page with a repeated line** (two `button "Delete"` rows in a table, one removed): the diff must mark one `-` line under the right row, not mark both or none. (Task 1.)
4. **A label that matches a control in two roles** (a `textbox "Name"` and a `combobox "Name"`): counts across all roles, so it is "matched 2", never a silent pick. (Task 4.)
5. **A `fill_form` whose earlier field changes the page** (picking a country reveals a "State" field): each field is resolved just before it runs, not all up front. (Task 5.)

---

### Task 1: The page-changes core (pure)

**Files:**
- Create: `src-tauri/src/autorun/page_changes.rs` (add `pub mod page_changes;` in `src-tauri/src/autorun/mod.rs`)
- Modify: `src-tauri/src/autorun/page_compact.rs` (`forget` also calls `page_changes::forget`)
- Test: `src-tauri/tests/suite/page_changes.rs` (+ `mod page_changes;` in `main.rs`)

**Interfaces:**
- Produces:
  - `pub const UNCHANGED_PAGE: &str = "page: unchanged since your last answer";`
  - `pub const CHANGES_HEADER: &str = "page: changes since your last answer (\"page\": \"whole\" for the page)";`
  - `pub struct Change { pub marker: char /* '+', '-' or ' ' for an ancestor */, pub line: String, pub new_index: Option<usize> /* index into the new lines, for '+' */ }`
  - `pub fn diff(old: &[String], new: &[String]) -> Vec<Change>`: the changed lines, each under its unmarked ancestors (by indentation, as `query` prints them), each ancestor once, in page order. Empty when nothing changed.
  - `pub fn render(changes: &[Change]) -> String`: `CHANGES_HEADER`, then one line per change: `+`/`-` then the line with its indentation kept; an ancestor line is printed with a leading space so it lines up.
  - `pub fn remembered(n: SlotNo, owner: &str) -> Option<(String, Vec<String>)>`: the address (fragment removed) and whole lines last given to that reader.
  - `pub fn remember_whole(n: SlotNo, owner: &str, address: &str, lines: Vec<String>)`
  - `pub fn forget(n: SlotNo)`
  - `pub fn same_address(a: &str, b: &str) -> bool`: equal once each loses its `#fragment`.

- [ ] **Step 1: Write the failing tests** in `tests/suite/page_changes.rs`:
  - `a_new_button_in_a_dialog_shows_under_the_dialog`: old `["RootWebArea \"x\"", " main \"\"", "  dialog \"Confirm delete\"", "   text \"Loading...\""]`, new replaces the text line with `"   text \"Delete Ann Perera?\""` and adds `"   button \"Yes\" -> {\"role\":\"button\",\"name\":\"Yes\"}"`. `render(&diff(..))` equals, line for line: the header; ` RootWebArea "x"`; `  main ""`; `   dialog "Confirm delete"`; `-   text "Loading..."`; `+   text "Delete Ann Perera?"`; `+   button "Yes" -> {...}`.
  - `nothing_changed_is_an_empty_diff`: `diff(&a, &a).is_empty()`.
  - `one_of_two_equal_rows_removed_marks_one_line`: a table with rows `row "Ann"` / `button "Delete"` and `row "Bo"` / `button "Delete"`; remove the Bo row and its button. Exactly two `-` changes (`row "Bo"`, its `button "Delete"`), and no `+`.
  - `an_ancestor_is_printed_once_for_two_changes_under_it`.
  - `memory_is_per_slot_and_reader_and_forget_clears_it` (hold `serial::page_menu()`): remember for `(SlotNo(1),"a:")`; `remembered(SlotNo(1),"b:")` and `remembered(SlotNo(2),"a:")` are `None`; `page_compact::forget(SlotNo(1))` makes `remembered(SlotNo(1),"a:")` `None`.
  - `the_fragment_does_not_change_the_address`: `same_address("https://h/p#a", "https://h/p#b")`, and `!same_address("https://h/p", "https://h/q")`.

- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite page_changes::` — expected: fails to compile (module missing).

- [ ] **Step 3: Implement** `page_changes.rs`. The diff is an LCS over whole lines (O(n·m) with a `u16`/`u32` table is fine at a few thousand lines; above 4000×4000 cells return a diff that marks everything, which rule 6 then turns into the usual page). Memory: a `Mutex<Vec<(usize, String, String, Vec<String>)>>` capped at `4 * SLOT_COUNT` entries, as `page_compact::LAST` is.

- [ ] **Step 4: Run** the module's tests — expected: PASS.

- [ ] **Step 5: Commit** `feat(v2): page changes - line diff and per-reader page memory`.

---

### Task 2: Page changes in discovery answers

**Files:**
- Modify: `src-tauri/src/browser/snapshot.rs` (`render_read`, `snapshot_read_within`)
- Modify: `src-tauri/src/autorun/page_changes.rs` (the task-local scope)
- Modify: `src-tauri/src/ai_bridge/autorun_common.rs` (`page_whole`, `compacted` gains the mode)
- Modify: `src-tauri/src/ai_bridge/mod.rs` (routes at ~753, 780, 785, the try route, the replay route ~914, the get_autorun_page route)
- Test: `src-tauri/tests/suite/page_changes.rs`, `src-tauri/tests/suite/page_compact.rs` (existing tests must still pass)

**Interfaces:**
- Consumes: Task 1's `diff`, `render`, `remembered`, `remember_whole`, `same_address`, `UNCHANGED_PAGE`.
- Produces:
  - `pub enum Mode { Remember, Changes }` in `page_changes`.
  - `pub async fn scoped<F: Future>(n: SlotNo, owner: &str, mode: Mode, fut: F) -> F::Output` in `page_changes`: runs a page read with the reader's memory available; when the read inside it is a whole-page read (no `within`) that answered, what it gave the reader is remembered on the way out (see the rules below).
  - `pub fn page_whole(body: &str) -> bool` in `autorun_common`: `"page": "whole"` (trimmed, any case), exported from `ai_bridge` next to `page_full`.

How the rules land (spec "The answer, in order of the first rule that applies"):
- `"page": false`: the route does not scope the read for changes or memory (nothing is remembered; the old memory stays).
- `"page": "full"`: unchanged path; the route scopes it `Remember` (the full tree is what the reader has).
- `"page": "whole"`: scoped `Remember`: the usual page is printed and remembered.
- discover action / actions with none of those: scoped `Changes`.
- start, try, replay, get_autorun_page: scoped `Remember`.
- Inside `snapshot_read_within` with `within == None` and a scope present: read the address (move `ai_bridge::autorun_page::page_address`'s JS into a `browser` helper both can call; the ai_bridge one keeps its signature), keep the uncut, uncollapsed printed lines (`printed_lines(..).0` texts, before `page_compact::collapse` and `within_cap`) as the "whole lines". In `Changes` mode, when `remembered` exists and `same_address` holds: an empty diff makes the read's `text` exactly `UNCHANGED_PAGE`; otherwise when `render(&diff)` has fewer lines than the usual page text would have (`text.lines().count()`), the read's `text` is the rendered changes. Either way, the read's `lines` (what is filed) are the usual page's lines plus the `SnapLine` data of every `+` line (by `new_index`), deduplicated. In every other case the read is exactly today's.
- On a 200 answer, the scope remembers `(address, whole lines)`. A non-200 answer or a read that failed remembers nothing.
- The JSON single-action answer and the batch's text answer both carry the read's `text`, so no answer-format change is needed beyond the text itself.

- [ ] **Step 1: Write the failing tests** (snapshot-level, using `render_frames_read` style inputs and the scope directly; hold `serial::page_menu()`):
  - `a_second_read_with_one_new_line_answers_changes`: scope `Changes`, read page A, then page A + one button: text starts with `CHANGES_HEADER` and contains `+` and the button line.
  - `the_same_page_twice_answers_unchanged`: second text == `UNCHANGED_PAGE`.
  - `a_changed_address_answers_the_usual_page`.
  - `a_page_changed_everywhere_answers_the_usual_page` (diff not shorter).
  - `remember_mode_prints_the_usual_page_and_remembers_it`: `Remember` then `Changes` with the same page gives `UNCHANGED_PAGE`.
  - `a_plus_line_past_the_cap_is_filed`: a page of 130 lines where the new line is line 125: the read's `lines` contain that line's data even though the usual page cuts at 120.
  - `page_whole_reads_the_option`: `page_whole(r#"{"page":"whole"}"#)`, `page_whole(r#"{"page":" Whole "}"#)`, `!page_whole(r#"{"page":"full"}"#)`, `!page_whole("{}")`.
  - Route-level (use the fake discovery browser the `autorun_discovery` suite module already uses; put these tests in `page_changes.rs` if the fake is reachable, else in `autorun_discovery.rs`): `page_false_leaves_the_memory_alone` (Review Focus 1: action with page, action with `"page": false`, action with page → the third diffs against the first), `a_failed_answer_remembers_nothing` (Review Focus 2).
- [ ] **Step 2: Run** `cargo test --test suite page_changes::` — expected: FAIL.
- [ ] **Step 3: Implement** as described above. Keep `page_compact`'s collapse working exactly as today for the usual page.
- [ ] **Step 4: Run** `cargo test --test suite page_changes::` then `cargo test --test suite page_compact::` then `cargo test --test suite browser_snapshot::` — expected: PASS.
- [ ] **Step 5: Commit** `feat(v2): discovery answers print only what changed on the page`.

---

### Task 3: fill_form - reading the step and refusing it in saves

**Files:**
- Modify: `src-tauri/src/autorun/action_help.rs`
- Modify: `src-tauri/src/ai_bridge/autorun_try.rs` (`tried_action_from`, `describe_action`)
- Test: `src-tauri/tests/suite/fill_form.rs` (+ `mod fill_form;`)

**Interfaces:**
- Produces (in `action_help`):
  - `pub const FILL_FORM_KIND: &str = "fill_form";`
  - `pub const SAVED_FILL_FORM: &str = "fill_form is a discovery action: save the actions its answer listed";`
  - `pub const FILL_FORM_MAX_FIELDS: usize = 30;`
  - `pub enum FieldValue { Text(String), Bool(bool) }` (a number is read as its text)
  - `pub struct FillForm { pub within: Option<Target>, pub fields: Vec<(String, FieldValue)> }` (order as sent)
  - `pub fn fill_form_from(raw: &Value) -> Option<Result<Action, String>>`: `None` for any other kind; else the marked stand-in action, or the refusal.
  - `pub fn fill_form_ask(action: &Action) -> Option<FillForm>`: the step a stand-in carries.
  - The stand-in is `Action::CheckText { value: "\u{1}fill_form:" + <compact JSON of the original object> }`, exactly as `screenshot_action` marks a picture.
- Refusals (exact wording is the builder's, each names what is wrong): a key other than `kind`, `within`, `fields` (lists the keys it takes, as `screenshot_from` does); `fields` not an object; 0 fields; more than 30; a value that is not text, a number or true/false; a `within` that is not a locator.
- A saved script or component holding `fill_form`, or a `check_text` carrying the stand-in mark, is refused with `SAVED_FILL_FORM` (extend `refuse_raw_saved_action` and `refusal_for` the way they handle `screenshot`, including nested actions).
- `describe_action` of the stand-in: `fill_form <n> fields` (never a value).
- `tried_action_from` reads `fill_form_from` right after `screenshot_from`.

- [ ] **Step 1: Write the failing tests** in `fill_form.rs`:
  - `fields_keep_their_order`: `{"kind":"fill_form","fields":{"B":"1","A":true,"C":3}}` → `fill_form_ask` gives `[("B",Text "1"),("A",Bool true),("C",Text "3")]`.
  - `within_is_read_as_a_locator`: `"within":{"role":"dialog","name":"Add employee"}` → `Some(Target::One(..))`.
  - `stray_key_is_refused_naming_the_keys`: `{"kind":"fill_form","fields":{"A":"x"},"submit":true}` → `Err` containing `submit` and `within, fields`.
  - `no_fields_and_thirty_one_fields_are_refused`.
  - `a_list_value_is_refused`.
  - `a_saved_script_with_fill_form_is_refused`: `refuse_raw_saved_action(&json!({"kind":"fill_form","fields":{"A":"x"}}))` → `Err(SAVED_FILL_FORM)`; the same inside a `when_visible`'s nested actions.
  - `describe_never_shows_a_value`: `describe_action(&stand_in)` == `"fill_form 2 fields"` and contains no field value.
- [ ] **Step 2: Run** `cargo test --test suite fill_form::` — expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --test suite fill_form::` and `cargo test --test suite action_help::` — expected: PASS.
- [ ] **Step 5: Commit** `feat(v2): fill_form step - read, described, refused in saves`.

---

### Task 4: fill_form - matching a label to one control and choosing its action

**Files:**
- Create: `src-tauri/src/browser/fill_form.rs` (add `pub mod fill_form;` in `browser/mod.rs`)
- Test: `src-tauri/tests/suite/fill_form.rs`

**Interfaces:**
- Consumes: `action_help::{FieldValue}`, `locator::{resolve_explained, Target, LocatorStep}`.
- Produces:
  - `pub const FORM_ROLES: [&str; 8] = ["textbox","searchbox","spinbutton","combobox","listbox","checkbox","radio","switch"];`
  - `pub enum Planned { Act { action: Action, locator: Target, matched: usize }, AlreadySet, Failed(String) }`
  - `pub async fn within_target<D: Driver>(d: &mut D, within: &Target) -> Result<Result<(Target, usize), String>, CdpError>`: the `within` as it will be written (with `nth: 0` on its last link when it matched several) and how many it matched; `Err` text when it matched no visible element.
  - `pub async fn plan_field<D: Driver>(d: &mut D, within: Option<&Target>, label: &str, value: &FieldValue) -> Result<Planned, CdpError>`
- Matching: for each role in `FORM_ROLES`, count visible matches of `{role, name: label, exact: true}` chained after `within`; sum across roles. Exactly 1 → that one, locator written with `exact: true`. 0 → repeat with `exact` left out (case-insensitive contains). Still not exactly 1 → `Failed` saying how many matched (`"no form control is named \"<label>\""` / `"<k> form controls match \"<label>\""` — builder's wording).
- Kind and action (spec table): read via CDP on the one handle: tag name, `type`, `isContentEditable`, checked state (`.checked`, else `aria-checked == "true"`).
  - `input` text-like (`text, email, number, tel, url, search, date, time, datetime-local, month, week`, or no type), `textarea`, contenteditable, with `Text` → `Fill { selector, value }`.
  - `input type=password` → `Failed("passwords are never typed by an assistant")`.
  - `select` with `Text` → `SelectOption { selector, label: Some(text), value: None, .. }`.
  - checkbox / switch with `Bool(b)`: state == b → `AlreadySet`; else `Click`.
  - radio with `Bool(true)`: checked → `AlreadySet`; else `Click`. radio with `Bool(false)` → `Failed("a radio is turned off by choosing another")`.
  - a `combobox`/`listbox` that is not a `select`, or any other element → `Failed` naming its role and saying to use a saved component or explicit actions.
  - a value of the wrong type (text for a checkbox, true/false for a text box) → `Failed` saying what the control takes.
- The written locator is `Target::One({role, name, exact?})` or, with `within`, `Target::Chain(within links + that step)`.

- [ ] **Step 1: Write the failing tests** with a fake `Driver` (copy the smallest existing fake that answers `resolve` and `Runtime.callFunctionOn`, e.g. from `tests/suite/browser_snapshot.rs` or `browser_page_until.rs`; keep it in `fill_form.rs`):
  - `a_text_box_gets_fill_with_an_exact_locator`
  - `a_native_select_gets_select_option_by_label`
  - `a_ticked_checkbox_asked_true_is_already_set` and `an_unticked_checkbox_asked_true_is_clicked`
  - `radio_false_fails`, `password_fails`, `a_custom_combobox_fails_naming_its_role`
  - `a_label_matching_in_two_roles_fails_with_matched_2` (Review Focus 4)
  - `exact_first_then_contains`: no exact `"Email"`, one `"Work email"` → `Act` with no `exact`.
  - `within_matching_two_writes_nth_0_and_says_2`
- [ ] **Step 2: Run** `cargo test --test suite fill_form::` — expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --test suite fill_form::` — expected: PASS.
- [ ] **Step 5: Commit** `feat(v2): fill_form matches each label to one control and picks its action`.

---

### Task 5: fill_form in discover_autorun_actions

**Files:**
- Modify: `src-tauri/src/ai_bridge/discovery.rs` (`discover_actions_heard`, `autorun_discover_action`, `batch_rows`/`Row`)
- Modify: `src-tauri/src/autorun/discovery_map.rs` only if a helper is needed to file a whole with its count (prefer calling `record_matched_counts`)
- Test: `src-tauri/tests/suite/fill_form.rs` or `autorun_discovery.rs` (where the fake discovery browser is)

**Interfaces:**
- Consumes: Task 3 `fill_form_ask`; Task 4 `within_target`, `plan_field`, `Planned`.
- Produces: `pub(crate) async fn run_fill_form<B: DiscoveryBrowser>(slot, root, organization, project, form: &FillForm, heard: &mut Heard, at: usize, stop_on_failure: bool) -> FillFormOutcome` and `pub(crate) struct FillFormOutcome { pub lines: Vec<String>, pub actions: Vec<Action>, pub failed: bool, pub blocked: usize, pub stopped: Option<(u16, String)> }` (`stopped` when the browser failed or the discovery ended).

Behaviour:
- A single `action` that is a `fill_form` goes through the batch path (`discover_actions_paged` with one action), so both shapes answer the same text.
- In the batch loop, a `fill_form` stand-in is one of the 20 (`refuse_batch_size` unchanged). For it: resolve `within` once; then for each field in order: check the call budget (`action_budget(1, call_elapsed_ms())`) and the stop flag as the loop does for an action; `plan_field` (just before running it, Review Focus 5); on `Act`, run the concrete action through `discover_action_heard` (one `heard` index per field), file its locator as seen with `record_matched_counts(.., &locator, matched, matched, now)` in the discovery's area, and add it to `actions` when it worked; `AlreadySet` adds nothing; a `Failed` field or a failed action stops the step when `stop_on_failure`.
- Explore keeps and `types_a_credential` happen per field already, because each field's action goes through `discover_action_heard`. The no-save guard's `blocked` counts add up.
- Answer, in the batch's text, at the step's position:
  ```
  3. fill_form (within dialog "Add employee": matched 2, the first used - nth 0)
     First name: ok {"kind":"fill","selector":{"role":"textbox","name":"First name","exact":true},"value":"Ann"}
     Active: ok, already set - nothing done
     Password: failed: passwords are never typed by an assistant
     actions: [<every action it ran, as JSON, ready to paste>]
  ```
  The `within ...` part appears only when `within` was given; the `matched` part only when it matched several. Values are the assistant's own and appear in the JSON as sent (never logged: `applog` lines use `describe_action`).
- The page after the batch is read as today (so Task 2's changes rule applies).

- [ ] **Step 1: Write the failing tests** with the fake discovery browser:
  - `fill_form_runs_each_field_and_lists_the_actions`: two text fields → two `fill` rows `ok`, an `actions:` line with both, and both locators recorded as seen (the discovery map's `wholes` or link counts contain them, so a save of the listed actions is accepted).
  - `a_failed_field_stops_the_step_by_default` and `stop_on_failure_false_carries_on`.
  - `a_single_fill_form_action_answers_like_a_batch`.
  - `fill_form_counts_as_one_of_twenty`: 20 actions including one `fill_form` with 5 fields is accepted.
  - `a_field_shown_by_an_earlier_field_is_found` (Review Focus 5: the fake reveals "State" after "Country" is selected).
- [ ] **Step 2: Run** `cargo test --test suite fill_form::` — expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --test suite fill_form::` then `cargo test --test suite autorun_discovery::` — expected: PASS.
- [ ] **Step 5: Commit** `feat(v2): fill_form fills a form in one discovery step`.

---

### Task 6: fill_form in try_autorun_action

**Files:**
- Modify: `src-tauri/src/ai_bridge/autorun_try.rs` (`autorun_try`, `try_action`)
- Test: `src-tauri/tests/suite/fill_form.rs`

**Interfaces:**
- Consumes: Task 3, Task 4.
- Behaviour: a `fill_form` tried runs each field through the try path the same way Task 5 runs it through the discovery path (plan just before, run, file the locator with its count, stop on the first failure), and the try's answer carries the same per-field lines and `actions` array (as a `fill_form` field of the JSON answer: `{"lines": [...], "actions": [...]}`). The try's page is unchanged (it is scoped `Remember`, Task 2).
- [ ] **Step 1: Failing tests:** `a_tried_fill_form_answers_its_actions`, `a_tried_fill_form_stops_at_a_failed_field`.
- [ ] **Step 2: Run** `cargo test --test suite fill_form::` — expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --test suite fill_form::` and `cargo test --test suite autorun_try::` (if that module exists; else the module holding try tests) — expected: PASS.
- [ ] **Step 5: Commit** `feat(v2): try_autorun_action takes fill_form`.

---

### Task 7: Guide, tool text, example, full suite

**Files:**
- Modify: `src-tauri/src/autorun/guide.rs` ("Seeing the page" ~1907, "Discovering the app" ~2031, next to the `screenshot` line ~2079)
- Modify: the `discover_autorun_actions` and `try_autorun_action` descriptions (in `src-tauri/src/ai_tools.rs` and/or `src-tauri/src/mcp.rs`, wherever the description text lives; keep both in step if both carry it)
- Modify: `src-tauri/src/autorun/action_help.rs` (an example for `fill_form`, marked discovery-only, where examples live, without adding it to the saved-kind list `kinds()` that the guide tests compare to `Action`)
- Modify: `src-tauri/src/autorun/roles/*.md` only where they list discovery-only actions next to `screenshot`

Copy (the guide, briefly; builder may tighten wording but not change meaning):
- Seeing the page: after the first answer, a discovery action's page shows only what changed since your last page from that browser - `+` lines that appeared and `-` lines that went, under the lines they sit in - or `page: unchanged since your last answer`. A new address, or changes longer than the page, gives the usual page. `"page": "whole"` asks for the usual page; `"page": "full"` the whole tree; `"page": false` none.
- Discovering the app: `{ "kind": "fill_form", "within": <locator, optional>, "fields": { "<label>": <text, number or true/false>, ... } }` fills up to 30 fields in order, each matched by its accessible name to exactly one form control; it fills text boxes, picks native lists by label, and ticks checkboxes, switches and radios only when they need it. Password fields, custom dropdowns and date pickers fail (use a saved component or explicit actions). Its answer lists the actions it ran: save those - a script never holds `fill_form`.

- [ ] **Step 1:** Update the texts. If a test pins guide sections or tool descriptions (`autorun_guide.rs`, `ai_tools.rs` suite modules, `docs-site` guard), run it and update only what the new text legitimately changes; never weaken a guard.
- [ ] **Step 2: Run** the full Rust suite: `cd src-tauri && cargo test --tests -- --skip autorun_recorder::` then `cargo test --test suite autorun_recorder::` — expected: all pass (report the counts).
- [ ] **Step 3: Run** `npx tsc --noEmit` and `npm test` from the repo root — expected: pass (no frontend change is expected; this proves the bindings did not move).
- [ ] **Step 4: Commit** `docs(v2): guide and tool text for page changes and fill_form`.
