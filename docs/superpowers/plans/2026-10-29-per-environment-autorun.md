# Per-environment Auto Run Data Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give each environment its own Auto Run scripts, areas, components, quirks, API templates and run history, in a folder per environment. Existing data is copied in once at the update. Switching environment switches that folder. Copying between environments is the person's own action.

**Architecture:** A new `autorun::roots` module owns the layout: `<appdata>/autorun/` is the shared root and `<appdata>/autorun/envs/<env-id>/` is an environment root. `store::configured_root()` (and `commands::autorun::root(app)`, which reads it) now returns the **active environment's root**. Every per-environment store already takes a `root`, so it is unchanged. The shared stores (environments, accounts, sessions, proposals, test files and a few machine files) resolve the shared root from whatever root they are given (`roots::shared_of`), so callers that are handed an environment root still read and write shared data in the right place. Per-environment writes go through `roots::write_file`/`roots::guard`. These refuse a root that stopped being the active one, which stops a save that was in flight during a switch from landing anywhere.

**Tech Stack:** Rust (Tauri 2, tauri-specta), React 19 + TypeScript, TanStack Query, vitest/jsdom, Rust integration tests in `src-tauri/tests/suite/`.

**Spec:** `docs/superpowers/specs/2026-10-29-per-environment-autorun-design.md`. Read it before any task: it is the authority, and this plan only adds decisions.

## Global Constraints

- Rust owns all file I/O. The webview never builds a path.
- No DELETE call to Azure DevOps, anywhere. Nothing in this plan talks to Azure DevOps.
- User-facing errors and log lines added here name no path and no URL. A raw I/O error goes to `applog` only, never into a returned sentence.
- `src/bindings.ts` is generated: after changing any command or event, run `cd src-tauri && cargo test --test bindings` and commit the regenerated file. Never hand-edit it.
- All Rust tests are modules of `src-tauri/tests/suite/` with a `mod` line in `suite/main.rs`. Never add a `#[cfg(test)]` module or a new file directly under `tests/`. A test that touches the store root (`set_root`, `set_shared_root`, `select_environment`) takes `serial::autorun()`. A test that reads the log tail also takes `serial::log_tail()` first (lock order: `agents() -> log_tail() -> autorun()`).
- One build or test command at a time. Never set `CARGO_TARGET_DIR` or `--target-dir`. Run one module's tests with `cd src-tauri && cargo test --test suite <module>::`.
- Caching: no new cache on either side. Webview queries are reset or invalidated through the existing React Query client. Rust adds no memo of file contents.
- Theming: colours only from tokens (`text-text`, `bg-surface`, `border-border`…). Icons only from `src/lib/actionIcons.ts`, named for what the button does. `src/ui-consistency.test.ts` is a gate: never weaken it.
- Folder and file names (fixed by this plan): `envs/`, `envs/_seed/`, `removed-envs/`, `removed-envs/<env-id>-<epoch ms>/removed.json`, staging folders `.envs-staging-<ms>` (shared root) and `envs/.copying-<env-id>-<ms>` (never listed as an environment).
- An environment folder name is valid only when `environments::valid_id(name)` or `name == "_seed"`. No other name may become a path.
- Keep-for: removed environment folders are purged after `30` days (reuse `store::DAY_MS`, making it `pub` if it is not).
- No new assistant tool. Copying is the person's action in the app.
- No version bump and no `changelog.ts` entry: the release adds those.
- Docs never name a hidden feature (`docs-site/src/guard.test.ts` holds the term list).
- Commits use a Bash heredoc: `git commit -F - <<'EOF' … EOF`, ending with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Confirm with `git log -1`. Never stage `.test-cases/`.

## Store classification (the pinned list)

Every path under the Auto Run root, which module owns it, and its scope. `roots::PER_ENV_ITEMS` and `roots::SHARED_ITEMS` hold the top-level names exactly as below. The Task 1 tripwire test fails on any `root.join("<name>")` whose first segment is in neither list.

| Top-level item | Owner module(s) | Scope | At the update it is copied to |
| --- | --- | --- | --- |
| `scripts/` | `autorun::store` (`scripts_dir`); marks and suspected defects live in the script | per env | every environment |
| `deleted-scripts/` | `autorun::store` (`deleted_dir`) | per env | every environment |
| `orders/` | `autorun::store` (`order_path`, run order) | per env | every environment |
| `projects/<slug>.nav.json` | `autorun::nav`, `area_repair`, `menu_setup` | per env | every environment |
| `projects/<slug>.json` (sign-in recipe) | `autorun::recipe` | per env | every environment |
| `projects/<slug>-components.json` | `autorun::components` | per env | every environment |
| `projects/<slug>-quirks.json` (quirks and validation rules) | `autorun::quirks` | per env | every environment |
| `projects/<slug>-setups.json` | `autorun::setups` | per env | every environment |
| `projects/<slug>-map.json` | `autorun::discovery_map`, `seen_check` | per env | every environment |
| `projects/<slug>-mapping.json` | `autorun::mapping_summary` | per env | every environment |
| `projects/<slug>-divergences.json` | `autorun::divergences` | per env | every environment |
| `projects/<slug>.pwexport.json` | `pw_export::mapping` | per env, **export environment only** | the export environment (the active one at the update) only |
| `templates/` | `api_templates::store` | per env | every environment |
| `fixtures/` | `api_templates::fixture_store` | per env | every environment |
| `flows/` | `api_templates::flow_store` | per env | every environment |
| `runs/` | `autorun::store` (`runs_dir`) | per env (history) | the active environment only |
| `shots/` | `autorun::store` (`shots_dir`) | per env (history) | the active environment only |
| `downloads/` | `autorun::store` (`all_downloads_dir`, supervised downloads) | per env (history) | the active environment only |
| `reports/` | `autorun::store` (`reports_dir`), `autorun::report` | per env (history) | the active environment only |
| `replays/` | `autorun::replay_record` | per env (history) | the active environment only |
| `explore/` | `autorun::explore` | per env (history) | the active environment only |
| `captures/` | `autorun::capture_tools` (`CAPTURES_DIR`) | per env (history) | the active environment only |
| `environments.json` | `crate::environments` | shared | stays |
| `accounts/` | `autorun::accounts` | shared (keyed by env id) | stays |
| `sessions/` | `autorun::accounts`, `autorun::sessions` | shared (keyed by env id) | stays |
| `proposals/` | `crate::environments` | shared (keyed by env id) | stays |
| `test-files/` | `crate::test_files` | shared | stays |
| `test-made.json` | `autorun::test_made`, `autorun::cleanup` | shared (each entry names its environment) | stays |
| `allow-written.json` | `crate::allow_record` | shared (machine) | stays |
| `last-browser.txt` | `autorun::store` (`LAST_BROWSER`) | shared (machine) | stays |
| `accounts.json` | `crate::environments`, `crate::backup` (legacy single list) | shared, legacy | stays |
| `approvals/`, `removed-scripts/` | nothing reads them (left by older versions) | unclassified leftovers | not copied, left alone |

Outside the Auto Run root and untouched: `<appdata>/pw-sync/` (Sync records, keyed by clone and project: one repo, shared) and the clone path in `app-settings.json` (shared).

**Callers of the root.** All of these now get the **active environment's root**. None changes, because the shared stores normalise inside themselves:

- `store::configured_root()` callers: `ai_bridge.rs` (`autorun_root()`, used at 28 sites, plus lines 1072, 2778, 2882, 3510, 3760, 7806), `commands/autorun.rs` (lines 508, 691, 1772) and `autorun/report.rs:771`. All are per environment.
- `commands::autorun::root(app)` callers:
  - Per environment: `api_templates` (11), `autorun` (51), `autorun_publish`, `autorun_divergence` (4), `autorun_record` (3), `autorun_record_signin`, `autorun_replay`, `autorun_setup`, `discovery`.
  - Shared through normalisation: `environments` (9), `test_files` (4), `ai_tools` (3), `ai_bridge` (2) and `modules` (1). These all reach the allow record.
  - The export environment: `pw_export` (3) and `pw_repo` (6), handled in Task 7.
- `lib.rs:632-641` (`set_root`, `purge_deleted_scripts`, `sweep_orphan_downloads`) is replaced by the layout start-up in Tasks 1 and 2.

## Review Focus

