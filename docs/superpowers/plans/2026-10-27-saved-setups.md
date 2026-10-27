# Saved setups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An assistant keeps the actions that worked in an explore session, saves a range of them as a named setup with a find rule, and days later asks for it by name: an existing test record is handed back, or a new one is built in the explore session and put on its change list.

**Architecture:** Four pieces. (1) The explore change list learns, at record time, when each verified create's save happened and where its id sat in the answer (`autorun/explore.rs`). (2) An in-memory kept-actions store keyed by the Auto Run root and explore session, owned by the assistant that started it (`autorun/kept.rs`), fed from the one place every discovery action ends (`ai_bridge::discover_action_heard`). (3) Pure find-rule logic (`autorun/setup_find.rs`) and the setups file with save validation (`autorun/setups.rs`, `projects/<slug>-setups.json`, its own write lock). (4) `run_setup` (`autorun/setup_run.rs`), which drives the caller's own browser through `ai_bridge::discover_action_in` one action at a time and records what it built with `explore::record_change`. Four bridge routes and four MCP tools, Auto Run module only. No Tauri command, no screen.

**Tech Stack:** Rust (serde, tokio), the existing CDP `Driver`, sqlcmd through `db::query::run_query`; TypeScript only for the AI Bridge tool list (`src/lib/mcpTools.ts`).

**Spec:** `docs/superpowers/specs/2026-10-23-saved-setups-design.md` (approved 2026-10-27). Its "Settled after the builder check" and "Owner decision 5" sections override the earlier ones.

## Global Constraints

