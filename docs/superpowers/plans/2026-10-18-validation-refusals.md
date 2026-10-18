# The Application's Refusals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** after each page-changing action, report what the page newly showed and any request the application refused (message, trace id); fail discovery actions on a refused request; remember refusals per area and list them in the assistant's guide.

**Architecture:** one shared check in `runner::run_expanded`'s action loop (which tries, discovery, replays and unattended runs all go through) fills two new fields on `ActionOutcome`. Discovery's JSON and batch lines, and run failure lines, read those fields. Rules live in the discovery map's `AreaMap.refusals`; the guide reads them; a new MCP tool lets the assistant record or remove one.

**Tech Stack:** Rust (Tauri 2, CDP driver in `src-tauri/src/browser`), page JS embedded with `include_str!`, vitest (jsdom) for that JS, tauri-specta bindings.

**Spec:** `docs/superpowers/specs/2026-10-18-validation-refusals-design.md`

## Global Constraints

- TCM only listens: no request sent to the application, no HTTP DELETE anywhere.
- A response body is read only for a counted refused request (Task 2's filter), never stored or logged, and only after `api_checks::shown_body` redaction is anything of it shown.
- A trace id, body or status is never written into the rules (`AreaMap.refusals`); the rule's message is the application's words as shown (Avin, option A).
- No fixed delay on actions that changed nothing: the "late" read of an action is the next action's "before" read (Task 3).
- Wording, verbatim: `the page showed: "<text>"`; `the app refused: "<message>" (<METHOD> <path without query>, <status>[, trace id <id>])`.
- New `ErrorClass::AppRefused`, key `app_refused`, label `the application refused the action`; the existing `PageRefused` (CDP protocol error) keeps its key and constant.
- Message: collapsed whitespace, at most 300 characters; at most 5 messages per action.
- Rules: at most 30 per area, least recently seen dropped; dedupe on `(page, message)` case-insensitive after whitespace collapse.
- Guide section heading `## Known validation rules`; at most 10 lines per area and 40 in all, most recently seen first.
- Rust tests only under `src-tauri/tests/suite/` (one module per file, `mod` line in `suite/main.rs`); `src/bindings.ts` regenerated with `cargo test --test bindings`, never hand-edited.
- One build or test command at a time; never set `CARGO_TARGET_DIR`.

## Review Focus

1. A run's own `api_request` (TCM's fetch) returns 400 → must not count as the page's refusal. Test in Task 3.
2. A success toast ("Saved") after a click in discovery → reported in `page_said`, the action stays ok and the batch goes on. Test in Task 4.
3. A message already on the page before the click (a standing banner) → not reported as new. Test in Task 1.
4. A saved negative case in an unattended run (a click gets 400, then `expect_visible` of the message) → the step passes. Test in Task 3.
5. A 401 or a background poll's 400 during a click → not a refusal. Test in Task 2.

---

### Task 1: What the page shows - the message read