1. **The update fails part-way** (anti-virus lock, disk full, unreadable `environments.json`). No `envs/` may appear half-built. The old flat layout stays in charge for that session through `store::set_root(shared)`. The copy is retried on the next start, and leftover staging folders are removed first. Pinned by `a_failed_migration_leaves_no_envs_folder_and_retries` (Task 2).
2. **An assistant's save that is in flight during a switch.** It captured the old environment's root, so the write must be refused with `roots::ENVIRONMENT_SWITCHED` and must never land in either folder. Pinned by `a_save_with_a_root_from_before_a_switch_is_refused` (Task 3).
3. **An editor left open across a switch** (script, areas, components, recipe, order, Playwright export) must not save the old environment's content into the new one. It closes on a switch. Pinned by `script_editor_closes_when_the_environment_changes` (Task 9).
4. **Hundreds of MB of run history at the update.** The owner's `shots/` alone is 376 MB. History goes only to the active environment and is hard-linked file by file when the file system allows (a copy otherwise), so the first start stays fast. Pinned by `migration_puts_history_only_in_the_active_environment` (Task 2).
5. **A backup from before the update imported onto an updated machine.** Its flat files must fan out into every environment, as the update does, and never land in the never-read legacy location. Pinned by `a_pre_update_backup_fills_every_environment` (Task 8).

---

### Task 1: Environment roots, the shared-root normalisation and the pinned classification

**Files:**
- Create: `src-tauri/src/autorun/roots.rs`
- Modify: `src-tauri/src/autorun/mod.rs` (add `pub mod roots;`)
- Modify: `src-tauri/src/autorun/store.rs:20-40` (the layout state), `store.rs:1531-1550` (`last-browser.txt` goes through `shared_of`)
- Modify: `src-tauri/src/environments.rs:100-107` (`file_path`, `proposals_path_for`), `environments.rs:239,273-291` (legacy `accounts.json` and `sessions/` go through `shared_of`)
- Modify: `src-tauri/src/autorun/accounts.rs:101-114` (`session_path_for`, `accounts_path_for`, `sessions_dir_for`)
- Modify: `src-tauri/src/test_files.rs:83-85` (`folder`)
- Modify: `src-tauri/src/autorun/test_made.rs:55-57` (`path` becomes `pub fn path` and goes through `shared_of`)
- Modify: `src-tauri/src/allow_record.rs:59-61` (`path` goes through `shared_of`)
- Modify: `src-tauri/src/autorun/cleanup.rs` (`lines_with_templates`: templates for `query.environment` come from `roots::env_root_beside(root, &query.environment)`)
- Modify: `src-tauri/src/commands/autorun.rs:507-516` (`root(app)`: `configured_root()` or `Err(NO_DATA_DIRECTORY)`, with no fallback to a derived path)
- Modify: `src-tauri/src/commands/environments.rs` (`move_bridge_to`: call `store::select_environment(&file.active)` after `environments::set_active`)
- Modify: `src-tauri/src/lib.rs:628-641` (start-up: flat or per environment, and purge and sweep run in every environment root)
- Create: `src-tauri/tests/suite/autorun_env_roots.rs`, plus `mod autorun_env_roots;` in `src-tauri/tests/suite/main.rs`

