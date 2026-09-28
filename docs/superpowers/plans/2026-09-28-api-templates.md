# API Templates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the assistant build, prove and run declarative "API templates" that write test data through the application's own endpoints, shown to the user in a hidden tab they can only remove from, with DB and API calls logged to their own activity folder.

**Architecture:** A new Rust module `api_templates` (model + checks, pure request/capture/expect functions, file store, a runner generic over Auto Run's `Browsers`/`Driver`) is reached from four new AI-bridge routes and four MCP tools, gated exactly like Auto Run. The runner signs in with Auto Run's recipe and accounts, reads the anti-forgery token from a page, and sends each step with `fetch` from inside that page. A new `activity_log` module takes the detailed DB and API records out of the app log. A new hidden sidebar tab lists templates.

**Tech Stack:** Rust (Tauri 2, tauri-specta, tokio, serde_json), React 19 + TypeScript, vitest, Testing Library.

**Spec:** `docs/superpowers/specs/2026-09-28-api-templates-design.md` - read it first; this plan argues from it.

## Global Constraints

- Work on a branch: `git switch -c feat/api-templates` before Task 1.
- Commits: Bash heredoc only - `git commit -F - <<'EOF' ... EOF` - subject `feat(v2): ...`, body ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Confirm with `git log -1`.
- Every Rust test is an integration test in `src-tauri/tests/suite/<module>.rs` with a `mod <module>;` line in `suite/main.rs` (alphabetical). Never a `#[cfg(test)]` module in `src/`.
- Tests touching process-wide state take a lock from `src-tauri/tests/suite/serial.rs`.
- `src/bindings.ts` is generated: `cd src-tauri && cargo test --test bindings`. Never hand-edit it. No specta type, field or doc comment may contain `bearer`, `access_token`, `refresh_token`, `id_token`.
- One build or test command at a time on this machine.
- Methods: `GET` and `POST` only. Per step 30 s; per run 3 min. Response bodies read up to 64 KB; log bodies capped at 500 characters; failure excerpts at most 500 characters. Run history keeps the last 20. Activity files kept 30 days.
- Template `id`: lowercase ASCII letters, digits, `-`, `_`; 1-100 chars. (Lowercase is a tightening of the spec: Windows file names are case-insensitive, so `Pms-X` and `pms-x` would be one file.)
- The anti-forgery token, cookies and account passwords never reach a log line, an activity record, a run record, a bridge response or the bindings.
- User- and assistant-facing sentences name no URL host; raw browser/transport errors go to `applog` only.
- Colours from tokens only; icons from `src/lib/actionIcons.ts`; `src/ui-consistency.test.ts` is never weakened.
- The feature is offered where Auto Run is: `crate::ai_tools::autorun_offered()` in Rust, `autoRunVisible()` / `autoRunToolsShown()` in the webview.

## Review Focus

1. **A value containing `{{...}}`** (a cycle named `Q3 {{draft}}`) must be sent literally - substitution is one pass over the template, never over substituted values. Test in Task 3.
2. **A step answered with an HTML page** (a 200 login page, an error page) where `expect.json` or a `capture` needs JSON must fail that step with "the response was not JSON", not panic or pass. Test in Task 3.
3. **A placeholder in `query` or `path`** (`?cycleId={{cycleId}}`) must be substituted and percent-encoded (a space becomes `%20`, `&` never splits the query). Test in Task 3.
4. **A browser left running** after a failed, timed-out or panicking run - `Browsers::close` must run on every path. Test in Task 5 with a fake `Browsers` that counts closes.
5. **An impossible date** (`2026-02-30`) or a number sent as a string (`"33"` for a `number` param) must be refused before anything runs. Test in Task 2.

---

### Task 1: Activity log, and DB statements moved into it

**Files:**
- Create: `src-tauri/src/activity_log.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod activity_log;`; setup ~L396: init beside `applog::init`; register command)
- Modify: `src-tauri/src/db/query.rs` (call sites L116, L121, L125, L134, L177, L183, L187, L196, L291)
- Modify: `src-tauri/src/commands/misc.rs` (new command beside `open_app_log_dir` L48)
- Modify: `src/screens/Settings.tsx` (Logs button row L631-665)
- Test: `src-tauri/tests/suite/activity_log.rs` (new), `src-tauri/tests/suite/ai_bridge.rs` (`mod db_tests` L1972-2100), `src-tauri/tests/suite/serial.rs`, `src-tauri/tests/suite/common.rs`, `src/screens/Settings.test.tsx`

**Interfaces:**
- Produces:
  - `pub enum Kind { Db, Api }`
  - `pub const KEEP_DAYS: u64 = 30;`
  - `pub fn init(dir: PathBuf)` - stores the dir (re-callable, for tests), creates it, prunes.
  - `pub fn directory() -> Option<PathBuf>`
  - `pub fn file_name(kind: Kind, date: &str) -> String` -> `"db-2026-09-28.jsonl"` / `"api-2026-09-28.jsonl"`
  - `pub fn record(kind: Kind, entry: serde_json::Value)` - inserts `"at": applog::stamp()` and appends one line to today's file (UTC date = `applog::stamp()[..10]`); no dir set -> dropped.
  - `pub fn prune(dir: &Path, now: SystemTime, keep_days: u64)` - removes only `db-*.jsonl` / `api-*.jsonl` older than `keep_days` by mtime.
  - Command `open_activity_log_dir() -> Result<(), String>`; errors `"The activity folder is not set up yet."` / `"Could not open the activity folder."`
  - Test helpers: `serial::activity_log() -> MutexGuard<'static, ()>`; `common::activity_records(dir: &Path, kind: &str) -> Vec<serde_json::Value>` (reads every `{kind}-*.jsonl` line).