**Files:**
- Create: `src-tauri/src/browser/page_messages.js` (one JS expression evaluating to `{messages: [{text, where}], step: string|null}`)
- Create: `src-tauri/src/browser/page_messages.rs`; add `pub mod page_messages;` in `src-tauri/src/browser/mod.rs`
- Test: `src/lib/pageMessages.test.ts` (vitest, jsdom, loads the .js file as `casesPage.test.ts` loads its page JS); `src-tauri/tests/suite/page_messages.rs` (+ `mod page_messages;` in `suite/main.rs`)

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)] pub struct PageMessage { pub text: String, #[serde(rename = "where")] pub place: String }`
  - `pub struct PageRead { pub messages: Vec<PageMessage>, pub step: Option<String> }`
  - `pub async fn read_page_messages<D: Driver>(d: &mut D) -> PageRead` (an eval failure reads as empty, never an error)
  - `pub fn new_messages(before: &[PageMessage], after: &[PageMessage]) -> Vec<PageMessage>` (after minus before by collapsed text; ≤5; each ≤300 chars)
  - `pub const MAX_MESSAGES: usize = 5; pub const MAX_MESSAGE_CHARS: usize = 300;`

The JS reads, in the document and recursively in every iframe whose `contentDocument` is reachable (try/catch, as `browser/locator.rs:494` does), the visible (`offsetParent` not null or fixed-position) text of: `[role=alert]`, `[role=status]`, `[aria-live]`; elements whose `className` contains (case-insensitive) one of `toast`, `alert`, `notif`, `snackbar`, `swal`, `bootbox`, `invalid`, `error`, `feedback`, `validation`, `message`; the element an `[aria-invalid=true]` field names in `aria-describedby`. Nested matches are reported once (the outermost). `where` is `"page"` or `"frame <title|name|id>"`. `step` is the text of the active item of a step indicator (`[aria-current=step]`, or `.active`/`.current` inside an element whose class contains `step` or `wizard`), else null.

- [ ] **Step 1: vitest for the JS** - `src/lib/pageMessages.test.ts`:
  - `"a toast and a field message are read, a hidden one is not"`: body with `<div class="toast toast-error">Weightage must total 100.</div>`, `<input aria-invalid="true" aria-describedby="m"><span id="m">Name is required</span>`, `<div class="alert" style="display:none">old</div>` → messages texts `["Weightage must total 100.", "Name is required"]`, both `where: "page"`.
  - `"a message inside a same-origin iframe names its frame"`: an `<iframe title="Eval Rules">` whose document holds `<div role="alert">At least one assessment component must be enabled.</div>` → one message, `where: "frame Eval Rules"`.
  - `"the wizard's active step is named"`: `<ul class="wizard-steps"><li>Details</li><li class="active">Assessment Components</li></ul>` → `step: "Assessment Components"`.
- [ ] **Step 2:** `npx vitest run src/lib/pageMessages.test.ts` → FAIL (file missing).
- [ ] **Step 3:** write `page_messages.js`; Step 1 passes.
- [ ] **Step 4: Rust tests** - `tests/suite/page_messages.rs`:
  - `a_message_on_the_page_before_is_not_new`: before `[{text:"Welcome back", place:"page"}]`, after `[Welcome back, "Saved."]` → `["Saved."]`.
  - `whitespace_does_not_make_a_message_new`: `"Saved.\n  "` before, `"Saved."` after → empty.
  - `at_most_five_messages_of_at_most_300_chars`: 7 new, one 400 chars → 5 returned, longest 300 chars.
  - `an_unreadable_page_reads_as_no_messages`: `ScriptedDriver` whose eval errors → `PageRead { messages: [], step: None }`.
- [ ] **Step 5:** implement `page_messages.rs` (`include_str!("page_messages.js")`, `page::eval_value`); `cd src-tauri && timeout 1800 cargo test --test suite -- page_messages::` → PASS.
- [ ] **Step 6:** commit `feat(v2): read what the page newly shows after an action`.

### Task 2: What the application refused - counted requests and their message

**Files:**
- Modify: `src-tauri/src/browser/net_record.rs` (`NetEntry` gains `pub kind: Option<String>` from `requestWillBeSent.type`, `pub same_site: bool` - the request's origin equals the frame's document origin `documentURL` / the top page's, compared and never stored as text - and `pub polling: bool` mirrored from `Times`)
- Modify: `src-tauri/src/autorun/api_checks.rs` (make `body_of` and `Body` `pub(crate)`)
- Create: `src-tauri/src/autorun/refusals.rs`; `pub mod refusals;` in `src-tauri/src/autorun/mod.rs`
- Test: `src-tauri/tests/suite/autorun_refusals.rs` (+ mod line); extend `tests/suite/net_record.rs`

**Interfaces:**
- Consumes: `Driver::net_since(mark)`, `api_checks::body_of`, `api_checks::shown_body`.
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)] pub struct RefusedRequest { pub method: String, pub path: String /* no query */, pub status: u16, pub message: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub trace_id: Option<String> }`
  - `pub fn counts(e: &NetEntry, own: &[String]) -> bool` - kind XHR or Fetch, `same_site`, not `polling`, not in `own` (request ids TCM sent), finished, status 400-499 except 401, 403, 404.
  - `pub fn message_and_trace(status: u16, body: Option<&str>) -> (String, Option<String>)` - JSON body: message = first non-empty of `message`, `Message`, `error_description`, `error` (string, or `error.message`), `detail`, `title`, then the first entry of `errors` (object of field → list or string, or a list); trace id = first of `traceId`, `TraceId`, `trace_id`, `traceIdentifier`. Non-JSON, non-HTML body: its first 200 chars. Otherwise (none, HTML, gone) the message is `"<status> <reason>"` (`400 Bad Request`, `409 Conflict`, `422 Unprocessable Entity`, else `"<status>"`). Message collapsed, ≤300 chars, passed through `shown_body` redaction first.
  - `pub async fn refused_since<D: Driver>(d: &mut D, mark: u64, own: &[String]) -> Vec<RefusedRequest>` (at most 3, in start order)
  - `pub fn app_refused_line(r: &RefusedRequest) -> String` - `the app refused: "<message>" (<METHOD> <path>, <status>[, trace id <id>])`
  - `pub const APP_REFUSED: &str = "the app refused: ";`

- [ ] **Step 1: failing tests** in `autorun_refusals.rs`:
  - `an_aspnet_problem_body_gives_its_title_and_trace_id`: `(400, Some(r#"{"title":"One or more validation errors occurred.","errors":{"Weightage":["Weightage must total 100."]},"traceId":"00-4bf9-00"}"#))` → `("One or more validation errors occurred.", Some("00-4bf9-00"))`.
  - `a_message_key_wins_over_title`: `{"message":"Code already exists","title":"Conflict"}` → `"Code already exists"`.
  - `errors_alone_give_their_first_entry`: `{"errors":{"Name":["Name is required"]}}` → `"Name is required"`.
  - `an_html_or_missing_body_gives_the_status`: `(422, Some("<html>..."))` and `(400, None)` → `"422 Unprocessable Entity"`, `"400 Bad Request"`.
  - `a_secret_in_the_body_is_never_shown`: `{"message":"bad token abc","token":"abc"}`-style body run through redaction → the returned message has no `abc` if `shown_body` hides it (assert against `shown_body`'s own output for the same input).
  - `which_requests_count` (Review Focus 5): entries for 400 XHR same-site (counts), 401, 403, 404, 500, a poll's 400, a Document 400, an other-site 400, an `own` id 400 → only the first counts.
  - `the_line_names_method_path_status_and_trace`: `app_refused_line` of `{POST, /api/eval-rules, 400, "Weightage must total 100.", Some("00-4bf9-00")}` → `the app refused: "Weightage must total 100." (POST /api/eval-rules, 400, trace id 00-4bf9-00)`.
- [ ] **Step 2:** `timeout 1800 cargo test --test suite -- autorun_refusals:: net_record::` → FAIL.
- [ ] **Step 3:** implement; `net_record.rs` test `the_type_site_and_poll_of_a_request_are_kept`.
- [ ] **Step 4:** tests PASS; commit `feat(v2): read the message and trace id of a request the application refused`.

### Task 3: The runner reports both after each action

**Files:**
- Modify: `src-tauri/src/browser/actions.rs` (`ActionOutcome` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub page_said: Vec<PageMessage>` and `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub refused: Vec<RefusedRequest>`; `passed`/`failed` set both empty)
- Modify: `src-tauri/src/autorun/runner.rs` (`InRun` gains `pub refusals_fail: bool`; the action loop)
- Modify: `src-tauri/src/ai_bridge.rs:5626` (`try_action`'s `InRun` sets `refusals_fail: true`)
- Modify: `src-tauri/src/autorun/patterns.rs` (`ErrorClass::AppRefused`, key, label, classification by `refusals::APP_REFUSED` prefix or a detail containing it after a page note; `tail()` strips the `; the page showed: ...` note before classifying)
- Modify: `src-tauri/src/browser/page_errors.rs` + `src-tauri/src/autorun/page_errors.rs` (a 5xx entry keeps its request id; the step's page-error line gains `, trace id <id>` when the body has one)
- Test: `src-tauri/tests/suite/autorun_runner.rs`, `autorun_patterns.rs`, `autorun_page_errors.rs`; regenerate `src/bindings.ts`

**Interfaces:**
- Consumes: Task 1 `read_page_messages`, `new_messages`; Task 2 `refused_since`, `app_refused_line`.
- Produces: `ActionOutcome.page_said`, `ActionOutcome.refused`; `pub const PAGE_SHOWED: &str = "the page showed: ";` in `runner.rs`; `pub fn page_showed_note(m: &[PageMessage]) -> String` (`the page showed: "a"; "b"`).

Per action for which `changes_the_page(action)` is true (the predicate already in `runner.rs`):
1. Before it: `before = read_page_messages(d)`, `mark = d.net_mark()`. This same read is the previous page-changing action's late read: messages new against that action's after-read are added to **its** `page_said` (`out[prev]`) - no extra wait anywhere.
2. After it (after the existing blocked-save, dialog and session checks): `after = read_page_messages(d)`; `outcome.page_said = new_messages(&before.messages, &after.messages)`; `outcome.refused = refused_since(d, mark, &own)` where `own` holds the request ids of this step's `api_request` fetches (the ids it already collects for `page_error_book().exclude`).
3. `run.refusals_fail` and an ok outcome with `refused` non-empty → the outcome fails: `detail = app_refused_line(&refused[0])` (+ `; ` + `page_showed_note` when `page_said` is non-empty); the step stops as any failed action does.
4. Otherwise nothing fails. When the step fails (any action not ok), the failed action's detail gains, from the nearest earlier action of the step with `page_said` or `refused`: ` - after <describe_action> ` + `app_refused_line` or `page_showed_note`; and when a `refused` is attached, the failure classes as `AppRefused`.

- [ ] **Step 1: failing tests** (`autorun_runner.rs`, with `ScriptedDriver` scripting the message eval and network events):
  - `a_try_fails_on_a_refused_request`: `refusals_fail: true`, a click whose events include an XHR 400 with the ASP.NET body → outcome not ok, detail starts `the app refused: "One or more validation errors occurred."` and ends `trace id 00-4bf9-00)`.
  - `a_run_reports_but_does_not_fail_on_a_refusal` (Review Focus 4): `refusals_fail: false`, click → 400, then `expect_visible` of the message that the scripted page holds → both actions ok; `out[0].refused.len() == 1`.
  - `a_run_step_that_fails_after_a_refusal_names_it`: `refusals_fail: false`, click → 400, then `wait_for` that times out → the failure's detail contains `after click` and `the app refused:`; `patterns::classify` of it is `AppRefused`.
  - `a_message_after_a_click_is_reported_not_failed`: click, after-read holds a new "Saved." → ok, `page_said == [Saved.]`.
  - `a_late_message_goes_to_the_action_that_caused_it`: click 1 (after-read empty), click 2's before-read holds "Name is required" → `out[0].page_said == [Name is required]`, `out[1].page_said` empty.
  - `the_runs_own_api_request_is_not_a_refusal` (Review Focus 1): an `api_request` whose fetch gets 400 → `refused` empty on every action.
  - `a_5xx_page_error_line_names_its_trace_id` (`autorun_page_errors.rs`): a 500 with `{"traceId":"t-1"}` → the step's page-error line contains `trace id t-1`; no `refused` on the action.
- [ ] **Step 2:** `timeout 1800 cargo test --test suite -- autorun_runner:: autorun_patterns:: autorun_page_errors::` → FAIL.
- [ ] **Step 3:** implement; `timeout 1800 cargo test --test bindings` regenerates `src/bindings.ts` (`PageMessage`, `RefusedRequest`, the two fields).
- [ ] **Step 4:** PASS (and the whole `autorun_` filter); commit `feat(v2): each action reports what the page showed and what the app refused`.

### Task 4: Discovery answers and batch lines

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (`discover_action_in` JSON at ~3579: `"page_said"` and `"app_refused"` (the `RefusedRequest`s) only when non-empty; `batch_line` appends `; the page showed: "..."` and, for a failed action, nothing new (its detail already says it); the batch stop: when the action that stopped the batch has no refusal of its own and an earlier action of the batch has `page_said`, the stop line becomes `Stopped at the failure: N actions were not run. After action K (<describe_action>) the page showed: "<text>".`)
- Test: `src-tauri/tests/suite/autorun_discovery.rs`

The last page-changing action of a single discovery action or a batch has no next action to
give it a late read: `discover_action_in` / `discover_actions_in` read the messages once more
just before their closing page read (one eval, no wait) and add what is new against that
action's after-read to its `page_said`.

**Interfaces:**
- Consumes: Task 3's outcome fields; `describe_action`, `one_short_line`, `not_run_line`.

- [ ] **Step 1: failing tests:**
  - `a_success_toast_does_not_stop_a_batch` (Review Focus 2): three actions, the first's after-read shows "Saved." → all three ran; line 1 ends `; the page showed: "Saved."`; the JSON of action 1 has `page_said`.
  - `a_refused_request_stops_the_batch`: action 2 gets a 400 → line 2 `2. failed: the app refused: ...`; `Stopped at the failure: 1 action was not run.`
  - `a_later_failure_points_back_to_the_message`: action 1 shows "At least one assessment component must be enabled.", action 2 (`expect_visible "Goals"`) fails → the stop line ends `After action 1 (click "Save & Continue") the page showed: "At least one assessment component must be enabled."`
  - `an_action_with_nothing_new_has_no_page_said_key`.
- [ ] **Step 2:** `timeout 1800 cargo test --test suite -- autorun_discovery::` → FAIL; **Step 3:** implement; **Step 4:** PASS; commit `feat(v2): discovery names what the page showed and what the app refused`.

### Task 5: Remembering the rules per area

**Files:**
- Modify: `src-tauri/src/autorun/discovery_map.rs` (`AreaMap` gains `#[serde(default)] pub refusals: Vec<RefusalRule>`; module notes: the map holds no record text **except** `refusals`, which keep the application's words as shown)
- Modify: `src-tauri/src/ai_bridge.rs` (discovery and tries remember after each action); `src-tauri/src/commands/autorun.rs` and `src-tauri/src/autorun/replay.rs` / `replay_to.rs` (runs and replays remember from failed steps)
- Test: `src-tauri/tests/suite/discovery_map.rs`, `autorun_discovery.rs`

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)] pub struct RefusalRule { pub page: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub step: Option<String>, pub after: String, pub message: String, pub source: RuleSource, #[serde(default, skip_serializing_if = "Option::is_none")] pub case_id: Option<i32>, pub first_seen: u64, pub last_seen: u64, pub count: u32 }`
  - `#[derive(... Copy, Serialize, Deserialize)] #[serde(rename_all = "snake_case")] pub enum RuleSource { Discovery, Run, Assistant }`
  - `pub const MAX_RULES_PER_AREA: usize = 30;`
  - `pub fn remember_refusal(map: &mut DiscoveryMap, area: &str, rule: RefusalRule)` - into the area's entry (created if missing, via the same canonical lookup `record_outcome` uses), dedupe `(page, message)` per Global Constraints: a repeat sets `last_seen`, `count += 1`, `after`, `step`; past 30 the least recently seen goes. A rule never goes into `""`.
  - `pub fn forget_refusal(map: &mut DiscoveryMap, area: &str, page: Option<&str>, message: &str) -> bool`
  - `pub fn rules_of<'a>(map: &'a DiscoveryMap, area: &str) -> Vec<&'a RefusalRule>` (most recently seen first)

When remembered (spec Part 2): in discovery and tries, every `refused` (message = the request's message) and every `page_said` message of an action followed by a failure in the same batch; in runs and replays, only from a failed step: its `refused` and the `page_said` of the action before the failure. `page` = `discovery_map::page_path` of the page the action ran on; `step` = Task 1's `PageRead.step` at that action; `after` = `describe_action`. A re-exploration keeps `refusals`. Area for runs: the script's `area`, or the area the last `return_to_area` that ran in the case took it to; none known → not remembered.

- [ ] **Step 1: failing tests** (`discovery_map.rs`): `a_repeat_counts_and_moves_last_seen`; `the_thirty_first_rule_drops_the_least_recently_seen`; `a_rule_never_goes_to_the_unattributed_bucket`; `a_re_exploration_keeps_the_rules`; `a_map_written_before_rules_reads_with_none`. (`autorun_discovery.rs`): `a_refused_request_in_discovery_is_remembered_without_its_trace_id` (the saved map JSON holds the message and no `00-4bf9-00`); `a_success_toast_alone_is_never_remembered`.
- [ ] **Step 2:** `timeout 1800 cargo test --test suite -- discovery_map:: autorun_discovery::` → FAIL; **Step 3:** implement; **Step 4:** PASS (and `autorun_`); commit `feat(v2): remember the application's refusals per area`.

### Task 6: The assistant records or removes a rule

**Files:**
- Modify: `src-tauri/src/mcp.rs` (tool `record_validation_rule`, route call `"record_validation_rule" => call("POST", "/autorun-validation-rule", ...)`), `src-tauri/src/ai_bridge.rs` (route + handler), `src-tauri/src/ai_tools.rs` (listed beside `record_autorun_quirk` in every group it is in), `src/lib/mcpTools.ts` (an entry beside `record_autorun_quirk`, same group, summary `Remembers (or removes) a validation rule the application showed in an area.`) and the `mcpTools.test.ts` lists that name the quirk tool
- Test: `src-tauri/tests/suite/autorun_bridge.rs`, `tcm_mcp.rs`; `src/lib/mcpTools.test.ts`

**Interfaces:**
- Consumes: Task 5 `remember_refusal`, `forget_refusal`.
- Tool input: `{ message: string (required), after: string (required unless remove), area?: string, page?: string, step?: string, remove?: boolean }`. `area` defaults to the discovery's area, `page` to the current page path; no area known → 400 `"Name the area: no discovery is running."`. Source `assistant`. Answers `Remembered for <area>.` / `Removed from <area>.` / `No such rule in <area>.`

- [ ] **Step 1: failing tests:** `record_validation_rule_saves_into_the_discovery_area`, `record_validation_rule_with_remove_forgets_it`, `record_validation_rule_without_an_area_is_refused` (bridge); the tool appears in `tools_list` with `message` required (`tcm_mcp.rs`); vitest `every tool the MCP server advertises has a switch in the UI` passes with the new entry.
- [ ] **Step 2:** run `timeout 1800 cargo test --test suite -- autorun_bridge:: tcm_mcp::` and `npx vitest run src/lib/mcpTools.test.ts` → FAIL; **Step 3:** implement; **Step 4:** PASS; commit `feat(v2): record_validation_rule - the assistant remembers or removes a rule`.

### Task 7: The guide

**Files:**
- Modify: `src-tauri/src/autorun/discovery_map.rs` (`pub fn rules_section(map: &DiscoveryMap, area: Option<&str>) -> String`), `src-tauri/src/ai_bridge.rs` (`autorun_guide_with_quirks` appends it right after `explore_section`; `area` = the running discovery's area), `src-tauri/src/autorun/guide.rs` (full guide's discovery section: a paragraph on `page_said`, "the app refused", judging each message against the test case, reporting an unexpected refusal to the user as "the test case or its preconditions look wrong" with the message, checking an expected one with `expect_visible`, and when to call `record_validation_rule`; the quick rules gain one line pointing at it), `docs-site/src/content/auto-run.ts` (one sentence in the discovery part), then `npm run docs:build` (commits `src-tauri/help/`)
- Test: `src-tauri/tests/suite/discovery_map.rs`, `autorun_guide.rs`; the docs guard tests (`docs-site/src/guard.test.ts` via `npm test`)

**Interfaces:**
- Line format, verbatim shape: `- <area>, step "<step>": after <after> - "<message>" (seen <count> times)`; without a step: `- <area>, <page>: after ...`; `(seen once)` for 1.
- Empty when there are no rules (no empty heading).

- [ ] **Step 1: failing tests:** `the_rules_section_lists_the_discovery_areas_rules_newest_first`; `without_a_discovery_every_area_shows_at_most_ten_and_forty_in_all`; `no_rules_no_heading`; `autorun_guide.rs`: the full guide mentions `record_validation_rule` and `the app refused`, the quick rules name `## Known validation rules`.
- [ ] **Step 2:** FAIL; **Step 3:** implement + docs sentence + `npm run docs:build`; **Step 4:** `timeout 1800 cargo test --test suite -- discovery_map:: autorun_guide::` and `npm test` PASS; commit `feat(v2): the guide lists the known validation rules`.

### Task 8: Gate

- [ ] `cd src-tauri && cargo test --tests`, then `npm test`, then `npm run build` - one at a time, all green.
- [ ] Owner hand checks (cannot run here): a discovery on Eval Rules with both components off shows the refusal on the Save & Continue action; a try that hits a server 400 shows its trace id; the guide lists the rule on the next discovery; a saved negative case still passes unattended.