**Interfaces:**
- Consumes: `crate::environments::{valid_id, active_id, load_or_init}`, `store::{DAY_MS, purge_deleted_scripts, sweep_orphan_downloads}`.
- Produces (in `autorun::roots`):
  - `pub const ENVS_DIR: &str = "envs"; pub const SEED_ID: &str = "_seed"; pub const REMOVED_DIR: &str = "removed-envs";`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum CopyTo { Every, ActiveOnly }`
  - `pub const PER_ENV_ITEMS: [(&str, CopyTo); 14]`: scripts, deleted-scripts, orders, projects, templates, fixtures, flows (`Every`); runs, shots, downloads, reports, replays, explore, captures (`ActiveOnly`)
  - `pub const SHARED_ITEMS: [&str; 9]`: environments.json, accounts, sessions, proposals, test-files, test-made.json, allow-written.json, last-browser.txt, accounts.json
  - `pub fn is_export_record(rel: &Path) -> bool`: `projects/<anything>.pwexport.json`
  - `pub fn valid_folder_id(id: &str) -> bool`
  - `pub fn shared_of(root: &Path) -> PathBuf`: returns `root.parent().parent()` when `root` is `<x>/envs/<valid folder id>`, and `root` itself otherwise
  - `pub fn env_id_of(root: &Path) -> Option<String>`
  - `pub fn env_root(shared: &Path, env_id: &str) -> PathBuf`
  - `pub fn env_root_beside(root: &Path, env_id: &str) -> PathBuf`: the sibling environment root when `root` is an environment root, and `root` itself in the flat layout
  - `pub fn is_per_env_layout(shared: &Path) -> bool`: `shared/envs` is a directory
  - `pub fn env_roots(shared: &Path) -> Vec<PathBuf>`: every `envs/<valid folder id>` that exists, sorted
- Produces (in `autorun::store`). The layout state becomes a private enum `Layout { Unset, Flat(PathBuf), PerEnv { shared: PathBuf, active: Option<PathBuf> } }`, and:
  - `pub fn set_root(root: PathBuf)`: unchanged signature. Sets `Flat(root)` (tests, and a failed update).
  - `pub fn set_shared_root(shared: PathBuf)`: sets `PerEnv { shared, active: None }`.
  - `pub fn shared_root() -> Option<PathBuf>`
  - `pub fn select_environment(env_id: &str) -> Result<PathBuf, String>`: under `PerEnv`, sets the active root and creates it. Under `Flat`, keeps and returns the flat root. Under `Unset`, `Err(NO_DATA_DIRECTORY sentence)`. An invalid id gives `Err("that environment is not there any more")`.
  - `pub fn configured_root() -> Option<PathBuf>`: `Flat` gives the root. `PerEnv` with an active root gives it. `PerEnv` without one resolves `environments::active_id(&shared)` (which makes Default as today), selects it and returns it, with no store lock held across that call. `Unset` gives `None`.
  - `pub fn reselect_from_disk() -> Result<Option<PathBuf>, String>`: re-reads the active id under `PerEnv` (used by Task 8).

- [ ] **Step 1: Write the failing tests** in `autorun_env_roots.rs`. Each test that touches the layout takes `serial::autorun()` and ends with `set_root` back to a temp dir.

```rust
#[test] fn shared_of_strips_an_environment_root() {
    // shared_of(t/envs/env-0000000a) == t; shared_of(t/envs/_seed) == t;
    // shared_of(t) == t; shared_of(t/envs/Not-An-Id) == t/envs/Not-An-Id
}
#[test] fn env_id_of_reads_only_valid_folder_ids() {
    // Some("env-0000000a"), Some("_seed"); None for t, t/envs, t/envs/../x, t/envs/ENV-1
}
#[test] fn flat_root_is_its_own_shared_root() {
    // set_root(t) -> configured_root()==Some(t), shared_root()==Some(t), select_environment("env-0000000a")==Ok(t)
}
#[test] fn configured_root_follows_the_selected_environment() {
    // set_shared_root(t); configured_root() == t/envs/<Default id> (Default made by load_or_init)
    // save a second env with environments::save_env; select_environment(&b) -> configured_root()==t/envs/<b>
}
#[test] fn per_environment_stores_write_under_the_environment_root() {
    // e = t/envs/env-0000000a. For each of: store::save_script, save_run, save_order, delete_script (deleted-scripts),
    // nav::nav_path, components::components_path, quirks::quirks_path, setups::setups_path,
    // discovery_map::map_path, divergences::path, mapping_summary::summary_path, recipe::save_recipe,
    // pw_export::mapping::path, api_templates::store::templates_dir, fixture_store::fixtures_dir,
    // flow_store::flows_dir, explore::session_path, capture_tools::capture_file, replay_record::dir,
    // store::reports_dir, store::shot_path, store::downloads_dir:
    // assert!(path.starts_with(&e)) (or the written file exists under e)
}
#[test] fn shared_stores_write_under_the_shared_root_even_when_given_an_environment_root() {
    // e as above, handed to: environments::load_or_init(&e) -> t/environments.json exists, e/environments.json does not;
    // accounts::accounts_path_for(&e, id), sessions_dir_for, environments::proposals_path_for,
    // test_files::folder, test_made::path, allow_record::AllowRecord::path, store::last_browser (reads t/last-browser.txt):
    // assert!(p.starts_with(&t) && !p.starts_with(t.join("envs")))
}
#[test] fn every_root_join_in_src_is_classified() {
    // Walk src-tauri/src/**/*.rs (CARGO_MANIFEST_DIR). Collect the first path segment of every
    // `root.join("<lit>")` and every `root.join(CONST)` whose CONST is a `&str` const in the same file.
    // Each segment must be in roots::PER_ENV_ITEMS names or roots::SHARED_ITEMS, unless the file is in
    // NOT_AUTORUN_ROOTS (pw_repo/**, pw_export/clone.rs, pw_export/export.rs, autorun/pw_rules.rs,
    // guide.rs, workspace.rs, commands/guide.rs), where `root` is the clone or the help folder.
    // On failure, name the file and the segment.
}
#[test] fn cleanup_preview_in_another_environment_uses_its_templates() {
    // A delete template saved only under t/envs/<b>; a test-made entry for <b>; preview with the active root
    // t/envs/<a> and query.environment=<b> -> that line can be deleted (has a template).
}
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cd src-tauri && cargo test --test suite autorun_env_roots::`
Expected: compile errors (`roots` and the new store functions do not exist yet).

- [ ] **Step 3: Implement `autorun/roots.rs`** with the Interfaces above.

- [ ] **Step 4: Implement the store layout state** (`set_root`, `set_shared_root`, `shared_root`, `select_environment`, `configured_root`, `reselect_from_disk`).

- [ ] **Step 5: Route the shared stores through `roots::shared_of`.** This covers every path function listed under Files. `cleanup` reads templates through `env_root_beside`.

- [ ] **Step 6: Wire the start-up and the switch.**
  - In `lib.rs`, use `shared = dir.join("autorun")`. If `roots::is_per_env_layout(&shared)`, call `set_shared_root(shared)`; otherwise call `set_root(shared)`. Task 2 inserts the update before this.
  - Then run `purge_deleted_scripts` and `sweep_orphan_downloads` for each of `roots::env_roots(&shared)` (or for the flat root). The existing log line becomes one total.
  - `commands::autorun::root(app)` returns `configured_root().ok_or(NO_DATA_DIRECTORY)`.
  - `move_bridge_to` selects the new environment right after `environments::set_active` succeeds.

- [ ] **Step 7: Run the module and the neighbours it touches**

Run: `cd src-tauri && cargo test --test suite autorun_env_roots::`, then `cargo test --test suite environments::`, then `cargo test --test suite autorun_cleanup::`, then `cargo test --test suite test_files::`.
Expected: all PASS. The existing suites still pass because the flat `set_root` keeps its meaning.

- [ ] **Step 8: Run the whole Rust suite once**

Run: `cd src-tauri && cargo test --tests`
Expected: PASS.

- [ ] **Step 9: Commit** with the message `feat(v2): Auto Run roots per environment, shared stores normalised`.

---

### Task 2: The one-time copy at the update, and the seed

**Files:**
- Modify: `src-tauri/src/autorun/roots.rs` (migration and seed)
- Modify: `src-tauri/src/environments.rs` (`load_or_init_locked` when it makes Default, and `save_env_with` for a new id, call `roots::seed_into`)
- Modify: `src-tauri/src/lib.rs` (call the migration before the layout choice made in Task 1)
- Create: `src-tauri/tests/suite/autorun_env_migration.rs`, plus its `mod` line

**Interfaces:**
- Consumes: Task 1's `PER_ENV_ITEMS`, `CopyTo`, `is_export_record`, `env_root`, `valid_folder_id`, `store::set_shared_root`/`set_root`.
- Produces:
  - `pub struct MigrationReport { pub environments: usize, pub items: usize }`
  - `pub fn migrate_once(shared: &Path) -> Result<Option<MigrationReport>, String>`: `Ok(None)` when `envs/` already exists.
  - `pub fn fan_out(flat: &Path, envs_dir: &Path, ids: &[String], active: Option<&str>) -> Result<usize, String>`: copies `flat`'s per-environment items into `envs_dir/<id>/` for each id, and returns the count of top-level items copied. `Every` items are real copies into every id. `ActiveOnly` items go to `active` only (or to the only id when `active` is `None`) and are hard-linked file by file with `std::fs::hard_link`, falling back to `std::fs::copy`. Export records go to `active` only. Task 8 reuses this.
  - `pub fn seed_into(shared: &Path, env_id: &str) -> Result<bool, String>`: renames `envs/_seed` to `envs/<env_id>` when the seed exists and the target does not. Returns whether it did.

The migration algorithm (the decisions):
1. If `shared/envs` exists, return `Ok(None)`.
2. Remove any leftover `shared/.envs-staging-*` folder. A failure to remove one is logged and does not stop the update.
3. If none of the `PER_ENV_ITEMS` exists under `shared`, create an empty `shared/envs` and return `Ok(Some(report with 0 and 0))`.
4. Read the environment ids and the active id from `environments.json` without creating one (add `pub fn read_ids(root: &Path) -> Result<Option<(Vec<String>, String)>, String>` to `environments.rs`, under its lock). An unreadable file returns `Err`. A missing file means no environments: the ids are `["_seed"]` and the active id is `_seed`.
5. Run `fan_out(shared, &staging, &ids, Some(active))` into `staging = shared/.envs-staging-<ms>`. Then `std::fs::rename(staging, shared/envs)`. Any error removes the staging folder (best effort) and returns `Err`, and `envs/` never exists half-built.
6. Never touch the originals.

`lib.rs`:
- `Ok(Some(r))` logs exactly `format!("Auto Run: per-environment data set up for {} environment(s), {} item(s) copied", r.environments, r.items)`, then `set_shared_root`.
- `Ok(None)` calls `set_shared_root`.
- `Err(e)` logs a warning with no path (`"Auto Run: the per-environment copy did not finish and will be tried again at the next start"`), logs `e` at debug level only if it carries no path (otherwise leave it out), then calls `store::set_root(shared)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn migration_copies_every_item_into_each_environment() {
    // environments.json with env-0000000a (active) and env-0000000b; scripts/case-1.json, projects/p.nav.json,
    // templates/x/t.json at the flat root -> both envs/<id>/ hold all three, byte-equal
}
#[test] fn migration_puts_history_only_in_the_active_environment() {
    // runs/run-1.json, shots/a.png -> present under envs/<active>/, absent under envs/<other>/
}
#[test] fn migration_puts_export_records_only_in_the_active_environment() {
    // projects/p.pwexport.json -> only envs/<active>/projects/p.pwexport.json
}
#[test] fn migration_without_environments_keeps_a_seed() {
    // no environments.json -> envs/_seed/scripts/case-1.json exists, and no other folder under envs
}
#[test] fn migration_leaves_the_originals_untouched() {
    // the flat scripts/case-1.json still exists with the same bytes
}
#[test] fn migration_runs_once() {
    // a second migrate_once returns Ok(None) and copies nothing new
}
#[test] fn a_failed_migration_leaves_no_envs_folder_and_retries() {
    // a leftover .envs-staging-1 and a garbage environments.json -> Err, no envs/;
    // after fixing environments.json -> Ok(Some(_)), envs/ exists, the leftover staging is gone
}
#[test] fn fresh_install_gets_an_empty_envs_folder() {
    // an empty shared root -> Ok(Some(MigrationReport{environments:0, items:0})), envs/ exists and is empty
}
#[test] fn first_environment_made_after_the_update_takes_the_seed() {
    // envs/_seed/scripts/case-1.json; environments::load_or_init(shared) makes Default
    // -> envs/<Default id>/scripts/case-1.json exists, envs/_seed does not
}
#[test] fn a_second_new_environment_starts_empty() {
    // after the Default above, save_env a new environment -> envs/<new> has no scripts
}
#[test] fn migration_log_line_names_no_path() {
    // serial::log_tail() then serial::autorun(): run the lib's report formatting via a pub fn
    // roots::migration_log_line(&MigrationReport) -> String; assert it contains "2 environment(s)" and no ':' '\\' '/'
}
```

(`roots::migration_log_line(r: &MigrationReport) -> String` is the one formatter that `lib.rs` logs.)

- [ ] **Step 2: Run them and confirm they fail.** Run: `cd src-tauri && cargo test --test suite autorun_env_migration::`. Expected: compile errors.
- [ ] **Step 3: Implement `read_ids`, `fan_out`, `migrate_once`, `seed_into` and `migration_log_line`.** Wire `seed_into` into Default creation and new-environment saves, and wire the migration into `lib.rs`.
- [ ] **Step 4: Run the module.** Expected: PASS. Then run `cargo test --test suite environments::`. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(v2): copy Auto Run data into each environment once, at the update`.

---

### Task 3: The switch guard, in-flight protection and the reload event

**Files:**
- Modify: `src-tauri/src/autorun/roots.rs` (the switch lock, the write guard and allowed targets)
- Modify the writes of every per-environment store so they go through the roots module. Each line below is a call that changes:
  - `autorun/store.rs:123,633,738,831,1188,1385,1621`: `fs::write` and `atomic_write` become `roots::write_file`/`roots::write_bytes`, and the rename aside takes `roots::guard(root)?` first.
  - `autorun/nav.rs`, `components.rs:171`, `quirks.rs`, `setups.rs`, `discovery_map.rs:597`, `divergences.rs`, `explore.rs`, `mapping_summary.rs`, `recipe.rs`, `replay_record.rs` and `capture_tools.rs`: each `atomic_write` becomes `roots::write_file`.
  - `api_templates/store.rs:125,218`, `fixture_store.rs:97,164`, `flow_store.rs` and `pw_export/mapping.rs`: the same change.