- [ ] **Step 1: Write the failing tests** in `tests/suite/activity_log.rs` (+ `mod activity_log;`):

```rust
#[test] fn a_record_lands_in_todays_file_for_its_kind() {
    let _g = crate::serial::activity_log();
    let dir = tempfile::tempdir().unwrap();
    activity_log::init(dir.path().to_path_buf());
    activity_log::record(Kind::Db, json!({ "sql": "SELECT 1" }));
    let recs = crate::common::activity_records(dir.path(), "db");
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["sql"], "SELECT 1");
    assert!(recs[0]["at"].as_str().unwrap().len() >= 19);
    assert!(crate::common::activity_records(dir.path(), "api").is_empty());
}
#[test] fn file_names_are_kind_and_date() {
    assert_eq!(activity_log::file_name(Kind::Api, "2026-09-28"), "api-2026-09-28.jsonl");
}
#[test] fn prune_keeps_thirty_days_and_touches_nothing_else() {
    // old db-/api- files (mtime 31 days back via File::set_modified) go; a 29-day-old one,
    // and an old `notes.txt`, stay.
}
```

- [ ] **Step 2: Update the five `db_tests` in `tests/suite/ai_bridge.rs`** (L1972, L2008, L2040, L2066, L2085) to take `serial::activity_log()` as well as `serial::log_tail()`, init the activity log in a tempdir, and assert:
  - the activity record has `"verdict": "refused"` with `"why"` equal to `READ_ONLY_SENTENCE` / `WRITES_OFF`, and `"sql"` equal to the statement **in full** (reads too, no 200-char cut);
  - a run write has `"verdict": "write"` and `"ok"`, `"duration_ms"` present;
  - **no** `applog::recent(400)` line contains the SQL text; a summary line starting `db query ` does exist.

- [ ] **Step 3: Run to see them fail**

Run: `cd src-tauri && cargo test --test suite activity_log:: ai_bridge::db_tests::`
Expected: FAIL - `activity_log` not found.

- [ ] **Step 4: Implement `activity_log.rs`**, then in `db/query.rs` replace every `applog::info(log_line/refusal_log_line/inline)` with:
  - `activity_log::record(Kind::Db, json!({ "connection": "<server>/<database>", "verdict": "read"|"write"|"refused"|"batch"|"lookup", "why"?: ..., "sql": <full>, "ok"?: bool, "rows"?: i64, "duration_ms"?: u64 }))` - refusals recorded at refusal time; runs recorded **after** the process returns (time it with `Instant`).
  - `rows`: a new `pub fn rows_affected(stdout: &str) -> Option<i64>` summing every `(N row(s) affected)`.
  - one `applog::info` summary with no SQL: `db query (Write) on {server}/{db}: ok, 3 rows, 120 ms` / `db query refused on {server}/{db}: {why}` / `db batch (...) on ...: ok` / `db lookup on {server}/{db}`.
  - Delete `log_line`/`refusal_log_line` if nothing else uses them.

- [ ] **Step 5: Wire setup and the command.** `lib.rs`: `applog::init(dir.clone()); activity_log::init(dir.join("activity"));`, add `misc::open_activity_log_dir` to `collect_commands!`. Regenerate bindings: `cd src-tauri && cargo test --test bindings`.

- [ ] **Step 6: Settings button.** Test in `Settings.test.tsx` next to L718: `"Open activity folder asks Rust to open it"` - clicking the button calls `commands.openActivityLogDir` once. Then add an outline `Button` "Open activity folder" (`IconBrowse`, `aria-hidden`) after "Open log folder", same error handling (`toast.error(r.error)`). Visible in every build - the DB tools are not hidden.

- [ ] **Step 7: Run everything touched**

Run: `cd src-tauri && cargo test --test suite activity_log:: ai_bridge::` then `npx vitest run src/screens/Settings.test.tsx`
Expected: PASS.

- [ ] **Step 8: Commit** - `feat(v2): database statements go to their own activity log, not the app log`

---

### Task 2: Template model and checks

**Files:**
- Create: `src-tauri/src/api_templates/mod.rs` (types + checks; declares `pub mod exec; pub mod store; pub mod runner; pub mod guide;` as those tasks land)
- Modify: `src-tauri/src/lib.rs` (`pub mod api_templates;`)
- Test: `src-tauri/tests/suite/api_templates.rs` (new, `mod api_templates;`)

**Interfaces:**
- Produces (all `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]`, `#[serde(deny_unknown_fields)]`):
  - `ApiTemplate { id, title, module, effect: Effect, description: String, sources: Vec<String>, antiforgery: Antiforgery, params: Vec<Param>, steps: Vec<Step>, outputs: Vec<String>, #[serde(default, skip_serializing_if = "Option::is_none")] proven: Option<Proven> }`
  - `Effect` = `create | edit | delete` (`rename_all = "lowercase"`)
  - `Antiforgery { page: String }`
  - `Param { name, #[serde(rename = "type")] kind: ParamType, #[serde(default)] required: bool, #[serde(default)] description: Option<String>, #[serde(default)] lookup: Option<String> }`
  - `ParamType` = `string | number | boolean | date | list`
  - `Step { name, method: Method, path, #[serde(default)] query: BTreeMap<String, String>, #[serde(default)] json: Option<serde_json::Value>, #[serde(default)] form: Option<BTreeMap<String, String>>, #[serde(default)] expect: Expect, #[serde(default)] capture: BTreeMap<String, String> }`
  - `Method` = `GET | POST` (`rename_all = "UPPERCASE"`)
  - `Expect { #[serde(default = "ok_status")] status: u16 /* 200 */, #[serde(default)] json: Option<serde_json::Value> }` with `Default` = `{ 200, None }`
  - `Proven { at: String, origin: String, account: String, outputs: BTreeMap<String, serde_json::Value> }`
  - `pub fn parse_draft(v: &serde_json::Value) -> Result<ApiTemplate, Vec<String>>` - serde errors become one sentence; then `check`; a draft with `proven` is refused: `"proven is written by the app - leave it out"`.
  - `pub fn check(t: &ApiTemplate) -> Vec<String>` - every problem, in template order.
  - `pub fn check_values(t: &ApiTemplate, values: &serde_json::Map<String, serde_json::Value>) -> Vec<String>`
  - `pub fn valid_id(id: &str) -> bool`

