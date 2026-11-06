# Shared API Library Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One library of API templates, flows and fixtures per org/project that every Auto Run environment sees, with each item's confirmation tracked separately per environment, and environment copy / backup import that never carry API definitions or accounts.

**Architecture:** Definitions move from `<env root>/{templates,fixtures,flows}/<slug>/` to `<shared>/api-library/{templates,fixtures,flows}/<slug>/`; run histories stay where they are under each environment root. Each environment keeps `<env root>/api-state/<slug>.json`, which records per item the content hash it confirmed or failed, so replacing a definition invalidates every other environment's state by hash mismatch without writing into their folders. A one-time, crash-safe, idempotent fold moves the old per-environment definitions into the library at start-up (and after a backup import).

**Tech Stack:** Rust (Tauri 2, serde, sha2, tauri-specta), React 19 + TypeScript, vitest + Testing Library, the docs-site help guide.

**Spec:** `docs/superpowers/specs/2026-11-06-shared-api-library-design.md`

## Global Constraints

- No HTTP DELETE to Azure DevOps anywhere (this work touches none).
- Every Rust test is an integration test in `src-tauri/tests/suite/<module>.rs` with a `mod <module>;` line in `src-tauri/tests/suite/main.rs` (alphabetical order there). Never a `#[cfg(test)]` module, never a new file directly under `tests/`.
- A test that touches process-wide state (the active environment, the template run slot, the log tail) takes its lock from `src-tauri/tests/suite/serial.rs`, as the neighbouring tests of the same area do.
- `src/bindings.ts` is regenerated with `cd src-tauri && cargo test --test bindings` after any change to a `specta::Type`; never hand-edited.
- User-facing text (UI, Blocked sentences, assistant answers, log lines shown to the person) names no path or URL. Errors from the file system are logged, never shown.
- Colours only from theme tokens (`text-warning`, `bg-danger/15`, …); `src/ui-consistency.test.ts` is never weakened.
- Caching: no new cache. The webview keeps using the existing queries (`apiTemplatesOverview`, `apiFixturesList`).
- Writes into an environment root go through `roots::write_file` / `roots::create_dir_all` / `roots::remove_file` (the switch guard). The fold holds `roots::switch_lock()` for writing and never calls a guarded write while holding it.
- Mark words, exactly: **Unconfirmed here**, **Failed here**; a confirmed item shows no new mark. The old "unproven" wording becomes "unconfirmed here".
- Blocked sentence for an unconfirmed setup fixture that fails contains, exactly: `this fixture has not worked on this environment yet`.
- Kept losers: `<shared>/removed-api/<ms>/`, purged after 30 days (`roots::REMOVED_KEPT_DAYS`), never a Copy Auto Run data source, never an environment.
- The changelog (`src/lib/changelog.ts`) is NOT edited by this plan; the release adds it.
- One build or test command at a time on this machine. One build cache: `src-tauri/target` (never `CARGO_TARGET_DIR`).
- Commits: Bash heredoc `git commit -F - <<'EOF' … EOF`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Never stage `.test-cases/`.

## Rulings on what the spec leaves open (binding for every task)