- Modify: `src-tauri/src/autorun/store.rs` (`SaveScriptsError` gains `EnvironmentSwitched`, which displays `roots::ENVIRONMENT_SWITCHED`), and the bridge's script-save status mapping in `ai_bridge.rs`, which answers 409 for it
- Modify: `src-tauri/src/commands/environments.rs` (`refuse_switch` adds the browser-lease refusal; `switch_with` holds `roots::switch_lock().write()` across `move_bridge_to`; `env_set_active` emits the event)
- Modify: `src-tauri/src/events.rs` (add the event) and `src-tauri/src/lib.rs` (register it in `collect_events!`; the scripts and data listeners forward only when the changed root equals `store::configured_root()`)
- Create: `src-tauri/tests/suite/autorun_env_switch.rs`, plus its `mod` line
- Regenerate: `src/bindings.ts`

**Interfaces:**
- Consumes: Task 1's `store::select_environment` and `configured_root`, `roots::env_id_of`, `crate::autorun::lease::a_browser_is_held()`.
- Produces:
  - `pub const ENVIRONMENT_SWITCHED: &str = "the environment was switched while this was being saved - nothing was saved; read it again in the environment now active";`
  - `pub fn switch_lock() -> &'static std::sync::RwLock<()>`
  - `pub fn guard(root: &Path) -> Result<std::sync::RwLockReadGuard<'static, ()>, String>`: takes the read lock. It refuses when `env_id_of(root)` is `Some` and `root` is neither `store::configured_root()` nor an allowed target. A flat root is never refused.
  - `pub fn write_file(root: &Path, path: &Path, text: &str) -> Result<(), String>` (guard, then `ai_tools::atomic_write`) and `pub fn write_bytes(root: &Path, path: &Path, bytes: &[u8]) -> Result<(), String>` (guard, then `fs::write`)
  - `pub struct AllowedTarget` (RAII) and `pub fn allow_writes_to(root: &Path) -> AllowedTarget`: lets writes into one non-active environment root until the guard drops. It is used by the copy tasks and by the export environment's records.
  - `pub const SWITCH_BROWSER_SIGNED_IN: &str = "the environment cannot be switched while an Auto Run browser is signed in to an account - close it first";` in `commands/environments.rs`
  - `events::AutorunEnvironmentChanged { pub env_id: String }`, emitted as `autorun-environment-changed`

**Lock rule** (write it as a comment on `switch_lock`): never call a store write while holding `switch_lock()` in any mode. `switch_with` takes the write lock only around `move_bridge_to` (the file write and `select_environment`), after the existing slot locks and the template claim.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_save_with_a_root_from_before_a_switch_is_refused() {
    // set_shared_root(t); two envs a, b; select a; let ra = configured_root(); select b;
    // store::save_scripts_atomically(&ra, ..) -> Err(SaveScriptsError::EnvironmentSwitched);
    // neither t/envs/a/scripts nor t/envs/b/scripts holds the case
}
#[test] fn a_save_in_the_active_environment_goes_through() { /* select a; save with ra -> Ok, file under t/envs/a */ }
#[test] fn flat_layout_saves_are_never_refused() { /* set_root(t); nav::put_path(t,..) -> Ok */ }
#[test] fn allowed_target_accepts_writes_while_held() {
    // active a; components::put(rb, ..) -> Err(ENVIRONMENT_SWITCHED); with allow_writes_to(&rb) held -> Ok; dropped -> Err again
}
#[test] fn switch_is_refused_while_a_browser_holds_an_account() {
    // lease::try_acquire(env, "admin", Holder::Browser{..}) held -> refuse_switch(false) == Err(SWITCH_BROWSER_SIGNED_IN)
}
#[test] fn per_env_stores_write_only_through_roots() {
    // source scan of the per-env owner files from the classification table:
    // no `ai_tools::atomic_write(` and no `std::fs::write(` outside autorun/roots.rs
}
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `cd src-tauri && cargo test --test suite autorun_env_switch::`. Expected: compile errors.
- [ ] **Step 3: Implement the guard, the write helpers and the allowed targets.** Move every listed write onto them.
- [ ] **Step 4: Implement the switch changes.** Add the lease refusal (after the template check in `refuse_switch`), take the write lock around `move_bridge_to`, and emit `AutorunEnvironmentChanged` from `env_set_active` on success. Filter the lib listeners by the configured root.
- [ ] **Step 5: Regenerate the bindings.** Run: `cd src-tauri && cargo test --test bindings`. Expected: PASS, and `src/bindings.ts` gains `autorunEnvironmentChanged`.
- [ ] **Step 6: Run the module, then the whole Rust suite.** Run `cargo test --test suite autorun_env_switch::`, then `cargo test --tests`. Expected: PASS.
- [ ] **Step 7: Commit** with the message `feat(v2): a switch reloads, and a save from before it is refused`.

---

### Task 4: Removing an environment keeps its folder for 30 days

**Files:**
- Modify: `src-tauri/src/autorun/roots.rs` (the removed area)
- Modify: `src-tauri/src/environments.rs` (`remove_env` retires the folder after the file write; a failure is logged and not fatal)
- Modify: `src-tauri/src/lib.rs` (at start, `roots::purge_removed` logs `"Auto Run: {n} removed environment folder(s) older than 30 days purged"` when n > 0)
- Create: `src-tauri/tests/suite/autorun_env_removed.rs`, plus its `mod` line