- [ ] **Step 1: Write the failing tests** (a `fn draft() -> Value` fixture building the spec §4 example minus `proven`):

```rust
#[test] fn the_spec_example_parses_and_checks_clean() { assert!(parse_draft(&draft()).is_ok()); }
#[test] fn an_unknown_field_is_refused_at_any_level() { /* top-level "extra", and steps[0].headers */ }
#[test] fn a_draft_carrying_proven_is_refused() { /* message contains "proven is written by the app" */ }
#[test] fn ids_are_lowercase_filename_safe_and_short() {
    assert!(valid_id("pms-create_draft-cycle"));
    for bad in ["", "Pms-x", "a/b", "a..b", "a b", &"x".repeat(101)] { assert!(!valid_id(bad)); }
}
#[test] fn only_relative_paths_on_the_same_origin() {
    // each refused with one sentence naming the step:
    for p in ["https://evil.test/x", "//evil.test/x", "/\\evil.test", "hr/x", "/hr/../x", "/hr/%2e%2e/x"] { ... }
}
#[test] fn a_step_has_at_most_one_body() { /* json + form -> refused */ }
#[test] fn a_placeholder_must_name_a_param_or_an_earlier_capture() {
    // step 1 uses {{cycleId}} captured in step 2 -> refused, naming step 1 and "cycleId";
    // {{nope}} anywhere (path, query value, json leaf, json key, form value) -> refused
}
#[test] fn capture_paths_are_checked() { /* "$.a", "$.a[0]", "$.a[*].id" ok; "a.b", "$..a", "$.a[x]" refused */ }
#[test] fn outputs_must_be_captured_somewhere() { /* outputs ["nope"] -> refused */ }
#[test] fn all_problems_come_back_together() { /* 3 faults -> check().len() == 3 */ }
#[test] fn values_are_checked_against_the_params() {
    // missing required; undeclared "extra"; "33" for number; "2026-02-30" and "28/09/2026" for date;
    // "yes" for boolean; {} for list -> each its own sentence naming the param
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cd src-tauri && cargo test --test suite api_templates::`
Expected: FAIL - module `api_templates` not found.

- [ ] **Step 3: Implement the types, `parse_draft`, `check`, `check_values`, `valid_id`** in `api_templates/mod.rs`. Path rule: starts with exactly one `/`, contains no `\`, no `..` segment after percent-decoding, and `recipe::origin_of(&format!("https://x.invalid{path}"))` still equals `https://x.invalid`. Placeholder scan and capture-path syntax come from Task 3's `exec::placeholders` and `exec::parse_capture_path` - write those two functions here first if Task 3 has not landed (they belong in `exec.rs`). Date: `YYYY-MM-DD` that is a real calendar day.

- [ ] **Step 4: Run tests** - same command. Expected: PASS.

- [ ] **Step 5: Commit** - `feat(v2): API template model and the checks a draft must pass`

---

### Task 3: Requests, placeholders, captures and expectations (pure)

**Files:**
- Create: `src-tauri/src/api_templates/exec.rs`
- Test: `src-tauri/tests/suite/api_templates.rs`

