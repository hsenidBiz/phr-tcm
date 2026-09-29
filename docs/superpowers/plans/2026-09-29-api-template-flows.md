# API Template Flows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give each module's wizard a saved flow of stages, refuse any API template prove or run whose required earlier stages are not done for that record (asked of the database), give the assistant step-by-step progress, and draw each flow as a map on the API Templates tab.

**Architecture:** A pure flow model (`api_templates/flow.rs`) and its file store (`flow_store.rs`); a gate (`gate.rs`) that runs stage checks through a small `StageDb` trait - sqlcmd behind it in the app, a fake in tests; the bridge wires the gate into prove and run before any browser opens, and adds two routes; the tab gains a self-drawn flow map laid out by a pure function.

**Tech Stack:** Rust (tauri 2, tauri-specta, tokio, serde), React 19 + TypeScript, vitest + Testing Library.

**Spec:** `docs/superpowers/specs/2026-09-29-api-template-flows-design.md` (builds on `docs/superpowers/specs/2026-09-28-api-templates-design.md`).

## Global Constraints

- Everything is offered exactly where Auto Run is: `ai_tools::autorun_offered()` in Rust, `autoRunToolsShown()` / `useAutoRunVisible()` in the webview. Never named in `src/lib/changelog.ts` or How To Use.
- Every Rust test is in `src-tauri/tests/suite/` (one binary); a new file gets a `mod` line in `suite/main.rs`. Never a `#[cfg(test)]` module in `src/`.
- `src/bindings.ts` is generated: `cd src-tauri && cargo test --test bindings`. Never hand-edited.
- Tests touching the activity log take `serial::activity_log()`; the process-wide Auto Run root takes the lock its existing tests take (see `suite/ai_bridge.rs` `root_with_recipe_and_account`).
- One build or test command at a time on this machine.
- No user-facing sentence includes a raw database, browser or transport error; those go to `applog` / the activity log.
- Colours from tokens only; `src/ui-consistency.test.ts` and the a11y suite unchanged. Icons from `src/lib/actionIcons.ts`.
- Flow ids and stage ids follow `api_templates::valid_id`. At most 30 stages. A check at most `db::guard::MAX_SQL_CHARS` characters, 15 seconds per check.
- Refusal sentence shapes (spec §5), verbatim:
  - `this template's flow {flow} is no longer saved`
  - `stage "{stage}" is no longer in flow {flow}`
  - `this template belongs to a flow, and flow checks need a database: choose one on the AI Bridge tab`
  - `{Stage title} is not done for {subject} {value} - do it first with {template id} ({template title}). Then: {next stage titles joined ", "}.` (the `Then:` part only when there is a following stage)
  - `... no template performs {Stage title} yet: prove one first` (replacing the `do it first with` clause when no saved template performs it)
- Commits via Bash heredoc with the trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; never name the secret unlock.

## Review Focus