**Interfaces:**
- Consumes: Task 1's `env_root` and `REMOVED_DIR`, `store::DAY_MS`.
- Produces:
  - `pub const REMOVED_KEPT_DAYS: u64 = 30;`
  - `#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)] pub struct RemovedFolder { pub key: String, pub env_id: String, pub name: String, pub removed_at_ms: u64, pub reason: String }`. `key` is the folder name `<env-id>-<ms>`. `reason` is `"removed"`, `"replaced"` or `"replaced by an import"`.
  - `pub fn retire(shared: &Path, env_id: &str, name: &str, reason: &str, now_ms: u64) -> Result<Option<String>, String>`: renames `envs/<id>` to `removed-envs/<id>-<now_ms>` and writes `removed.json` inside it. Returns `None` when there was no folder.
  - `pub fn removed_folders(shared: &Path, now_ms: u64) -> Vec<RemovedFolder>`: only folders within 30 days with a readable `removed.json`, newest first.
  - `pub fn removed_root(shared: &Path, key: &str) -> Option<PathBuf>`: `Some` only for a key that `removed_folders` would list now.
  - `pub fn purge_removed(shared: &Path, now_ms: u64) -> usize`

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn removing_an_environment_keeps_its_autorun_folder_in_the_removed_area() {
    // envs/<b>/scripts/case-1.json; environments::remove_env(&t, b) -> no envs/<b>;
    // removed_folders(t, now) has one entry {env_id:b, name:"B", reason:"removed"} whose folder holds scripts/case-1.json
}
#[test] fn removed_folders_lists_only_the_last_30_days() { /* retire at now-31d and now-1d -> only the second */ }
#[test] fn purge_removes_folders_older_than_30_days_only() { /* returns 1; the recent one stays */ }
#[test] fn a_removed_environment_without_a_folder_is_removed_cleanly() { /* remove_env Ok, removed_folders empty */ }
#[test] fn removed_root_refuses_unknown_and_expired_keys() { /* "../x", "env-0000000b-1" (31 days) -> None */ }
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `cd src-tauri && cargo test --test suite autorun_env_removed::`. Expected: compile errors.
- [ ] **Step 3: Implement the removed area.** Call `retire` from `remove_env` (with the removed environment's name, read before the write) and wire `purge_removed` at start.
- [ ] **Step 4: Run the module and `environments::`.** Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(v2): a removed environment's Auto Run data is kept for 30 days`.

---

### Task 5: Copy a whole environment's Auto Run data

**Files:**
- Create: `src-tauri/src/autorun/env_copy.rs` (the whole-environment part; Task 6 adds the one-case part)
- Modify: `src-tauri/src/autorun/mod.rs` (`pub mod env_copy;`)
- Create: `src-tauri/src/commands/env_copy.rs`, and add `pub mod env_copy;` to `commands/mod.rs`
- Modify: `src-tauri/src/lib.rs` (register the commands in `collect_commands!`)
- Create: `src-tauri/tests/suite/autorun_env_copy.rs`, plus its `mod` line
- Regenerate: `src/bindings.ts`

**Interfaces:**
- Consumes: Task 1's `PER_ENV_ITEMS`, `CopyTo`, `is_export_record`, `env_root` and `shared_of`; Task 3's `switch_lock`; Task 4's `retire`, `removed_folders` and `removed_root`; `commands::environments::{refuse_switch, SWITCH_BUSY, SWITCH_TEMPLATE_RUNNING}`, `commands::autorun_slots::opening`, `commands::autorun::lock_every_slot`, `api_templates::runner::claim`.
- Produces (in `autorun::env_copy`, all `serde::Serialize + specta::Type`):
  - `pub struct CopySource { pub key: String, pub name: String, pub removed: bool, pub days_left: Option<u32> }`. `key` is an env id, or `removed:<folder key>`.
  - `pub struct CopyChoices { pub target_empty: bool, pub sources: Vec<CopySource> }`
  - `pub struct CopyDone { pub items: u32, pub kept_aside: bool }`
  - `pub const COPY_NEEDS_CONFIRM: &str = "this environment already has Auto Run data - confirm to replace it";`
  - `pub const COPY_ONTO_ITSELF: &str = "an environment cannot be copied onto itself";`
  - `pub fn is_empty_env(env_root: &Path) -> bool`: no file under any `Every` item, leaving export records out.
  - `pub fn copy_choices(shared: &Path, envs: &EnvFile, target_id: &str, now_ms: u64) -> Result<CopyChoices, String>`
  - `pub fn source_root(shared: &Path, envs: &EnvFile, key: &str, now_ms: u64) -> Result<PathBuf, String>`
  - `pub fn copy_environment(shared: &Path, source: &Path, target_id: &str, target_name: &str, replace: bool, now_ms: u64) -> Result<CopyDone, String>`
- Produces (commands, in `commands::env_copy`):
  - `#[tauri::command] pub fn env_copy_choices(app: tauri::AppHandle, target_id: String) -> Result<CopyChoices, String>`
  - `#[tauri::command] pub async fn env_copy_autorun(app: tauri::AppHandle, target_id: String, source_key: String, replace: bool) -> Result<CopyDone, String>`

The copy (the decisions):
- It copies `Every` items only, and never export records. Run history is never copied.
- The target's history and its export records stay in place.
- Order of operations:
  1. Copy the source's `Every` items into `envs/.copying-<target>-<ms>`.
  2. Rename the target's history items and export records into that staging folder.
  3. Retire the rest of the target as `"replaced"`, but only if it is not empty. This is the backup folder the spec asks for, and it is listed as a removed source for 30 days.
  4. Rename the staging folder onto the target.

  An error before step 3 puts the moved items back and removes the staging folder. An error at step 3 or 4 is reported as `"the copy did not finish - see Settings > Logs"`, and nothing is deleted.
- A non-empty target without `replace` returns `Err(COPY_NEEDS_CONFIRM)`.
- The command holds the switch's own guards whatever the target: `opening().write()`, `lock_every_slot()` (busy gives `SWITCH_BUSY`), `refuse_switch(slots.any_open())`, the template claim, and `roots::switch_lock().write()` for the file work. When the target is the active environment, it emits `AutorunEnvironmentChanged`.
- It logs `"Environments: copied Auto Run data into one environment ({items} item(s))"`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn copy_into_an_empty_environment_brings_every_script_and_area() { /* source a: scripts + projects/p.nav.json + templates; target b empty -> b has them; CopyDone{kept_aside:false} */ }
#[test] fn copy_never_brings_history_or_export_records() { /* a has runs/, shots/, projects/p.pwexport.json -> b has none */ }
#[test] fn copy_onto_a_non_empty_environment_asks_first() { /* replace=false -> Err(COPY_NEEDS_CONFIRM); b unchanged */ }
#[test] fn replacing_keeps_the_old_contents_aside_and_the_targets_history() {
    // b has scripts/case-9.json, runs/run-1.json, projects/p.pwexport.json; replace=true
    // -> b/scripts == a's; b/runs/run-1.json and b/projects/p.pwexport.json still there;
    //    removed_folders lists b with reason "replaced", holding scripts/case-9.json
}
#[test] fn a_removed_folder_is_a_source() { /* retire c; copy_choices lists removed:<key> labelled removed, days_left 30; copy from it works */ }
#[test] fn copy_choices_say_whether_the_target_is_empty_and_leave_the_target_out() { }
#[test] fn copying_an_environment_onto_itself_is_refused() { /* Err(COPY_ONTO_ITSELF) */ }
#[test] fn an_unknown_source_key_is_refused() { /* "removed:../x", "env-ffffffff" -> Err, no path in the text */ }
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `cd src-tauri && cargo test --test suite autorun_env_copy::`. Expected: compile errors.
- [ ] **Step 3: Implement `env_copy` (whole environment) and the two commands.** Register the commands.
- [ ] **Step 4: Regenerate the bindings.** Run: `cd src-tauri && cargo test --test bindings`. Expected: PASS.
- [ ] **Step 5: Run the module.** Expected: PASS.
- [ ] **Step 6: Commit** with the message `feat(v2): copy Auto Run data from another environment`.

---

### Task 6: Copy one case to another environment

**Files:**
- Modify: `src-tauri/src/autorun/env_copy.rs` (the one-case part)
- Modify: `src-tauri/src/commands/env_copy.rs` (two commands) and `lib.rs` (register them)
- Modify: `src-tauri/tests/suite/autorun_env_copy.rs` (append the tests)
- Regenerate: `src/bindings.ts`

**Interfaces:**
- Consumes: Task 3's `roots::allow_writes_to`; `store::{load_script, save_script}`, `components::{load_components, find, put}`, `nav::{load_nav, find_area, put_path}`, `recipe::{load_recipe, save_recipe}`, `accounts::load_accounts_for`, `api_templates::{fixture_store, flow_store}` loaders, and `browser::actions::Action::UseComponent`.
- Produces:
  - A struct with derives `Debug, Clone, PartialEq, serde::Serialize, specta::Type`:

    ```rust
    pub struct CaseCopyPlan {
        pub target_has_script: bool,
        pub components_added: Vec<String>,
        pub components_kept: Vec<String>,
        pub area_added: Option<String>,
        pub area_kept: Option<String>,
        pub uses_default_area: bool,
        pub recipe_added: bool,
        pub missing_account: Option<String>,
        pub missing_api: Vec<String>,
    }
    ```

  - `pub const CASE_COPY_NEEDS_CONFIRM: &str = "that environment already has a script for this case - confirm to replace it";`
  - `pub const NO_SCRIPT_TO_COPY: &str = "this case has no script to copy";`
  - `pub const COPY_TO_ACTIVE: &str = "that is the environment you are in - pick another one";`
  - `pub fn plan_case_copy(source: &Path, target: &Path, target_env_id: &str, org: &str, project: &str, case_id: i32) -> Result<CaseCopyPlan, String>`
  - `pub fn copy_case(source: &Path, target: &Path, target_env_id: &str, org: &str, project: &str, case_id: i32, replace: bool) -> Result<CaseCopyPlan, String>`
  - Commands:
    - `#[tauri::command] pub fn auto_run_copy_case_plan(app: tauri::AppHandle, organization: String, project: String, case_id: i32, target_id: String) -> Result<CaseCopyPlan, String>`
    - `#[tauri::command] pub fn auto_run_copy_case(app: tauri::AppHandle, organization: String, project: String, case_id: i32, target_id: String, replace: bool) -> Result<CaseCopyPlan, String>`

The rules (the decisions):
- **Components.** Walk `Action::UseComponent` names through every step's actions. Recurse into each found component's own actions, and into nested action bodies, until no new name appears.
  - A name missing in the target is added to `components_added` and `put` there.
  - A name present but different (compare `serde_json::to_value`) is added to `components_kept`, and the target's version is used.
  - An equal one is skipped.
- **Area.** When `script.area` is non-blank, resolve it with `find_area` in the source.
  - Missing in the target: `area_added`, written with `put_path` (the whole entry, variants included).
  - Different: `area_kept`.
  - When the area is blank, set `uses_default_area = true` and copy nothing.
- **Recipe.** When the target has none and the source has one: `recipe_added`, written with `save_recipe`.
- **API data.** `missing_api` lists `script.setup.fixture` ids and `script.preconditions[].flow` ids that the target lacks, deduplicated and in order. They are **not** copied (see the gap rulings).
- **Account.** `missing_account` is `script.account` when the target environment's accounts lack that key.
- **The script.** It is written last with `store::save_script`, with `suspected_defect` and `cleared_defect` set to `None`. A target script without `replace` gives `Err(CASE_COPY_NEEDS_CONFIRM)` and nothing is written.
- **Scope.** All target writes run with `allow_writes_to(target)` held. The source is the active root. A target equal to the active id gives `COPY_TO_ACTIVE`. Log `"Auto Run: copied case {case_id} to another environment"`.