**Interfaces:**
- Produces:
  - `pub fn placeholders(s: &str) -> Vec<String>` - names inside `{{ }}`, trimmed.
  - `pub enum Seg { Key(String), Index(usize), All }`; `pub fn parse_capture_path(p: &str) -> Result<Vec<Seg>, String>`
  - `pub fn capture(body: &serde_json::Value, path: &[Seg]) -> Option<serde_json::Value>` - `All` maps the rest over an array; empty result -> `None`.
  - `pub fn substitute(v: &serde_json::Value, vars: &BTreeMap<String, serde_json::Value>) -> serde_json::Value` - a string that is exactly one placeholder becomes the var's value (typed); otherwise text substitution; keys substituted as text; one pass.
  - `pub fn substitute_str(s: &str, vars: &BTreeMap<String, serde_json::Value>) -> String` - strings inserted as-is, other values as their JSON text.
  - `#[derive(Serialize)] #[serde(tag = "kind", rename_all = "lowercase")] pub enum Body { None, Json { value: serde_json::Value }, Form { fields: BTreeMap<String, String> } }`
  - `#[derive(Serialize)] pub struct BuiltRequest { pub method: Method, pub url: String /* path + "?" + encoded query */, pub body: Body }`
  - `pub fn build_request(step: &Step, vars: &BTreeMap<String, serde_json::Value>) -> BuiltRequest` - query pairs in map order, keys and values percent-encoded after substitution.
  - `pub fn check_expect(e: &Expect, status: u16, body_text: &str) -> Result<Option<serde_json::Value>, String>` - returns the parsed JSON when the body parses; errors: `"expected status 200, got 400"`, `"the response was not JSON"` (only when `e.json` is set), `"expected success = true, got false"` (partial match, first mismatching key, nested objects recurse).
  - `pub fn excerpt(text: &str) -> String` - whitespace collapsed, at most 500 chars.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_whole_value_placeholder_keeps_its_type() {
    let vars = btree(&[("cycleId", json!(273)), ("ids", json!([1,2]))]);
    assert_eq!(substitute(&json!({"a":"{{cycleId}}","b":"{{ids}}"}), &vars), json!({"a":273,"b":[1,2]}));
}
#[test] fn a_placeholder_inside_text_is_text() {
    assert_eq!(substitute_str("cycle {{cycleId}}!", &btree(&[("cycleId", json!(273))])), "cycle 273!");
}
#[test] fn substituted_values_are_never_substituted_again() {                    // Review Focus 1
    let vars = btree(&[("name", json!("Q3 {{draft}}")), ("draft", json!("X"))]);
    assert_eq!(substitute_str("{{name}}", &vars), "Q3 {{draft}}");
}
#[test] fn query_values_are_substituted_and_encoded() {                         // Review Focus 3
    // query {handler: "Step", stepKey: "{{k}}"} with k = "a b&c" ->
    assert_eq!(req.url, "/hr/pmsv10/performancecycle?handler=Step&stepKey=a%20b%26c");
}
#[test] fn capture_reads_keys_indexes_and_every_element() {
    let body = json!({"cycleId":274,"stages":[{"stageId":"s1"},{"stageId":"s2"}]});
    // "$.cycleId" -> 274; "$.stages[1].stageId" -> "s2"; "$.stages[*].stageId" -> ["s1","s2"]; "$.nope" -> None
}
#[test] fn expect_is_a_partial_match() { /* {"success":true} matches {"success":true,"cycleId":1} */ }
#[test] fn an_html_answer_fails_a_json_expectation_cleanly() {                  // Review Focus 2
    let e = Expect { status: 200, json: Some(json!({"success": true})) };
    assert_eq!(check_expect(&e, 200, "<!DOCTYPE html><html>").unwrap_err(), "the response was not JSON");
}
#[test] fn excerpts_are_capped_at_500() { assert!(excerpt(&"x".repeat(900)).chars().count() <= 500); }
```

- [ ] **Step 2: Run to see them fail** - `cd src-tauri && cargo test --test suite api_templates::` -> FAIL (unresolved `exec`).
- [ ] **Step 3: Implement `exec.rs`** with the signatures above.
- [ ] **Step 4: Run tests** -> PASS (Task 2's tests too).
- [ ] **Step 5: Commit** - `feat(v2): building a template's requests, captures and checks`

---

### Task 4: Template store and the tab's two commands

**Files:**
- Create: `src-tauri/src/api_templates/store.rs`, `src-tauri/src/commands/api_templates.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (`collect_commands!`)
- Test: `src-tauri/tests/suite/api_templates.rs`

**Interfaces:**
- Consumes: `ApiTemplate`, `valid_id` (Task 2); `crate::autorun::recipe::{project_slug, load_recipe}`; `crate::ai_tools::atomic_write`.
- Produces:
  - `pub fn templates_dir(root: &Path, org: &str, project: &str) -> PathBuf` = `root/templates/<project_slug>`
  - `pub fn load(root, org, project, id: &str) -> Result<Option<ApiTemplate>, String>`
  - `pub fn save(root, org, project, t: &ApiTemplate) -> Result<(), String>` - atomic; refuses an invalid id.
  - `pub fn list(root, org, project) -> Result<Vec<SavedTemplate>, String>` - sorted by `module`, then `title`; a file that no longer parses is skipped and logged (`applog::warn`), never fatal.
  - `pub fn remove(root, org, project, id) -> Result<(), String>` - deletes `<id>.json` and `<id>.runs.json`; missing is not an error.
  - `#[derive(Serialize, Deserialize, specta::Type)] pub struct RunRecord { at: String, account: String, ok: bool, failed_step: Option<String>, detail: Option<String>, outputs: BTreeMap<String, Value> }`
  - `pub fn append_run(root, org, project, id, r: RunRecord) -> Result<(), String>` - newest first, keeps 20.
  - `#[derive(Serialize, specta::Type)] pub struct SavedTemplate { template: ApiTemplate, runs: Vec<RunRecord> }`
  - Commands (refuse with `"not available in this build"` unless `ai_tools::autorun_offered()`):
    - `api_templates_overview(app, organization: String, project: String) -> Result<TemplatesOverview, String>` where `TemplatesOverview { origin: Option<String> /* recipe start_url origin */, templates: Vec<SavedTemplate> }`
    - `api_templates_remove(app, organization: String, project: String, id: String) -> Result<(), String>` - logs `api template removed: {id}` to `applog`.

- [ ] **Step 1: Write the failing tests** (tempdir root; parsed spec example + a `proven` block):
  - `save_then_load_round_trips`
  - `list_is_grouped_by_module_then_title`
  - `a_broken_file_is_skipped_not_fatal`
  - `run_history_keeps_the_newest_twenty` (append 25; `list` shows 20, newest first)
  - `remove_takes_the_template_and_its_history`
  - `an_invalid_id_never_reaches_the_disk` (`save` with id `"../x"` -> `Err`, nothing written)