1. **A check whose answer has no row count** (the assistant wrote `SET NOCOUNT ON;` in it, or sqlcmd's output was capped): must count as *could not run*, never as *not done* or *done*. Pinned in Task 3.
2. **A number subject sent as a string** (`"cycleId": "274"`), a float, or a negative: refused with a sentence naming the subject and its type before any statement runs - never quoted into the SQL. Pinned in Task 1.
3. **A saved flow file that no longer parses**: the tab's overview and `list_api_templates` still answer with every other flow and template, and a template on that flow is refused with the "no longer saved" sentence rather than a 500. Pinned in Tasks 2 and 4.
4. **A template saved before this change** (no `stage` field) keeps loading, listing and running exactly as before. Pinned in Task 1 (serde) and Task 4 (a run with no database chosen still works).
5. **A diamond-shaped flow** (two stages both requiring `setup`, a third requiring both): the third sits in column 2, both arrows are drawn, and the gate requires both. Pinned in Tasks 1 and 7.

---

### Task 1: The flow model and its checks

**Files:**
- Create: `src-tauri/src/api_templates/flow.rs`
- Modify: `src-tauri/src/api_templates/mod.rs` (declare `pub mod flow;`; `ApiTemplate` gains `stage`)
- Create: `src-tauri/tests/suite/api_template_flows.rs`; Modify: `src-tauri/tests/suite/main.rs` (`mod api_template_flows;`)

**Interfaces:**
- Produces (all in `crate::api_templates::flow`, every struct `#[serde(deny_unknown_fields)]`, `Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type`):
  - `pub enum SubjectType { Number, String }` (`#[serde(rename_all = "lowercase")]`)
  - `pub struct Subject { pub name: String, #[serde(rename = "type")] pub kind: SubjectType }`
  - `pub struct Stage { pub id: String, pub title: String, #[serde(default)] pub requires: Vec<String>, #[serde(default)] pub optional: bool, #[serde(default)] pub creates: bool, pub check: String }`
  - `pub struct FlowSaved { pub at: String, pub sample: serde_json::Value }`
  - `pub struct Flow { pub id, pub title, pub module: String, pub subject: Subject, #[serde(default)] pub sources: Vec<String>, pub stages: Vec<Stage>, #[serde(default, skip_serializing_if = "Option::is_none")] pub saved: Option<FlowSaved> }`
  - `pub struct StageRef { pub flow: String, pub id: String }`
  - `pub const MAX_STAGES: usize = 30;`
  - `pub fn check_flow(f: &Flow) -> Vec<String>` - every rule in spec §3, all problems together; a draft carrying `saved` is a problem.
  - `pub fn parse_flow(v: &serde_json::Value) -> Result<Flow, Vec<String>>` - serde then `check_flow`, like `api_templates::parse_draft`.
  - `pub fn substitute_check(check: &str, subject: &Subject, value: &serde_json::Value) -> Result<String, String>` - typed substitution (spec §3), then `db::guard::classify` must be `Verdict::Read`.
  - `pub fn required_before<'a>(f: &'a Flow, stage: &str) -> Vec<&'a Stage>` - transitive closure of `requires`, in declaration order, excluding the stage itself.
  - `pub fn creating_stage(f: &Flow) -> Option<&Stage>`
  - `pub fn check_stage_ref(t: &ApiTemplate, f: Option<&Flow>) -> Vec<String>` - spec §4's rules (flow saved, stage exists, subject capture+output for the creating stage, subject param of matching type otherwise). `f: None` with `t.stage: Some` yields the "no longer saved" sentence.
- `ApiTemplate` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub stage: Option<flow::StageRef>` after `outputs`.

- [ ] **Step 1: Write the failing tests** in `suite/api_template_flows.rs`, a `flow()` fixture being the spec §3 example with `competencies` `optional: true` and real-looking checks containing `{{cycleId}}`:
  - `a_valid_flow_has_no_problems` - `check_flow(&flow()).is_empty()`.
  - One test per rule, each asserting the returned `Vec` contains a sentence naming the offending id: unknown field (via `parse_flow` of JSON with `"colour": 1`); bad flow id `"Pms X"`; duplicate stage id; subject name `"cycle id"`; subject type `"date"` (serde error surfaced by `parse_flow`); zero creating stages; two creating stages; creating stage with `requires`; non-creating stage with empty `requires`; `requires` naming `"nope"`; a stage requiring itself; a cycle `a -> b -> a` (reported once, naming both); a stage requiring an optional stage; 31 stages; a check without `{{cycleId}}`; a check with `{{other}}`; a check that is `UPDATE ...`; a draft with `saved`.
  - `substitution_is_typed` - number `274` gives `... = 274`; string subject `"O'Brien"` gives `N'O''Brien'`; `json!("274")`, `json!(2.5)`, `json!(-1)`, `json!(true)` for a number subject are `Err` naming `cycleId` and `number`; a string subject given `json!(5)` is `Err`.
  - `substitution_is_classified_again` - a check whose substituted text is not a read (construct via a string subject that closes a comment is impossible after escaping - instead assert `substitute_check("SELECT 1 WHERE x = {{cycleId}}; DELETE FROM t", ..)` is `Err`).
  - `required_before_is_the_transitive_closure` - for `publish`: `["setup","rules","participants"]` in declaration order; never contains `competencies`; for a diamond (`b` and `c` require `a`, `d` requires `b` and `c`): `required_before(d) == [a, b, c]`.
  - `a_template_names_its_stage` - `check_stage_ref` cases: flow `None` -> the "no longer saved" sentence verbatim; unknown stage -> the "no longer in flow" sentence verbatim; creating-stage template without a `cycleId` capture -> problem; with the capture but not in `outputs` -> problem; participants template without a `cycleId` param -> problem; with a `cycleId` param of type `string` -> problem naming the type; a correct one -> empty.
  - `a_template_saved_before_flows_still_reads` - an existing template JSON with no `stage` key deserializes, `stage == None`, and re-serializes without a `stage` key.

- [ ] **Step 2: Run to confirm they fail**
  Run: `cd src-tauri && cargo test --test suite api_template_flows::`
  Expected: compile errors for the missing `flow` module.

- [ ] **Step 3: Implement `flow.rs`** with the interfaces above. Cycle detection by depth-first search over `requires` with a visiting set; `required_before` by walking `requires` from the stage and then filtering `f.stages` in order. Substitution replaces every `{{name}}` occurrence with the typed literal; a number must satisfy `Value::as_u64()` (non-negative integers only). Add the `stage` field to `ApiTemplate`, and make `api_templates::check` NOT look at `stage` (flows are checked where a flow can be loaded - Task 4).

- [ ] **Step 4: Run to confirm they pass**
  Run: `cd src-tauri && cargo test --test suite api_template_flows::` then `cargo test --test suite api_templates::`
  Expected: all pass; the existing template tests unchanged.

- [ ] **Step 5: Commit**
  `git add src-tauri/src/api_templates src-tauri/tests/suite/api_template_flows.rs src-tauri/tests/suite/main.rs` - "feat(v2): API template flows - the model and the checks a flow must pass"

### Task 2: Where flows are kept

**Files:**
- Create: `src-tauri/src/api_templates/flow_store.rs`; Modify: `src-tauri/src/api_templates/mod.rs` (`pub mod flow_store;`)
- Test: `src-tauri/tests/suite/api_template_flows.rs`

**Interfaces:**
- Consumes: `flow::Flow`, `flow::check_flow`; `store::templates_dir`'s project-slug rule.
- Produces (`crate::api_templates::flow_store`):
  - `pub fn flows_dir(root: &Path, org: &str, project: &str) -> PathBuf` - `<root>/flows/<same slug templates_dir uses>`.
  - `pub fn load(root, org, project, id: &str) -> Result<Option<Flow>, String>`
  - `pub fn save(root, org, project, f: &Flow) -> Result<(), String>` - writes `<id>.json`; the caller sets `saved`.
  - `pub fn list(root, org, project) -> Result<Vec<Flow>, String>` - sorted by `title`; a file that does not parse is skipped with an `applog::warn`, as `store::list` treats a template that does not read.
  - `pub fn remove(root, org, project, id: &str) -> Result<(), String>` - an invalid id is refused; a missing file is not an error.

- [ ] **Step 1: Write the failing tests:** `flows_live_beside_templates` (dir is `<root>/flows/<slug>` with the slug `templates_dir` uses for the same org/project); `save_then_load_round_trips` (including `saved`); `list_skips_a_file_that_does_not_parse` (write `bad.json` = `"{"` beside a good flow: `list` returns the good one only); `remove_deletes_the_file`; `remove_refuses_an_id_with_a_path_in_it` (`"..\\x"` -> `Err`).
- [ ] **Step 2: Run** `cargo test --test suite api_template_flows::` - Expected: FAIL, module missing.
- [ ] **Step 3: Implement `flow_store.rs`** reusing `store`'s slug function (make it `pub(crate)` if private) and its write-then-rename pattern if `store::save` has one.
- [ ] **Step 4: Run** the same command - Expected: PASS.
- [ ] **Step 5: Commit** - "feat(v2): where API template flows are kept"

### Task 3: The gate - stage checks against the database

**Files:**
- Create: `src-tauri/src/api_templates/gate.rs`; Modify: `src-tauri/src/api_templates/mod.rs` (`pub mod gate;`)
- Test: `src-tauri/tests/suite/api_template_flows.rs`

**Interfaces:**
- Consumes: Task 1 (`Flow`, `Stage`, `substitute_check`, `required_before`, `creating_stage`), `store::SavedTemplate`, `db::{Runner, Connection, sqlcmd::run_sql}`, `db::query::rows_affected`, `activity_log::{record, Kind}`.
- Produces (`crate::api_templates::gate`):
  - `pub trait StageDb { fn label(&self) -> String; fn read(&self, sql: &str) -> impl std::future::Future<Output = Result<bool, String>>; }` - `Ok(true)` when the statement returned at least one row.
  - `pub struct SqlcmdStageDb<R: Runner> { pub runner: R, pub exe: PathBuf, pub conn: Connection }` implementing `StageDb`: `run_sql`, then `rows_affected(&text)`: `Some(n)` -> `Ok(n > 0)`; `None` -> `Err("the check's answer carried no row count")`. `label()` is `server/database`, as `db::query`'s `conn_label`.
  - `pub enum StageState { Done, NotDone, CouldNotRun }`
  - `pub struct CheckFor<'a> { pub purpose: &'static str, pub template: Option<&'a str> }` - purpose is `"gate"`, `"prove"`, `"progress"` or `"save"`.
  - `pub async fn stage_state<D: StageDb>(db: &D, flow: &Flow, stage: &Stage, value: &Value, why: &CheckFor<'_>) -> StageState` - substitutes, runs with a 15 s `tokio::time::timeout`, records one `Kind::Db` activity entry `{ "verdict": "flow check", "connection", "sql", "flow", "stage", "purpose", "template", "ok", "done", "duration_ms" }`, and one `applog::info` line with no SQL: `db flow check on {label}: {flow}/{stage} {done|not done|could not run}`. The raw error goes only to the activity entry (`"error"`).
  - `pub async fn gate<D: StageDb>(db: &D, flow: &Flow, stage_id: &str, value: &Value, templates: &[SavedTemplate], template_id: &str) -> Result<(), String>` - checks every stage in `required_before` (all of them, in order); `Err` with the refusal sentence from Global Constraints naming the **earliest** missing stages (not done, every `requires` done); a `CouldNotRun` stage yields `the check for {Stage title} could not be run - see the activity folder in Settings, Logs` and takes precedence over "not done". The creating stage's `stage_id` returns `Ok(())` without a query.
  - `pub struct StageProgress { pub id: String, pub title: String, pub state: &'static str, pub optional: bool, pub templates: Vec<String> }` (Serialize) - state is `"done" | "next" | "blocked" | "skippable" | "could_not_check"`.
  - `pub async fn progress<D: StageDb>(db: &D, flow: &Flow, value: &Value, templates: &[SavedTemplate]) -> Vec<StageProgress>` - every stage checked once; `next` = not done and every `requires` done; `skippable` = optional and not done (and not blocked); `blocked` otherwise; `could_not_check` for `CouldNotRun`.
  - `pub fn templates_on(templates: &[SavedTemplate], flow: &str, stage: &str) -> Vec<&SavedTemplate>`

- [ ] **Step 1: Write the failing tests** with a `FakeDb { answers: HashMap<String /* stage id found in the sql */, Result<bool, String>>, calls: Mutex<Vec<String>> }` (match a stage by a marker string placed in each fixture check, e.g. `/*rules*/`):
  - `the_creating_stage_is_never_gated` - `gate(.., "setup", ..)` is `Ok` and `calls` is empty.
  - `every_required_stage_done_lets_it_through` - participants with setup and rules done -> `Ok`; `calls` has exactly setup and rules; competencies never queried.
  - `a_missing_stage_is_named_with_its_template` - rules not done, one saved template `pms-set-eval-rules` on it -> `Err` equal to `"Evaluation rules is not done for cycleId 274 - do it first with pms-set-eval-rules (Set the evaluation rules). Then: Participants."` when gating `publish`.
  - `a_missing_stage_with_no_template_says_so` - message contains `no template performs Evaluation rules yet: prove one first`.
  - `a_check_that_cannot_run_is_not_not_done` - rules `Err("Login failed")` -> message contains `the check for Evaluation rules could not be run` and never contains `Login failed` or `is not done`.
  - `the_diamond_requires_both` - diamond flow, `b` done, `c` not -> refused naming `c`.
  - `progress_marks_each_stage` - setup done, rules not: setup `done`, rules `next`, competencies `blocked`, participants `blocked`, publish `blocked`; with rules done too: competencies `skippable`, participants `next`.
  - `a_check_without_a_row_count_could_not_run` (Review Focus 1) - `SqlcmdStageDb` with the `FakeRunner` pattern from `suite/db_batch.rs` answering `"1\n"` with no `(1 row affected)` footer -> `read` is `Err`; answering `"1\n\n(1 row affected)\n"` -> `Ok(true)`; `"\n(0 rows affected)\n"` -> `Ok(false)`.
  - `every_check_is_in_the_activity_log_and_no_sql_in_the_app_log` - under `serial::activity_log()`, with a temp activity dir: one `db-*.jsonl` line per check carrying `"verdict":"flow check"`, the full SQL, `flow`, `stage`, `purpose`; the app log tail (the helper the existing activity-log tests use) contains `db flow check on` and no `SELECT`.
- [ ] **Step 2: Run** `cargo test --test suite api_template_flows::` - Expected: FAIL, module missing.
- [ ] **Step 3: Implement `gate.rs`** as specified.
- [ ] **Step 4: Run** the same command - Expected: PASS.
- [ ] **Step 5: Commit** - "feat(v2): the flow gate - a stage's earlier stages asked of the database"

### Task 4: The bridge - gating prove and run, saving flows, progress, the list

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (template handlers, `route`, `api_template_list`, new handlers)
- Modify: `src-tauri/src/api_templates/runner.rs` (`preflight` adds `flow::check_stage_ref`)
- Test: `src-tauri/tests/suite/ai_bridge.rs` (its API template section, near `fn on()`)

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces:
  - `pub fn real_stage_db(ctx: &BridgeContext) -> Result<gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)>` - `db_ready(ctx)`, with every one of its refusals replaced by the Global Constraints sentence `this template belongs to a flow, and flow checks need a database: choose one on the AI Bridge tab` when nothing is chosen (keep `db_ready`'s own sentences for a missing login, an unreadable store, sqlcmd not installed).
  - `api_template_prove` and `api_template_run` gain a parameter after `open`: `open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>` with `D: gate::StageDb`; `route` passes `real_stage_db`.
  - `pub async fn api_template_flow_save<D: StageDb>(ctx: &BridgeContext, body: &str, open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>) -> (u16, String)` - body `{ flow, sample, replace?, why? }`; spec §6.
  - `pub async fn api_template_flow_progress<D: StageDb>(ctx, body, open_db) -> (u16, String)` - body `{ flow: "<id>", subject: <value> }`; 200 with `{ "flow", "subject", "stages": [StageProgress] }`.
  - `route` arms: `("POST", "/api-template-flow-save")`, `("POST", "/api-template-flow-progress")`.
  - `GET /api-templates` answers `{ "templates": [ ...each row as today plus "stage" ], "flows": [ { id, title, module, subject, stages: [{ id, title, requires, optional, creates, templates: [ids] }] } ] }`.

Behaviour, in `run_api_template_request` order: `preflight` (now including `check_stage_ref` against `flow_store::load`; an unreadable flow file counts as not saved, logged) -> if the template has a `stage` and it is not the creating stage: `open_db`, then `gate::gate` with `values[subject.name]` (a 400 with the gate's sentence) -> `claim` -> run -> on a successful **prove** of a template with a `stage`: `gate::stage_state` for its own stage with the subject from `values` or, for the creating stage, from `report.outputs`; anything but `Done` -> not saved, 502 with `every step passed, but {Stage title} is still not done for {subject} {value}, so the template was not saved; {report.message()}`. `open_db` is called at most once per request and never for a template without a `stage`. A successful `api_template_flow_save` tells the tab through the same templates sink a prove or run uses (`set_templates_sink`'s callback, with the flow id), so the tab reloads.

- [ ] **Step 1: Write the failing tests** (the existing `FakeBrowsers` and `root_with_recipe_and_account` helpers; a `FakeDb` as in Task 3, moved to `suite/common.rs` as `pub struct FakeStageDb` so both modules share it):
  - `a_template_on_a_flow_is_refused_before_any_browser_opens` - flow saved, rules not done, run of a participants template -> 400 with the rules sentence; `FakeBrowsers` recorded no `open`.
  - `no_database_chosen_refuses_a_flow_template` - `open_db` returning the `real_stage_db` sentence -> 400 with it verbatim.
  - `a_template_without_a_stage_never_asks_for_a_database` (Review Focus 4) - `open_db` panics if called; the run proceeds to the browser as before.
  - `a_template_whose_flow_is_gone_is_refused` - stage points at a flow id with no file -> 400 with `this template's flow pms-performance-cycle is no longer saved`; the same with an unreadable flow file (Review Focus 3).
  - `a_prove_that_does_not_complete_its_stage_is_not_saved` - every step passes, own stage check `Ok(false)` -> 502 containing `is still not done for cycleId`; `store::load` is `None`.
  - `a_creating_prove_checks_the_captured_subject` - creating template, capture `cycleId` 274, the own check's SQL contains `274`.
  - `saving_a_flow_runs_every_check_on_the_sample` - 200; one check per stage; answer lists each stage's result; file saved with `saved.sample`.
  - `saving_a_flow_tells_the_tab` - with a test sink installed the way the existing prove test installs one, a successful save sends the flow id.
  - `saving_a_flow_with_a_failing_check_is_refused` - 400 naming the stage, nothing saved.
  - `replacing_a_flow_needs_replace_and_why_and_lists_orphans` - without `replace` -> 400; with `replace` + `why` dropping `rules` while a saved template performs it -> 200 whose answer names that template.
  - `progress_answers_each_stage` - 200 JSON with the Task 3 states.
  - `the_list_carries_flows_and_stages` - `GET /api-templates` JSON has `templates[0].stage` and `flows[0].stages[1].templates == ["pms-set-eval-rules"]`.
  - `the_new_routes_are_gated_like_the_rest` - extend the existing `PATHS` array test at the `autorun_guard_for` test to include both new paths.
  Update the existing tests that read `GET /api-templates` as an array to read `["templates"]`.
- [ ] **Step 2: Run** `cargo test --test suite ai_bridge::` - Expected: the new tests FAIL (missing handlers/params).
- [ ] **Step 3: Implement** the interfaces and behaviour above.
- [ ] **Step 4: Run** `cargo test --test suite ai_bridge::` then `cargo test --test suite api_templates_runner::` - Expected: PASS.
- [ ] **Step 5: Commit** - "feat(v2): API templates on a flow are refused until their earlier stages are done"

### Task 5: The assistant's tools and guide

**Files:**
- Modify: `src-tauri/src/mcp.rs` (two tool definitions after `run_api_template`; two dispatch lines), `src-tauri/src/ai_tools.rs` (`DEV_ONLY_TOOLS`), `src-tauri/src/api_templates/guide.rs` (Flows section in `BASE`)
- Modify: `src/lib/mcpTools.ts` (`DEV_ONLY_TOOLS`, the API templates group, its `PAIR_ROWS` summary), `src/screens/AiBridge.tsx` (the API templates breakdown `<li>` mentions flows), `docs-site/src/guard.test.ts` (term list)
- Test: `src-tauri/tests/suite/tcm_mcp.rs`, `src-tauri/tests/suite/api_templates.rs` (guide), `src/lib/mcpTools.test.ts`, `src/screens/AiBridge.test.tsx`

**Interfaces:**
- Tool `save_api_flow` -> `call("POST", "/api-template-flow-save", &args.to_string())`; inputSchema `flow` (object, required), `sample` (required; "the subject of a real record, found with db_query"), `replace` (boolean), `why` (string). Description: saves a flow after checking it and running every stage's check once on the sample; needs a database chosen on the AI Bridge tab; does not need the API templates switch.
- Tool `get_api_flow_progress` -> `call("POST", "/api-template-flow-progress", ..)`; inputSchema `flow` (string id, required), `subject` (required). Description: which stages are done for this record and which come next - call it before every run of a template on a flow, and run a `next` stage's template.
- `list_api_templates` description: now also returns the project's flows and each template's stage.
- Guide `BASE` gains `## Flows` covering: the flow format with the spec §3 example; that a template on a flow is refused until its earlier stages are done; optional = may be skipped; the order of work - **map the wizard first** (its steps, where their progress is stored, one check per stage, `save_api_flow` with a sample), **then** templates stage by stage from the creating stage, and **before every run** `get_api_flow_progress`.
- `PAIR_ROWS.get_api_template_guide.summary`: `"Map a module's stages, then build, prove and run templates that write test data through the application's own endpoints, in the order the application allows."`

- [ ] **Step 1: Write the failing tests:** in `tcm_mcp.rs` extend the locked-app assertion loop (it iterates `DEV_ONLY_TOOLS`) - it covers the new names once they are in the list; add `the_flow_tools_reach_their_routes` to the existing tool->route table test with both rows. In `api_templates.rs`: `the_guide_explains_flows` - `guide::text(&[], None)` contains `save_api_flow`, `get_api_flow_progress`, `map the wizard first` (case-insensitive) and `optional`. In `mcpTools.test.ts`: the Rust/TS `DEV_ONLY_TOOLS` sync test passes only with both names (it already reads Rust's list - confirm it fails first). In `guard.test.ts`: add both tool names to the hidden term list (the guard test then proves the built help never names them).
- [ ] **Step 2: Run** `cargo test --test suite tcm_mcp::` and `npx vitest run src/lib/mcpTools.test.ts` - Expected: FAIL.
- [ ] **Step 3: Implement** the tool definitions, dispatch, lists, guide section, row summary and breakdown text.
- [ ] **Step 4: Run** `cargo test --test suite tcm_mcp::`, `cargo test --test suite api_templates::`, `npx vitest run src/lib/mcpTools.test.ts src/screens/AiBridge.test.tsx docs-site/src/guard.test.ts` - Expected: PASS.
- [ ] **Step 5: Commit** - "feat(v2): the assistant saves flows and asks what comes next"

### Task 6: The tab's data - flows in the overview, removing a flow

**Files:**
- Modify: `src-tauri/src/commands/api_templates.rs`, `src-tauri/src/lib.rs` (register the command), `src/bindings.ts` (regenerated)
- Test: `src-tauri/tests/suite/api_templates.rs`

**Interfaces:**
- `TemplatesOverview` gains `pub flows: Vec<Flow>` (from `flow_store::list`).
- `#[tauri::command] #[specta::specta] pub fn api_templates_remove_flow(app: tauri::AppHandle, org: String, project: String, id: String) -> Result<(), String>` - `refuse_unless_offered()` first; `flow_store::remove`; emits `ApiTemplatesChanged { id }`. Match `api_templates_remove`'s parameter shape exactly (if it takes no `AppHandle` and emits elsewhere, follow it).
- Generated TS: `commands.apiTemplatesRemoveFlow(org, project, id)`, types `Flow`, `Stage`, `Subject`, `SubjectType`, `StageRef`, `FlowSaved`; `ApiTemplate.stage?: StageRef | null`.

- [ ] **Step 1: Write the failing tests:** `the_overview_carries_flows` (a saved flow appears in `api_templates_overview(..).flows`); `removing_a_flow_is_refused_where_auto_run_is_not_offered` - follow the existing locked-command test for `api_templates_remove` in this file.
- [ ] **Step 2: Run** `cargo test --test suite api_templates::` - Expected: FAIL.
- [ ] **Step 3: Implement**, then regenerate: `cd src-tauri && cargo test --test bindings`.
- [ ] **Step 4: Run** `cargo test --test suite api_templates::`, `cargo test --test bindings`, `npx tsc --noEmit` - Expected: PASS, no type errors.
- [ ] **Step 5: Commit** - "feat(v2): the API Templates tab reads flows and can remove one"

### Task 7: The flow map

**Files:**
- Create: `src/lib/flowLayout.ts`, `src/lib/flowLayout.test.ts`
- Create: `src/screens/ApiTemplates/FlowMap.tsx`, `src/screens/ApiTemplates/RemoveFlow.tsx`
- Modify: `src/screens/ApiTemplates/index.tsx`, `src/screens/ApiTemplates/TemplateRow.tsx`
- Test: `src/screens/ApiTemplates/index.test.tsx`

**Interfaces:**
- `flowLayout.ts`:
  - `export type Placed = { id: string; col: number; row: number; x: number; y: number; h: number }`
  - `export const GEOMETRY = { colW: 208, gapX: 56, gapY: 16, head: 40, perTemplate: 24, pad: 12 }` - a box is `head + perTemplate * max(1, templates) + pad` tall.
  - `export function layoutFlow(flow: Flow, templateCounts: Record<string, number>): { boxes: Placed[]; edges: Array<{ from: string; to: string }>; width: number; height: number }` - column = longest `requires` path from the creating stage; row = declaration order within the column; `x = col * (colW + gapX)`; `y` stacks boxes in a column with `gapY`; `width`/`height` bound every box.
  - `export function edgePath(from: Placed, to: Placed): string` - cubic SVG path from `from`'s right-middle to `to`'s left-middle.
- `FlowMap({ flow, templates, onOpenTemplate, onRemove }: { flow: Flow; templates: SavedTemplate[]; onOpenTemplate: (id: string) => void; onRemove: () => void })` - header (title, module, "Tracks {subject.name}", saved date via `TemplateRow`'s `stampDate`, a Remove flow button with the delete icon from `actionIcons.ts`); a `div` with `overflow-x-auto` holding a `relative` box of `width`x`height`: an `aria-hidden` `<svg>` of `edgePath`s (stroke from a token class) and absolutely positioned stage boxes (title; `Optional` label and `border-dashed` when `optional`; each template as a button - title plus the effect badge `TemplateRow` uses - calling `onOpenTemplate(id)`; `No template yet` in `text-faint` when none); then an `<ol className="sr-only">` text equivalent: `"{title}. Requires: {titles or 'nothing'}. {Optional. }Templates: {titles or 'none yet'}."` per stage.
- `RemoveFlow` - as `RemoveTemplate`, naming the flow and `"{n} templates perform its stages; they stay, and are refused until a flow with their stage is saved again."`; calls `commands.apiTemplatesRemoveFlow`.
- `TemplateRow` gains optional props `open?: boolean`, `onOpenChange?: (open: boolean) => void` (uncontrolled when absent, as today) and `stage?: { text: string; missing: boolean }` rendered as `Stage: {text}` (`text-warning` plus `(no longer saved)` when `missing`); its `<li>` gets `id={"api-template-" + t.id}`.
- `index.tsx`: per module, that module's flow maps first, then its templates; a module with a flow but no templates still shows. `openId` state drives the rows; `onOpenTemplate` sets it and `document.getElementById(...)?.scrollIntoView({ block: "nearest" })`. Search also matches stage titles and flow titles; a flow shows while searching if its title, module, a stage title or one of its templates matches.

- [ ] **Step 1: Write the failing tests.** `flowLayout.test.ts`: `columns_follow_the_longest_path` (spec example: setup 0, rules 1, competencies 2, participants 2, publish 3); `the_diamond_sits_after_both` (Review Focus 5: `d` in column 2, edges `a->b, a->c, b->d, c->d`); `boxes_in_a_column_do_not_overlap` (`y + h + gapY <= next.y`); `a_box_grows_with_its_templates`; `edgePath_starts_right_and_ends_left` (path starts at `from.x + colW` and ends at `to.x`). `index.test.tsx` (mock `apiTemplatesOverview` with the spec flow and two templates, one on `rules`, one with a `stage` pointing at a removed stage): the map's accessible list names every stage in order with its requires; `Optional` appears once; `No template yet` appears for competencies, participants and publish; clicking the `rules` template button opens that row (`aria-expanded="true"` on its toggle); the orphaned template row shows `no longer saved`; `Remove flow` opens the confirmation and confirming calls `apiTemplatesRemoveFlow("acme","Web","pms-performance-cycle")`; searching `evaluation` keeps the flow and the rules template; the map container has `overflow-x-auto`.
- [ ] **Step 2: Run** `npx vitest run src/lib/flowLayout.test.ts src/screens/ApiTemplates` - Expected: FAIL.
- [ ] **Step 3: Implement** the components and layout.
- [ ] **Step 4: Run** `npx vitest run src/lib/flowLayout.test.ts src/screens/ApiTemplates src/ui-consistency.test.ts` and `npx tsc --noEmit` - Expected: PASS.
- [ ] **Step 5: Commit** - "feat(v2): the API Templates tab draws each flow as a map"

### Task 8: Whole-branch gate

- [ ] **Step 1:** `cd src-tauri && cargo test --tests` - Expected: every suite passes.
- [ ] **Step 2:** `npm test` - Expected: every file passes (App.test.tsx's documented flake aside: re-run once).
- [ ] **Step 3:** `npx tsc --noEmit` and `npm run build` - Expected: clean.
- [ ] **Step 4:** `npm run docs:build` is NOT needed (no documented screen changed); confirm `docs-site/src/guard.test.ts` passed in Step 2.
- [ ] **Step 5:** Commit any fix-ups; leave the branch for the owner's "merge".