- [ ] **Step 1: Write the failing tests** (appended to `autorun_env_copy.rs`)

```rust
#[test] fn copying_a_case_brings_its_components_area_and_recipe() { /* plan lists them; copy writes all four; the script has no suspected_defect */ }
#[test] fn copying_a_case_never_overwrites_the_targets_component_or_area() { /* differing 'Login form' -> components_kept, target file unchanged */ }
#[test] fn nested_components_are_brought_too() { /* A uses B -> both added */ }
#[test] fn a_case_already_scripted_in_the_target_asks_first() { /* replace=false -> Err(CASE_COPY_NEEDS_CONFIRM), nothing written */ }
#[test] fn a_missing_account_and_missing_api_data_are_named_not_blocking() { /* missing_account Some("admin"); missing_api ["fx-1","flow-2"]; copy still Ok */ }
#[test] fn a_case_with_no_script_is_refused() { /* Err(NO_SCRIPT_TO_COPY) */ }
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `cd src-tauri && cargo test --test suite autorun_env_copy::`. Expected: compile errors.
- [ ] **Step 3: Implement the case plan and copy, and the commands.** Register the commands.
- [ ] **Step 4: Regenerate the bindings.** Run: `cd src-tauri && cargo test --test bindings`. Expected: PASS.
- [ ] **Step 5: Run the module.** Expected: PASS.
- [ ] **Step 6: Commit** with the message `feat(v2): copy one case's script to another environment`.

---

### Task 7: The Playwright export environment

**Files:**
- Modify: `src-tauri/src/environments.rs`:
  - `EnvFile` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub playwright_export: Option<String>`
  - Default creation sets it
  - `remove_env` refuses the export environment
- Modify: `src-tauri/src/commands/pw_export.rs`:
  - `ensure_export_env` is checked in `pw_export_preview`, `pw_export_save_map` and `pw_export_write`
  - two new commands
- Modify: `src-tauri/src/commands/pw_repo.rs`:
  - `ensure_export_env` is checked in `pw_sync_states`, `update` (which serves `pw_sync_update`), `pw_sync_add_new` and `pw_handoff`
  - `pw_repo_rename_folder_check`/`pw_repo_rename_folder` pass the export environment's root as `maps`, under `allow_writes_to`
- Modify: `src-tauri/src/lib.rs` (after the layout start-up: `environments::adopt_export_env(&shared)`, which never makes a file)
- Modify: `src-tauri/src/commands/environments.rs` (`EnvListView` gains `pub playwright_export: String`)
- Create: `src-tauri/tests/suite/pw_export_env.rs`, plus its `mod` line
- Regenerate: `src/bindings.ts`

**Interfaces:**
- Consumes: Task 1's `env_root_beside` and `is_export_record`; Task 3's `allow_writes_to`.
- Produces:
  - `pub fn export_env(root: &Path) -> Result<Environment, String>`: an unset or unknown id adopts the active one and persists it.
  - `pub fn adopt_export_env(root: &Path) -> Result<bool, String>`: only when `environments.json` exists.
  - `pub fn set_export_env(root: &Path, id: &str) -> Result<EnvFile, String>`: moves every `projects/*.pwexport.json` from the old export environment's root to the new one's. The new one's same-named file is replaced and the replacement is logged.
  - `pub const REMOVE_EXPORT_ENV: &str = "this is the Playwright export environment - name another one on the Playwright tab first";`
  - `pub fn not_export_env(name: &str) -> String` → `format!("Export and Sync run only in {name}, the Playwright export environment - switch to {name} on the AI Bridge tab first")`
  - `#[derive(serde::Serialize, specta::Type)] pub struct PwExportEnv { pub id: String, pub name: String, pub is_active: bool }`
  - `#[tauri::command] pub fn pw_export_env(app: tauri::AppHandle) -> Result<PwExportEnv, String>`
  - `#[tauri::command] pub fn pw_set_export_env(app: tauri::AppHandle, id: String) -> Result<PwExportEnv, String>`

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn the_export_environment_starts_as_the_active_one() { /* old file without the field -> export_env == active, now persisted */ }
#[test] fn export_is_refused_outside_the_export_environment() { /* the pure check behind ensure_export_env returns not_export_env("Automation") */ }
#[test] fn removing_the_export_environment_is_refused() { /* Err(REMOVE_EXPORT_ENV) */ }
#[test] fn changing_the_export_environment_moves_its_records() { /* a/projects/p.pwexport.json -> b/projects/p.pwexport.json, gone from a */ }
#[test] fn renaming_a_repo_folder_moves_placements_in_the_export_environment_while_another_is_active() { }
#[test] fn adopt_export_env_makes_no_environments_file() { }
```

(Expose the pure check as `pub fn check_export_env(root: &Path) -> Result<(), String>` in `crate::environments`, so the command wrappers stay thin.)

- [ ] **Step 2: Run them and confirm they fail.** Run: `cd src-tauri && cargo test --test suite pw_export_env::`. Expected: compile errors.
- [ ] **Step 3: Implement the field, the functions and the refusals in the seven commands.** Add the two new commands and the start-up adoption.
- [ ] **Step 4: Regenerate the bindings.** Then run `cargo test --test suite pw_export_env::`, then `cargo test --test suite environments::`. Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(v2): Playwright export runs only in the export environment`.

---

### Task 8: Backup and restore across the new layout

**Files:**
- Modify: `src-tauri/src/backup.rs`:
  - `excluded` adds: `autorun/removed-envs`, `autorun/.envs-staging-*`, `autorun/envs/.copying-*` and `autorun/envs/<any>/shots`, ASCII case-insensitive with either separator
  - `collect_files` skips the legacy originals (`autorun/<PER_ENV item>`) when `data_dir/autorun/envs` is a directory
  - `safe_relative_path` refuses `autorun/envs/<x>/…` unless `roots::valid_folder_id(x)`
  - `restore_files` fans out a backup from before the update
- Modify: `src-tauri/src/commands/misc.rs:436-460` (`import_app_backup`):
  - refuses with `refuse_switch(false)`'s sentence while something runs
  - holds `roots::switch_lock().write()` around `restore_files`
  - calls `store::reselect_from_disk()` after it
- Modify: `src-tauri/tests/suite/backup.rs` (append the tests)

**Interfaces:**
- Consumes: Task 1's `PER_ENV_ITEMS` and `valid_folder_id`; Task 2's `fan_out`; Task 4's `retire`; Task 7's export record rule (inside `fan_out`); `store::reselect_from_disk`.
- Produces:
  - `pub fn is_pre_update(files: &[BackupFile]) -> bool`: no entry under `autorun/envs/`, and at least one under `autorun/<PER_ENV item>/`.
  - In `restore_files`, when the machine has `autorun/envs` and `is_pre_update` holds:
    1. Each per-environment entry is written under `autorun/.import-<ms>/` instead of its own path.
    2. After the loop, and after `environments.json` was restored, each environment in the restored list is retired with reason `"replaced by an import"` (when it has a folder).
    3. `fan_out(staging, envs_dir, ids, Some(active))` runs. With no environments, it fills `_seed`.
    4. The staging folder is removed.
  - The return count is unchanged in meaning (files restored).

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_backup_carries_every_environment_and_the_seed_but_not_shots_or_the_removed_area() { }
#[test] fn a_backup_leaves_out_the_legacy_originals() { /* autorun/scripts/case-1.json on an updated machine -> not in collect_files */ }
#[test] fn a_pre_update_backup_fills_every_environment() {
    // machine: envs/a, envs/b; doc: autorun/environments.json (a active, b), autorun/scripts/case-5.json,
    // autorun/runs/run-1.json -> envs/a and envs/b have scripts/case-5.json; only envs/a has runs/run-1.json;
    // no autorun/scripts/case-5.json written at the flat path; removed_folders lists a and b as "replaced by an import"
}
#[test] fn a_post_update_backup_restores_the_folders_as_they_are() { }
#[test] fn an_invalid_environment_folder_in_a_backup_is_skipped() { /* autorun/envs/..evil/x.json and autorun/envs/NOPE/x.json */ }
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `cd src-tauri && cargo test --test suite backup::`. Expected: FAIL.
- [ ] **Step 3: Implement the exclusions, the skip, the path check and the fan-out.** Add the import command's guards and the reselect.
- [ ] **Step 4: Run `backup::`.** Expected: PASS. Then run the whole Rust suite once (`cargo test --tests`). Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(v2): backups carry each environment's Auto Run data`.

---

### Task 9: UI: which environment is on screen, reload on a switch, the export environment