- [ ] **Step 2: Run to see them fail** - `cargo test --test suite api_templates::` -> FAIL.
- [ ] **Step 3: Implement `store.rs` and the two commands**; register them; regenerate bindings (`cargo test --test bindings`). Check `ApiTemplate` / `SavedTemplate` appear in `src/bindings.ts` and no forbidden token name does.
- [ ] **Step 4: Run tests** -> PASS, including `cargo test --test bindings`.
- [ ] **Step 5: Commit** - `feat(v2): where API templates and their run history are kept`

---

### Task 5: The runner

**Files:**
- Create: `src-tauri/src/api_templates/runner.rs`
- Modify: `src-tauri/src/commands/autorun_replay.rs` (make `RealBrowsers` `pub(crate)` with `pub(crate) fn new(which: Browser, watch: bool) -> Self`)
- Test: `src-tauri/tests/suite/api_templates_runner.rs` (new, `mod api_templates_runner;`), using `common::{ScriptedDriver, quick, account, recipe}`

**Interfaces:**
- Consumes: Tasks 2-4; `autorun::replay::Browsers`; `autorun::signin::{prepare, sign_in}`; `autorun::sessions::{forget_session, now_ms}`; `browser::actions::{execute_in, Action, Policy}`; `browser::page::{document, call_value, eval_value}`; `activity_log::{record, Kind}`.
- Produces:
  - `pub enum Mode { Prove { replace: bool, why: Option<String> }, Run }`
  - `pub struct RunRequest { pub org: String, pub project: String, pub account: String, pub values: serde_json::Map<String, Value>, pub mode: Mode, pub template: ApiTemplate }`
  - `#[derive(Serialize)] pub struct StepReport { name: String, handler: Option<String>, status: Option<u16>, ok: bool, detail: String }`
  - `#[derive(Serialize)] pub struct RunReport { ok: bool, template: String, outputs: BTreeMap<String, Value>, created: BTreeMap<String, Value> /* everything captured before a failure */, steps: Vec<StepReport>, failed: Option<String> }`
  - `pub fn preflight(root: &Path, req: &RunRequest, existing: Option<&ApiTemplate>) -> Result<(), Vec<String>>` - `check` + `check_values` + `prepare` (recipe and account) + the replace rule: an existing id without `replace: true` and a non-empty `why` -> `a template called "{id}" already exists - send replace: true and a why to change it`.
  - `pub struct RunClaim` (drop releases); `pub fn claim() -> Option<RunClaim>` - one process-wide `AtomicBool`.
  - `pub async fn run_template<B: Browsers>(browsers: &mut B, root: &Path, req: &RunRequest, timing: &Timing) -> RunReport` - does NOT save; the caller (Task 6) saves on a proven success and appends run history.
  - `pub const FETCH_FN: &str` - a JS `function (req, token)` run with `call_value` on `document()`: builds `fetch(req.url, { method, credentials: "same-origin", headers: { RequestVerificationToken: token, Accept: "application/json" } , body })` (`Json` -> `JSON.stringify` + `Content-Type: application/json`; `Form` -> `FormData`), aborts after 30 s, returns `{ status, contentType, finalUrl: r.url, redirected: r.redirected, text: (await r.text()).slice(0, 65536) }`.
  - `pub const TOKEN_FN: &str` - `function () { const i = this.querySelector('input[name="__RequestVerificationToken"]'); return i ? i.value : null; }`