- No UI. Nothing in Auto Run, the Components window, Settings or anywhere else shows or runs a setup. Only the four assistant tools read and write them. (The AI Bridge tab's tool list names the tools, as it names every tool; it never shows a setup.)
- Setups are not used by scripts or runs; an unattended run never builds anything.
- A setup never creates a second record when a suitable one exists, never names a record outside the test prefix, never builds without a find rule that ran.
- No password, login, host, URL, query string or cookie in the setups file, an answer or a log line. A setup never types a username or password and never signs in.
- The database rule is SELECT only, whatever the AI Bridge write switch says (`run_query(.., writes_on: false, ..)` and `guard::classify == Verdict::Read`).
- No HTTP DELETE anywhere; setups never talk to Azure DevOps.
- No new cache (`src/lib/cache.ts`, `src-tauri/src/cache/` untouched). The kept-actions store is memory-only state, not a cache.
- No em or en dashes in guide, role, skill, tool description or answer text. No hidden feature named anywhere.
- Rust tests only in `src-tauri/tests/suite/`, one module per file with a `mod` line in `suite/main.rs`; never `#[cfg(test)]` in `src/`.
- Source files are CRLF: edit with the Edit/Write tools, never sed.
- `src/bindings.ts` is generated; this feature adds no Tauri command, so it must not change.
- One build/test at a time on this machine (the owner may have `tauri dev` running). Commands: `cd src-tauri && cargo test --test suite <module>::`, `npx vitest run <path>`, `npx tsc --noEmit`. Never `CARGO_TARGET_DIR` or `--target-dir`.
- Serial locks (`tests/suite/serial.rs`): order `agents -> log_tail -> autorun`, enforced by `serial_order.rs`. A test that sets the store root or uses the bridge `route` takes `crate::serial::autorun()`; tests that only use their own temp root need none (the kept store is keyed by root, so temp roots never meet). No test here needs wiremock; if one ever does, use an unpooled `MockServer` per test (the pooled-address caveat in `suite/ado_network.rs:191`).
- Commits use a Bash heredoc (`git commit -F - <<'EOF'`), ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Work on a feature branch (`feat/saved-setups`), never `main`. No release.

## Review Focus

1. **A damaged setups file** (hand-edited, cut short): save, list and run refuse naming `projects/<slug>-setups.json`, and a save never overwrites it. Test in Task 4.
2. **A setup whose area was renamed or removed since it was saved**: the run refuses with the area sentence before the find or any action runs. Test in Task 5.
3. **`{prefix}` written inside a quoted SQL literal** (`LIKE '{prefix}%'`): the save refuses it (substituted, it would break the statement or match by text). Test in Task 3.
4. **An input value that occurs by chance elsewhere in the range**, or is very short: every replaced place is listed in the save answer, and a value under 3 characters is refused. Test in Task 4.
5. **A save that names a kept action the 200 cap has dropped**: refused naming the first number still kept; numbers never shift. Tests in Task 2 and Task 4.

---

## Code this plan stands on (path:line, as read on 2026-10-27)

- Explore change list: `src-tauri/src/autorun/explore.rs` - `ExploreRun` 35-42, `ChangeEntry` 76-86 (no save time, no id location), `compared` 309-316 (private), `request_line` 271, `verify_create` 563-584 (proves the id is anywhere in a 2xx POST/PUT answer, returns only the request line), `keep` 709-717, `record_change` 727-770, `lock` 118. Test-made list for undo: `keep_made` 675.
- Discovery state: `src-tauri/src/commands/autorun.rs:334-369` (`DiscoveryState`, built by struct literal in 18 places, tests included, so this plan adds NO field to it); `Session.discovery` 110, `Session.slot` 139.
- Where every discovery action ends: `src-tauri/src/ai_bridge.rs:5428-5610` (`discover_action_heard`; the component's expanded actions at 5476-5479, per-action loop 5495-5537, final `outcome` 5538, writes noted to the session 5591-5608). Batches go through it per action (`discover_actions_heard` 5950-6063, `MAX_BATCH` 20 at 5621). `discover_action_in` 5377 is `pub`.
- Explore start makes the change list: `ai_bridge.rs:5020-5035`; route arms `/autorun-discover-start` 564-584, `/autorun-explore-change` 604-613; `record_change_in` 6428-6484 reads the capture at 6477.
- Slot choice per route: `src-tauri/src/commands/autorun_slots.rs:36-74` (`need_of`), `slot_for` 136+ (an Act/Read with no browser of yours is refused, and an extra browser is unbound when freed, so a list keyed by browser would not survive `end_autorun_discovery`).
- Route module by path: `ai_bridge.rs:861` (`/autorun-` is Auto Run's); write-smell words `ai_bridge.rs:844`.
- Refusals for tried actions: `refuse_as_a_tried_action` `ai_bridge.rs:4422` (sign_in refused), `refuse_outside_the_recipe` 8359, `runner::policy_for` `autorun/runner.rs:167`. Going home: `went_home` `ai_bridge.rs:4825` (private). Area trip action: `Action::ReturnToArea` `browser/actions.rs:285`.
- Components: `autorun/components.rs` - file path 63, write lock 74, `update_with` 153-173, `fill_text` 284, `expand` 336 (fills `{{name}}` text inputs), `is_credential` 649, `check_component` 658, version/changes rule 793-802. `component_to_try` `ai_bridge.rs:6090`.
- Seen check: `seen_check::load_checked_map` `autorun/seen_check.rs:1684`, `check_component_seen` 2634.
- Database: `db_ready` `ai_bridge.rs:10605`; `db_query` 11044; `db::query::run_query` `db/query.rs:168`; `guard::classify` `db/guard.rs:199`; sqlcmd output is tab-separated with a header, a dash line and a `(N rows affected)` footer, `ROW_CAP` 200, `CHAR_CAP` 60000 (`db/sqlcmd.rs:25-29, 297-347, 413`); row count read as `gate::SqlcmdStageDb` does (`api_templates/gate.rs:41-62`); string literal as `N'...'` (`api_templates/flow.rs:291-305`).
- Environment: `environments.rs:41` (`test_prefix`, 3 to 20 of letters, digits, `-`), `active` 301; prefix compared case-insensitively in `api_templates/fixture_run.rs:570`. Accounts: `autorun/accounts.rs:17` (`username`, `password`), `load_accounts` 116.
- Capture: `browser/net_capture.rs:65` (`MAX_CAPTURED` 1000); `ActionOutcome` `browser/actions.rs:1079` has no matched text. `locator::resolve` `browser/locator.rs:516`, `page::call_value_with_handles` `browser/page.rs:81`.
- Tools: `ai_tools.rs:150-184` (`AUTORUN_TOOLS`), `CLAUDE_AUTORUN_RULES` 1416, `CURSOR_AUTORUN_ENTRIES` 1438, `plan_allow` 1513; files written via `commands/ai_tools.rs:323-343`. MCP declarations `mcp.rs:993-1016`, dispatch 2566-2567, `agent` property added to every Auto Run tool by `with_agent_property` 1309. `/tcm:explore` skill body `ai_tools.rs:722-766`. Guide "Exploring a task" `autorun/guide.rs:1979-2019`. Role `autorun/roles/explorer.md`. Webview tool list `src/lib/mcpTools.ts:145-153, 323-359`.
- Tests to model on: `tests/suite/autorun_explore.rs` (helpers 41-69, `save`/`text`/`started` 355-389), `tests/suite/autorun_discovery.rs` (`FakeBrowser` 89, `slot` 131, `exploring` 163, `root_with_recipe_and_account` 79, `signin_app`), `tests/suite/explore_send.rs` (a capture in a fake browser), `tests/suite/allow_rules.rs:120-175`, `tests/suite/autorun_guide.rs:2436-2460`, `tests/suite/tcm_mcp.rs:3504`, `common::account()` (username `kim`, password `common::PASSWORD`).

## Rulings this plan makes (the spec leaves them open or the code forces them)

- Actions are kept only in explore sessions: settled 2 makes a plain discovery's list unusable, so none is kept (`get_kept_actions` there says so).
- A kept list belongs to the assistant (its `Caller`) and its explore session, not to a slot: the slot binding can be dropped when an extra browser is freed, and `DiscoveryState` is gone after `end_autorun_discovery`. A new discovery start by the same assistant drops its previous list. At most 16 lists are held process-wide (oldest dropped).
- Numbers are stable: 1 is the first action kept; past 200 the oldest go and the rest keep their numbers.
- A kept `use_component` keeps the steps that actually ran (the expanded actions `component_to_try` returned, draft or saved), so the save never re-reads the components file.
- An action that typed a saved account's username or password, or filled a target whose text names "password", is never kept.
- The change list keeps, for each verified create, the save's start time and the shortest JSON pointer to the id in its answer. A create proven only in a text (non-JSON) answer has no place and cannot be saved into a setup.
- A create belongs to the range when its save started in `[from.started_ms, min(to.ended_ms + 2000, next kept action's started_ms))`.
- `returns` names a change-list entry number (`{ "entry": n }`); the first create in the range by default.
- Inputs come only from `fill` actions; a value must be at least 3 characters; it is replaced as a substring in every string of every action in the range; values of `unique` and `names_the_record` inputs must start with the prefix (save and run).
- `{prefix}` in SQL becomes `N'<prefix>'` and must stand outside quotes. A browser find rule is at most 10 actions ending in `expect_visible`, `expect_text` or `expect_contains_text`; the found name is the first line of the trimmed `innerText` of that check's element.
- Both the find and the build start with the trip to the setup's area (`return_to_area` with its name) or, with no area, home.
- A setup runs one action per `discover_action_in` call (no 20-action batch limit), stops at the first failure, and records the creates whose saves it saw so far.
- In an explore session started `no_save`, the find runs and the build is refused.
- `last_used_at` is stamped on a found record and on a build.
- A generated unique value is `<prefix><slug of the setup name>-<now_ms mod 36^7 in base 36, 7 characters>`; slug keeps ASCII letters and digits, any other run becomes one `-`, trimmed, at most 30 characters.
- The database rule is proven at save only when it can run; otherwise the setup is saved with `"database_checked": "not run: Database Read Access is off"`.
- "Database Read Access on" means the Database Access module is on and `db_ready(ctx)` succeeds.

---

### Task 1: The change list keeps when each verified create was saved and where its id sat

**Files:**
- Modify: `src-tauri/src/autorun/explore.rs` (`ChangeEntry` 76-86, `verify_create` 563-584, `keep` 709-717, `record_change` 727-770, `record_send_create` 778-797)
- Test: `src-tauri/tests/suite/autorun_explore.rs` (add `proof: None` to the four `ChangeEntry` literals at 257, 300, 313, 914)

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] pub struct SaveProof { pub saved_ms: u64, #[serde(default, skip_serializing_if = "Option::is_none")] pub id_at: Option<String> }`
  - `ChangeEntry` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub proof: Option<SaveProof>` (old files still read).
  - `#[derive(Debug, Clone, PartialEq)] pub struct CreateProof { pub request: String, pub saved_ms: u64, pub id_at: Option<String> }`
  - `pub fn create_proof(captured: &[&Captured], floor_ms: u64, id: &str, earlier: &[ChangeEntry]) -> Option<CreateProof>` - exactly `verify_create`'s rule; `verify_create` keeps its signature and becomes `create_proof(..).map(|p| p.request)`.
  - `pub fn id_pointer(body: &Value, id: &str) -> Option<String>` - the shortest RFC 6901 pointer to a value equal to `id` (string trimmed, or number as written); ties go to document order (serde_json keeps order here: `preserve_order`).
  - `pub fn id_at(body: &Value, pointer: &str) -> Option<String>` - the string or number at `pointer`, as text; anything else `None`.
  - `pub fn same_save(a: &str, b: &str) -> bool` - `compared(a) == compared(b)`.

- [ ] **Step 1: Write the failing tests** in `autorun_explore.rs` (use the `save`, `text`, `created`, `started` helpers at 355-389):
  - `a_verified_create_keeps_when_and_where_its_id_was_saved`: `save("POST", "/hr/c?x=1", 2000, text(r#"{"data":{"Id":274,"Name":"AUTOTEST a"}}"#))`, `record_change(root, id, created("leave type","274","AUTOTEST a"), None, &[&post], 1000)` then `load`: `changes[0].proof == Some(SaveProof { saved_ms: 2000, id_at: Some("/data/Id".into()) })`; `create_proof(..).unwrap().request == "POST /hr/c"`.
  - `the_shortest_place_of_the_id_is_kept`: body `{"items":[{"Id":274}],"Id":274}` gives `id_pointer == Some("/Id")`.
  - `a_text_answer_verifies_but_keeps_no_place`: body `created 274` (not JSON): recorded `verified == Some(true)`, `proof == Some(SaveProof { saved_ms: 2000, id_at: None })`.
  - `an_unverified_create_keeps_no_proof`: a GET answer: `proof == None`.
  - `id_at_reads_a_string_or_a_number`: `id_at(&json!({"a":{"b":" x7 "}}), "/a/b") == Some("x7")`, `/n` on `{"n":12}` is `Some("12")`, a missing pointer and an object value are `None`.
  - `same_save_compares_as_the_change_list_does`: `same_save("POST /hr/c?x=1", "post /HR/c/")` true; `same_save("POST /p?handler=Save", "POST /p?handler=Delete")` false.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_explore::` - expect compile failure (`SaveProof`, `create_proof`, `id_at` not found).
- [ ] **Step 3: Implement.** `create_proof` is `verify_create`'s loop returning the matching capture's `request_line`, `started_ms`, and `id_pointer` of its parsed `Body::Text` answer (`None` when it does not parse as JSON). `keep` takes `proof: Option<SaveProof>`; `record_change` passes it for a verified create only; `record_send_create` passes `None`. The pointer is key names and indexes only: no value goes into `id_at`.
- [ ] **Step 4: Run** `cd src-tauri && cargo test --test suite autorun_explore::` and `cargo test --test suite explore_send::` - all pass.
- [ ] **Step 5: Commit** `feat(v2): an explore change list keeps when a verified create was saved and where its id sat`.

---

### Task 2: Kept actions and `get_kept_actions`

**Files:**
- Create: `src-tauri/src/autorun/kept.rs` (+ `pub mod kept;` in `autorun/mod.rs`)
- Modify: `src-tauri/src/autorun/explore.rs` (`ExploreRun` gains `pub setup_running: bool`; it derives `Default`, and tests build it with `..ExploreRun::default()`)
- Modify: `src-tauri/src/ai_bridge.rs` - `discover_action_heard` (5428-5610): time the action, keep it; route arm `/autorun-discover-start` (564-584): tell the store a discovery started; new arm `("POST", "/autorun-kept-actions")`
- Test: `src-tauri/tests/suite/autorun_kept.rs` (+ `mod autorun_kept;` in `suite/main.rs`)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces (`autorun::kept`):
  - `pub const KEPT_MAX: usize = 200;` `pub const LISTS_MAX: usize = 16;`
  - `pub const NOT_AN_EXPLORE: &str = "actions are kept only in an explore session - start one with start_autorun_discovery and explore: true";`
  - `#[derive(Debug, Clone, PartialEq, Serialize)] pub struct KeptAction { pub n: u32, pub action: Action, #[serde(skip_serializing_if = "Option::is_none")] pub steps: Option<Vec<Action>>, pub area: Option<String>, #[serde(skip)] pub started_ms: u64, #[serde(skip)] pub ended_ms: u64 }`
  - `#[derive(Debug, Clone, PartialEq, Default)] pub struct KeptList { pub session: String, pub started_area: Option<String>, pub actions: Vec<KeptAction>, pub dropped: u32 }` (`next` number = `dropped + actions.len() + 1`)
  - `pub fn owner_of(caller: &crate::autorun::agents::Caller) -> String` - `""` for `Caller::Default`, else `"<id>:<name or empty>"`.
  - `pub fn started(root: &Path, owner: &str, session: Option<&str>, area: Option<&str>)` - drops `owner`'s previous list; with `Some(session)` a new empty list is bound to the owner; `None` (a plain discovery or a mapping run) binds none.
  - `pub fn keep(root: &Path, session: &str, action: &Action, steps: Option<Vec<Action>>, area: Option<&str>, started_ms: u64, ended_ms: u64) -> Option<u32>` - the number given, or `None` when not kept (`types_a_credential`). Creates the list lazily when no `started` came first (tests). Past `KEPT_MAX` drops the oldest and counts it in `dropped`.
  - `pub fn types_a_credential(action: &Action, steps: Option<&[Action]>, accounts: &[Account]) -> bool` - any `fill` (in `Action::each`, top level or in `steps`) whose value equals a saved account's username or password (exact, trimmed), or whose target's serialized text contains "password" (case ignored).
  - `pub fn last_of(root: &Path, owner: &str) -> Result<KeptList, String>` - the owner's bound list (it outlives the discovery's end), else `Err(NOT_AN_EXPLORE)`.
  - `pub fn list_of(root: &Path, session: &str) -> Option<KeptList>`
  - `pub fn kept_answer(list: &KeptList) -> serde_json::Value` - `{ "session", "area": started_area, "actions": [{ "n", "action", "steps"? , "area" }], "first": <lowest n kept>, "cut": dropped > 0 }`.
- Produces (`ai_bridge`): `pub fn kept_actions_for(root: &Path, owner: &str) -> (u16, String)` - 200 with `kept_answer`, or 409 `NOT_AN_EXPLORE`.
- Store: one `static` `Mutex` holding `lists: HashMap<(PathBuf, String /*session*/), KeptList>` and `owners: HashMap<(PathBuf, String /*owner*/), String /*session*/>`; lock poisoning recovered as `explore::lock` does.

- [ ] **Step 1: Write the failing tests** in `autorun_kept.rs` (fake browser from `autorun_discovery::{slot, leave_page, exploring, root_with_recipe_and_account}`; an explore state as `autorun_explore.rs:47` builds it; session from `explore::begin`):
  - `a_successful_explore_action_is_kept_in_order`: two `discover_action_in` clicks that pass; `kept::list_of(root, id)` holds `n` 1 and 2, `action` equal to each sent action, `area == Some("Leave")`.
  - `a_failed_action_is_not_kept`: a click on a selector the page lacks (`ok` false): list empty.
  - `a_plain_discovery_keeps_nothing`: `exploring("Leave")` with no `explore`: no list; `started(root, "a", None, None)` then `last_of(root, "a") == Err(NOT_AN_EXPLORE.into())`.
  - `a_setup_run_keeps_nothing`: `ExploreRun { setup_running: true, .. }`: a passing click keeps nothing.
  - `a_use_component_is_kept_as_itself_with_the_steps_it_ran`: a `use_component` with a `draft` that passes: `action` is the `use_component`, `steps == Some(<the draft's expanded actions>)`.
  - `a_fill_of_a_login_or_password_is_never_kept`: fills with value `"kim"`, with `common::PASSWORD`, and into `{"label":"Password"}` are not kept; a fill of `"AUTOTEST leave"` is.
  - `the_list_keeps_the_last_200_and_numbers_never_shift` (Review Focus 5): 205 direct `keep` calls: `actions.first().n == 6`, `actions.last().n == 205`, `dropped == 5`, `kept_answer["cut"] == true`, `kept_answer["first"] == 6`.
  - `a_new_discovery_by_the_same_assistant_drops_the_old_list`: `started(root,"a",Some(s1),..)`, keep one, `started(root,"a",Some(s2),..)`: `list_of(root, s1) == None`, `last_of(root,"a").unwrap().session == s2`.
  - `the_list_outlives_the_end_of_the_discovery`: keep one, `end_discovery_closing(&mut browser)`, then `last_of` still holds it.
  - `another_assistants_list_is_not_yours`: owner `"a"` bound; `last_of(root, "b")` is `Err(NOT_AN_EXPLORE)`.
  - `kept_actions_answer_shows_typed_values_and_no_login`: `kept_actions_for` 200; JSON `actions[0]["action"]["value"] == "AUTOTEST leave"`; the body contains neither `"kim"` nor `PASSWORD`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_kept::` - fails to compile (`autorun::kept` missing).
- [ ] **Step 3: Implement `autorun/kept.rs`** with the interfaces above. Then in `discover_action_heard`: note `sessions::now_ms()` before the run loop; after `outcome` (5538) keep the action when `outcome.ok`, the discovery is an explore session with a change list, it is not a mapping run, `setup_running` is false, and no save of this action was blocked (`ran.blocked` of its runs empty). Pass `steps` = the component's expanded actions when there is one, `area` = `area_name`, and the accounts from `accounts::load_accounts(root)` (an unreadable file keeps nothing typed into a fill: treat as "typed a credential"). A keep never fails the action. In the `/autorun-discover-start` arm, on status 200 read the slot's `discovery` (`session_slot(slot).lock().await`) and call `kept::started(root, &kept::owner_of(caller), explore_session, area)`; on any other status do nothing. Add the `/autorun-kept-actions` arm (`autorun_root()` then `kept_actions_for(&root, &kept::owner_of(caller))`); `need_of` stays `None` for it (no browser needed).
- [ ] **Step 4: Run** `cd src-tauri && cargo test --test suite autorun_kept::`, then `autorun_explore::` and `autorun_discovery::` - all pass.
- [ ] **Step 5: Commit** `feat(v2): an explore session keeps the actions that worked, for the assistant that ran them`.

---

### Task 3: Find rules (database and browser), test records only

**Files:**
- Create: `src-tauri/src/autorun/setup_find.rs` (+ `pub mod setup_find;`)
- Test: `src-tauri/tests/suite/autorun_setup_find.rs` (+ `mod` line)

**Interfaces:**
- Produces (`autorun::setup_find`):
  - `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] pub struct DbFind { pub sql: String, pub id_column: String, #[serde(default)] pub name_column: String }`
  - `#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)] pub struct FindRule { #[serde(default, skip_serializing_if = "Option::is_none")] pub database: Option<DbFind>, #[serde(default, skip_serializing_if = "Option::is_none")] pub browser: Option<Vec<Action>> }`
  - `#[derive(Debug, Clone, PartialEq, Serialize)] pub struct FoundRecord { #[serde(skip_serializing_if = "Option::is_none")] pub id: Option<String>, pub name: String }`
  - `pub const PREFIX: &str = "{prefix}"; pub const BROWSER_FIND_MAX: usize = 10;`
  - `pub const ONLY_TEST_RECORDS: &str = "A find rule must look only for records named with the test prefix.";`
  - `pub const DATABASE_OFF: &str = "This setup checks the database for an existing record, and Database Read Access is off on the AI Bridge tab. Ask the person to switch it on.";`
  - `pub const NEEDS_A_FIND_RULE: &str = "A setup needs a find rule: a database SELECT, a browser check, or both.";`
  - `pub fn starts_with_prefix(name: &str, prefix: &str) -> bool` - trimmed, case ignored (as `fixture_run.rs:570`).
  - `pub fn check_find(rule: &FindRule) -> Vec<String>` - every problem, one sentence each: no rule at all `NEEDS_A_FIND_RULE`; database: not one SELECT ("A database find rule is one SELECT statement."), no `{prefix}` or no `name_column` (`ONLY_TEST_RECORDS`), `{prefix}` inside a quoted literal ("Write {prefix} outside quotes: the app puts it in as a quoted value."), no `id_column` ("A database find rule names its id_column."), any other `{`...`}` placeholder ("A database find rule takes no placeholder but {prefix}."); browser: empty or more than 10 actions, a `sign_in` or a `use_component` anywhere, a last action that is not `expect_visible` / `expect_text` / `expect_contains_text` ("A browser find rule ends in a check: expect_visible, expect_text or expect_contains_text."), `{prefix}` absent from that last action (`ONLY_TEST_RECORDS`).
  - `pub fn sql_with_prefix(sql: &str, prefix: &str) -> Result<String, String>` - every `{prefix}` becomes `N'<prefix>'` (`'` doubled); the result must classify as `Verdict::Read`.
  - `pub fn browser_with_prefix(actions: &[Action], prefix: &str) -> Result<Vec<Action>, String>` - `{prefix}` replaced in every string of every action (serialize, replace, deserialize).
  - `pub fn first_test_record(text: &str, db: &DbFind, prefix: &str) -> Result<Option<FoundRecord>, String>` - reads sqlcmd's answer: first non-empty line is the header (tab-split, columns compared case-insensitively), the dash line skipped, rows until a blank line or the footer; `db::query::rows_affected(text)` must be `Some`, else `Err("the find rule's answer carried no row count")`; a missing column is `Err("the find rule's answer has no column <name>")`; rows whose name cell is empty or `NULL` or does not `starts_with_prefix` are skipped; the first that does is `FoundRecord { id: Some(<id cell>), name }`.
  - `pub trait RowSource { fn select(&self, sql: &str) -> impl std::future::Future<Output = Result<String, String>> + Send; }`
  - `pub struct SqlcmdRows<R: Runner> { pub runner: R, pub exe: PathBuf, pub conn: Connection }` implementing `RowSource` with `db::query::run_query(&self.runner, &self.exe, &self.conn, false, sql)` (its `Err((_, said))` is `Err(said)`; those sentences are already redacted).
  - `#[derive(Debug, Clone, PartialEq)] pub enum DbOutcome { Found(FoundRecord), NotFound, CouldNotRun(String) }`
  - `pub async fn find_in_database<S: RowSource>(rows: &S, db: &DbFind, prefix: &str) -> DbOutcome`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum FindChoice { Database, Browser, Refused }` and `pub fn choose(rule: &FindRule, database_ready: bool) -> FindChoice` - database when it has one and `database_ready`; else browser when it has one; else `Refused` (said as `DATABASE_OFF`).

- [ ] **Step 1: Write the failing tests** in `autorun_setup_find.rs`:
  - `a_database_rule_must_select_with_the_prefix_and_name_its_columns`: `check_find` on `SELECT Id, Name FROM dbo.LeaveType WHERE Name LIKE {prefix} + N'%'` with `id_column "Id"`, `name_column "Name"` is empty; without `{prefix}` contains `ONLY_TEST_RECORDS`; with `name_column ""` contains `ONLY_TEST_RECORDS`; `UPDATE dbo.LeaveType SET x = 1 WHERE Name LIKE {prefix}` contains "one SELECT statement"; `id_column ""` contains "id_column"; `{{x}}` in the SQL is refused.
  - `the_prefix_inside_quotes_is_refused` (Review Focus 3): `... WHERE Name LIKE '{prefix}%'` contains "outside quotes".
  - `the_prefix_goes_in_as_a_quoted_value`: `sql_with_prefix("SELECT Id, Name FROM T WHERE Name LIKE {prefix} + N'%'", "AUTOTEST") == Ok("SELECT Id, Name FROM T WHERE Name LIKE N'AUTOTEST' + N'%'")`.
  - `a_row_named_outside_the_prefix_is_skipped`: text `"Id\tName\n--\t----\n7\tReal Person\n9\tautotest leave\n\n(2 rows affected)\n"` gives `Ok(Some(FoundRecord { id: Some("9"), name: "autotest leave" }))`.
  - `no_prefixed_row_is_not_found`: only `Real Person` and a `NULL` name: `Ok(None)`.
  - `an_answer_with_no_row_count_or_column_could_not_run`: the footer removed gives `Err` containing "no row count"; `name_column "Title"` gives `Err` containing "no column Title".
  - `a_database_error_could_not_run`: a fake `RowSource` answering `Err("the database is not reachable right now (VPN or network?) - try again")` gives `DbOutcome::CouldNotRun(<that sentence>)`; a fake answering a prefixed row gives `Found`.
  - `the_database_rule_is_preferred_when_it_can_run`: both rules: `choose(.., true) == Database`, `choose(.., false) == Browser`; database only and not ready: `Refused`; browser only and ready: `Browser`.
  - `a_browser_rule_ends_in_a_check_that_uses_the_prefix`: last action a `click` is refused; `expect_visible` of `{"role":"row","name":"Leave A"}` gives `ONLY_TEST_RECORDS`; 11 actions are refused; a `sign_in` is refused; `[click "#search", expect_visible {"role":"cell","name":"{prefix}"}]` passes; `browser_with_prefix` turns that name into `AUTOTEST`.
  - `starts_with_prefix_ignores_case_and_spaces`: `(" autotestX", "AUTOTEST")` true, `("XAUTOTEST", "AUTOTEST")` false.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_setup_find::` - fails to compile.
- [ ] **Step 3: Implement** the interfaces. The quoted-literal test: walk the statement once, toggling "inside a literal" on each `'` (a doubled `''` stays inside); a `{prefix}` seen while inside is the refusal. Never log the SQL from here (`run_query` already writes it to the activity log, as `db_query` does).
- [ ] **Step 4: Run** `cd src-tauri && cargo test --test suite autorun_setup_find::` - all pass.
- [ ] **Step 5: Commit** `feat(v2): find rules for saved setups, which only ever match test records`.

---

### Task 4: The setups file and `save_setup`

**Files:**
- Create: `src-tauri/src/autorun/setups.rs` (+ `pub mod setups;`). The existing `autorun/setup.rs` and `autorun::Setup` are the script fixture setup: do not touch or reuse them; this type is `SavedSetup`.
- Modify: `src-tauri/src/ai_bridge.rs` - new arm `("POST", "/autorun-setup-save")`, `need_of` stays `None`
- Test: `src-tauri/tests/suite/autorun_setups.rs` (+ `mod` line)

**Interfaces:**
- Consumes: `explore::{ExploreSession, ChangeEntry, Change, SaveProof, same_save}` (Task 1); `kept::{KeptList, KeptAction, last_of, owner_of, NOT_AN_EXPLORE}` (Task 2); `setup_find::{FindRule, check_find, browser_with_prefix, starts_with_prefix, RowSource, find_in_database, DbOutcome}` (Task 3).
- Produces (`autorun::setups`):
  - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] pub struct SetupInput { pub name: String, pub description: String, #[serde(default)] pub unique: bool, #[serde(default)] pub names_the_record: bool }`
  - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] pub struct SetupCreate { pub kind: String, pub name: String /* template */, pub request: String /* METHOD /path[?handler=] */, pub id_at: String /* JSON pointer */ }`
  - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] pub struct SavedSetup { pub name: String, pub description: String, #[serde(default)] pub area: Option<String>, pub inputs: Vec<SetupInput>, pub actions: Vec<Action>, pub find: FindRule, pub creates: Vec<SetupCreate>, pub returns: usize /* index into creates */, pub version: u32, pub changes: u32, pub saved_at: u64, #[serde(default)] pub last_used_at: u64 }`
  - `#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)] pub struct SetupFile { pub setups: Vec<SavedSetup> }`
  - `pub fn setups_path(root: &Path, org: &str, project: &str) -> PathBuf` - `root/projects/<project_slug>-setups.json`.
  - `pub fn load_setups(root: &Path, org: &str, project: &str) -> Result<SetupFile, String>` - missing is empty; a file that does not read is `Err("the setups file projects/<slug>-setups.json could not be read")` (no full path, as `components::load_components` 106-114).
  - A private `write_lock()` and `update_with` exactly as `components.rs:74, 153-173` (own static, `ai_tools::atomic_write`, nothing written when unchanged or refused). Never the components file or its lock.
  - `#[derive(Deserialize, Debug, Clone)] #[serde(deny_unknown_fields)] pub struct SaveAsked { pub name: String, #[serde(default)] pub description: String, #[serde(default)] pub from: u32, #[serde(default)] pub to: u32, #[serde(default)] pub area: Option<String>, #[serde(default)] pub inputs: Vec<InputAsked>, #[serde(default)] pub find: FindRule, #[serde(default)] pub returns: Option<ReturnsAsked>, #[serde(default)] pub why: Option<String> }` (Task 6 adds `#[serde(default)] pub remove: bool`; until then a body with `remove` is refused as an unknown field, never saved); `InputAsked { action: u32, name: String, #[serde(default)] description: String, #[serde(default)] unique: bool, #[serde(default)] names_the_record: bool }`; `ReturnsAsked { entry: u32 }`.
  - `pub struct SaveFrom<'a> { pub kept: &'a KeptList, pub session: &'a ExploreSession, pub prefix: &'a str, pub allowed: &'a dyn Fn(&str) -> bool /* policy.allows(url) */, pub seen: &'a dyn Fn(Option<&str>, &[Action]) -> Result<(), String>, pub area_known: &'a dyn Fn(&str) -> bool, pub now: u64 }`
  - `#[derive(Debug, Clone, PartialEq, Serialize)] pub struct Replaced { pub input: String, pub action: u32 /* kept number */, pub field: String /* dotted path in the action's JSON, e.g. "value", "selector.name" */ }`
  - `#[derive(Debug, Clone, PartialEq)] pub struct Built { pub setup: SavedSetup, pub replaced: Vec<Replaced>, pub cut: bool }`
  - `pub fn build_setup(asked: &SaveAsked, from: &SaveFrom) -> Result<Built, Vec<String>>`
  - `#[derive(Debug, Clone, PartialEq, Serialize)] pub struct SetupSaved { pub saved: String, pub version: u32, pub changes: u32 }`
  - `pub fn save(root: &Path, org: &str, project: &str, built: Built, why: Option<&str>) -> Result<SetupSaved, String>` - by `nav::module_key` name; a taken name without `why` is `Err("<name> is already saved: say why it changes in \"why\".")`; otherwise version + 1 and changes + 1, `last_used_at` carried over; a new one is version 1, changes 0.
  - `pub fn find_setup<'a>(f: &'a SetupFile, name: &str) -> Option<&'a SavedSetup>`
  - `pub fn no_such_setup(name: &str) -> String` - `there is no setup called "<name>" - list_setups lists them`.
  - `pub fn fresh_value(prefix: &str, setup: &str, now_ms: u64) -> String` (the ruling's format).
  - `pub const NO_CREATE_IN_RANGE: &str = "These actions made no create the app verified: record what these actions create first, with record_explore_change.";`
- Produces (`ai_bridge`): `pub async fn setup_save_for<S: RowSource>(root: &Path, org: &str, project: &str, owner: &str, body: &str, rows: Option<&S>, now: u64) -> (u16, String)` - a body that is not a `SaveAsked` is 400 in serde's words. Then: `kept::last_of(root, owner)` (409 `NOT_AN_EXPLORE`), `explore::load` of its session (409 on unreadable, as `record_change_in` does), the active environment's prefix, `runner::policy_for(Some(&recipe))`, `seen_check::load_checked_map` + `check_component_seen`, `nav::load_nav` + `find_area`; `build_setup` (400 with every sentence joined by a space); the database rule once when `rows` is `Some` (`CouldNotRun(why)` is 400 "The database find rule did not run: <why>"); `save` (409 with its sentence). The route arm passes `rows` = `Some(SqlcmdRows { runner: RealRunner, exe, conn })` when the Database Access module is on and `db_ready(ctx)` is `Ok`, else `None`, and the owner `kept::owner_of(caller)`.
- Save answer (200): `{ "saved", "version", "changes", "actions": <count>, "area", "inputs": [names], "replaced": [Replaced], "creates": [{ "kind", "request" }], "returns": <kind of the returned create>, "cut": bool, "database_checked": "found" | "not found" | "not run: Database Read Access is off" | absent with no database rule }`.

`build_setup` decides, collecting every problem before answering:
1. `name` 1 to 60 characters of letters, digits, spaces, `-`, `_`; `description` not blank.
2. `from` 1..=`to`; both kept. A number below the first kept: "Action <n> is no longer kept: the list keeps the last 200, from action <first>." A number past the last: "Action <n> was not kept: get_kept_actions lists them."
3. The range: each kept action's `steps` (when it was a `use_component`) or itself, in order. No `sign_in`, no `use_component` left, no `navigate`/`open_tab` the `allowed` check refuses ("Action <n> goes to an address outside the sign-in recipe's sites."), every action `validate()`s.
4. `area`: the asked one, else `kept[from].area`, else `kept.started_area`; when set it must be `area_known`, else `nav::unrecorded_area(area)`.
5. Inputs: `action` in the range and a `fill`; name `[A-Za-z0-9_]{1,40}`, unique among inputs, not `username`/`password` (`components::is_credential` rule); value at least 3 characters ("The value of <name> is too short to stand for an input: at least 3 characters."); `unique` or `names_the_record` value starts with the prefix ("<name> must start with the test prefix <prefix>."); at most one `names_the_record`. Each value is replaced by `{{name}}` as a substring in every string of every action in the range (serialize to JSON, walk, deserialize), every place recorded as a `Replaced`; a `navigate`/`open_tab` address that would hold one is refused ("An address cannot come from an input: action <n>.").
6. Creates: the session's `Change::Created { verified: true }` entries with a `proof` whose `saved_ms` falls in the range window (ruling). None: `NO_CREATE_IN_RANGE`. One whose `proof.id_at` is `None`: "The save that made <kind> <id> answered in a form the app cannot read again, so it cannot be built from: record it, or save without it." Each becomes `SetupCreate { kind, name: <recorded name with input values replaced>, request: entry.request, id_at }`. `returns.entry` must be one of them ("Entry <n> is not a create these actions made."); default the first.
7. Find: `check_find`; a browser rule after `browser_with_prefix(.., prefix)` has every target either equal (as JSON) to a target of some action in the kept list, or passing `seen(area, ..)` for that action; else "The browser find rule's action <i> uses a locator this session never used and the map never saw."
8. `cut` = `kept.dropped > 0`.

- [ ] **Step 1: Write the failing tests** in `autorun_setups.rs` (build the `KeptList` and `ExploreSession` directly; `seen` and `area_known` as closures; prefix `"AUTOTEST"`):
  - `a_setup_saves_its_range_with_inputs_as_templates`: kept 1 `click "#new"` (1000-1100), 2 `fill "#name" "AUTOTEST leave A"`, 3 `click "#save"` (1300-1400), 4 `expect_visible {"role":"row","name":"AUTOTEST leave A"}`; session entry 1 verified create `leave type` / `274` / `AUTOTEST leave A`, request `POST /hr/leave/save`, `proof { saved_ms: 1350, id_at: Some("/data/Id") }`; input `{action: 2, name: "leave_name", unique: true, names_the_record: true}`; database rule valid. `build_setup` then `save`: the file at `projects/<slug>-setups.json` holds `actions[1]` fill value `{{leave_name}}`, `actions[3]` name `{{leave_name}}`, `creates == [SetupCreate { kind: "leave type", name: "{{leave_name}}", request: "POST /hr/leave/save", id_at: "/data/Id" }]`, `version 1`, `changes 0`, `last_used_at 0`; `replaced == [{leave_name, 2, "value"}, {leave_name, 4, "selector.name"}]`; the components file does not exist.
  - `every_problem_is_named_at_once`: a unique value `"leave A"`, `to: 9` past the list, and no find rule: `Err` of 3 sentences, one each containing "test prefix AUTOTEST", "Action 9 was not kept", `NEEDS_A_FIND_RULE`.
  - `a_range_with_no_verified_create_is_refused`: `Err(vec![NO_CREATE_IN_RANGE])`; a create saved at 5000 (outside the window) counts as none.
  - `a_create_without_a_readable_place_is_refused`: `id_at: None` gives the "cannot read again" sentence.
  - `a_navigate_outside_the_sites_or_built_from_an_input_is_refused`: `allowed` false for `https://elsewhere.example/x` names it; a navigate holding the input's value gives "An address cannot come from an input: action <n>."
  - `a_short_or_chance_input_value_is_refused_or_listed` (Review Focus 4): value `"AU"` is refused (too short); value `"AUTOTEST1"` that also appears inside a later `expect_contains_text` value is replaced there and listed in `replaced`.
  - `a_number_the_list_dropped_is_refused_by_name` (Review Focus 5): a list with `dropped: 5` (first `n` 6), `from: 3`: "Action 3 is no longer kept: the list keeps the last 200, from action 6."
  - `a_taken_name_needs_why_and_becomes_the_next_version`: second save without `why` is `Err` naming it; with `why` gives `version 2`, `changes 1`; `"Leave  Type"` and `"leave type"` are the same name.
  - `a_damaged_setups_file_is_never_overwritten` (Review Focus 1): file text `{ x`; `save` is `Err` containing "projects/" and "-setups.json could not be read"; the file still reads `{ x`; `load_setups` is the same `Err`.
  - `a_browser_find_locator_must_have_been_used_or_seen`: a find rule whose check target equals kept action 4's (after `{prefix}` is filled) passes with `seen` always `Err`; a new target with `seen` `Err` is refused; with `seen` `Ok` passes.
  - `a_use_component_is_saved_as_the_steps_it_ran`: a kept `use_component` with `steps` of 2 actions: the setup's `actions` hold those 2, no `use_component`.
  - `fresh_value_is_prefix_name_and_stamp`: `fresh_value("AUTOTEST", "Leave type", 1_760_000_000_000) == "AUTOTESTLeave-type-gj6k3cw"`; `+1` ms ends `gj6k3cx`.
  - Route-level, through `setup_save_for` with a temp root and a fake `RowSource`: `saving_outside_an_explore_session_is_refused` (owner with no list: `(409, NOT_AN_EXPLORE)`); `the_database_rule_is_run_once_at_save_when_it_can` (fake `Err("x")`: 400 starting "The database find rule did not run"; `rows: None`: 200 with `"database_checked": "not run: Database Read Access is off"`); `the_save_answer_has_no_login_or_address` (body has no `kim`, no `https://`, no `?`).
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_setups::` - fails to compile.
- [ ] **Step 3: Implement** `autorun/setups.rs` and `setup_save_for` as specified; the route arm resolves `rows` and the owner as stated. Log one line per save, `Auto Run setup <name>: saved, version <v>` (`applog::info`), nothing else.
- [ ] **Step 4: Run** `cd src-tauri && cargo test --test suite autorun_setups::` and `components::` - all pass.
- [ ] **Step 5: Commit** `feat(v2): an assistant saves kept actions as a named setup with a find rule`.

---

### Task 5: `run_setup`

**Files:**
- Create: `src-tauri/src/autorun/setup_run.rs` (+ `pub mod setup_run;`)
- Modify: `src-tauri/src/ai_bridge.rs` - `went_home` (4825) becomes `pub(crate)`; new arm `("POST", "/autorun-setup-run")` ending with `roles::with_rules(.., RULES_EXPLORE)`
- Modify: `src-tauri/src/commands/autorun_slots.rs:39-55` - `"/autorun-setup-run"` is `Need::Act`
- Modify: `src-tauri/src/autorun/setups.rs` - `pub fn stamp_used(root: &Path, org: &str, project: &str, name: &str, now: u64) -> Result<(), String>`
- Test: `src-tauri/tests/suite/autorun_setup_run.rs` (+ `mod` line)

**Interfaces:**
- Consumes: `explore::{record_change, Change, same_save, id_at, request_line, NO_EXPLORE_SESSION}`; `setups::{load_setups, find_setup, no_such_setup, fresh_value, SavedSetup, SetupCreate}`; `setup_find::{choose, FindChoice, find_in_database, DbOutcome, browser_with_prefix, starts_with_prefix, RowSource, DATABASE_OFF}`; `ai_bridge::{discover_action_in, DiscoveryBrowser, went_home, discovery_must_not_save}`.
- Produces (`autorun::setup_run`):
  - `pub const FOUND: &str = "Found an existing record";` `pub const BUILT: &str = "Built a new record";`
  - `pub const BUILD_NOT_SAVED: &str = "No existing record was found, and this explore session must not save, so nothing was built.";`
  - `pub fn find_could_not_run(why: &str) -> String` - `The find rule could not run, so nothing was built: <why>`.
  - `pub fn action_failed(n: usize, what: &str, detail: &str) -> String` - `Action <n> (<what>) failed: <detail>. The setup may need saving again; what it created so far is on the change list.`
  - `pub fn not_confirmed(request: &str) -> String` - `The record was built, but it could not be confirmed: the save <request> did not answer with an id where it did when the setup was saved. Record it with record_explore_change.`
  - `pub fn values_for(setup: &SavedSetup, given: &serde_json::Map<String, Value>, prefix: &str, now_ms: u64) -> Result<BTreeMap<String, String>, Vec<String>>` - every input: given (string or number), else `fresh_value` when `unique`, else "<name> needs a value."; a key no input has: "<setup> has no input <key>."; a `unique` or `names_the_record` value outside the prefix: "<name> must start with the test prefix <prefix>.".
  - `pub fn ids_of_built(captured: &[&Captured], floor_ms: u64, creates: &[SetupCreate]) -> Vec<Option<String>>` - per create, in order: the first capture started at or after `floor_ms`, not used by an earlier create, `same_save(request_line(method, url_path_query), create.request)`, 2xx, `Body::Text` that parses, `id_at` at `create.id_at`; else `None`.
  - `#[derive(Debug, Clone, PartialEq, Serialize)] pub struct BuiltCreate { pub entry: u32, pub kind: String, pub id: String, pub name: String, pub verified: bool }`
  - `pub fn record_built(root: &Path, session: &str, setup: &SavedSetup, values: &BTreeMap<String, String>, captured: &[&Captured], floor_ms: u64) -> (Vec<BuiltCreate>, Vec<String> /* requests not confirmed */)` - each id found goes through `explore::record_change(.., Change::Created { kind, id, name: <filled name template>, verified: false }, Some(request), captured, floor_ms)`, so verification, the proof and the test-made list are the change list's own; an id not found, or recorded but not verified, is one `not_confirmed` request.
  - `pub async fn matched_text<D: Driver>(d: &mut D, target: &Target) -> Option<String>` - `locator::resolve`, first handle, `innerText` first line trimmed, at most 200 characters.
  - `pub async fn run_setup_in<B: DiscoveryBrowser, S: RowSource>(slot: &mut Option<B>, root: &Path, org: &str, project: &str, body: &str, rows: Option<&S>, timing: &Timing, now: u64) -> (u16, String)`

`run_setup_in`, in order (body `{ "name": <setup>, "values": { <input>: <value> } }`):
1. 400 for an unreadable body; 409 `NO_EXPLORE_SESSION` with no browser, no discovery or no change list (`explore_session()`); 409 the load error for a damaged setups file; 404 `no_such_setup`; 409 `nav::unrecorded_area` when the setup's area is no longer recorded (before anything drives the browser); 400 `values_for`'s sentences joined by a space.
2. Set `ExploreRun.setup_running = true` for the whole run and clear it on every way out (wrap the inner run).
3. `choose(&setup.find, rows.is_some())`: `Refused` is 409 `DATABASE_OFF`. Database: `find_in_database(rows, db, prefix)`; `CouldNotRun(why)` is 409 `find_could_not_run(why)`. Browser: trip (step 5) then each action of `browser_with_prefix` through `discover_action_in`; a non-200 or `ok: false` on any but the last action is 409 `find_could_not_run(<that action's detail>)`; the last failing is not found; the last passing reads `matched_text` of its target, and only a text that `starts_with_prefix` is found (`FoundRecord { id: None, name }`).
4. Found: `stamp_used`, answer 200 `{ "said": FOUND, "found": true, "record": FoundRecord, "setup": <name> }`, nothing else runs.
5. Not found: when `discovery_must_not_save` is true, 409 `BUILD_NOT_SAVED`. The trip: with an area, `discover_action_in(Action::ReturnToArea { area: Some(area) })`, else `went_home`; a failed trip is 409 `find_could_not_run` (find) or `action_failed(0, "the trip to the area", ..)` (build).
6. `floor = sessions::now_ms()`; expand the actions with the values (`components::expand` of a `Component { name: setup.name, inputs: <text inputs>, actions: setup.actions, .. }`); each through `discover_action_in`; at the first non-200 or `ok: false`: `record_built` over the capture so far and 200 `{ "said": action_failed(..), "built": false, "failed_at": n, "recorded": [BuiltCreate] }`.
7. All passed: `record_built`; `stamp_used`; 200 `{ "said": BUILT or not_confirmed(first), "built": true, "confirmed": <no request unconfirmed>, "record": { "id": <the returns create's id>, "name": <names_the_record value, else its filled name> }, "values": <values used>, "recorded": [BuiltCreate], "setup": <name> }`. With any request unconfirmed, `said` is `not_confirmed`, never `BUILT`.
8. Log `Auto Run setup <name>: found an existing record` / `built a new record` / `stopped at action <n>` (`applog::info`), never a value, SQL or address.

The route arm: `unattended_run_is_using_the_browser()` first, as `/autorun-explore-change` does; `rows` as in Task 4; `Timing::supervised()`; `session_slot(slot).lock().await` for the whole run.

- [ ] **Step 1: Write the failing tests** in `autorun_setup_run.rs` (take `crate::serial::autorun()` in each test that drives a fake browser; save the setup with `setups::save` of a hand-built `SavedSetup`; `area: None` except where stated; a fake driver that answers `Page.navigate` as `signin_app(true)` does; a fake `RowSource`):
  - `run_setup_needs_an_explore_session`: `None` slot, `exploring("Leave")`, and an explore state with no change list each give `(409, NO_EXPLORE_SESSION.to_string())`.
  - `a_found_record_builds_nothing`: rows answer a prefixed row `9 / AUTOTEST leave`: 200, `said == FOUND`, `record == {"id":"9","name":"AUTOTEST leave"}`; the driver received no `Input.*` call and no `Page.navigate` (a database find makes no trip); `last_used_at == now`.
  - `a_database_rule_that_cannot_run_builds_nothing`: rows `Err("x")`: `(409, find_could_not_run("x"))`; no input events; change list empty.
  - `database_off_and_no_browser_rule_refuses`: rows `None`, database rule only: `(409, DATABASE_OFF.to_string())`.
  - `a_browser_rule_that_cannot_run_builds_nothing`: browser rule `[click "#missing", expect_visible {..{prefix}..}]`: 409 starting "The find rule could not run"; no build action ran.
  - `a_browser_rule_whose_check_fails_builds`: a page with no matching row: the build actions ran and the answer has `"built"`.
  - `the_first_failing_build_action_stops_the_run`: action 2 clicks a missing element: 200, `built == false`, `failed_at == 2`, `said` contains "may need saving again"; action 3 never ran.
  - `values_are_checked_before_anything_runs`: missing non-unique value, an unknown key, and a unique `"leave"` without the prefix: 400 naming all three; no driver call made.
  - `a_unique_value_left_out_is_made_fresh`: answer `values.leave_name == fresh_value("AUTOTEST", <setup name>, now)`; the setups file does not contain it afterwards (no values from earlier runs).
  - `the_setups_area_gone_refuses_before_the_find` (Review Focus 2): area `"Old Leave"` not in the nav file: `(409, nav::unrecorded_area("Old Leave"))`; rows never asked (fake counts calls: 0).
  - `a_no_save_session_finds_but_does_not_build`: discovery `no_save: true`, nothing found: `(409, BUILD_NOT_SAVED.to_string())`.
  - `a_setup_run_keeps_no_actions`: after a build, `kept::list_of(root, session)` holds nothing from it.
  - Pure: `ids_of_built_reads_the_id_from_the_same_place`: floor 2000; `save("POST", "/hr/leave/save?x=1", 3000, text(r#"{"data":{"Id":301}}"#))` gives `Some("301")`; a GET on that path, a POST at 1500, and an answer `{"Id":301}` (no `/data/Id`) give `None`; two creates of the same request take the first and second matching saves in order.
  - `a_built_record_is_recorded_verified_and_can_be_undone`: `record_built` with that capture: `BuiltCreate { entry: 1, kind: "leave type", id: "301", name: "AUTOTEST x", verified: true }`; the session's entry 1 is a verified create with `proof.id_at == Some("/data/Id")`; `test_made::list(root)` has it under the session; `explore::unaccounted` of the session after `note_writes` of that save is empty.
  - `a_save_without_the_id_where_expected_is_not_a_clean_build`: an answer `{"ok":true}`: `record_built` gives no `BuiltCreate` and `["POST /hr/leave/save"]`; through `run_setup_in` the answer has `confirmed == false`, `said == not_confirmed("POST /hr/leave/save")`, and does not contain `BUILT`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_setup_run::` - fails to compile.
- [ ] **Step 3: Implement** as specified.
- [ ] **Step 4: Run** `cd src-tauri && cargo test --test suite autorun_setup_run::`, then `autorun_kept::`, `autorun_explore::`, `autorun_discovery::` - all pass.
- [ ] **Step 5: Commit** `feat(v2): run_setup finds a test record or builds one in the explore session, on its change list`.

---

### Task 6: `list_setups` and removing a setup

**Files:**
- Modify: `src-tauri/src/autorun/setups.rs` - `pub fn remove(root: &Path, org: &str, project: &str, name: &str) -> Result<String, String>` (the saved name, or `no_such_setup`), `pub fn list_answer(f: &SetupFile) -> serde_json::Value`
- Modify: `src-tauri/src/autorun/setups.rs` - `SaveAsked` gains `#[serde(default)] pub remove: bool`
- Modify: `src-tauri/src/ai_bridge.rs` - `setup_save_for` reads `remove: true` first (no explore session needed, no `why`): 200 `{ "removed": <name> }`, 404 `no_such_setup`, 409 for a damaged file; new arm `("POST", "/autorun-setups")` answering `list_answer` (409 for a damaged file); `need_of` stays `None`
- Test: `src-tauri/tests/suite/autorun_setups.rs`

**Interfaces:**
- `list_answer`: `{ "setups": [{ "name", "description", "area", "inputs": [{ "name", "description", "unique", "names_the_record" }], "find": ["database"?, "browser"?], "saved_at": RFC 3339, "last_used": RFC 3339 or null, "version" }] }`, sorted by name key. No actions, no SQL text, no values.

- [ ] **Step 1: Write the failing tests:**
  - `list_setups_names_each_setup_without_values`: two saved setups (one with both rules, one browser only, one used at 1_760_000_000_000): `find == ["database","browser"]` and `["browser"]`, `last_used` `"2025-10-09T08:53:20.000Z"` and `null`; the answer contains no `{{`, no `SELECT`, no `AUTOTEST leave A`.
  - `removing_a_setup_leaves_the_others`: `setup_save_for` with `{ "name": "leave type", "remove": true }` and an owner with no kept list: `(200, {"removed":"Leave type"})`; the other setup is still listed; again gives 404 `no_such_setup("leave type")`.
  - `remove_and_list_refuse_a_damaged_file`: both answer 409 naming the file; the file is unchanged.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_setups::` - the new tests fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cd src-tauri && cargo test --test suite autorun_setups::` - all pass.
- [ ] **Step 5: Commit** `feat(v2): list_setups, and save_setup with remove`.

---

### Task 7: The four tools, their allow rules, the guide, the explorer's role and `/tcm:explore`

**Files:**
- Modify: `src-tauri/src/ai_tools.rs` - `AUTORUN_TOOLS` (150-184) gains `"get_kept_actions", "save_setup", "list_setups", "run_setup"` after `"get_explore_changes"`; `CLAUDE_AUTORUN_RULES` (1416) and `CURSOR_AUTORUN_ENTRIES` (1438) gain the same four; the explore `CommandSpec` body (722-766) gains one line
- Modify: `src-tauri/src/mcp.rs` - four declarations after `get_explore_changes` (1017); dispatch after 2567: `"get_kept_actions" => call("POST", "/autorun-kept-actions", &args.to_string())`, `"save_setup" => call("POST", "/autorun-setup-save", ..)`, `"list_setups" => call("POST", "/autorun-setups", ..)`, `"run_setup" => call("POST", "/autorun-setup-run", ..)`
- Modify: `src-tauri/src/autorun/guide.rs` - a `### Saved setups` subsection inside `## Exploring a task`, before "End with a summary" (2015)
- Modify: `src-tauri/src/autorun/roles/explorer.md` - one line
- Modify: `src/lib/mcpTools.ts` - `MCP_TOOLS` entries after `get_explore_changes` (150-153) and `AUTORUN_TOOLS` (357-358)
- Test: `src-tauri/tests/suite/allow_rules.rs` (142-163 owner's list), `tcm_mcp.rs` (tool lists ~144, routes 3504-3505), `ai_tools.rs` (594-595, 2012-2019, 2483-2484), `autorun_guide.rs` (new test beside 2436), `autorun_roles.rs` (201-215), `src/lib/mcpTools.test.ts` (521-522)

**Interfaces:** routes from Tasks 2, 4, 5 and 6. Tool declarations (schemas with `schema(json!({..}), &[required])` as at 993-1016; the `agent` property is added by `with_agent_property`):
- `get_kept_actions`: "The actions of your explore session that worked, in order and numbered from 1, with the area the session started in and the values you typed (never a login or password), so you can save some of them as a setup with save_setup. Kept in memory for your latest explore session until you start another discovery, so it still answers after end_autorun_discovery. `cut` true means the oldest went past 200: their numbers are not reused." No properties.
- `save_setup`: "Save kept actions from your explore session as a named setup that builds a precondition, so a later session can ask for it with run_setup. Give the range (`from`, `to` from get_kept_actions), which typed values become inputs, and a find rule that only looks for records named with the test name prefix. Record what the actions create with record_explore_change first: the setup learns each create's save from the change list. Every problem is named at once. A taken name is a change and needs `why`. With `remove` true and `name`, removes that setup." Properties: `name` (string), `description` (string), `from` (number), `to` (number), `area` (string, optional: the area a run starts from, the session's by default), `inputs` (array of `{ action: number, name: string, description: string, unique: boolean, names_the_record: boolean }`), `find` (`{ database: { sql, id_column, name_column }, browser: [actions] }`, one or both), `returns` (`{ entry: number }`, optional), `why` (string), `remove` (boolean). Required: `name`.
- `list_setups`: "This project's saved setups: each name, what it builds, its inputs, which find rules it has (database, browser) and when it last found or built a record. Look here before you build a precondition by hand." No properties.
- `run_setup`: "Ask for a saved setup by name in your explore session. Its find rule runs first: when a test record that fits exists, it is handed back and nothing is built. Otherwise the setup's actions run in your browser with `values` (a unique input left out gets a fresh name with the test prefix), and the new record is handed back; every record it builds is on the change list, to be undone like any other. Needs an explore session started with start_autorun_discovery and explore true." Properties: `name` (string), `values` (object). Required: `name`.
- `mcpTools.ts` labels and summaries: `get_kept_actions` "Kept actions" / "The actions of the assistant's explore session that worked, numbered, to save as a setup."; `save_setup` "Save a setup" / "Save kept actions as a named setup that builds a test record, with a rule to find one that exists."; `list_setups` "Saved setups" / "The project's saved setups: what each builds, its inputs and when it was last used."; `run_setup` "Run a setup" / "Find a test record that fits, or build one in the explore session, on its change list."

Guide text (exact):

```
### Saved setups

Before you build a precondition by hand, call `list_setups`: a saved setup may build it, or find
one that is there already. `run_setup` with its name and `values` first looks for a record that
fits and hands it back when one exists; only when none does it build one in your explore session,
and every record it builds is on the change list, to be read and undone like any other. A setup
runs only in an explore session.

While you explore, every action that works is kept; `get_kept_actions` lists them, numbered. After
you build by hand a precondition a later case will need again, record what it created with
`record_explore_change`, then save the actions that built it with `save_setup`: a name, what it
builds, the range (`from` and `to`), the typed values that become inputs (mark a record's name
`unique`, so each run makes a new one), and always a find rule. A find rule only looks for records
named with the test name prefix: a `database` SELECT that uses `{prefix}` outside quotes and names
its id and name columns, a `browser` list of actions that ends in a check using `{prefix}`, or
both. The database rule is used while Database Read Access is on; otherwise the browser rule is
used, and with neither the run is refused. Read the places the save answer says each input
replaced, and save again if one is a match by chance.

Two assistants running the same setup at once may both find nothing and both build: split the
work so they do not.
```

Explorer role line (after the `record_explore_change` bullet): `- Before you build a precondition by hand, look for a saved setup with \`list_setups\` and run it with \`run_setup\`; after building one a later case will need again, save it with \`save_setup\` and a find rule.`

`/tcm:explore` line, after step 3's last line: `"   Before setting up a precondition by hand, check \`list_setups\` and use \`run_setup\`; save one a later case needs again with \`save_setup\` (the guide's Saved setups)."`

- [ ] **Step 1: Write the failing tests:**
  - `autorun_guide.rs` `the_guide_explains_saved_setups`: the "Exploring a task" section (split as 2442 does) contains `### Saved setups` and, flattened, `` `list_setups` ``, `` `run_setup` ``, `` `get_kept_actions` ``, `` `save_setup` ``, `{prefix}`, "always a find rule", "both find nothing and both build"; no U+2014 or U+2013.
  - `allow_rules.rs` `the_lists_are_the_owners_less_the_retired_names`: the sorted list gains `mcp__tcm__get_kept_actions`, `mcp__tcm__list_setups`, `mcp__tcm__run_setup`, `mcp__tcm__save_setup` (the existing `every_rule_names_a_real_tool_of_its_module` and `cursor_gets_the_same_tools` then cover them).
  - `ai_tools.rs` suite: the four are `Some(Module::AutoRun)` from `module_of_tool`; the explore command body contains `` `list_setups` `` and `` `run_setup` ``.
  - `tcm_mcp.rs`: the four tool-to-route pairs; the four are listed with Auto Run on and absent with it off; each carries the `agent` property.
  - `autorun_roles.rs` `the_explorer_carries_the_owners_rules` also holds `"list_setups"` and `"save_setup"`.
  - `src/lib/mcpTools.test.ts`: `moduleOfTool` is `"auto_run"` for the four.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_guide::` (one module at a time, then `allow_rules::`, `ai_tools::`, `tcm_mcp::`, `autorun_roles::`) and `npx vitest run src/lib/mcpTools.test.ts` - the new assertions fail.
- [ ] **Step 3: Implement** the lists, declarations, dispatch, guide, role and skill text as given.
- [ ] **Step 4: Run** the same commands - all pass. Then `cd src-tauri && cargo test --test bindings` and `git diff --stat -- src/bindings.ts` - no change. Then `npx tsc --noEmit`.
- [ ] **Step 5: Commit** `feat(v2): the saved setup tools, their allow rules and the guide's Saved setups`.

---

## Final gate (after Task 7, before handing back)

- [ ] `cd src-tauri && cargo test --test suite` (the whole suite, once; nothing else running), `npx vitest run`, `npx tsc --noEmit`. A single `App.test.tsx` failure that passes on a re-run is its known flake; two different ones are not.
- [ ] `git grep -n "DELETE" -- src-tauri/src/autorun/kept.rs src-tauri/src/autorun/setups.rs src-tauri/src/autorun/setup_find.rs src-tauri/src/autorun/setup_run.rs` finds nothing; `git grep -nP "[\x{2013}\x{2014}]"` on the changed text files finds nothing new.
- [ ] Owed to a live org (cannot be tested here, list them in the report): an explore session that creates a leave type, saves it as a setup with a database rule, and a new chat that runs it twice (found the second time); the same with Database Read Access off and a browser rule; undo of a built record through `undo_explore_creates`.