**Files:**
- Modify: `src/lib/environments.ts`:
  - `export function isEnvironmentScopedKey(key: readonly unknown[]): boolean`
  - `forgetEnvironmentData` resets every query that `isEnvironmentScopedKey` accepts
- Create: `src/hooks/useEnvironmentChangeSync.ts` (listens to `events.autorunEnvironmentChanged`, invalidates `envKeys.list` and calls `forgetEnvironmentData`), mounted in `src/App.tsx` beside `useAutoRunDataSync`
- Create: `src/hooks/useCloseOnEnvironmentChange.ts`: `export function useCloseOnEnvironmentChange(onClose: () => void): void`. It reads `subscribeActiveEnv`/`activeEnvIdSnapshot`, remembers the id at mount, and on a change calls `onClose()` and `toast("The environment changed, so this was closed without saving.")`.
- Use it in: `src/screens/AutoRun/ScriptEditor.tsx`, `AreasDialog.tsx`, `ComponentsDialog.tsx`, `RecipeEditor.tsx`, `ExecutionOrderDialog.tsx`, `DiscoveryDialog.tsx`, `src/screens/Playwright/PlaywrightExportDialog.tsx` and `src/screens/ApiTemplates/ImportTemplates.tsx`
- Create: `src/components/EnvironmentLabel.tsx`: `export default function EnvironmentLabel({ prefix }: { prefix: string })`. It renders `"{prefix} {active name}"`, uses the `EnvironmentPill` styling, has `aria-label="Environment"`, and renders nothing until the list is known. It is shown on Auto Run (`prefix="Scripts for"`, in the Test cases toolbar row at `AutoRun/index.tsx:855`) and on API Templates (`prefix="Templates for"`, in its header), whatever the number of environments.
- Modify: `src/screens/Playwright/index.tsx` (header at line 202):
  - "Exports from <name>", with a `Change…` select of the environments
  - choosing one opens a confirm: "Make {name} the Playwright export environment? The next Sync compares {name}'s scripts with the repo, so cases that differ show as out of date. Nothing is written to the repo until you sync."
  - confirming calls `commands.pwSetExportEnv`
- Modify: `src/screens/Playwright/Sync.tsx`: a refusal from `pwSyncStates` is shown in place of the lists, as its own sentence (no retry button)
- Tests:
  - `src/lib/environments.test.ts`
  - `src/hooks/useCloseOnEnvironmentChange.test.tsx` (new)
  - `src/screens/AutoRun/ScriptEditor.test.tsx`
  - `src/screens/Playwright/index.test.tsx`
  - `src/screens/Playwright/Sync.test.tsx`
  - `src/screens/AutoRun/index.test.tsx`
  - `src/screens/ApiTemplates/index.test.tsx`

**Interfaces:**
- Consumes: the bindings from Tasks 3 and 7: `events.autorunEnvironmentChanged`, `commands.pwExportEnv()`, `commands.pwSetExportEnv(id)`, `EnvListView.playwright_export`.
- Produces: `isEnvironmentScopedKey`, `useCloseOnEnvironmentChange` and `EnvironmentLabel`, all used by Task 10.

`isEnvironmentScopedKey` accepts a key whose first element is a string that starts with `"autorun-"`, except `"autorun-agents"` and `"autorun-discovery-active"`, which are about browsers and not data. It also accepts keys whose first element is one of: `envKeys.apiTemplates[0]`, `FIXTURES_KEY`, the flows key, `"pw-sync"`, `"pw-export"`, `"pw-export-env"`. Before writing the set, grep `queryKey:` under `src/screens/AutoRun`, `src/screens/ApiTemplates`, `src/screens/Playwright` and `src/hooks`, and classify every key found. `"test-files"` and `"db-databases"` stay out (they are shared).

- [ ] **Step 1: Write the failing tests**

```ts
// environments.test.ts
it("treats Auto Run, API template and Playwright export queries as per-environment", () => {
  for (const k of [["autorun-script", 5], ["autorun-nav", "o", "p"], ["autorun-runs"], [FIXTURES_KEY], ["api-templates"], ["pw-sync", "c", "o", "p"]])
    expect(isEnvironmentScopedKey(k)).toBe(true);
  for (const k of [["test-files", "o", "p"], ["db-databases"], ["environments"], ["autorun-agents"]])
    expect(isEnvironmentScopedKey(k)).toBe(false);
});
// useCloseOnEnvironmentChange.test.tsx
it("closes once when the active environment changes, never on mount", ...);
// ScriptEditor.test.tsx
it("script_editor_closes_when_the_environment_changes", ...); // setActiveEnvId("env-0000000b") -> onClose called, commands.autoRunSaveScript not called
// AutoRun/index.test.tsx and ApiTemplates/index.test.tsx
it("names the environment whose scripts are on screen", ...); // getByLabelText("Environment") has "Scripts for Staging"
// Playwright/index.test.tsx
it("shows the export environment and asks before changing it", ...);
// Sync.test.tsx
it("shows the export-environment refusal in place of the lists", ...);
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `npx vitest run src/lib/environments.test.ts src/hooks/useCloseOnEnvironmentChange.test.tsx src/screens/AutoRun/ScriptEditor.test.tsx src/screens/Playwright src/screens/AutoRun/index.test.tsx src/screens/ApiTemplates/index.test.tsx`. Expected: FAIL.
- [ ] **Step 3: Implement** the key predicate, the two hooks, the label, the Playwright header and Sync's refusal.
- [ ] **Step 4: Run the same tests, then `npx tsc --noEmit`, then `npm test`.** Expected: PASS, including `src/ui-consistency.test.ts`. One `App.test.tsx` failure that passes on a re-run is the documented flake.
- [ ] **Step 5: Commit** with the message `feat(v2): screens name their environment and reload on a switch`.

---

### Task 10: UI: copy dialogs, and the removal and import wording

**Files:**
- Create: `src/components/CopyAutoRunDialog.tsx`: `export default function CopyAutoRunDialog({ target, onClose }: { target: EnvView; onClose: () => void })`
- Modify: `src/components/EnvironmentsDialog.tsx`:
  - each environment row gets a `Copy Auto Run data…` button (`aria-label={`Copy Auto Run data into ${env.name}`}`)
  - the remove confirmation reads "Remove {name}? Its accounts and saved sign-ins go now. Its Auto Run scripts, areas and templates are kept for 30 days and can be copied into another environment from Copy Auto Run data…"
  - removing the export environment shows Rust's refusal
- Create: `src/screens/AutoRun/CopyCaseDialog.tsx`: `export default function CopyCaseDialog({ caseId, org, project, onClose }: { caseId: number; org: string; project: string; onClose: () => void })`
- Modify: `src/screens/AutoRun/ScriptEditor.tsx`: a `Copy to environment…` button in the footer beside Save, shown only when there are two or more environments and the case has a saved script
- Modify: `src/lib/actionIcons.ts`: add `IconCopyToEnvironment` if no existing name says "copy to another place". `IconCopyToSuite` is not reused: icons are named for what they do.
- Modify: `src/screens/Settings.tsx:370-480`: the import confirmation adds "Importing replaces this computer's Auto Run data in every environment with the backup's."
- Tests: `src/components/CopyAutoRunDialog.test.tsx` (new), `src/screens/AutoRun/CopyCaseDialog.test.tsx` (new), `src/components/EnvironmentsDialog.test.tsx`, `src/screens/AutoRun/ScriptEditor.test.tsx`

**Interfaces:**
- Consumes: the bindings from Tasks 5 and 6:
  - `commands.envCopyChoices(targetId)` gives `CopyChoices`
  - `commands.envCopyAutorun(targetId, sourceKey, replace)` gives `CopyDone`
  - `commands.autoRunCopyCasePlan(org, project, caseId, targetId)` and `commands.autoRunCopyCase(org, project, caseId, targetId, replace)` give `CaseCopyPlan`
  - Task 9's `useCloseOnEnvironmentChange`
- Produces: nothing for later tasks.

Dialog behaviour (the decisions):
- **CopyAutoRunDialog.**
  - It shows a source select. Removed sources read "{name} (removed, {days_left} days left)".
  - When `target_empty` is false, it shows the line "This replaces {target}'s scripts, areas, components, quirks and API templates with {source}'s. {target}'s current ones are kept for 30 days and can be copied back from this list." and the button reads `Replace`. Otherwise the button reads `Copy`, and `replace` is sent as `!target_empty`.
  - On success: `toast.success("Copied Auto Run data into {target}")` and close. A Rust refusal shows as `toast.error(message)`.
- **CopyCaseDialog.**
  - It shows a target select (the other environments).
  - On choosing a target, it calls the plan and lists, in this order:
    - "Brings: …" (components, area, the sign-in)
    - "Uses {target}'s own: …" (components and area kept)
    - "Add the account {key} in {target}'s Accounts before running."
    - "{target} lacks the API data this case needs: …"
    - "This case uses its module's default area: check {target} has it."
  - When `target_has_script` is true, the button reads `Replace {target}'s script`; otherwise it reads `Copy`.