Run order (`run_template`):
1. `browsers.open()`; every exit path below goes through `browsers.close(d)`.
2. `sign_in`; `!ok` -> failed report: `could not sign in as "{key}": {detail} - sign that account in once from Auto Run, then try again`.
3. `execute_in(Navigate { url: origin + antiforgery.page }, Policy::only(recipe.origins()))`; read `location.href`. If its path (case-insensitive, query ignored) differs from the page's path, the session was stale: `forget_session`, sign in once more, navigate once more; still different -> fail `the token page sent us to another page - check the template's antiforgery page`. Read the token with `TOKEN_FN`; `null` -> `no anti-forgery token on {page}`.
4. For each step: `d.set_deadline(Some(now + 30 s))`; `build_request`; `call_value(FETCH_FN, [req, token])`; a `redirected` answer whose `finalUrl` path differs from the request path -> fail `was sent to another page - the session may have ended`; `check_expect`; apply every `capture` (none found -> fail `capture {name} found nothing at {path}`); record `activity_log::record(Kind::Api, { template, mode, account, origin, step, method, url, handler, status, duration_ms, request: excerpt(body), response: excerpt(text) })`.
5. The whole run sits in `tokio::time::timeout(180 s, ...)`; elapsing -> fail `the run took longer than 3 minutes`.
6. One `applog::info` summary: `api template {id}: {prove|run} {ok|failed at <step>}, {n} steps`.

- [ ] **Step 1: Write the failing tests.** A fake `Browsers` (`opened`, `closed` counters) handing out a `ScriptedDriver` that: answers `Page.navigate` + pushes the `load` lifecycle event (pattern in the `common.rs` notes); answers `location.href`; answers `Runtime.callFunctionOn` by `functionDeclaration` - `TOKEN_FN` -> `"tok-123"`, `FETCH_FN` -> the next queued response. Sign-in uses `common::stateful_app`-style answers or a saved session.
  - `a_clean_run_captures_and_returns_outputs` - two steps; `outputs == {"cycleId": 274}`; the second request's form carried `"274"`.
  - `the_token_goes_in_the_header_and_nowhere_else` - `FETCH_FN` got `"tok-123"` as its second argument; the serialized `RunReport`, every activity record, and `applog::recent(400)` contain no `"tok-123"` (take `serial::log_tail()` + `serial::activity_log()`).
  - `a_failure_stops_the_run_and_says_what_was_created` - step 2 answers 400; `failed == Some("Evaluation rules")`, `created == {"cycleId": 274}`, step 3 never fetched.
  - `a_stale_session_at_the_token_page_signs_in_once_more` - first `location.href` is `/hr/security/login`, second is the page; `ok`.
  - `a_login_redirect_mid_run_fails_rather_than_signing_in_again` - a step answers `redirected: true` to the login path; failed; exactly one sign-in happened.
  - `the_browser_is_closed_on_every_path` (Review Focus 4) - success, step failure, sign-in failure, token missing: `closed == opened == 1` each time.
  - `only_one_run_at_a_time` - `claim()` then `claim()` -> second is `None`; dropping the first frees it.
  - `preflight_needs_replace_and_why_for_an_existing_id`
- [ ] **Step 2: Run to see them fail** - `cargo test --test suite api_templates_runner::` -> FAIL.
- [ ] **Step 3: Implement `runner.rs`** and the `RealBrowsers` visibility change.
- [ ] **Step 4: Run tests** -> PASS; then `cargo test --test suite autorun` to confirm replay is unaffected.
- [ ] **Step 5: Commit** - `feat(v2): running an API template from inside a signed-in page`

---

### Task 6: Bridge routes, MCP tools, the switch, the change event

**Files:**
- Create: `src-tauri/src/api_templates/guide.rs`
- Modify: `src-tauri/src/ai_bridge.rs` (`BridgeContext` L19-49 + its `Debug` L54-75; `autorun_guard_for` L347; route arms L219-305; a sink beside `set_intake_sink` L93)
- Modify: `src-tauri/src/commands/ai_bridge.rs` (`set_bridge_context` L82 gains `api_writes: bool` last; install the sink in `bridge_status`)
- Modify: `src-tauri/src/events.rs` + `lib.rs` `collect_events!`
- Modify: `src-tauri/src/mcp.rs` (`tools_list` after the Auto Run entries ~L360; `tools_call` beside L601-670)
- Modify: `src-tauri/src/ai_tools.rs` (`DEV_ONLY_TOOLS` L127)
- Test: `src-tauri/tests/suite/ai_bridge.rs`, `tcm_mcp.rs`, `ai_tools.rs`, `api_templates.rs`

**Interfaces:**
- Consumes: Tasks 2-5.
- Produces:
  - `BridgeContext.api_writes: bool` (default false).
  - `pub const API_WRITES_OFF: &str = "API templates are switched off - turn them on under API templates on the AI Bridge tab";`
  - Routes (all behind the Auto Run guard, now `path.starts_with("/autorun-") || path.starts_with("/api-template")`):
    - `GET /api-template-guide` -> 200 `guide::text(accounts: &[String], origin: Option<&str>)`
    - `GET /api-templates` -> 200 JSON array of `{ id, title, module, effect, params, outputs, last_run }`
    - `POST /api-template-prove` body `{ template, account, values, replace?, why?, browser? }`
    - `POST /api-template-run` body `{ id, account, values, browser? }`
    - Status: switch off / preflight problems -> 400 (problems joined with newlines); busy -> 409 `another API template is running - wait for it to finish`; no root -> 503; run failed -> 502 with the `RunReport` JSON; ok -> 200 with the `RunReport` JSON. `browser`: `"edge"` (default) or `"chrome"` via `Browser::from_name`.
    - On a proven success: `store::save` with `proven = { at: applog::stamp(), origin, account, outputs }`, `why` logged. On every run: `store::append_run`. Both then `templates_changed(id)`.
  - `pub fn set_templates_sink(f: Box<dyn Fn(String) + Send + Sync>)` (a `OnceLock`, like `INTAKE_SINK`).
  - `#[derive(Clone, Serialize, specta::Type, tauri_specta::Event)] pub struct ApiTemplatesChanged { pub id: String }`
  - MCP tools: `get_api_template_guide`, `list_api_templates`, `prove_api_template`, `run_api_template`, mapped to the routes above; all four in `DEV_ONLY_TOOLS`. Descriptions: one paragraph each, from spec §7.

- [ ] **Step 1: Write the failing tests**
  - `ai_bridge.rs`: `api_template_routes_are_404_when_auto_run_is_not_offered` (all four paths, via `autorun_guard_for(path, false)`); `prove_and_run_are_refused_while_the_switch_is_off` (400, body == `API_WRITES_OFF`, nothing launched); `guide_and_list_answer_with_the_switch_off`; `a_prove_with_a_bad_draft_lists_every_problem` (400, 3 lines); `no_api_template_route_smells_like_a_write` (`smells_like_a_write("POST", p)` false for all four).
  - `tcm_mcp.rs`: the four names are in `tools_list` when offered and absent when not; `tools_call("run_api_template")` goes to `POST /api-template-run` with the args as body.
  - `ai_tools.rs`: `DEV_ONLY_TOOLS` contains the four names.
  - `api_templates.rs`: `the_guide_names_the_accounts_and_origin_but_no_password`.
  - Update every `BridgeContext { ... }` literal in tests (L17, L1560) with `api_writes: false` or `..Default::default()`.
- [ ] **Step 2: Run to see them fail** - `cargo test --test suite ai_bridge:: tcm_mcp:: ai_tools:: api_templates::` -> FAIL.
- [ ] **Step 3: Implement** the guide, context field, guard, routes, sink, event, MCP entries, `DEV_ONLY_TOOLS`, `set_bridge_context` parameter. Regenerate bindings.
- [ ] **Step 4: Run tests** -> PASS, plus `cargo test --test bindings`.
- [ ] **Step 5: Commit** - `feat(v2): the assistant's four API template tools, behind their own switch`

