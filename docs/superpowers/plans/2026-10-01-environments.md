# Environments + Areas Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One active environment switches the website, database and accounts Auto Run, API templates and the assistant use; the assistant proposes test logins from the environment's database; and Auto Run's one-path-per-module becomes named areas.

**Architecture:** A new Rust module `environments` owns the environment list and the active one (one JSON file under the Auto Run data root, default passwords in Credential Manager). Everything that signs in already goes through `signin::prepare`, which becomes the single place the *effective* recipe (environment address over recipe address) and the environment's account are produced; accounts and sessions resolve their files through the active environment id. The webview's existing database choice (`saveSelectedDb`) is the one database setting and is driven by switching. Areas extend `nav.rs`'s `ModulePath` with a name; scripts gain an optional `area`.

**Tech Stack:** Rust (Tauri 2, serde, specta/tauri-specta), React 19 + TypeScript, vitest + Testing Library, integration tests in `src-tauri/tests/suite/`.

**Spec:** `docs/superpowers/specs/2026-10-01-environments-design.md`

## Global Constraints

- An environment is identified by its id, never by its address. Two environments may share an address. Nothing keys per-environment data by origin.
- The database write rule in `db/guard.rs` (`_devlogin` users only) is unchanged.
- Default passwords live only in the `SecretStore` (`db::credentials::SecretStore`, `DbSecrets` state), target `env-default-password:<env id>`; never in a file, the webview, a log or a command's output. The webview only sees `has_default_password: bool`.
- Passwords never reach `applog`, run files, the activity log or events (`signin::redact` as today). The ONLY exception: `get_accounts` returns passwords for an environment with `test_environment: true`.
- Auto Run and API Templates stay gated exactly as today (`ai_tools::autorun_offered()`, `DEV_ONLY_TOOLS` + its TS mirror in `src/lib/mcpTools.ts`). The Environment card is shown wherever the Company database card is.
- No HTTP DELETE to Azure DevOps anywhere; removals are local file deletes.
- Never name the secret unlock. No changelog, help-site or README changes in this branch (the owner's session does docs after).
- Rust tests only in `src-tauri/tests/suite/` (new module => `mod` line in `suite/main.rs`). Never hand-edit `src/bindings.ts`; regenerate with `cd src-tauri && cargo test --test bindings`.
- Colours from Tailwind tokens; icons from `src/lib/actionIcons.ts`; `src/ui-consistency.test.ts` and `src/a11y.test.tsx` pass unchanged.
- One test command at a time. Commits via Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Branch `feat/environments`, created from `feat/learning-quirks`.
- Copy fixed by the spec: Test environment warning `The AI assistant can read the full logins of this environment's accounts. Use only for test environments.`; empty address `Using the sign-in recipe's address`; Auto Run header `Environment <name> - <host>`; unrecorded area `the area "<name>" is not recorded - record it in Auto Run, Areas`; proposals heading `Proposed by the assistant (N)`; no default password `no default password set - type one`.

## Review Focus

1. **Switching while something runs.** A switch during a run, recording, check or Try would mix two environments' accounts and sessions mid-run. Expected: refused with a sentence. Pinned in Task 1.
2. **An environment whose database no longer exists** (a saved custom login removed or reset). Expected: switching still works, sets no database, and says the database is not set up any more. Pinned in Task 5.
3. **Same address, two environments.** Expected: separate saved sessions; one is never restored into the other. Pinned in Task 2.
4. **Area names that differ only in case**, including an old paths file whose modules differ only in case. Expected: names are unique case-insensitively; on migration the first wins and the rest are logged. Pinned in Task 8.
5. **A proposed account whose key already exists** in the environment. Expected: "Add selected" asks to replace, per key; nothing is overwritten silently. Pinned in Task 6.

---

### Task 1: The environment model and store

**Files:**
- Create: `src-tauri/src/environments.rs`, `src-tauri/src/commands/environments.rs`
- Modify: `src-tauri/src/lib.rs` (mod + specta builder registration), `src-tauri/src/commands/mod.rs`
- Test: `src-tauri/tests/suite/environments.rs` (+ `mod environments;` in `suite/main.rs`)

**Interfaces:**
- Produces:
  - `pub struct Environment { pub id: String, pub name: String, pub start_url: String, pub allowed_origins: Vec<String>, pub db_id: String, pub test_environment: bool }` (serde, `test_environment` default false)
  - `pub struct EnvFile { pub active: String, pub environments: Vec<Environment> }`
  - `pub fn load_or_init(root: &Path, current_db: Option<&str>) -> Result<EnvFile, String>` - first call creates `Default` (empty address, `db_id` = `current_db` or the first `DB_PRESETS` id), copies `root/accounts.json` to the Default environment's accounts file (Task 2's path) if present, saves, returns.
  - `pub fn active(root: &Path) -> Result<Environment, String>`; `pub fn active_id(root: &Path) -> Result<String, String>`
  - `pub fn validate(file: &EnvFile, known_db_ids: &[String]) -> Result<(), String>`
  - `pub fn save_env(root, env: Environment, known_db_ids) -> Result<EnvFile, String>` (add when the id is empty: generate one; else replace)
  - `pub fn remove_env(root, id) -> Result<EnvFile, String>` (refused for active and last; deletes that env's accounts/sessions/proposals files)
  - `pub fn set_active(root, id) -> Result<EnvFile, String>`
  - `pub fn password_target(id: &str) -> String` = `env-default-password:<id>`
  - Commands (specta): `env_list(current_db: Option<String>) -> EnvListView`, `env_save(env: EnvInput) -> EnvListView`, `env_remove(id) -> EnvListView`, `env_set_active(id) -> EnvListView`, `env_set_default_password(id, password)`, `env_clear_default_password(id)`. `EnvView` = Environment fields + `has_default_password: bool`; `EnvListView { active: String, environments: Vec<EnvView> }`.

- [ ] **Step 1: Write failing tests** in `tests/suite/environments.rs`:
  - `first_load_creates_default_and_moves_accounts`: temp root with `accounts.json` holding one account; `load_or_init(root, Some("qa-read"))` → one env named `Default`, `db_id == "qa-read"`, `start_url == ""`, active = its id; the account is readable through Task 2's `load_accounts` once that exists (in this task assert the copied file exists at `root/accounts/<id>.json`).
  - `two_environments_may_share_an_address`: two envs with `start_url = "https://hr.example.com"` validate OK.
  - `names_are_unique_ignoring_case`: `Dev` + `dev` → Err containing `already`.
  - `bad_address_and_origins_are_refused`: `ftp://x` → Err; allowed origin `https://x/path` → Err (reuse `autorun::recipe::origin_of` and the recipe's bare-origin rule).
  - `unknown_database_is_refused`: `db_id` not in `known_db_ids` → Err.
  - `active_and_last_cannot_be_removed`.
  - `removing_deletes_only_that_environments_files`.
  - `default_password_never_in_the_file`: set a password through the command layer with `MemoryStore`; the environments JSON on disk does not contain it; `env_list` output has `has_default_password: true` and no password field.
  - `switching_is_refused_while_recording_or_running` (Review Focus 1): with the recorder claimed (`commands::autorun_record::claim()`), `env_set_active` returns Err naming that something is being recorded or run.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite environments::` - expect compile failure (module missing).
- [ ] **Step 3: Implement** `environments.rs` (pure validation + atomic JSON at `root/environments.json`; ids `env-<8 lowercase hex>` from a hash of name+time; never reused) and the commands (`root` from `commands::autorun::root(&app)`, `known_db_ids` from `db::credentials::databases(store)`, refusal via the existing `refuse_while_recording` / `replay_is_running` checks).
- [ ] **Step 4: Run** the same command - PASS; then `cargo test --test bindings`.
- [ ] **Step 5: Commit** `feat(v2): environments - a named website, database and default password, one active`.

### Task 2: Accounts and saved sessions per environment

**Files:**
- Modify: `src-tauri/src/autorun/accounts.rs` (`accounts_path`, `session_path`), `src-tauri/src/autorun/sessions.rs`
- Test: `src-tauri/tests/suite/environments.rs` (extend)

**Interfaces:**
- Consumes: `environments::active_id(root)`.
- Produces: `accounts_path(root)` → `root/accounts/<active id>.json`; `session_path(root, key)` → `root/sessions/<active id>/<key>.json`. Public signatures of `load_accounts`, `find_account`, `save_accounts`, `save_session`, `load_fresh_session`, `forget_session` are UNCHANGED (callers keep working). Add `pub fn accounts_path_for(root, env_id) -> PathBuf` for Task 4 and removal.

- [ ] **Step 1: Failing tests:**
  - `accounts_follow_the_active_environment`: save accounts in env A, switch to B → `load_accounts` empty; save in B, switch back → A's list.
  - `same_address_different_sessions` (Review Focus 3): A and B share `start_url`; `save_session(root, "hr.sup", s)` while A active; switch to B → `load_fresh_session(root, "hr.sup", 480, now)` is `None`; switch back → `Some`.
  - `legacy_session_files_are_not_read`: a file at the old `root/sessions/hr.sup.json` is ignored.
- [ ] **Step 2: Run** `cargo test --test suite environments::` - FAIL.
- [ ] **Step 3: Implement** the path changes (create dirs on write; an Err from `active_id` propagates where the function returns Result, and makes `load_fresh_session` return None).
- [ ] **Step 4: Run** - PASS; then `cargo test --test suite autorun` to catch callers (fix any test that assumed the old paths by initialising an environment in its temp root).
- [ ] **Step 5: Commit** `feat(v2): accounts and saved sign-in sessions are kept per environment`.

### Task 3: The effective recipe, and the environment on records

**Files:**
- Modify: `src-tauri/src/autorun/recipe.rs` (add `effective_recipe`), `src-tauri/src/autorun/signin.rs` (`prepare`), `src-tauri/src/autorun/runner.rs:99`, `src-tauri/src/autorun/replay.rs:359`, `src-tauri/src/commands/api_templates.rs:54`, `src-tauri/src/ai_bridge.rs:437`, `src-tauri/src/autorun/mod.rs` (`LocalRun`), `src-tauri/src/api_templates/mod.rs` (`Proven`) + where proofs/runs are written
- Test: `src-tauri/tests/suite/environments.rs` (extend)

**Interfaces:**
- Produces:
  - `pub fn effective_recipe(recipe: &SignInRecipe, env: &Environment) -> SignInRecipe` - when `env.start_url` is non-empty, it replaces `start_url` and `allowed_origins`; otherwise the recipe is returned unchanged.
  - `pub fn load_effective_recipe(root, org, project) -> Result<Option<SignInRecipe>, String>` in `recipe.rs` (load + active env + `effective_recipe`). `signin::prepare` and the four direct callers above use it. `autorun_record_signin.rs:188` and `auto_run_load_recipe` keep the RAW recipe (they edit it).
  - `LocalRun.environment: Option<String>` and `Proven.environment: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), set to the active environment's NAME when a run is created / a template is proven.

- [ ] **Step 1: Failing tests:**
  - `empty_address_keeps_the_recipe`, `address_replaces_address_and_allowed_sites`.
  - `prepare_signs_in_at_the_environment_address`: recipe `https://a`, env `https://b` → `prepare(...)`.0.start_url == `https://b`.
  - `only_the_helper_reads_the_recipe_address`: source scan of `src-tauri/src` - `load_recipe(` appears only in `recipe.rs`, `commands/autorun.rs` (`auto_run_load_recipe`), `commands/autorun_record_signin.rs`; any other file using it fails the test with its path.
  - `old_runs_and_proofs_still_load`: a LocalRun JSON and an ApiTemplate JSON without `environment` deserialize; serialising them back has no `environment` key.
- [ ] **Step 2: Run** `cargo test --test suite environments::` - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** - PASS; then `cargo test --test suite autorun` and `cargo test --test suite api_template`; regenerate bindings.
- [ ] **Step 5: Commit** `feat(v2): sign-in, navigation and API templates use the active environment's address`.

### Task 4: Proposed accounts, and `get_accounts`

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (routes `POST /accounts-propose`, `GET /accounts`), `src-tauri/src/mcp.rs` (tools `propose_accounts`, `get_accounts`; add both to `DEV_ONLY_TOOLS`), `src/lib/mcpTools.ts` (TS mirror, Auto Run row), `src-tauri/src/environments.rs` (proposals store), `src-tauri/src/commands/environments.rs` (`env_proposals() -> Vec<ProposedAccount>`, `env_dismiss_proposals()`, `env_add_proposals(picks: Vec<AccountInput>, replace: Vec<String>) -> Vec<String>`)
- Test: `src-tauri/tests/suite/environments.rs`, the existing MCP tool-list test module (find it: it asserts `DEV_ONLY_TOOLS`)

**Interfaces:**
- Produces: `pub struct ProposedAccount { pub key: String, pub label: String, pub username: String, pub role: Option<String> }`, stored at `root/proposals/<env id>.json`, REPLACED on every `propose_accounts` call; max 100; keys by `accounts::valid_key`; usernames non-empty.
- `get_accounts` response: `{ environment: <name>, test_environment: bool, accounts: [{ key, label, username, password? }] }` - `password` present only when `test_environment`; otherwise a `note` string saying the environment is not marked as a test environment.
- `env_add_proposals`: each pick has a password (the webview sends the typed one, or empty = use the default password from the SecretStore; empty with no default → refused for that key); a key already in the accounts and not listed in `replace` → returned in the result list as needing confirmation and NOT written.

- [ ] **Step 1: Failing tests:**
  - `proposals_replace_and_validate` (bad key refused, 101 refused, second call replaces the first).
  - `get_accounts_hides_passwords_outside_test_environments` and `get_accounts_shows_them_in_a_test_environment`.
  - `get_accounts_is_not_logged`: after a `GET /accounts` in a test environment, the applog tail does not contain the password (use the suite's log-tail lock from `suite/serial.rs`).
  - `adding_an_existing_key_needs_confirmation` (Review Focus 5) and `empty_password_uses_the_default`.
  - Tool list: both tools listed only when the Auto Run row is on and Auto Run is offered.
- [ ] **Step 2: Run** `cargo test --test suite environments::` (+ the MCP tool-list module) - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** - PASS; regenerate bindings; `npx vitest run src/lib/mcpTools` if it has a test.
- [ ] **Step 5: Commit** `feat(v2): the assistant proposes test logins and reads them in test environments`.

### Task 5: Environment card, environments dialog, title-bar pill

**Files:**
- Create: `src/components/EnvironmentsDialog.tsx`, `src/components/EnvironmentPill.tsx`, `src/lib/environments.ts` (query keys + `switchEnvironment(id)`)
- Modify: `src/screens/AiBridge.tsx` (Environment card above "Company database"), `src/components/TitleBar.tsx` (pill beside `BetaPill`), `src/lib/dbServer.ts` usage
- Test: `src/components/EnvironmentsDialog.test.tsx`, `src/screens/AiBridge.test.tsx` (extend), `src/components/TitleBar.test.tsx` (extend or create)

**Interfaces:**
- Consumes: Task 1 commands (`commands.envList(selectedDbSnapshot())`, `envSave`, `envRemove`, `envSetActive`, `envSetDefaultPassword`, `envClearDefaultPassword`), `saveSelectedDb(id)` / `selectedDbSnapshot()` from `src/lib/dbServer.ts`, `commands.dbDatabases()`.
- Produces: `switchEnvironment(id: string): Promise<{ dbMissing: boolean }>` - calls `envSetActive`, then `saveSelectedDb(env.db_id)` when that id is in `dbDatabases()`, else `saveSelectedDb("")` and returns `dbMissing: true`.
- Behaviour: changing the database on the Company database card while environments exist calls `envSave({ ...active, db_id })` - one setting, not two.

- [ ] **Step 1: Failing vitest tests:**
  - AiBridge: the Environment card lists environments; choosing `QA` calls `env_set_active` and the database card shows QA's database.
  - AiBridge: choosing an environment whose `db_id` is not in `db_databases` (Review Focus 2) leaves no database chosen and shows `the database this environment uses is not set up any more - pick one`.
  - AiBridge: changing the database card calls `env_save` with the new `db_id`.
  - EnvironmentsDialog: add with an empty name shows the command's refusal inline; the Test environment switch shows the fixed warning; the default password input is type=password, Set calls `env_set_default_password`, the field is cleared after and the row reads "Default password set"; Remove on the active one shows the refusal; Remove asks first.
  - TitleBar: pill shows the active name only when there are 2+ environments.
- [ ] **Step 2: Run** `npx vitest run src/components/EnvironmentsDialog.test.tsx src/screens/AiBridge.test.tsx src/components/TitleBar` - FAIL.
- [ ] **Step 3: Implement** (Modal like the app's other dialogs; pill uses BetaPill's token classes).
- [ ] **Step 4: Run** the same + `npx vitest run src/ui-consistency.test.ts src/a11y.test.tsx` + `npx tsc --noEmit` - PASS.
- [ ] **Step 5: Commit** `feat(v2): switch environments from the AI Bridge tab; the title bar names the active one`.

### Task 6: Auto Run - site address, header, accounts with proposals, environment on runs and proofs

**Files:**
- Modify: `src/screens/AutoRun/SiteAddressDialog.tsx`, `src/screens/AutoRun/index.tsx` (header), `src/screens/AutoRun/AccountsDialog.tsx`, `src/screens/AutoRun/PastRuns.tsx`, `src/screens/ApiTemplates/TemplateRow.tsx` (proof line)
- Test: their existing test files (extend)

**Interfaces:**
- Consumes: Task 1 (`envList`, `envSave`), Task 3 (`LocalRun.environment`, `Proven.environment`), Task 4 (`envProposals`, `envDismissProposals`, `envAddProposals`).
- SiteAddressDialog now saves the ACTIVE environment's `start_url` / `allowed_origins` via `envSave`; empty shows `Using the sign-in recipe's address` and the recipe's address as placeholder.

- [ ] **Step 1: Failing tests:**
  - Header reads `Environment QA - qa.example.com` (host from the effective address: env's, else recipe's).
  - SiteAddressDialog save calls `env_save` with the new address, never `auto_run_save_recipe`.
  - AccountsDialog: `Proposed by the assistant (2)` lists proposals; password cell shows `default password` or `no default password set - type one`; Add selected with a key already present (Review Focus 5) asks `Replace hr.sup?` per key and calls `env_add_proposals` with `replace` only for confirmed keys; Dismiss calls `env_dismiss_proposals`.
  - PastRuns shows the run's environment name when present.
  - TemplateRow proof line shows `on QA` when `proven.environment` is set, else the host as today.
- [ ] **Step 2: Run** `npx vitest run src/screens/AutoRun src/screens/ApiTemplates` - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same + ui-consistency/a11y + `npx tsc --noEmit` - PASS.
- [ ] **Step 5: Commit** `feat(v2): Auto Run shows and edits the active environment, and lists the logins the assistant proposed`.

### Task 7: Guides

**Files:**
- Modify: `src-tauri/src/autorun/guide.rs` and the live-section assembly in `src-tauri/src/ai_bridge.rs` (`autorun_guide_with_quirks`), `src-tauri/src/api_templates/guide.rs` (+ its live assembly)
- Test: the existing guide tests (extend)

- [ ] **Step 1: Failing tests:** the Auto Run guide served through the bridge contains `## Environments`, the active environment's name, `get_accounts`, `propose_accounts`, and "never invent a password"; the API templates guide contains a line that templates run against the active environment (with its name).
- [ ] **Step 2: Run** the guide test module - FAIL.
- [ ] **Step 3: Implement** (static text in guide.rs; the name in the live section).
- [ ] **Step 4: Run** - PASS.
- [ ] **Step 5: Commit** `feat(v2): the guides explain environments and proposing logins`.

### Task 8: Areas - model, routing, scripts

**Files:**
- Modify: `src-tauri/src/autorun/nav.rs` (`ModulePath`, `find_path`, `put_path`, `remove_path`, `route_for`, `ModuleView`, guide section), `src-tauri/src/autorun/mod.rs` (`CaseScript.area`), `src-tauri/src/autorun/replay.rs` (route by script area), `src-tauri/src/autorun/runner.rs`, script save paths (`autorun/store.rs` `save_script`/`save_scripts_atomically` callers: `commands/autorun.rs::save_script_from_editor`, `ai_bridge.rs::save_autorun_scripts`, `auto_run_import_scripts`)
- Test: `src-tauri/tests/suite/autorun_nav.rs` (extend)

**Interfaces:**
- Produces:
  - `ModulePath` gains `#[serde(default)] pub area: String`; on load, an empty `area` takes `module`'s value. Area names unique per project ignoring case.
  - `pub fn find_area<'a>(nav: &'a NavFile, area: &str) -> Option<&'a ModulePath>`
  - `pub fn route_for<'a>(nav, area: Option<&str>, module: Option<&str>, account: Option<&str>) -> Result<Option<&'a ModulePath>, String>`: a non-empty `area` → `find_area` or Err(`the area "<name>" is not recorded - record it in Auto Run, Areas`); else today's Module behaviour and sentences unchanged.
  - `put_path` / `remove_path` key on `area`.
  - `CaseScript.area: Option<String>` (`serde(default, skip_serializing_if = "Option::is_none")`).
  - `pub fn check_areas(nav: &NavFile, scripts: &[CaseScript]) -> Result<(), String>` - every named area is recorded; the Err names the recorded areas. Called by every script save path.
  - `ModuleView` gains `area: String`; the guide's nav section lists `area - module - arrived` and tells the assistant to set `area` when the case's screen is not its module's default area.

- [ ] **Step 1: Failing tests:**
  - `an_old_paths_file_reads_as_areas_named_after_modules`.
  - `old_modules_differing_only_in_case_keep_the_first` (Review Focus 4) and `area_names_are_unique_ignoring_case` on `put_path` for a NEW name that clashes (re-record of the same name still replaces).
  - `two_areas_under_one_module_both_route`; `a_scripts_area_wins_over_its_module`; `no_area_routes_by_module_as_today` (assert today's exact sentences for no Module / no path); `an_unrecorded_area_refuses_the_case`.
  - `saving_a_script_with_an_unrecorded_area_is_refused` (editor, bridge and import paths).
- [ ] **Step 2: Run** `cargo test --test suite autorun_nav::` - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** - PASS; then `cargo test --test suite autorun`; regenerate bindings.
- [ ] **Step 5: Commit** `feat(v2): Auto Run routes a case to a named area, falling back to its Module`.

### Task 9: Areas - recording and the UI

**Files:**
- Modify: `src-tauri/src/commands/autorun_record.rs` (`auto_run_record_start` gains `area: String` after `module`; `auto_run_try_module_path(module)` → takes `area`; `ModuleRecordResult` gains `area`), `src-tauri/src/commands/autorun.rs` (`auto_run_remove_module_path` takes `area`), `src/screens/AutoRun/ModulePathsDialog.tsx` (becomes the Areas dialog; keep the file name or rename to `AreasDialog.tsx` - your choice, update imports), `src/screens/AutoRun/index.tsx` (Setup row `Areas` with its count), the script editor component (area select)
- Test: `src-tauri/tests/suite/autorun_record.rs` or the existing record tests; `src/screens/AutoRun/*.test.tsx`

**Interfaces:**
- Consumes: Task 8 (`ModuleView.area`, `put_path`/`remove_path` by area, `check_areas`).

- [ ] **Step 1: Failing tests:**
  - Rust: recording with `area = "Manage Cycle"`, `module = "PMS"` saves a path with both; a second area under PMS keeps the first; Try and Remove address an area by name.
  - vitest: the Areas dialog groups areas under their module; "Record an area" asks for module and area name and prefills the area name with the module's when the module has none yet; Re-record / Try / Remove act on that area; the Setup row reads `Areas` with the count; the script editor's area select lists recorded areas plus a blank `the case's Module` option and saves `area`.
- [ ] **Step 2: Run** `cargo test --test suite autorun_record` then `npx vitest run src/screens/AutoRun` - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same + ui-consistency/a11y + `npx tsc --noEmit` + bindings - PASS; finally `npm test` and `cd src-tauri && cargo test --tests` once each.
- [ ] **Step 5: Commit** `feat(v2): record several named areas per module`.