- Both dialogs use the shared modal portal (the app's modal pattern, not a full-screen overlay; see the CLAUDE.md jsdom note) and `useCloseOnEnvironmentChange`.

- [ ] **Step 1: Write the failing tests**

```ts
it("offers removed environments as sources, labelled removed", ...);
it("asks before replacing a non-empty environment and sends replace=true", ...);
it("lists what a case copy brings, keeps and lacks before copying", ...);
it("asks before replacing the target's script", ...);
it("says the Auto Run data of a removed environment is kept for 30 days", ...); // EnvironmentsDialog
it("offers Copy to environment only with two or more environments", ...);     // ScriptEditor
```

- [ ] **Step 2: Run them and confirm they fail.** Run: `npx vitest run src/components/CopyAutoRunDialog.test.tsx src/screens/AutoRun/CopyCaseDialog.test.tsx src/components/EnvironmentsDialog.test.tsx src/screens/AutoRun/ScriptEditor.test.tsx`. Expected: FAIL.
- [ ] **Step 3: Implement the two dialogs, the buttons, the icon and the wording.**
- [ ] **Step 4: Run the same tests, then `npx tsc --noEmit`, then `npm test`.** Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(v2): copy Auto Run data and single scripts between environments`.

---

### Task 11: The assistants' guide and the How To Use text

**Files:**
- Modify: `src-tauri/src/autorun/guide.rs:1058-1100` (the "## Environments" section) and `guide.rs:2774-2805` (`active_environment_section`)
- Modify: `src-tauri/src/autorun/recipe.rs:724` (`NO_SITE_ADDRESS`)
- Modify: `src-tauri/tests/suite/autorun_guide.rs` and `src-tauri/tests/suite/autorun_recipe.rs:473` (the exact-text assertion)
- Modify: `docs-site/src/content/ai-tools.ts` (`environment-pick` and `edit-environments`), `docs-site/src/content/auto-run.ts` (a tip, and a how-to "Copy a script to another environment"), `docs-site/src/content/api-templates.ts` (a tip) and `docs-site/src/content/playwright.ts` (a tip on the export environment)
- Rebuild: `src-tauri/help/` with `npm run docs:build`
- Modify: `docs/backlog.html` (the open live checks below, under the per-environment heading)

**Interfaces:**
- Consumes: the wording of Tasks 5, 6, 7 and 10 (button names: `Copy Auto Run data…`, `Copy to environment…`, `Exports from`, `Change…`).
- Produces: nothing.

Exact copy (the decisions):
- Append to the "## Environments" section as its own paragraph:

  > Each environment has its own Auto Run scripts, areas, components, quirks, discovery map, saved setups and API templates, fixtures and flows, because each can run a different version of the app. Everything you read and save belongs to the active environment only: a script you save now is the active environment's, and no other environment sees it. If the environment is switched while you are working, a save you started is refused; read again and carry on in the environment now active. When the active environment lacks a script, an area or a component that another environment has, do not rewrite it from memory: ask the person to copy it in the app (Edit environments, Copy Auto Run data…, for everything; Copy to environment… on the case's script, for one case), then read it here and change only what differs. You cannot copy between environments yourself.

- `active_environment_section` appends ` Its scripts, areas, components and API templates are its own.` after the passwords sentence.
- `NO_SITE_ADDRESS` becomes `"set the site address first - Auto Run, Setup, Site address - or copy this environment's Auto Run data from another one in Edit environments on the AI Bridge tab"`. It is still a prefix match for the two `contains("set the site address first")` tests.
- How To Use text:
  - `environment-pick` adds "Each environment also keeps its own Auto Run scripts, areas, components and API templates, so switching shows that environment's."
  - `edit-environments` adds "**Copy Auto Run data…** brings another environment's scripts, areas, components and API templates into this one; a removed environment's stay available there for 30 days."
  - An Auto Run how-to (3 steps) covers Copy to environment….
  - The Playwright tip: "Export and Sync run in one environment, shown at the top as **Exports from**. Change it there; in any other environment they are refused and say which one to switch to."

Screenshots are retaken by the controller after this task. The shots whose frames change:
- `auto-run-cases`, `auto-run-selected` and `auto-run-setup` (the "Scripts for" label in the toolbar row)
- `auto-run-script` (the Copy to environment… button)
- `api-templates-list`, `api-templates-details`, `api-templates-flows` and `api-templates-fixtures` (the "Templates for" label)

Confirm with `npm run docs:shots -- --dry-run`, then run `npm run docs:shots -- --only <id>` for each. Playwright has no shots. No new shot id is added.

- [ ] **Step 1: Write the failing tests.** In `autorun_guide.rs`, add `guide_says_each_environment_has_its_own_scripts` (the full guide contains "belongs to the active environment only" and "You cannot copy between environments yourself") and `active_environment_section_says_its_scripts_are_its_own`. Update the `NO_SITE_ADDRESS` assertion to the new text.
- [ ] **Step 2: Run them and confirm they fail.** Run: `cd src-tauri && cargo test --test suite autorun_guide::`, then `cargo test --test suite autorun_recipe::`. Expected: FAIL.
- [ ] **Step 3: Write the guide, the constant and the docs-site text.**
- [ ] **Step 4: Run** `cargo test --test suite autorun_guide::`, then `cargo test --test suite autorun_recipe::`, then `cargo test --test suite api_templates_runner::`, then `cargo test --test suite autorun_runner::`, then `npm run docs:build`, then `npx vitest run docs-site`. Expected: PASS, including `guard.test.ts` and `built.test.ts`.
- [ ] **Step 5: Add the owed live checks to `docs/backlog.html`:**
  - first start after the update on the owner's machine: two environments, scripts in both, history only in the active one, launch time noted
  - a switch with Auto Run open reloads the list
  - an assistant save during a switch is refused
  - Copy Auto Run data from a removed environment
  - Copy to environment for a case with a component
  - Export refused in Dev, Sync works in Automation
  - a backup from before the update imported
- [ ] **Step 6: Commit** with the message `docs(v2): the guide and How To Use explain per-environment Auto Run data`, including `src-tauri/help/` and `docs/backlog.html`.

---

## Gap rulings (decisions this plan made where the spec was silent)

1. **History is not copied into every environment at the update.**
   - Runs, shots, downloads, reports, replays, explore changes and captures go to the active environment only, hard-linked when possible.
   - Reason: these are results of runs, results must never mix, and the owner's `shots/` alone is 376 MB.
   - Scripts and everything that describes the app are copied into every environment, as the spec says.
2. **The export records** (`*.pwexport.json`) go to the export environment only at the update. They are never copied by "Copy Auto Run data…". Changing the export environment moves them to the new one.
3. **`test-made.json` stays shared.**
   - Each entry already names its environment, and Clean up previews any environment.
   - The preview reads the chosen environment's own delete templates.
4. **A whole-environment copy brings only what describes the app.** That means scripts, deleted scripts, run order, project files, templates, fixtures and flows. The target's run history and export records stay with it. Its replaced contents go to the removed area for 30 days, which is the backup folder the spec asks for, and they are offered as a removed source.
5. **"Saved setups it names" in a case copy.**
   - Scripts name no saved setup: saved setups are explore's.
   - A script's own `setup` names an API fixture, and its preconditions name API flows. These are listed in the confirmation when the target lacks them, and they are not copied.
   - A blank area (the module's default) is not copied and is named in the confirmation.
6. **In-flight protection is at the store, not in the bridge.**
   - Every per-environment write checks that the root it was handed is still the active one, or an explicitly allowed copy target. This covers assistant calls and app commands alike.
   - Store events for a non-active root are not forwarded to the window.
7. **A switch is also refused while an Auto Run browser holds an account lease** (`lease::a_browser_is_held`). Discovery and explore sessions are already refused through the slot locks.
8. **A failed update keeps the flat layout for that session** (`store::set_root(shared)`) and retries at the next start. In that session switching changes nothing on disk.
9. **The export environment is stored in `environments.json`** (`playwright_export`), beside the list it must stay consistent with. It is changed from the Playwright tab only.
10. **Importing a backup from before the update** keeps each replaced environment folder in the removed area as "replaced by an import". The import confirmation always carries one sentence about replacing Auto Run data in every environment, with no separate inspect step.
11. **`commands::autorun::root(app)`** no longer falls back to a path derived from the app handle. Before setup it answers `NO_DATA_DIRECTORY`.
12. **The runner window** (the manual test runner at `#runner`) reads no Auto Run data and is unchanged.