---

### Task 7: The switch and tool rows in the webview

**Files:**
- Create: `src/lib/apiTemplates.ts`
- Modify: `src/App.tsx` (context push L677-763), `src/screens/AiBridge.tsx` (a card after the `data-tour="ai-db"` card; the breakdown list ~L753), `src/lib/mcpTools.ts`
- Test: `src/lib/apiTemplates.test.ts` (new), `src/screens/AiBridge.test.tsx`, `src/lib/mcpTools.test.ts`, `src/App.test.tsx` (~L377)

**Interfaces:**
- Consumes: `commands.setBridgeContext(..., dbWrites, apiWrites)` (Task 6 bindings).
- Produces: `API_WRITES_KEY = "tcm-v2-api-writes"`; `loadApiWrites(): boolean`; `saveApiWrites(on: boolean): void`; `apiWritesSnapshot(): boolean`; `subscribeApiWrites(cb: () => void): () => void`.

- [ ] **Step 1: Write the failing tests**
  - `apiTemplates.test.ts`: off by default; `saveApiWrites(true)` persists and notifies subscribers.
  - `AiBridge.test.tsx`: `"the API templates switch is off, and stored when turned on"` - `getByRole("switch", { name: "API templates (create, edit and delete)" })`; `"capture mode and a locked release build show no API templates card"`; the breakdown card names the four tools only when offered.
  - `mcpTools.test.ts`: the dev-build row count goes from seven to eight; the new row is labelled `"API templates"` and carries all four names; the core/dev-only lists still match Rust.
  - `App.test.tsx`: the context push sends `apiWrites` as the last argument (false by default, true after `saveApiWrites(true)`).
- [ ] **Step 2: Run to see them fail** - `npx vitest run src/lib/apiTemplates.test.ts src/screens/AiBridge.test.tsx src/lib/mcpTools.test.ts src/App.test.tsx` -> FAIL.
- [ ] **Step 3: Implement.** `mcpTools.ts`: four `MCP_TOOLS` entries, four names in `DEV_ONLY_TOOLS`, one `TOOL_PAIRS` group, `PAIR_ROWS.get_api_template_guide = { label: "API templates", summary: "Build, prove and run templates that write test data through the application's own endpoints." }`. `AiBridge.tsx`: a card shown only when `autoRunToolsShown()`, one `Switch` with the aria-label above, and a `text-[11px] text-faint` note: "Off by default. Templates run as your Auto Run accounts, against the sign-in recipe's site." `App.tsx`: read `useSyncExternalStore(subscribeApiWrites, apiWritesSnapshot)`, pass it last, add it to the effect's deps.
- [ ] **Step 4: Run tests** -> PASS; then `npx tsc --noEmit`.
- [ ] **Step 5: Commit** - `feat(v2): API templates switch on the AI Bridge tab`

---

### Task 8: The hidden API Templates tab

**Files:**
- Create: `src/screens/ApiTemplates/index.tsx`, `src/screens/ApiTemplates/TemplateRow.tsx`, `src/screens/ApiTemplates/RemoveTemplate.tsx`
- Modify: `src/components/Sidebar.tsx` (L31, L49-64, L73-87), `src/components/navGlyphs.tsx` (+ motion CSS in `src/index.css` "Sidebar glyph motion"), `src/lib/extras.ts` (L115), `src/lib/prefs.ts` (L21), `src/App.tsx` (L71, L122, L165-181, L348-350, L1293-1306)
- Test: `src/screens/ApiTemplates/index.test.tsx` (new), `src/components/Sidebar.test.tsx`, `src/lib/extras.test.ts`, `src/tour/tourScript.test.ts`, `src/App.test.tsx`

**Interfaces:**
- Consumes: `commands.apiTemplatesOverview`, `commands.apiTemplatesRemove`, `events.apiTemplatesChanged` (Tasks 4, 6); `apiWritesSnapshot`/`subscribeApiWrites` (Task 7).
- Produces:
  - `Section` gains `"apitemplates"`; `CASE_ITEMS` gains, after `ai`, `{ id: "apitemplates", label: "API Templates", icon: GlyphApiTemplates, tone: "nav-ico nav-ico-apitemplates", note: "In Dev" }`.
  - `visibleCaseItems` hides both `autorun` and `apitemplates` unless `autoRun`.
  - `sectionShortcut` returns `undefined` beyond the 9th item.
  - `export function shouldLeaveHidden(section: string, shown: boolean, hydrated: boolean): boolean` replaces `shouldLeaveAutoRun` (true for `autorun` or `apitemplates`).
  - `GlyphApiTemplates` - lucide `Braces` geometry split into two parts, `<Glyph kind="apitemplates">`, a hover motion under the reduced-motion guard.
  - `export default function ApiTemplates({ org, project, onOpenAiBridge }: { org: string; project: string; onOpenAiBridge: () => void })`

- [ ] **Step 1: Write the failing tests**
  - `Sidebar.test.tsx`: `shortcutOrder(true)` ends `"ai", "apitemplates"`; `shortcutOrder(false)` unchanged; `sectionShortcut("apitemplates", true)` is `undefined`; `sectionShortcut("ai", true)` is still `"mod+9"`; locked release build hides it.
  - `extras.test.ts`: `shouldLeaveHidden("apitemplates", false, true)` is true; `(…, false, false)` is false (not hydrated).
  - `tourScript.test.ts`: keys do not contain `"apitemplates"`.
  - `App.test.tsx`: resetting extras while on the tab lands on Manual Entry; the heading shows "API Templates" with the "In Development" pill.
  - `ApiTemplates/index.test.tsx` (mock `commands` and `events` as other screen tests do):
    - `"groups templates by module and filters by title, module or id"`
    - `"a row shows its effect badge, parameter count, proven line and last run"`
    - `"expanding a row shows params, steps, sources, evidence and runs, read-only"` (no inputs, no edit buttons)
    - `"Remove asks first, names the template and its effect, then removes it"` - Keep it cancels; Remove calls `apiTemplatesRemove(org, project, id)` once
    - `"the list refreshes when the change event arrives"`
    - `"the header shows the environment host and the switch state, which opens AI Bridge"`
    - `"an empty project explains where templates come from"`