1. **Library folder is `api-library`, not `templates/`.** The flat-layout originals the 2026 env migration left in `<shared>/templates|fixtures|flows` are "kept as a backup and no longer read"; putting the library at the spec's example path would make that stale backup live again.
2. **Invalidation is by content hash, lazily.** A state entry holds the hash of the version it is about. Reading it against a different current definition yields Unconfirmed. This is how "replacing clears the other environments' state" and "import of a changed item is unconfirmed everywhere" are met without writing into other environments' folders (which `roots::guard` refuses). Identical content keeps its state automatically.
3. **Hashes.** SHA-256 hex of `serde_json::to_string` of the definition with app-written fields blanked: template with `proven: None`; flow with `saved: None`; fixture as is.
4. **`ApiTemplate::proven` stays** as "the last proof of this version, on some environment" — it is the global save/run gate (`fixture::validate`, `fixture_run::start`). A file written before this change loads unchanged.
5. **What confirms:** a successful prove that saves, an explore-session save, a successful **run** of the saved version (the spec's intent: "if it works, it becomes confirmed"), and a successful fixture build (which also confirms each template it ran). A fixture **save** confirms nothing; a changed fixture is unconfirmed everywhere, the saving environment included, until it is built.
6. **The retry rule (a confirmed item that fails).** A confirmed item keeps a `failure` strike. A failure flips it to Failed here only when (a) the entry is still valid for the current hash, (b) it already carries a strike, and (c) the new failure comes from the assistant (`TrySource::Assistant`: `run_api_template`, `prove_api_template` of identical content, an explore-session run of the saved template, `run_api_fixture`). A plain run (Auto Run setup, the person's Run/Rebuild) only records the strike. Any success clears the strike. So "a fresh try of the current version" = the assistant's try after a recorded failure of that same content on that environment.
7. **Failures that record nothing:** a run stopped by the person (`fixture_run::RUN_STOPPED`), a refusal before it ran (busy slot, no such fixture/template, active environment unreadable), a failed prove of a draft that differs from the saved template, Clean up runs.
8. **Flows** are Confirmed when at least one template performs one of their stages and every such template is Confirmed here; otherwise Unconfirmed. A flow is never Failed here.
9. **Blocked sentence choice:** the fixture's state read **before** the setup runs. Confirmed → today's `setup_failed(why)`. Unconfirmed or Failed here → `setup_not_worked_here(why)`. A `ready_to_run` refusal inside a setup counts as a failure of the fixture (it is how a missing account shows up) and uses the same choice.
10. **`PER_ENV_ITEMS`**: `flows` is dropped; `templates` and `fixtures` stay but as `CopyTo::ActiveOnly` (they now hold only run histories, which copies leave behind and keep in place); `api-state` is added as `ActiveOnly`. The env migration's `fan_out` still carries a never-migrated flat folder's `flows` (`roots::LEGACY_API_ITEMS`).
11. **The fold** runs only where the env migration may run (`roots::migration_allowed`: release builds, or a dev build with `TCM_MIGRATE_ENVS=1`), because a dev build shares the installed app's data folder and the installed older build would lose its templates.
12. **Removed environment folders** in the fold give only ids no live environment and no library file holds; their definition files are deleted after the fold either way. They never confirm anything.
13. **Unreadable definition** found by the fold: kept aside in `removed-api/<ms>/`, logged without a path, never fatal.
14. **Import of an identical template** (same hash as the saved one) writes nothing (so its `proven` and every confirmation stay) and is reported in a new `unchanged` list.
15. **Removing** a template/fixture/flow removes the shared definition (for every environment), this environment's history and this environment's state entry. Other environments' stale entries are harmless (they never match a missing definition).
16. **Readiness panel:** fixtures Failed here are counted separately and raise Needs attention with a pointer to the assistant; "failed on its last run" counts only fixtures that are not Failed here; Unconfirmed alone raises nothing.

## Review Focus

1. A template replaced from environment B must leave a fixture that used it Unconfirmed on A, and still Confirmed on B only if it was Confirmed there against the version just replaced — Task 2 `a_template_replaced_elsewhere_unconfirms_fixtures_here_but_not_on_the_proving_environment`.
2. Re-proving a template on A after B replaced it must not bring back A's old fixture confirmation (stale `uses`) — Task 2 `reproving_after_a_replace_elsewhere_does_not_revive_a_stale_fixture_confirmation`.
3. A run the person stopped, or one refused before it ran, must leave no mark — Task 4 `a_stopped_fixture_run_records_nothing_here` and `a_busy_or_refused_run_records_nothing_here`.
4. The assistant and an Auto Run setup recording state for the same project at once must not lose either write — Task 1 `concurrent_records_for_one_project_both_land`.
5. The fold meeting a definition that does not parse, or being cut short and run again, must lose nothing and finish — Task 3 `an_unreadable_definition_is_kept_aside_and_the_move_finishes` and `a_move_cut_short_after_its_mark_is_finished_by_the_next_start`.

---

## File Structure

New:
- `src-tauri/src/api_templates/library.rs` — where definitions, histories and state live (`ApiKind`, `library_dir`, `history_dir`, `state_path`, folder constants) and the content hashes.
- `src-tauri/src/api_templates/here.rs` — the per-environment state model: stored shape, `Here` as listings show it, reads, and every transition (confirm, failure, forget, carry fixture uses after a template save).
- `src-tauri/src/api_templates/library_move.rs` — the one-time fold and its crash recovery.
- `src-tauri/tests/suite/api_library.rs` — tests for `library.rs`, `here.rs` and the stores' new layout.
- `src-tauri/tests/suite/api_library_move.rs` — tests for the fold.
- `src/screens/ApiTemplates/HereBadge.tsx` — the one mark component every screen uses.

Modified (Rust): `api_templates/{mod.rs,store.rs,fixture_store.rs,flow_store.rs,fixture_run.rs,share.rs,guide.rs,flow_page.rs}`, `autorun/{roots.rs,env_copy.rs,backup_import.rs,setup.rs,guide.rs}`, `backup.rs`, `applog.rs`, `ai_bridge.rs` (bounded regions named per task), `mcp.rs`, `commands/api_templates.rs`.

Modified (TS): `src/screens/ApiTemplates/{TemplateRow.tsx,FlowMap.tsx,FixturesTab.tsx,index.tsx,ImportTemplates.tsx,RemoveTemplate.tsx,RemoveFixture.tsx,RemoveFlow.tsx}`, `src/screens/AutoRun/SetupPanel.tsx`, `src/components/CopyAutoRunDialog.tsx`, `src/dev/demo.ts`, their tests.

Modified (docs): `docs-site/src/content/{api-templates.ts,auto-run.ts,ai-tools.ts}`, the shots and built site.

---

### Task 1: Library paths, content hashes and the per-environment state model

**Files:**
- Create: `src-tauri/src/api_templates/library.rs`
- Create: `src-tauri/src/api_templates/here.rs`
- Modify: `src-tauri/src/api_templates/mod.rs` (add `pub mod library; pub mod here;`)
- Test: `src-tauri/tests/suite/api_library.rs` (+ `mod api_library;` in `suite/main.rs`)

**Interfaces:**
- Consumes: `autorun::roots::{shared_of, write_file, create_dir_all}`, `autorun::recipe::project_slug`, `api_templates::{ApiTemplate, fixture::Fixture, flow::Flow, store::SavedTemplate}`.
- Produces (`library.rs`):
  - `pub const LIBRARY_DIR: &str = "api-library"; pub const STATE_DIR: &str = "api-state"; pub const REMOVED_API_DIR: &str = "removed-api";`
  - `#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum ApiKind { Templates, Fixtures, Flows }` with `pub fn folder(self) -> &'static str` (`"templates"`, `"fixtures"`, `"flows"`).
  - `pub fn library_dir(root: &Path, kind: ApiKind, org: &str, project: &str) -> PathBuf` → `shared_of(root)/api-library/<folder>/<slug>`.
  - `pub fn history_dir(root: &Path, kind: ApiKind, org: &str, project: &str) -> PathBuf` → `root/<folder>/<slug>` (the env root; templates and fixtures only).
  - `pub fn state_path(root: &Path, org: &str, project: &str) -> PathBuf` → `root/api-state/<slug>.json`.
  - `pub fn template_hash(t: &ApiTemplate) -> String`, `pub fn fixture_hash(f: &Fixture) -> String`, `pub fn flow_hash(f: &Flow) -> String` (Ruling 3).
- Produces (`here.rs`):
  - `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)] pub struct HereFailure { pub at: String, pub step: Option<String>, pub detail: Option<String> }`
  - `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)] #[serde(tag = "state", rename_all = "snake_case")] pub enum Here { Confirmed { last_failure: Option<HereFailure> }, Unconfirmed, Failed { failure: HereFailure } }`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum TrySource { Assistant, Plain }`
  - Stored: `#[derive(Default, Serialize, Deserialize)] pub struct EnvApiState { #[serde(default)] pub templates: BTreeMap<String, ItemState>, #[serde(default)] pub fixtures: BTreeMap<String, ItemState> }` and `#[derive(Clone, Serialize, Deserialize)] pub struct ItemState { pub hash: String, #[serde(default)] pub uses: BTreeMap<String, String>, #[serde(default)] pub confirmed_at: Option<String>, #[serde(default)] pub failure: Option<HereFailure>, #[serde(default)] pub failed_here: bool }`
  - `pub fn load_state(root: &Path, org: &str, project: &str) -> EnvApiState` (missing → default; unreadable → default + `applog::warn` without a path)
  - `pub fn template_here(state: &EnvApiState, t: &ApiTemplate) -> Here`
  - `pub fn fixture_here(state: &EnvApiState, f: &Fixture, templates: &dyn Fn(&str) -> Option<ApiTemplate>) -> Here`
  - `pub fn flow_here(flow: &Flow, templates: &[SavedTemplate]) -> Here` (Ruling 8; uses `gate::templates_on` and each `SavedTemplate::here` added in Task 2 — until then implement against a `here_of: &dyn Fn(&ApiTemplate) -> Here` parameter: `pub fn flow_here(flow: &Flow, templates: &[&ApiTemplate], here_of: &dyn Fn(&ApiTemplate) -> Here) -> Here`)
  - `pub fn confirm_template(root: &Path, org: &str, project: &str, t: &ApiTemplate) -> Result<(), String>`
  - `pub fn confirm_fixture(root: &Path, org: &str, project: &str, f: &Fixture, ran: &[ApiTemplate]) -> Result<(), String>` (confirms the fixture with `uses` = each step template's hash from `ran`, and confirms each template in `ran`)
  - `pub fn record_template_failure(root: &Path, org: &str, project: &str, t: &ApiTemplate, failure: HereFailure, source: TrySource) -> Result<(), String>`
  - `pub fn record_fixture_failure(root: &Path, org: &str, project: &str, f: &Fixture, templates: &[ApiTemplate], failure: HereFailure, source: TrySource) -> Result<(), String>`
  - `pub fn after_template_saved(root: &Path, org: &str, project: &str, before: Option<&ApiTemplate>, saved: &ApiTemplate) -> Result<(), String>` — when `saved.proven.is_some()`: confirm `saved`; then, for every fixture entry here whose `uses[saved.id] == template_hash(before)`, set it to `template_hash(saved)`. Does nothing when `saved.proven` is `None`.
  - `pub fn forget(root: &Path, org: &str, project: &str, kind: ApiKind, id: &str) -> Result<(), String>`
  - All load-modify-save transitions hold one module mutex (`state_lock()`, the pattern of `store::runs_lock`) and write through `roots::create_dir_all` + `roots::write_file`.

Derivation (`template_here` / `fixture_here`), exact:
- no entry, or `entry.hash` ≠ current hash → `Unconfirmed`;
- fixture only: any step template whose current hash ≠ `entry.uses[id]` (or missing from `uses`, or the template is gone) → `Unconfirmed`;
- `entry.failed_here` → `Failed { failure }`;
- `entry.confirmed_at.is_some()` → `Confirmed { last_failure: entry.failure }`;
- otherwise `Unconfirmed`.

Failure transition (both kinds), exact, against the derived state before the write:
- `Unconfirmed` → `{ hash, uses, confirmed_at: None, failure: Some(new), failed_here: true }`
- `Failed` → keep, `failure = Some(new)`
- `Confirmed { last_failure: None }` → keep confirmed, `failure = Some(new)` (a strike)
- `Confirmed { last_failure: Some(_) }` and `Plain` → keep confirmed, `failure = Some(new)`
- `Confirmed { last_failure: Some(_) }` and `Assistant` → `confirmed_at: None, failed_here: true, failure: Some(new)`
Confirm transition: `{ hash, uses, confirmed_at: Some(applog::stamp()), failure: None, failed_here: false }`.

- [ ] **Step 1: Write the failing tests** in `src-tauri/tests/suite/api_library.rs` (tempdir as a flat root; build templates with the helper `api_templates_runner.rs` exposes, or a minimal JSON template as `api_fixtures.rs::template` does):

```rust
#[test] fn library_paths_sit_under_the_shared_root_and_histories_under_the_environment() {
    // root = <t>/envs/qa
    assert_eq!(library_dir(&root, ApiKind::Templates, ORG, PROJECT), t.join("api-library").join("templates").join(slug));
    assert_eq!(history_dir(&root, ApiKind::Fixtures, ORG, PROJECT), root.join("fixtures").join(slug));
    assert_eq!(state_path(&root, ORG, PROJECT), root.join("api-state").join(format!("{slug}.json")));
}
#[test] fn a_template_hash_ignores_its_proof() { assert_eq!(template_hash(&with_proof), template_hash(&without_proof)); assert_ne!(template_hash(&a), template_hash(&a_with_other_path)); }
#[test] fn a_flow_hash_ignores_its_sample() { /* saved Some vs None → equal */ }
#[test] fn nothing_recorded_reads_unconfirmed() { assert_eq!(template_here(&load_state(&root, ORG, PROJECT), &t), Here::Unconfirmed); }
#[test] fn a_confirmed_template_reads_confirmed_until_its_content_changes() {
    confirm_template(&root, ORG, PROJECT, &t).unwrap();
    assert_eq!(template_here(&load_state(&root, ORG, PROJECT), &t), Here::Confirmed { last_failure: None });
    assert_eq!(template_here(&load_state(&root, ORG, PROJECT), &t_changed), Here::Unconfirmed);
}
#[test] fn an_unconfirmed_item_that_fails_is_failed_here_with_its_step_and_detail() {
    record_template_failure(&root, ORG, PROJECT, &t, fail("2", "HTTP 500"), TrySource::Plain).unwrap();
    assert!(matches!(template_here(&s, &t), Here::Failed { failure } if failure.step.as_deref() == Some("2") && failure.detail.as_deref() == Some("HTTP 500")));
}
#[test] fn a_confirmed_item_stays_confirmed_after_one_failure() { /* confirm, fail(Assistant) → Confirmed { last_failure: Some(..) } */ }
#[test] fn a_plain_run_never_flips_a_confirmed_item() { /* confirm, fail(Plain), fail(Plain), fail(Plain) → still Confirmed */ }
#[test] fn the_assistants_retry_after_a_failure_flips_it_to_failed_here() { /* confirm, fail(Plain), fail(Assistant) → Failed */ }
#[test] fn a_success_clears_the_strike() { /* confirm, fail(Assistant), confirm, fail(Assistant) → Confirmed with last_failure Some (not Failed) */ }
#[test] fn a_fixture_reads_unconfirmed_when_a_template_it_uses_changed() {
    confirm_fixture(&root, ORG, PROJECT, &f, &[t1.clone(), t2.clone()]).unwrap();
    assert_eq!(fixture_here(&s, &f, &lookup(&[t1.clone(), t2_changed])), Here::Unconfirmed);
}
#[test] fn a_successful_build_also_confirms_its_templates() { /* after confirm_fixture: template_here(t1) and (t2) Confirmed */ }
#[test] fn a_failed_build_marks_only_the_fixture() { /* record_fixture_failure on unconfirmed fixture: fixture Failed, t1/t2 still Unconfirmed */ }
#[test] fn a_flow_is_confirmed_only_when_every_template_on_it_is() { /* none on it → Unconfirmed; one confirmed one not → Unconfirmed; all → Confirmed { last_failure: None }; a Failed template → Unconfirmed, never Failed */ }
#[test] fn an_unreadable_state_file_reads_as_nothing_recorded() { /* write "{" to state_path; load_state → default; template_here → Unconfirmed */ }
#[test] fn concurrent_records_for_one_project_both_land() {
    // 8 threads: 4 confirm_template(ta_i), 4 record_fixture_failure(fx_i, Plain); then every id is in the state file.
}
#[test] fn forget_drops_only_that_entry() { /* confirm t1, t2; forget Templates t1 → t1 Unconfirmed, t2 Confirmed */ }
#[test] fn the_state_file_names_no_definition() { /* serialised state holds hashes, no "steps"/"path" keys */ }
```

- [ ] **Step 2: Run them to see them fail**
Run: `cd src-tauri && cargo test --test suite api_library::`
Expected: compile errors — `library`/`here` modules do not exist.

- [ ] **Step 3: Implement `library.rs` and `here.rs`** with the signatures above. Hash with `sha2::Sha256` over `serde_json::to_string` (as `autorun/components.rs` does). The flow `here_of` parameter form stays until Task 2 swaps it to `&[SavedTemplate]`.

- [ ] **Step 4: Run the tests**
Run: `cd src-tauri && cargo test --test suite api_library::`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add src-tauri/src/api_templates/library.rs src-tauri/src/api_templates/here.rs src-tauri/src/api_templates/mod.rs src-tauri/tests/suite/api_library.rs src-tauri/tests/suite/main.rs
git commit -F - <<'EOF'
feat(v2): per-environment confirmation state for the API library

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 2: The stores read and write the shared library; listings carry `here`

**Files:**
- Modify: `src-tauri/src/api_templates/store.rs` (paths, `SavedTemplate`, `save`, `list`, `remove`)
- Modify: `src-tauri/src/api_templates/fixture_store.rs` (paths, `SavedFixture`, `list`, `remove`)
- Modify: `src-tauri/src/api_templates/flow_store.rs:19-21` (`flows_dir`)
- Modify: `src-tauri/src/api_templates/here.rs` (`flow_here` takes `&[SavedTemplate]`)
- Modify: `src-tauri/src/commands/api_templates.rs:33-65` (`TemplatesOverview.flow_here`)
- Modify: `src/dev/demo.ts` and any TS test fixture that builds a `SavedTemplate`/`SavedFixture`/`TemplatesOverview` object, adding `here`/`flow_here` so `tsc` stays green (no UI change yet)
- Regenerate: `src/bindings.ts`
- Test: `src-tauri/tests/suite/api_library.rs`; adjust `api_templates.rs:578,613,636`, `api_template_flows.rs:385-387`, `api_templates_runner.rs:1075`, `autorun_env_roots.rs:175-177,357-…`

**Interfaces:**
- Consumes: Task 1 `library::*`, `here::*`.
- Produces:
  - `store::templates_dir(root, org, project) -> PathBuf` now = `library_dir(root, Templates, …)` (definitions). New `store::template_runs_dir(root, org, project) -> PathBuf` = `history_dir(root, Templates, …)`. `runs_path` uses it.
  - `pub struct SavedTemplate { pub template: ApiTemplate, pub runs: Vec<RunRecord>, pub here: here::Here }`
  - `store::save(root, org, project, t) -> Result<(), String>`: reads the saved copy first (`before`), writes the definition into the library, then `here::after_template_saved(root, org, project, before.as_ref(), t)`; a state write that fails is logged (no path) and does not fail the save (Ruling: the safe direction is "Unconfirmed").
  - `store::remove` also calls `here::forget(.., ApiKind::Templates, id)`.
  - `fixture_store::fixtures_dir` = library; new `fixture_store::fixture_runs_dir`; `pub struct SavedFixture { pub fixture: Fixture, pub runs: Vec<FixtureRun>, pub here: here::Here }`; `remove` forgets.
  - `flow_store::flows_dir` = `library_dir(root, Flows, …)`.
  - `here::flow_here(flow: &Flow, templates: &[SavedTemplate]) -> Here`.
  - `TemplatesOverview { origin, templates, flows, flow_here: BTreeMap<String, Here> }`.

- [ ] **Step 1: Write the failing tests** (add to `api_library.rs`; three environments under one shared tempdir, each made active in turn the way `autorun_env_roots.rs` does it, under that file's serial lock):

```rust
#[test] fn a_template_saved_in_one_environment_is_listed_in_every_environment() { /* save in qa (proven); store::list(staging_root) has it */ }
#[test] fn a_proven_save_confirms_only_the_saving_environment() {
    // list(qa)[0].here == Confirmed{None}; list(staging)[0].here == Unconfirmed
}
#[test] fn run_histories_stay_per_environment() { /* append_run in qa; list(staging)[0].runs.is_empty(); file under qa/templates/<slug>/<id>.runs.json */ }
#[test] fn a_template_file_written_before_the_library_still_loads() { /* write the JSON shape of today's file (with proven) into the library dir; store::load → Some, proven kept */ }
#[test] fn a_template_replaced_elsewhere_unconfirms_fixtures_here_but_not_on_the_proving_environment() {
    // T v1 proven in A and B; fixture F built (confirm_fixture) in A and in B.
    // B saves T v2 (proven). list fixtures: A → Unconfirmed; B → Confirmed; T in A → Unconfirmed, in B → Confirmed.
}
#[test] fn reproving_after_a_replace_elsewhere_does_not_revive_a_stale_fixture_confirmation() {
    // continue: A saves T v3 (proven). Fixture in A stays Unconfirmed (its uses held v1, the replaced copy was v2).
}
#[test] fn an_import_style_save_without_proof_confirms_nothing() { /* store::save with proven None → here Unconfirmed in the saving env */ }
#[test] fn removing_a_template_removes_it_for_every_environment_and_forgets_it_here() { /* remove in qa; staging list empty; qa state has no entry */ }
#[test] fn the_overview_reports_each_flows_state_here() { /* overview_at(root).flow_here[flow.id] == Unconfirmed, then Confirmed after confirming its only template */ }
```

Adjust existing tests: `api_templates.rs:613,636` read the history through `store::template_runs_dir`; `api_template_flows.rs:387` asserts `dir == root.join("api-library").join("flows").join(slug)`; `autorun_env_roots.rs:175-177` asserts the three definition dirs are under `<shared>/api-library` (rename the helper call accordingly); rename `cleanup_preview_in_another_environment_uses_its_templates` to `cleanup_preview_in_another_environment_uses_the_shared_templates` and assert the delete template saved from Staging is found from either environment.

- [ ] **Step 2: Run to see them fail**
Run: `cd src-tauri && cargo test --test suite api_library::`
Expected: FAIL (definitions still per environment; `here` field missing).

- [ ] **Step 3: Implement** the Produces list. `store::list` / `fixture_store::list` read the state once (`here::load_state`) and compute each item's `here`; `fixture_store::list` looks templates up from one `store::list` read. `overview_at` fills `flow_here` with `here::flow_here` for each flow.

- [ ] **Step 4: Run the touched modules**
Run, one at a time: `cd src-tauri && cargo test --test suite api_library::`, then `api_templates::`, `api_template_flows::`, `api_fixtures::`, `api_templates_runner::`, `api_templates_share::`, `explore_send::`, `autorun_env_roots::`, `autorun_cleanup::`
Expected: PASS.

- [ ] **Step 5: Regenerate bindings and keep TypeScript green**
Run: `cd src-tauri && cargo test --test bindings`, then from the root `npx tsc --noEmit`
Expected: bindings rewritten with `Here`, `HereFailure`, `SavedTemplate.here`, `SavedFixture.here`, `TemplatesOverview.flow_here`; `tsc` passes after `demo.ts` and TS test fixtures get `here: { state: "confirmed", last_failure: null }` (use `{ state: "unconfirmed" }` for the demo template that today has `proven: null`) and `flow_here: {}`.
Run: `npx vitest run src/screens/ApiTemplates src/screens/AutoRun/SetupFixtures.test.tsx`
Expected: PASS (UI unchanged).

- [ ] **Step 6: Commit**
```bash
git add src-tauri/src/api_templates src-tauri/src/commands/api_templates.rs src-tauri/tests/suite src/bindings.ts src/dev/demo.ts src/screens
git commit -F - <<'EOF'
feat(v2): API templates, flows and fixtures live in one shared library

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 3: The one-time fold into the library

**Files:**
- Create: `src-tauri/src/api_templates/library_move.rs` (+ `pub mod library_move;` in `api_templates/mod.rs`)
- Modify: `src-tauri/src/autorun/roots.rs` — `SHARED_ITEMS` (add `"api-library"`, `"removed-api"`; array length 12), `start_up` (call the fold after `retry_seed`, only when `allowed`), `purge_removed` (also `removed-api/<ms>` older than 30 days), `is_on_screen` (a change under `removed-api` is not on screen)
- Modify: `src-tauri/src/applog.rs` — `pub fn stamp_of(t: SystemTime) -> String` (UTC, `stamp()`'s format)
- Test: `src-tauri/tests/suite/api_library_move.rs` (+ `mod api_library_move;`)

**Interfaces:**
- Consumes: Task 1/2 (`library::*`, `here::{EnvApiState, ItemState}`, hashes), `roots::{env_roots, REMOVED_DIR, switch_lock, ENVS_DIR, SEED_ID}`, `environments::read_names` (env names for `proven.environment`).
- Produces:
  - `#[derive(Debug, Default, PartialEq)] pub struct MoveReport { pub moved: usize, pub conflicts: usize, pub kept_aside: usize }`
  - `pub fn fold_in(shared: &Path, now_ms: u64) -> Result<Option<MoveReport>, String>` — takes `switch_lock().write()`; `Ok(None)` when no environment or removed folder holds a definition file and no staging folder is left.
  - `pub fn fold_in_held(held: &std::sync::RwLockWriteGuard<'static, ()>, shared: &Path, now_ms: u64) -> Result<Option<MoveReport>, String>` — for a caller that already holds the lock (backup import, Task 6).
  - `pub const STAGING_PREFIX: &str = ".api-library-staging-";`
  - `pub fn move_log_line(r: &MoveReport) -> String` — names no path, e.g. `"Auto Run: API templates, flows and fixtures are now shared by every environment (12 moved, 1 kept aside for 30 days)"`.

The fold, exact:
1. **Candidates.** For each kind and slug: every `<env>/<kind>/<slug>/<id>.json` (not `*.runs.json`) in each `env_roots(shared)` folder (the seed included), every such file in each `removed-envs/<key>/` folder, and the current library file. Unparseable → kept aside (Ruling 13).
2. **Recency** (string-compared `applog::stamp` values): template — the newest `ok` history line with mode `prove` or `explore` in that environment's `<id>.runs.json`, else `proven.at`, else `stamp_of(file mtime)`; fixture — newest `ok` run in that environment's fixture history, else mtime; flow — `saved.at`, else mtime; library file — `proven.at` (templates) else mtime. Ties: the library file, then the lower environment id.
3. **Winner** per id: newest among live environments and the library; removed-folder candidates only when neither has the id (Ruling 12). Among identical-content candidates, write the one with the newest `proven` (templates).
4. **Confirmations** (templates): environment E whose own copy hashes equal to the winner, has a `proven`, and either `proven.environment == E's name` or E's history has an `ok` line with `at >= proven.at`. Fixtures: E's copy hashes equal to the winner, E's newest fixture run is `ok`, and for every step template E's own copy of it hashes equal to the library winner; `uses` = those hashes. Merged into E's existing `api-state` (the fold's entries win for the ids it moved).
5. **Losers**: every other candidate → `removed-api/<ms>/<env folder or removed key or "library">/<kind>/<slug>/<id>.json`.
6. **Crash safety**: build `<shared>/.api-library-staging-<ms>/` holding `library/` (a copy of `api-library` plus the winners), `state/<env folder>/<slug>.json`, `removed/`, and `sources.json` (every definition file to delete); then write `<staging>.complete`. Finish: (a) if `staging/library` exists: rename `api-library` aside into `staging/library-old` (when present), rename `staging/library` to `api-library`; (b) write each state file into its environment (skip a folder that is gone) and delete it from staging; (c) rename `staging/removed` to `removed-api/<ms>`; (d) delete every listed source (missing is fine) and remove a now-empty `flows/` folder; (e) remove staging and its mark. A start that finds a marked staging folder runs only Finish; an unmarked one is deleted and the fold rebuilds. Renames use `ai_tools::rename_retrying_waits` with `roots::MIGRATION_RENAME_WAITS_MS`.

- [ ] **Step 1: Write the failing tests** in `api_library_move.rs` (fixtures written as files under `<t>/envs/<id>/…`, environments listed with `environments.json` as `autorun_env_migration.rs` does):

```rust
#[test] fn an_id_held_by_one_environment_moves_as_is_and_is_confirmed_where_it_was_proven() {
    // qa holds T (proven.environment "QA", history ok prove). fold_in → library has T; qa state Confirmed; staging Unconfirmed;
    // qa/templates/<slug>/T.json gone, qa/templates/<slug>/T.runs.json kept.
}
#[test] fn identical_copies_merge_and_keep_each_environments_confirmation() { /* qa and staging identical, both proven there → both Confirmed, report.conflicts == 0 */ }
#[test] fn the_newest_proven_version_wins_and_only_matching_environments_stay_confirmed() {
    // qa T(v1, proved 2026-10-01), staging T(v2, proved 2026-11-01). library == v2; staging Confirmed; qa Unconfirmed;
    // removed-api/<ms>/qa/templates/<slug>/T.json holds v1; report.conflicts == 1, kept_aside == 1.
}
#[test] fn a_fixture_is_confirmed_only_where_it_built_with_the_winning_templates() { /* fixture identical in qa/staging, both last run ok; T differs and staging wins → fixture Confirmed in staging, Unconfirmed in qa */ }
#[test] fn without_history_the_file_time_decides() { /* no runs files; newer mtime wins */ }
#[test] fn a_removed_folder_only_fills_ids_no_environment_holds_and_is_emptied() { /* removed-envs/x-1 holds T and U; qa holds T → library T from qa, U from removed; removed folder has no definition files left; no state written for it */ }
#[test] fn the_move_is_idempotent() { /* fold_in twice → second Ok(None), library unchanged */ }
#[test] fn an_unreadable_definition_is_kept_aside_and_the_move_finishes() { /* qa/templates/<slug>/bad.json = "{" → in removed-api, library lacks it, others moved */ }
#[test] fn a_move_cut_short_after_its_mark_is_finished_by_the_next_start() { /* build a marked staging folder by hand (library/, state/, sources.json), api-library renamed aside; fold_in → library in place, state written, sources gone, no staging left */ }
#[test] fn an_unmarked_staging_folder_is_discarded_and_rebuilt() {}
#[test] fn kept_losers_are_purged_after_30_days_and_never_offered_as_a_copy_source() {
    // removed-api/<old ms> purged by roots::purge_removed(now); env_copy::copy_choices(...).sources has no entry for it
}
#[test] fn start_up_folds_only_where_the_env_migration_may_run() { /* roots::start_up(shared, false) leaves env definitions in place; start_up(shared, true) folds */ }
#[test] fn the_move_log_line_names_no_path() { assert!(roots::path_free(&move_log_line(&r))); }
```

- [ ] **Step 2: Run to see them fail**
Run: `cd src-tauri && cargo test --test suite api_library_move::`
Expected: compile errors — `library_move` missing.

- [ ] **Step 3: Implement** `library_move.rs`, `applog::stamp_of`, and the `roots.rs` changes. `start_up` logs `move_log_line` (info) on `Ok(Some(_))` and a path-free warning on `Err`; a failed fold never fails start-up (the next start retries).

- [ ] **Step 4: Run**
Run, one at a time: `cd src-tauri && cargo test --test suite api_library_move::`, then `autorun_env_roots::`, `autorun_env_migration::`, `autorun_env_removed::`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add src-tauri/src/api_templates/library_move.rs src-tauri/src/api_templates/mod.rs src-tauri/src/autorun/roots.rs src-tauri/src/applog.rs src-tauri/tests/suite/api_library_move.rs src-tauri/tests/suite/main.rs
git commit -F - <<'EOF'
feat(v2): fold each environment's API definitions into the shared library once

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 4: Runs confirm and fail items; setup Blocked sentence

**Files:**
- Modify: `src-tauri/src/api_templates/here.rs` (two entry points below)
- Modify: `src-tauri/src/api_templates/fixture_run.rs` — `Ran` gains `templates: Vec<ApiTemplate>` (set from `Start`); `run_fixture_for(…, cancel, source: TrySource)`; `run_saved(…, may_run, source: TrySource)`; `run_fixture`/`run_fixture_within` pass `TrySource::Plain`; after `append`, call `here::note_fixture_run` except when the run was stopped (`RUN_STOPPED`) or the active environment could not be read
- Modify: `src-tauri/src/ai_bridge.rs` — `run_api_template_request` (region after the report, ~2400-2480), the explore `run_saved_in_session` history write (~2195-2208), `api_fixture_run` (~2728: pass `TrySource::Assistant`)
- Modify: `src-tauri/src/commands/api_templates.rs:114` (pass `TrySource::Plain`)
- Modify: `src-tauri/src/autorun/setup.rs` — constants and the setup branch (~400-450)
- Test: `src-tauri/tests/suite/api_library.rs`, `api_fixtures.rs`, `autorun_setup.rs`, `ai_bridge.rs` (template run/prove tests), `autorun_held_browser.rs:862-868` (source-order assertions still hold; update the `run_fixture_for(` needle if the call text changes)

**Interfaces:**
- Consumes: Task 1 transitions, Task 2 stores.
- Produces:
  - `pub fn note_template_run(root: &Path, org: &str, project: &str, ran: &ApiTemplate, ok: bool, failure: Option<HereFailure>, source: TrySource)` — loads the saved copy; does nothing when there is none or `template_hash(ran) != template_hash(saved)`; `ok` → `confirm_template`; else `record_template_failure`. Logs, never fails.
  - `pub fn note_fixture_run(root: &Path, org: &str, project: &str, f: &Fixture, ran: &[ApiTemplate], ok: bool, failure: Option<HereFailure>, source: TrySource)` — `ran` empty → reads the step templates from the store; `ok` → `confirm_fixture(f, ran)`; else `record_fixture_failure`. Logs, never fails.
  - `setup.rs`: `pub const NOT_WORKED_HERE: &str = "this fixture has not worked on this environment yet";` and `pub fn setup_not_worked_here(why: &str) -> String` = `format!("setup failed: {NOT_WORKED_HERE} - ask the assistant to make it work here ({why})")`.
  - Template failure step/detail: `step = report.failed.clone()`, `detail = Some(report.message())`. Fixture: `step = failed_step.map(|n| format!("step {n}"))`, `detail = report.failed.clone()`.
  - In `run_api_template_request`: `Mode::Run` → `note_template_run(.., report.ok, .., Assistant)`; `Mode::Prove` failed → `note_template_run(.., false, .., Assistant)` (it only lands when the draft equals the saved copy, Ruling 7); a successful prove that saved is already confirmed by `store::save`; `Mode::Cleanup` records nothing.
  - In setup: read `before = here::fixture_here(&load_state(..), f, &|id| now.templates…)` before `ready_to_run`; a `ready_to_run` refusal → `note_fixture_run(.., &[], false, Some(HereFailure{at, step: None, detail: Some(why)}), Plain)`; Blocked = `setup_failed(why)` when `before` is `Confirmed`, else `setup_not_worked_here(why)`. Same choice after a failed `run_fixture_for` (which recorded the state itself). `API_TEMPLATE_BUSY` and `RUN_STOPPED` keep today's sentence and record nothing.

- [ ] **Step 1: Write the failing tests**

`api_library.rs` (pure, no browser):
```rust
#[test] fn a_run_of_a_draft_that_differs_from_the_saved_copy_records_nothing() { note_template_run(&root, ORG, PROJECT, &t_changed, false, Some(fail), TrySource::Assistant); assert_eq!(here_of(t_saved), Here::Unconfirmed); assert!(!state_path(..).exists()); }
#[test] fn a_successful_run_of_the_saved_version_confirms_it() {}
#[test] fn a_fixture_note_with_no_ran_templates_reads_them_from_the_store() {}
```
`api_fixtures.rs` (with the existing fake-browser harness around line 950-1150):
```rust
#[tokio::test] async fn a_built_fixture_is_confirmed_here_with_the_templates_it_ran() { /* run_fixture_within ok → fixture_store::list(..)[0].here == Confirmed; each step template Confirmed */ }
#[tokio::test] async fn a_failed_build_marks_only_the_fixture_failed_here_with_its_step() { /* step 2 fails → Failed { failure.step == Some("step 2") }; templates unchanged */ }
#[tokio::test] async fn a_stopped_fixture_run_records_nothing_here() { /* cancel set → no state file */ }
#[tokio::test] async fn a_busy_or_refused_run_records_nothing_here() { /* claim held → run_saved Err(Busy); unknown id → Refused; no state file */ }
#[tokio::test] async fn the_persons_rebuild_never_flips_a_confirmed_fixture() { /* confirm, then two failing run_saved(.., Plain) → still Confirmed */ }
#[tokio::test] async fn the_assistants_retry_flips_a_confirmed_fixture_after_a_failure() { /* confirm, failing Plain, failing Assistant → Failed */ }
```
`autorun_setup.rs`:
```rust
#[tokio::test] async fn an_unconfirmed_setup_fixture_that_fails_blocks_with_not_worked_here_yet() { assert!(err.contains("this fixture has not worked on this environment yet")); }
#[tokio::test] async fn a_confirmed_setup_fixture_that_fails_blocks_with_todays_sentence() { assert!(err.starts_with("setup failed: ") && !err.contains(NOT_WORKED_HERE)); }
#[tokio::test] async fn a_setup_with_no_account_here_counts_as_a_failure_of_the_fixture() { /* fixture names an account key this env lacks → Blocked with NOT_WORKED_HERE; fixture Failed here */ }
#[tokio::test] async fn an_unconfirmed_setup_fixture_that_works_becomes_confirmed() {}
#[tokio::test] async fn a_plain_setup_run_never_rewrites_the_fixture() { /* definition file bytes unchanged after a failing setup */ }
```
`ai_bridge.rs` (next to the existing `api_template_run` tests):
```rust
async fn a_run_of_an_unconfirmed_template_that_works_confirms_it_here() {}
async fn a_failing_run_marks_an_unconfirmed_template_failed_here_with_the_step() {}
async fn a_failed_prove_of_a_different_draft_leaves_the_saved_templates_state_alone() {}
```

- [ ] **Step 2: Run to see them fail**
Run: `cd src-tauri && cargo test --test suite api_fixtures::` (then `autorun_setup::`)
Expected: compile errors (`run_fixture_for` arity, `NOT_WORKED_HERE` missing) / FAIL.

- [ ] **Step 3: Implement** the Produces list.

- [ ] **Step 4: Run**
Run, one at a time: `cd src-tauri && cargo test --test suite api_library::`, `api_fixtures::`, `autorun_setup::`, `autorun_setup_run::`, `ai_bridge::`, `explore_send::`, `autorun_held_browser::`, `autorun_cleanup::`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add src-tauri/src src-tauri/tests/suite
git commit -F - <<'EOF'
feat(v2): runs and builds confirm or fail API items on the active environment

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 5: Environment copy carries no API definitions and never accounts

**Files:**
- Modify: `src-tauri/src/autorun/roots.rs:35-50` — `PER_ENV_ITEMS` per Ruling 10 (`[(&str, CopyTo); 14]`: scripts, deleted-scripts, orders, projects as `Every`; templates, fixtures, api-state, runs, shots, downloads, reports, replays, explore, captures as `ActiveOnly`); `pub const LEGACY_API_ITEMS: [&str; 3] = ["templates", "fixtures", "flows"];`; `fan_out`, `migrate_with`'s "anything to copy" check and `newest_flat_change` also walk `"flows"` (placed under the active environment, as every non-`BY_RUN` item is)
- Modify: `src-tauri/src/autorun/env_copy.rs` — module comment and `copy_environment*` doc comments (no API templates); `COPY_NOTHING_TO_COPY` = `"that source has no scripts, areas, components or quirks to copy"`; `is_empty_env` unchanged in code (it already reads only `Every`)
- Test: `src-tauri/tests/suite/autorun_env_copy.rs`, `autorun_env_roots.rs:82`, `autorun_env_migration.rs:59-66`

**Interfaces:**
- Consumes: Task 3 (definitions are no longer in environment folders after the fold; a copy from a removed folder older than the fold finds only histories).
- Produces: `roots::LEGACY_API_ITEMS` (used by Task 6).

- [ ] **Step 1: Write the failing tests** (`autorun_env_copy.rs`, its existing helpers `three_envs`, `put`, `read`, `envs`):

```rust
#[test] fn copy_never_brings_api_templates_fixtures_or_flows() {
    // replaces the templates assertion in copy_into_an_empty_environment_brings_every_script_and_area:
    put(&ra, "templates/s/t.json", "A-def"); put(&ra, "flows/s/f.json", "A-flow");
    // copy A→B
    assert_eq!(read(&rb, "templates/s/t.json"), None); assert_eq!(read(&rb, "flows/s/f.json"), None);
}
#[test] fn copy_keeps_the_targets_api_state_and_run_histories() {
    put(&rb, "api-state/s.json", "B-state"); put(&rb, "templates/s/t.runs.json", "B-runs");
    // copy A→B with replace
    assert_eq!(read(&rb, "api-state/s.json").as_deref(), Some("B-state")); assert_eq!(read(&rb, "templates/s/t.runs.json").as_deref(), Some("B-runs"));
}
#[test] fn copy_leaves_accounts_untouched() {
    accounts::save_accounts_for(t, &a, &[acct("admin-a")]).unwrap(); accounts::save_accounts_for(t, &b, &[acct("admin-b")]).unwrap();
    // copy A→B
    let after_b = accounts::load_accounts_for(t, &b).unwrap();
    assert_eq!(keys(&after_b), ["admin-b"]); assert!(!keys(&after_b).contains(&"admin-a"));
    assert_eq!(keys(&accounts::load_accounts_for(t, &a).unwrap()), ["admin-a"]);
}
#[test] fn a_source_with_only_api_data_has_nothing_to_copy() { /* source holds templates/ and api-state/ only → Err(COPY_NOTHING_TO_COPY) */ }
#[test] fn a_target_with_only_api_data_counts_as_empty() { assert!(env_copy::is_empty_env(&rb_with_only_templates_and_state)); }
```
Update `autorun_env_roots.rs:82` to `assert_eq!(every, ["scripts", "deleted-scripts", "orders", "projects"]);`; in `autorun_env_migration.rs` add `put(t.path(), "flows/f.json", "FL")` and assert it reaches the active environment.

- [ ] **Step 2: Run to see them fail**
Run: `cd src-tauri && cargo test --test suite autorun_env_copy::`
Expected: FAIL (templates still copied).

- [ ] **Step 3: Implement** the roots and env_copy changes.

- [ ] **Step 4: Run**
Run, one at a time: `cd src-tauri && cargo test --test suite autorun_env_copy::`, `autorun_env_roots::`, `autorun_env_migration::`, `autorun_env_removed::`, `autorun_env_switch::`, `environments::`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add src-tauri/src/autorun/roots.rs src-tauri/src/autorun/env_copy.rs src-tauri/tests/suite
git commit -F - <<'EOF'
feat(v2): copying an environment no longer carries API definitions

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 6: Backups carry the library; older backups fold in

**Files:**
- Modify: `src-tauri/src/backup.rs` — `excluded` (second segment `removed-api`, or starting with `library_move::STAGING_PREFIX`); `is_app_item` (also true for a definition file — `.json` not `.runs.json` — under any `LEGACY_API_ITEMS` item); `restore_files_held` (after the `backup_import::commit` at ~699 succeeds, `library_move::fold_in_held(&held, &autorun, now)`, failure logged without a path, never failing the import)
- Modify: `src-tauri/src/autorun/backup_import.rs:172-190` — `build_from_flat` also copies the definition files (never `*.runs.json`) of each `LEGACY_API_ITEMS` item into every built environment, so the fold that follows the commit picks them up
- Test: `src-tauri/tests/suite/backup.rs`

**Interfaces:**
- Consumes: `library_move::{fold_in_held, STAGING_PREFIX}` (Task 3), `roots::LEGACY_API_ITEMS` (Task 5).
- Produces: nothing new for later tasks.

- [ ] **Step 1: Write the failing tests** (`backup.rs` suite, its existing data-dir helpers):

```rust
#[test] fn a_backup_includes_the_shared_api_library() { /* collect_files has autorun/api-library/templates/<slug>/t.json */ }
#[test] fn a_backup_leaves_out_kept_api_losers_and_library_staging() { /* autorun/removed-api/1/... and autorun/.api-library-staging-1/... not collected */ }
#[test] fn restoring_a_new_backup_restores_the_library() { /* library file deleted locally, restore → back */ }
#[test] fn a_pre_update_backup_with_only_templates_is_app_data_and_folds_into_the_library() {
    // files: autorun/templates/<slug>/t.json (flat). restore → library has t; no envs/<id>/templates/<slug>/t.json left
}
#[test] fn a_per_environment_backup_from_before_the_library_folds_in_on_restore() {
    // files: autorun/envs/qa/templates/<slug>/t.json + its .runs.json. restore → library has t, qa keeps t.runs.json, qa state Confirmed when its history says it worked
}
```

- [ ] **Step 2: Run to see them fail**
Run: `cd src-tauri && cargo test --test suite backup::`
Expected: FAIL.

- [ ] **Step 3: Implement** the changes above.

- [ ] **Step 4: Run**
Run, one at a time: `cd src-tauri && cargo test --test suite backup::`, `api_library_move::`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add src-tauri/src/backup.rs src-tauri/src/autorun/backup_import.rs src-tauri/tests/suite/backup.rs
git commit -F - <<'EOF'
feat(v2): backups carry the shared API library and fold older ones in

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 7: Export without state; import keeps identical items

**Files:**
- Modify: `src-tauri/src/api_templates/share.rs` — `TemplatesImportResult` gains `pub unchanged: Vec<String>` (titles)
- Modify: `src-tauri/src/commands/api_templates.rs:300-425` (`import_at`): before saving a template, load the saved copy; when `library::template_hash` matches, push the title to `unchanged` and write nothing; otherwise save as today (it arrives with `proven: None`, so Unconfirmed everywhere by Ruling 2). Flows are written as today.
- Modify: `src/screens/ApiTemplates/ImportTemplates.tsx` — show `unchanged` as one line ("N unchanged: already here, confirmations kept"); the two warning texts at lines 37 and 107 become: `"Anything new or changed arrives unconfirmed on every environment - ask the assistant to confirm it before relying on it."` and `"Templates and flows with the same id as ones saved here are replaced, and arrive unconfirmed on every environment - ask the assistant to confirm them before relying on them."`
- Regenerate: `src/bindings.ts`
- Test: `src-tauri/tests/suite/api_templates_share.rs`, `src/screens/ApiTemplates/index.test.tsx` (import tests near line 1037)

**Interfaces:**
- Consumes: `library::template_hash`, `here::load_state/template_here`.
- Produces: `TemplatesImportResult.unchanged`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn importing_an_identical_template_keeps_its_proof_and_confirmations() {
    // saved T proven + confirmed here; import a file with T (export strips proven)
    // result.unchanged == [T.title]; store::load(T).proven.is_some(); template_here == Confirmed
}
#[test] fn importing_a_changed_template_is_unconfirmed_on_every_environment() { /* qa and staging both Confirmed; import changed T → both Unconfirmed; proven None */ }
#[test] fn importing_a_new_template_is_unconfirmed_here() {}
#[test] fn an_export_carries_no_confirmation_state() { /* exported JSON has no "here", "proven", "confirmed_at", "failed_here" */ }
```
```tsx
test("an import says how many were already here and kept their confirmations", ...) // expects "1 unchanged"
test("the import warning says new or changed items arrive unconfirmed on every environment", ...)
```

- [ ] **Step 2: Run to see them fail**
Run: `cd src-tauri && cargo test --test suite api_templates_share::`
Expected: FAIL.

- [ ] **Step 3: Implement**; regenerate with `cd src-tauri && cargo test --test bindings`.

- [ ] **Step 4: Run**
Run, one at a time: `cd src-tauri && cargo test --test suite api_templates_share::`, `npx tsc --noEmit`, `npx vitest run src/screens/ApiTemplates/index.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add src-tauri/src/api_templates/share.rs src-tauri/src/commands/api_templates.rs src-tauri/tests/suite/api_templates_share.rs src/bindings.ts src/screens/ApiTemplates
git commit -F - <<'EOF'
feat(v2): importing an identical API template keeps its confirmations

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 8: The assistant sees each item's state on the active environment

**Files:**
- Modify: `src-tauri/src/api_templates/share.rs:49-52` — `UNPROVEN_FOR_ASSISTANT` reworded; add `FAILED_HERE_FOR_ASSISTANT`, `LAST_RUN_FAILED_HERE`
- Modify: `src-tauri/src/ai_bridge.rs` — `api_template_list` (~1525-1715): rows; `api_fixture_list` / `fixture_index` (~2575-2645): rows
- Modify: `src-tauri/src/api_templates/guide.rs:131-134` and the fixtures section (~421): the rule
- Modify: `src-tauri/src/mcp.rs:1166-1235` — descriptions of `list_api_templates`, `prove_api_template`, `run_api_template`, `run_api_fixture`
- Modify: `src-tauri/src/autorun/guide.rs:~1452` — the setup Blocked sentences
- Test: `src-tauri/tests/suite/ai_bridge.rs` (update 3248, 3300-3325, 3910-3935), `mcp_session.rs` or wherever MCP tool descriptions are pinned (grep `run_api_fixture` in `tests/suite`)

**Interfaces:**
- Consumes: `SavedTemplate.here`, `SavedFixture.here` (Task 2).
- Produces (exact copy):
  - `UNPROVEN_FOR_ASSISTANT = "not confirmed on this environment yet - try it here first (run_api_template, or run_api_fixture for a fixture); if it fails, work out the version that works here and prove it with replace: true and a why, which replaces it for every environment"`
  - `FAILED_HERE_FOR_ASSISTANT = "failed on this environment - work out the version that works here and prove it with replace: true and a why (this replaces it for every environment); if you cannot find one, leave it alone and do not try it again in this job"`
  - `LAST_RUN_FAILED_HERE = "confirmed here, but its last run here failed - try it once more; only if that also fails, work out the version that works here"`
  - Template rows (index and full): keep `"proven": bool` (proven on some environment — the fixture save gate); add `"here": "confirmed" | "unconfirmed" | "failed"`; full rows add `"here_note"` (one of the three sentences, `null` when confirmed with no strike) and, when failed, `"failed_here": { "at", "step", "detail" }`; when confirmed with a strike, `"last_failure_here": { … }`. Remove the `"unproven"` key. The index's once-only note names `UNPROVEN_FOR_ASSISTANT` when any row is unconfirmed and `FAILED_HERE_FOR_ASSISTANT` when any is failed.
  - Fixture rows (index and full): the same `"here"`, `"here_note"`, `"failed_here"`, `"last_failure_here"` keys.
  - Guide text (in `guide.rs`, replacing the `proven: false` paragraph): an item `list_api_templates` shows as `here: "unconfirmed"` is worth trying first; if it fails, work out the version that works here and save it (prove with `replace: true` and a why) — that replaces it for every environment; if none can be found, leave it: it stays failed here and is not retried in this job; `here: "failed"` means exactly that; a confirmed item whose last run here failed is tried once more before rediscovering; `proven: false` means it has never been proven on any environment and a fixture cannot use it yet.
  - MCP descriptions: `list_api_templates` adds "Each item says whether it is confirmed, unconfirmed or failed on the active environment."; `prove_api_template`/`run_api_template`/`run_api_fixture` add "An unconfirmed item is worth trying here; a replace changes it for every environment."

- [ ] **Step 1: Write the failing tests** (`ai_bridge.rs`, the harness the neighbouring `list_api_templates` tests use):

```rust
async fn list_api_templates_reports_each_templates_state_on_the_active_environment() {
    // three templates: confirmed, unconfirmed, failed (record_template_failure with step "2", detail "HTTP 500")
    assert_eq!(row(c)["here"], "confirmed"); assert!(row(c)["here_note"].is_null());
    assert_eq!(row(u)["here"], "unconfirmed"); assert_eq!(row(u)["here_note"], UNPROVEN_FOR_ASSISTANT);
    assert_eq!(row(f)["here"], "failed"); assert_eq!(row(f)["failed_here"]["step"], "2"); assert_eq!(row(f)["failed_here"]["detail"], "HTTP 500");
    assert!(row(u).get("unproven").is_none());
}
async fn a_confirmed_template_whose_last_run_failed_says_to_try_it_once_more() { assert_eq!(row["here_note"], LAST_RUN_FAILED_HERE); }
async fn the_same_template_reads_differently_on_another_environment() { /* switch active env → "unconfirmed" */ }
async fn the_fixture_listing_reports_its_state_here() { /* kind fixtures, index and id */ }
async fn the_index_says_the_unconfirmed_and_failed_sentences_once() {}
#[test] fn the_guide_says_try_then_rediscover_then_leave_alone() {
    let g = api_guide(); assert!(g.contains("replaces it for every environment")); assert!(g.contains("not retried in this job")); assert!(g.contains("here: \"unconfirmed\""));
}
#[test] fn the_tool_descriptions_say_the_rule() { /* the four descriptions contain "every environment" */ }
```
Update `ai_bridge.rs:3248` to the new guide wording, and 3310-3321 / 3916-3931 to `here`/`here_note` instead of `unproven`.

- [ ] **Step 2: Run to see them fail**
Run: `cd src-tauri && cargo test --test suite ai_bridge::`
Expected: FAIL.

- [ ] **Step 3: Implement** the Produces list.

- [ ] **Step 4: Run**
Run, one at a time: `cd src-tauri && cargo test --test suite ai_bridge::`, `mcp_session::`, `autorun_guide::`, `guide::`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add src-tauri/src src-tauri/tests/suite
git commit -F - <<'EOF'
feat(v2): the assistant sees each API item's state on the active environment

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 9: Screens show Unconfirmed here / Failed here; copy dialog note

**Files:**
- Create: `src/screens/ApiTemplates/HereBadge.tsx`
- Modify: `src/screens/ApiTemplates/TemplateRow.tsx` (badge at 234-238; details section 358-365; runs warning 385-387; `UNPROVEN_HINT` 135-136 replaced)
- Modify: `src/screens/ApiTemplates/FlowMap.tsx` (chip at 195-199; screen-reader line 214; a flow-level badge from a new `here: Here` prop)
- Modify: `src/screens/ApiTemplates/index.tsx` (pass `overview.flow_here[flow.id]` to `FlowMap`)
- Modify: `src/screens/ApiTemplates/FixturesTab.tsx` (badge per row; export `fixturesHereCounts`)
- Modify: `src/screens/ApiTemplates/RemoveTemplate.tsx`, `RemoveFixture.tsx`, `RemoveFlow.tsx` (say it is removed for every environment)
- Modify: `src/screens/AutoRun/SetupPanel.tsx:108-117,180-195,330-340` (Ruling 16)
- Modify: `src/components/CopyAutoRunDialog.tsx:102-110`
- Modify: `src-tauri/src/api_templates/flow_page.rs:215-265` (the flow page: "unconfirmed here" / "failed here" from `SavedTemplate.here`; `by_stage` keyed to `&SavedTemplate`)
- Test: `src/screens/ApiTemplates/index.test.tsx`, `FixturesTab.test.tsx`, `src/screens/AutoRun/SetupFixtures.test.tsx` (or `AutoRun.test.tsx` where the readiness lines are pinned), `src/components/CopyAutoRunDialog.test.tsx`, `src-tauri/tests/suite/api_template_flows.rs` (flow page)

**Interfaces:**
- Consumes: `Here` from `src/bindings.ts` (Task 2), `TemplatesOverview.flow_here`.
- Produces:
  - `export default function HereBadge({ here }: { here: Here }): JSX.Element | null` — `confirmed` → `null`; `unconfirmed` → `<Badge className="bg-warning/15 text-warning" title={UNCONFIRMED_HINT}>Unconfirmed here</Badge>`; `failed` → `<Badge className="bg-danger/15 text-danger" title={failedHint(here.failure)}>Failed here</Badge>`.
  - `export const UNCONFIRMED_HINT = "Not confirmed on this environment yet. Ask the assistant to try it here before relying on it.";`
  - `export function failedHint(f: HereFailure): string` → `"Failed on this environment" + (f.step ? ` at ${f.step}` : "") + (f.detail ? `: ${f.detail}` : "") + ". Ask the assistant to make it work here."`
  - `export function fixturesHereCounts(list: SavedFixture[]): { unconfirmed: number; failedHere: number }`
  - TemplateRow details: the "Not proven" section becomes a "Not confirmed here" section, shown when `here.state !== "confirmed"`, with the hint (and the failure's step and detail when failed); the "Proven" section stays as is (when and where it was last proven); the runs warning becomes "These runs may be from an earlier version." shown when unconfirmed and runs exist.
  - Readiness: Fixtures line `"<n> fixtures, <k> failed here"` (status `warning`) before today's last-run wording; attention reason `"<k> fixtures failed here - ask the assistant"`; `fixturesLastRuns(...).failed` counts only fixtures not Failed here.
  - Copy dialog note (exact): `"Scripts, areas, components, quirks and the sign-in recording come across. Accounts are not copied. API templates, flows and fixtures are already shared by every environment."` and the replace line: `` `This replaces ${target.name}'s scripts, areas, components and quirks with ${source.name}'s. ${target.name}'s current ones are kept for 30 days and can be copied back from this list.` ``
  - Remove dialogs: "It is removed for every environment, with this environment's run history." (template/fixture), "It is removed for every environment." (flow).

- [ ] **Step 1: Write the failing tests**

```tsx
test("an unconfirmed template is marked Unconfirmed here and a confirmed one shows no mark", ...)
test("a template that failed here says where it failed in its mark's title", ...) // title contains "at 2: HTTP 500"
test("the details say Not confirmed here and never claim this version's runs", ...)
test("a flow shows Unconfirmed here until every template on it is confirmed", ...)
test("a fixture that failed here is marked Failed here", ...)            // FixturesTab.test.tsx
test("the setup panel counts fixtures failed here and points to the assistant", ...) // "1 fixture failed here - ask the assistant"
test("the copy dialog says accounts are not copied and the API library is shared", ...) // CopyAutoRunDialog.test.tsx, exact text above
test("removing a template says it goes for every environment", ...)
```
```rust
#[test] fn the_flow_page_marks_templates_unconfirmed_or_failed_here() { /* page_html contains "unconfirmed here" and "failed here"; never "unproven" */ }
```
Replace the existing "Unproven" expectations in `index.test.tsx` (1112-1130 and the stage-chip test) with the new marks.

- [ ] **Step 2: Run to see them fail**
Run: `npx vitest run src/screens/ApiTemplates src/components/CopyAutoRunDialog.test.tsx src/screens/AutoRun/SetupFixtures.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Implement** the Produces list.

- [ ] **Step 4: Run**
Run, one at a time: `npx vitest run src/screens/ApiTemplates src/components/CopyAutoRunDialog.test.tsx src/screens/AutoRun`, `npx vitest run src/ui-consistency.test.ts`, `npx tsc --noEmit`, `cd src-tauri && cargo test --test suite api_template_flows::`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add src/screens src/components src-tauri/src/api_templates/flow_page.rs src-tauri/tests/suite/api_template_flows.rs
git commit -F - <<'EOF'
feat(v2): API screens mark items unconfirmed or failed on this environment

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 10: Help guide and screenshots

**Files:**
- Modify: `docs-site/src/content/api-templates.ts` — the `unproven` callout (205-212) becomes id `unconfirmed-here`, `locate: { text: "Unconfirmed here" }`, name "Unconfirmed here", does: "Not confirmed on this environment yet. It can run; ask the assistant to try it here before relying on it." Add a `failed-here` callout only if the demo data shows one. Line 131 (import): "Imported templates that are new or changed are marked **Unconfirmed here** on every environment." Tip at 449: "Templates, fixtures and flows are kept on this computer, per project, and shared by every environment. Each one is confirmed separately on each environment: **Unconfirmed here** means it has not worked on the active environment yet, **Failed here** that it failed there. Use **Export** and **Import** to share them with your team." How-to at 468: "A template marked **Unconfirmed here** or **Failed here** has not worked on this environment: ask the assistant to make it work here."
- Modify: `docs-site/src/content/auto-run.ts` — line 42: add "If the setup's fixture has not worked on this environment yet, the case says so: ask the assistant to make it work here."; line 976: "…saved setups and run history… API templates, flows and fixtures are shared by every environment."; the Fixtures readiness line (~201) mentions "failed here".
- Modify: `docs-site/src/content/ai-tools.ts:228` — "**Copy Auto Run data…** brings another environment's scripts, areas, components and quirks into this one (never its accounts; API templates, flows and fixtures are shared already); …"
- Modify: `src/dev/demo.ts` — one demo template `{ state: "unconfirmed" }` (the one that was unproven) and, if the guide gets a `failed-here` callout, one demo fixture `failed`.
- Re-take: the API Templates list shot(s) and the Copy Auto Run data dialog shot if the guide shows it.
- Build: `src-tauri/help/` (committed).

- [ ] **Step 1: Edit the content** as above.

- [ ] **Step 2: Run the guide's own tests**
Run: `npx vitest run docs-site/src`
Expected: FAIL only on the stale built page / missing shot position for `unconfirmed-here` (the guard and built tests).

- [ ] **Step 3: Re-take the shots** (CLAUDE.md "Help site"): confirm the owner's `tauri dev` is not running (port 1420 free), then `npm run docs:dev` (signed out, sample data), `npm run docs:shots -- --dry-run`, then `npm run docs:shots -- --only unconfirmed-here` (plus any other shot whose locate text changed), then close the dev app.
Expected: the dry run lists the shots; the capture writes the PNG and positions without error.

- [ ] **Step 4: Build and re-run the guide tests**
Run: `npm run docs:build`, then `npx vitest run docs-site/src`
Expected: PASS.

- [ ] **Step 5: Commit** (shots, positions and built site together)
```bash
git add docs-site src-tauri/help src/dev/demo.ts
git commit -F - <<'EOF'
docs(v2): the help guide explains the shared API library

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 11: Whole-suite gate

**Files:** none new.

- [ ] **Step 1: Rust suite**
Run: `cd src-tauri && cargo test --tests`
Expected: every test passes (`suite` and `bindings`).

- [ ] **Step 2: Bindings unchanged**
Run: `git status --short src/bindings.ts`
Expected: no change (it was regenerated in Tasks 2 and 7).

- [ ] **Step 3: Frontend suite**
Run: `npm test`
Expected: PASS (a single `App.test.tsx` flake that passes on re-run is the documented one).

- [ ] **Step 4: Typecheck and production build**
Run: `npx tsc --noEmit`, then `npm run build`
Expected: both succeed.

- [ ] **Step 5: Commit any fix-ups** the gate needed, with a message naming what was fixed.

Hand checks owed after merge (need a live org; list them in the backlog, not here): a fresh environment runs a shared fixture as a setup and gets the not-worked-here Blocked sentence; the assistant rediscovers and replaces a template from the second environment and the first one then reads Unconfirmed here.