- [ ] **Step 2: Run to see them fail** - `npx vitest run src/screens/ApiTemplates src/components/Sidebar.test.tsx src/lib/extras.test.ts src/tour src/App.test.tsx` -> FAIL.
- [ ] **Step 3: Implement.**
  - `index.tsx`: `useQuery({ queryKey: ["api-templates", org, project], queryFn: () => unwrap(commands.apiTemplatesOverview(org, project)), enabled: Boolean(org && project) })` (local data - no persistent cache). Listen with `events.apiTemplatesChanged.listen` -> `invalidateQueries`. Search `Input` (`aria-label="Search templates"`). Groups by `template.module`, expand with `Collapse` like `AutoRun/index.tsx:388-432`.
  - Effect badge tokens: create `text-success bg-success/15`, edit `text-warning bg-warning/15`, delete `text-danger bg-danger/15`.
  - `RemoveTemplate.tsx`: `Modal` with heading `Remove {title}?`, the effect sentence ("This template creates / edits / deletes data."), "It is removed from this machine with its run history. There is no undo; the assistant can prove it again.", ghost "Keep it" (`IconCancel`) and danger "Remove" (`IconRemove`).
  - Empty state: "Your assistant builds these from the application's code and proves each one before it appears here. Connect one and turn on API templates on the AI Bridge tab." with a Button that calls `onOpenAiBridge`.
  - `App.tsx`: lazy import; `TITLES.apitemplates = "API Templates"`; `TITLE_NOTES.apitemplates = "In Development"`; mount `{autoRunShown && section === "apitemplates" && <ApiTemplates org={org} project={project} onOpenAiBridge={() => setSection("ai")} />}`; use `shouldLeaveHidden`.
- [ ] **Step 4: Run tests** -> PASS; then the full frontend gate: `npm test` and `npx tsc --noEmit`.
- [ ] **Step 5: Commit** - `feat(v2): the API Templates tab, hidden like Auto Run`

---

### Task 9: Keep it out of the help site; full gate; acceptance

**Files:**
- Modify: `docs-site/src/guard.test.ts` (`FORBIDDEN`, L29)
- Possibly: `src-tauri/help/` + shots (only if Step 2 finds a Settings Logs shot)

- [ ] **Step 1: Guard terms.** Add `/api ?templates?/i` and `/api_template/i` to `FORBIDDEN`. Run `npx vitest run docs-site/src/guard.test.ts` -> PASS (nothing documented mentions them).
- [ ] **Step 2: Help-site shots.** `grep -rn "Open log folder" docs-site/src` - if a documented shot shows the Logs button row, follow CLAUDE.md's help-site procedure (close `tauri dev`; `npm run docs:dev`; `npm run docs:shots -- --dry-run`; `npm run docs:shots -- --only <shot-id>`; `npm run docs:build`) and commit shots, positions and built site together. If none shows it, note that and skip.
- [ ] **Step 3: Full gate, one at a time.** `cd src-tauri && cargo test --tests` -> all pass; `npm test` -> all pass (one `App.test.tsx` load flake that passes on re-run is known); `npx tsc --noEmit`; `npm run build`.
- [ ] **Step 4: Acceptance with the owner** (manual, against dev01, `npm run tauri dev`):
  1. Auto Run has a sign-in recipe for the project with `start_url` on `hrmmainphdev01.phrsandbox.dev` and an account; turn on API templates on the AI Bridge tab.
  2. In a Claude session registered on the bridge: `get_api_template_guide`; the assistant reads `D:\PMS_Module\HRM-PMS-NET\...\Pages\PerformanceCycle\Index.CycleSetup.cshtml.cs` and `Index.EvaluationRules.cshtml.cs`, looks up a rating method with `db_query`, and calls `prove_api_template` for **Create a draft performance cycle** (Cycle setup + Evaluation rules).
  3. Settle spec §11: does a plain GET of `/hr/pmsv10/performancecycle?mode=create` render `__RequestVerificationToken`? If not, stop and bring the finding back before changing the format.
  4. The template appears in the tab without a refresh; `run_api_template` with a new name returns a new `cycleId`; the row's last run updates; `activity/api-<today>.jsonl` holds the steps with no token; Remove takes it off the list.
- [ ] **Step 5: Commit** (only if Steps 1-2 changed files) - `feat(v2): keep API templates out of the help site`

---

## Self-review notes

- Spec §1-3 context; §4 -> Tasks 2-3; §5 -> Task 5 (+ saving in Task 6); §6 -> Task 1 (+ API records in Task 5); §7 -> Task 6; §8 -> Tasks 7-8; §9 -> Tasks 2, 5, 6; §10 -> every task + Task 9; §11 -> Task 9 Step 4.3; §12-14 need no code.
- Deviations from the spec, both tightenings: ids are lowercase-only (Global Constraints); DB records keep reads in full rather than the old 200-character cut (the activity log is the audit record, and it is no longer in bug reports).
