# Auto Run backlog 55-62 (the Definition Wizard fix round and the per-environment update) Implementation Plan

> **Owner decisions (2026-11-01):** build all of 55-62. Item 55 reverses the 2026-10-29 decision that the per-environment update copies scripts, orders, fixtures, flows, templates and projects into EVERY environment (`roots::PER_ENV_ITEMS`, `CopyTo::Every`): the update now puts the old flat folders under the ACTIVE environment only, and runs (with the reports, downloads and shots they name) under the environment each run file names. The owner's own install already migrated and was fixed by hand; it must not be touched again. Everything else below is a ruling made in this plan (see Rulings), open to the owner's veto before the work starts.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build backlog items 55-62 from `docs/autorun/backlog-2026-10.md` (section "The Definition Wizard fix round and the per-environment update (agreed 2026-11-01)", commit 079b5d52).

**Architecture:** Small, self-contained changes to the Rust Auto Run code (`src-tauri/src/autorun/`, `src-tauri/src/browser/`), the bridge (`src-tauri/src/ai_bridge.rs`), the MCP tool texts (`src-tauri/src/mcp.rs`) and, for item 55's import, one command and one button on the Auto Run Setup panel. Pure decisions live in pure functions the suite tests directly; the browser-facing part only calls them. The guide (`autorun/guide.rs`), role files (`autorun/roles/*.md`), MCP descriptions and skill texts (`ai_tools.rs`) change in the same task as the behaviour they describe.

**Tech Stack:** Rust (Tauri 2, tokio, serde, specta / tauri-specta), CDP over WebSocket, React 19 + TypeScript (vitest, jsdom), TanStack Query.

**Spec:** `docs/autorun/backlog-2026-10.md` items 55-62. Project rules: `CLAUDE.md`. Earlier plans this builds on: `docs/superpowers/plans/2026-10-29-per-environment-autorun.md` (the layout, `roots.rs`, `env_copy.rs`), `docs/superpowers/plans/2026-10-30-autorun-backlog-44-53.md` (item 50's cold-site retry, item 51's holds).

Line numbers below are this branch at 079b5d52.

---

## Rulings (product choices the spec leaves open)

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| R1 | **An install that already has `envs/` is never re-migrated or touched.** `migrate_with` already returns `Ok(None)` when `envs/` exists (`roots.rs:756-762`); this plan keeps that exit first and adds a test that pins it. A *finished staging copy* kept from an older build (the rename-only path, `roots.rs:765-779`) is rebuilt, not renamed in: its marker gains a layout version and a marker without it reads as stale. | The owner's data was fixed by hand; any automatic "repair" could undo that. A staging copy built by 2.4.3-beta.9 has the old every-environment layout. | An install that half-migrated under beta.9 copies once more at start (seconds to minutes, once). |
| R2 | The old flat folders stay where they are and are **no longer read** once `envs/` exists (nothing reads them: `lib.rs:689-694` sets the shared root; `store::scripts_dir` is per-environment). The app says so once, in the migration notice: "...The old Auto Run folders are kept as a backup and are no longer read." | The spec asks the app to say which; it is already true. | None in code; a wording change. |
| R3 | The "say so once" notice is a one-time toast on the Auto Run screen (backed by a small `migration-note.txt` in the shared root, read and removed by a command) plus the existing log line. | A log line alone is not "said" to the person; a toast needs no new UI. | The person misses the toast; the log still has it. |
| R4 | A run's environment is matched by **name**, trimmed and case-insensitive, against `environments.json`; two environments with the same name take the first listed; `None`, an empty name or an unknown one goes to the active environment. Shots, downloads and reports no run names go to the active environment. Replays, explore and captures go to the active environment (they carry no environment). | The run file stores the name as it was then (`mod.rs:719-722`); ids were not stored. | A run made under a since-renamed environment lands in the active one (as all runs did before). |
| R5 | Orders: an order goes to the active environment only when at least one of its case ids has a flat script; the others are not carried (they stay in the flat backup). | With every script under the active environment, an order with no script anywhere is the spec's "no script in an environment". | An order for not-yet-scripted cases is lost from the screen; Auto Run rebuilds an order from the PBI anyway. |
| R6 | **Import scripts** imports `case-*.json` files from the folder the person picks (or from its `scripts` child when it has one) into the **active** environment only, scripts only (no components, areas, setups, orders). A case that already has a script is listed and skipped unless the person confirms "Replace"; an identical one is counted as "already the same". A replaced script is kept aside like a deleted one (`store::delete_script`). The button sits on the Setup panel's Environment row. | "Into this environment" is the active one; components and areas have their own copy (`env_copy::copy_case`) and a folder of scripts carries none. The Environment row is the only place on the Auto Run screen that names the environment. | A script whose components the environment lacks fails to run until they are copied (the case card says what is missing, as for any script). |
| R7 | A case hold ends when its holder's **discovery start** is answered with anything but 200 and the hold was made by that call (a hold the agent had before is kept). Try and replay failures keep their hold. The refusal says how long it has been held: "(held for 37 minutes)" - a duration, not a clock time. | A failed try or replay is part of ongoing work; a failed start means no work began. A duration needs no time zone. | A hold made by a failed try still lasts until item 51's timeouts. |
| R8 | A cold first page: the sign-in's first navigation is retried after 5 s, 15 s and 30 s while the total stays under 2 minutes (`FIRST_PAGE_BUDGET_MS = 120_000`), only for a navigation timeout or a `net::ERR_` failure **while the browser still answers** (`Runtime.evaluate "1"`); the final failure says "the site is slow to start" and is not a harness failure. Each navigation keeps `Timing::nav_ms` (30 s). | Item 50 retries once after the marker; this is the step before it. A browser that stopped answering is still the browser's fault. | A dead site takes 2 minutes to fail instead of 30 s. |
| R9 | Item 58: the runtime guard (every discovery try, run and replay) refuses a loop with `except` before its first pass when no match's row reads an `except` text while one of those texts is on the page, naming what the first match's row resolved to. A discovery try records that verdict per area; the save refuses a loop whose last recorded verdict for the same selector and `except` was that refusal, and notes (does not refuse) a loop with `except` that was never tried in discovery. | The runtime guard protects every path that can delete; the save can only judge what discovery saw. Refusing untried loops would block saving every existing script. | An untried loop is saved; the runtime guard still stops it on the first run. |
| R10 | Item 59: `repeat_while_visible` gains `settle_ms` (100 to 30000, default `ROW_GONE_WAIT_MS` = 5000); after each pass it first waits for the page's requests (`actions::settle`), then for the row to go, up to `settle_ms` (no longer capped by `expect_ms`). A pass whose `then` fails after its marked row has gone is counted as done and the loop looks again (and ends when none is left). | "Follow the delete's own request" is what `settle` already does for actions. | A loop on a page that polls waits up to `SETTLE_CAP` per pass. |
| R11 | Item 61: a loop's `then` may hold `when_visible` (whose own `then` holds plain actions only) and `expect_hidden` (a wait, not an assertion: its failure fails the pass as any `then` action does). All other checks stay refused. | Those two are exactly the two shapes the spec names. | None. |
| R12 | Item 60: `has_text` is a new field of a locator step (contains, case-insensitive, whitespace folded; text or a field's value), applied before `nth`. `remember_text` notes an element's text under a name; `expect_text` gains `same_as` naming it (exactly one of `equals`, `matches`, `same_as`). The save refuses a `same_as` with no earlier `remember_text`, as it does for `remember_url`. | Mirrors Playwright's `filter({ hasText })` and the existing `remember_url`/`same_as` pair. | None beyond the vocabulary. |
| R13 | Item 61's one-step edit extends `patch_steps`: a patch may now ADD a step the saved script lacks (inserted in step order), and a patch with `"cleanup": true` adds or replaces the clean-up step. The `edits` entry must still declare the step. | `patch_steps` already sends one step without resending the others (`ai_bridge.rs:12719-12786`); only "add" was refused. | None. |
| R14 | Item 61's clean-up rule is a save **note**, not a refusal: when an action leaves a case step for the clean-up step and that step loses a check in the same save, the note says "a delete the case asks for stays in its step with its check; the clean-up holds a guarded fallback". | `gives_up` already makes the check's removal explicit; a refusal would block a legitimate move. | A rewriter ignores the note (the role text says it too). |
| R15 | Item 62: `get_accounts` shows key, label, username and `has_password` (no `role`: an account has no role field, `accounts.rs:17-24`; the label is what names it). No password in any environment. `propose_accounts` keeps passwords. The guide's live section drops "includes passwords". | The assistant never types a password. | An assistant that wanted a password for an API template body cannot have it (templates sign in by key). |

---

## Global Constraints

- Rust tests only in `src-tauri/tests/suite/`, one module per file, each with a `mod` line in `src-tauri/tests/suite/main.rs`; never a `#[cfg(test)]` module in `src/`, never a new file directly under `tests/`.
- A test that touches process-wide state (store root, the claims and agents registries, the log tail, statics, env vars) takes its lock from `tests/suite/serial.rs` (`serial::autorun()`, `serial::agents()`, `serial::log_tail()`; lock order agents -> log_tail -> autorun).
- `src/bindings.ts` is regenerated with `cd src-tauri && cargo test --test bindings` whenever a specta type or command changes (Tasks 2, 6, 7, 8, 9), and committed; never hand-edited.
- One build or test command at a time on this machine; wait for it. One build cache, `src-tauri/target`; never `CARGO_TARGET_DIR` / `--target-dir`.
- Run one module: `cd src-tauri && cargo test --test suite <module>::`. Frontend: `npx vitest run <file>`; typecheck `npx tsc --noEmit`.
- User-facing errors and log lines name no host, no query string, no full URL and no file path; a raw I/O or transport error goes to `applog` only. A landing page is named by path only (`browser::save_guard::path_of`). No password, token or cookie value in any answer, log or error.
- Role files stay within `ROLE_MAX_LINES` = 45 and `ROLE_MAX_CHARS` = 4000 (`autorun/roles.rs:183-184`), counted with the role's tool note (`tests/suite/autorun_roles.rs:77-81`). The writer and the healer are near their caps: a line that grows makes room elsewhere in the same file without changing any rule's meaning. Never raise a cap.
- The guide, role files, MCP descriptions and skill texts say the same thing about any behaviour a task changes; change them in that task.
- Never weaken a gate test (`ui-consistency.test.ts`, `tests/bindings.rs`, `ado_network.rs`, the role caps, `tcm_mcp.rs` tool-list tests). Tests that pin the OLD migration layout (`autorun_env_migration.rs::migration_copies_every_item_into_each_environment`) are rewritten to the owner's new rule, not deleted.
- No Azure DevOps DELETE, ever. Nothing here talks to Azure DevOps.
- Colours only from tokens; icons only from `src/lib/actionIcons.ts`.
- Never name the hidden features in code comments, commits, changelog or docs (`docs-site/src/guard.test.ts` holds the list).
- No version bump, no `changelog.ts` entry: the controller releases. Task 13 proposes the entry text.
- Commits: Bash heredoc, `git commit -F - <<'EOF' ... EOF`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; confirm with `git log -1`. Never stage `.test-cases/`.

## Review Focus

1. **An already-migrated install is disturbed** (its `envs/` re-copied, its flat folders read or changed, a stale staging copy from beta.9 renamed in). Pinned by Task 1's `an_existing_envs_folder_is_never_migrated_again` and `a_kept_copy_from_an_older_layout_is_rebuilt`.
2. **A run, or a shot another run names, landing in the wrong environment** (two runs sharing a shot name, a run naming a removed environment, a supervised downloads folder). Pinned by Task 1's `runs_and_what_they_name_go_to_the_environment_they_name` and `unnamed_or_unknown_runs_go_to_the_active_environment`.
3. **Import scripts overwriting a case without the person's yes, or while it is replayed.** Pinned by Task 2's `import_skips_a_case_with_a_different_script_unless_replace` and `import_refuses_to_replace_a_case_being_replayed`.
4. **The cold-site retry retrying what is not slowness** (a browser that stopped answering, a refused session, the password reset page) or typing a password twice. Pinned by Task 4's `slow_start_wait_none_when_the_browser_stopped_answering` and `first_page_retry_never_types`.
5. **A delete loop with `except` running a pass when its rows cannot be told apart.** Pinned by Task 5's `except_guard_refuses_when_no_row_reads_an_except_text_on_the_page` and `a_loop_with_unreadable_rows_deletes_nothing`.

---

## Task 1: The update puts the old data under the active environment, and runs where they were made (55, part 1)

**Files:**
- Modify: `src-tauri/src/autorun/roots.rs` (`CopyTo` and `PER_ENV_ITEMS` doc `:21-48`, `MigrationReport` `:172`, `migration_log_line` `:178`, `fan_out` `:690`, `migrate_with` `:754`, `sweep_stagings` / `complete_marker` `:848-898`)
- Modify: `src-tauri/src/environments.rs` (a name lookup beside `read_ids` `:280`)
- Modify: `src-tauri/src/lib.rs` (`:679-681`, write the note)
- Modify: `src-tauri/src/commands/autorun.rs` (one command), `src-tauri/src/lib.rs` (register it)
- Modify: `src/screens/AutoRun/index.tsx` (toast once on mount)
- Test: `src-tauri/tests/suite/autorun_env_migration.rs`, `src/screens/AutoRun/index.test.tsx`

**Interfaces:**
- `CopyTo` keeps its two variants (env_copy reads `Every` as "what an environment copy carries"); its doc now says the update itself puts every item under the active environment, except `BY_RUN`.
- Produces: `pub const BY_RUN: [&str; 4] = ["runs", "reports", "downloads", "shots"];`
- Produces: `pub fn read_names(root: &Path) -> Result<Option<(Vec<(String, String)>, String)>, String>` in `environments.rs` - `(id, name)` pairs and the active id; never makes a file.
- Produces: `pub fn run_target(run_env: Option<&str>, envs: &[(String, String)], active: &str) -> String` - R4.
- Produces: `pub struct Placement { pub runs: BTreeMap<String, String> /* run id -> env id */, pub shots: BTreeMap<String, String> /* shot name -> env id */ }` and `pub fn place_runs(flat: &Path, envs: &[(String, String)], active: &str) -> Result<Placement, String>` - reads each `runs/*.json` as `LocalRun` (an unreadable one is placed in the active environment and logged without a path); each `StepRecord::screenshot` (case steps and `cleanup`) maps to the run's environment; a shot two runs in different environments name goes to EACH of them (hard-linked).
- Changes: `pub fn fan_out(flat: &Path, envs_dir: &Path, envs: &[(String, String)], active: &str) -> Result<MigrationReport, String>` - every `PER_ENV_ITEMS` entry not in `BY_RUN` is copied into `active` only (orders filtered by R5: `orders/<pbi>.json` copied only when one of its ids has `scripts/case-<id>.json` in `flat`); `runs/<id>.json`, `reports/<id>.html` and `downloads/<id>/` go to `placement.runs[id]`; each shot to `placement.shots[name]`, unnamed ones to `active`; other files under a `BY_RUN` item go to `active`. Shots and downloads stay hard-linked (`LINKED_ITEMS`). The export record rule is unchanged.
- Changes: `MigrationOps::fan_out` takes the new signature.
- Changes: `pub struct MigrationReport { pub environments: usize, pub items: usize, pub scripts: usize, pub active_name: String, pub runs_by_env: Vec<(String, usize)> /* env name, runs */ }`
- Produces: `pub fn migration_notice(r: &MigrationReport) -> String` - e.g. `"Auto Run moved 117 scripts to Default; other environments start empty. 70 runs went to QA Automation and 12 to Default, as each run names. The old Auto Run folders are kept as a backup and are no longer read."` (the runs sentence only when there are runs; "1 script"/"1 run" singular; no path). `migration_log_line` becomes `format!("Auto Run: {}", migration_notice(r))`.
- Produces: `pub const MIGRATION_NOTE_FILE: &str = "migration-note.txt";` `pub fn write_migration_note(shared: &Path, text: &str)` (logs, never fails) and `pub fn take_migration_note(shared: &Path) -> Option<String>` (reads and removes it; add the name to `SHARED_ITEMS`, making it `[&str; 10]`).
- Changes: the staging marker is written as `"{environments} {items} by-run-1"`; `sweep_stagings` keeps a whole staging only when its marker's third token is `by-run-1` (R1). The report kept beside it is rebuilt from the marker plus a `migration-note.txt` written into the staging folder's parent only after the rename.
- Produces (command): `#[tauri::command] #[specta::specta] pub async fn auto_run_migration_note() -> Option<String>` - `take_migration_note(&shared)`.

- [ ] **Step 1: Write the failing tests** in `autorun_env_migration.rs` (take `serial::autorun()` where they set a root). Rewrite `migration_copies_every_item_into_each_environment` as `migration_puts_every_item_only_in_the_active_environment`: two environments (Default active, QA), flat `scripts/case-1.json`, `projects/p.nav.json`, `fixtures/f.json`, `templates/t.json`; after `migrate_once` each exists under `envs/<default>/` and NOT under `envs/<qa>/`; `envs/<qa>/` exists and is empty. New tests:
  - `runs_and_what_they_name_go_to_the_environment_they_name`: run `run-1` with `environment: "QA Automation"` (an environment named "qa automation" in the file, to pin case-folding) naming shot `shot-1-1.jpg`, plus `reports/run-1.html` and `downloads/run-1/a.xlsx`; run `run-2` with `environment: "Default"` naming `shot-2-1.jpg`. After migration: `run-1`, its report, its download and `shot-1-1.jpg` are under QA only; `run-2` and `shot-2-1.jpg` under Default only.
  - `unnamed_or_unknown_runs_go_to_the_active_environment`: runs with `environment: None`, `Some("")` and `Some("Gone")` land under the active one; `shots/shot-9-9.jpg` named by no run and `downloads/supervised/x.pdf` land under the active one.
  - `a_shot_two_environments_name_goes_to_both`: `run-1` (QA) and `run-2` (Default) both name `shot-1-1.jpg`: it exists under both.
  - `an_unreadable_run_goes_to_the_active_environment`: `runs/run-3.json` = `"not json"` lands under the active one; migration succeeds.
  - `an_order_with_no_scripted_case_is_not_carried`: `orders/10.json` = `[1, 2]` with `scripts/case-2.json` present, `orders/11.json` = `[5]` with no script: 10 is under the active environment, 11 is not; both stay in the flat folder.
  - `an_existing_envs_folder_is_never_migrated_again`: make `envs/<default>/scripts/case-1.json` = "kept" and a flat `scripts/case-2.json` and `runs/run-1.json`; `migrate_once` returns `Ok(None)`, `envs/` lists exactly what it did before (walk and compare names and bytes), the flat files are unchanged, and no `migration-note.txt` exists.
  - `a_kept_copy_from_an_older_layout_is_rebuilt`: a staging folder `.envs-staging-1` with a marker reading `"2 7"` (no version) is swept; the next `migrate_once` builds a fresh copy (assert QA's folder is empty, which the old layout would not be).
  - `migration_notice_counts_and_names_no_path`: a report with 117 scripts, active "Default", runs `[("QA Automation", 70), ("Default", 12)]` gives exactly the sentence in Interfaces; with one script and no runs, `"Auto Run moved 1 script to Default; other environments start empty. The old Auto Run folders are kept as a backup and are no longer read."`; neither contains `\` or `/`.
  - `the_migration_note_is_said_once`: after a migration, `take_migration_note(shared)` is `Some(notice)`, a second call `None`.
  - Keep `migration_leaves_the_originals_untouched`, `runs_are_copied_but_shots_are_linked`, the staging and rename tests; update their expectations only where they named a non-active environment.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_env_migration::` - expect FAIL.
- [ ] **Step 3: Implement** the Interfaces above in `roots.rs` and `environments.rs`; in `lib.rs` keep the `started.report` log and write the note (`roots::write_migration_note`, a plain `std::fs::write` in the shared root; failure only logged) right after a migration that returned `Some` with `items > 0`. Register `auto_run_migration_note` in `lib.rs`'s command list.
- [ ] **Step 4: Run** `cd src-tauri && cargo test --test suite autorun_env_migration::`, then `cargo test --test suite autorun_env_roots::` and `cargo test --test suite autorun_env_copy::` (env_copy reads `CopyTo`) - expect PASS. Regenerate bindings: `cd src-tauri && cargo test --test bindings`.
- [ ] **Step 5: Frontend test** in `src/screens/AutoRun/index.test.tsx`: `shows the per-environment move notice once` - mock `commands.autoRunMigrationNote` to resolve `"Auto Run moved 3 scripts to Default; ..."` once and `null` after; mount the screen; `findByText(/moved 3 scripts to Default/)` in the toast region; remount; the call is made again and no second toast shows. Run `npx vitest run src/screens/AutoRun/index.test.tsx` - FAIL, then implement in `index.tsx` (one `useEffect` on mount calling the command and `toast.info(note)` when it returns text; no query cache, it is a one-shot read), run again - PASS.
- [ ] **Step 6: Guide text.** In `autorun/guide.rs`'s environments paragraph (`:1101-1104`) nothing about the update is said to assistants; add nothing there. Run `npx tsc --noEmit`.
- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/autorun/roots.rs src-tauri/src/environments.rs src-tauri/src/lib.rs src-tauri/src/commands/autorun.rs src/bindings.ts src/screens/AutoRun/index.tsx src/screens/AutoRun/index.test.tsx src-tauri/tests/suite/autorun_env_migration.rs
git commit -F - <<'EOF'
fix(autorun): the per-environment update puts old data in the active environment only

Runs, and the reports, downloads and shots they name, go to the
environment each run names; an order goes only where one of its cases
has a script. An install that already has its environment folders is
never migrated again, and a finished copy kept from the older layout
is rebuilt rather than put in place. The app says once what it moved
and that the old folders are kept but no longer read.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

## Task 2: Import scripts into this environment (55, part 2)

**Files:**
- Modify: `src-tauri/src/autorun/env_copy.rs` (a new section after `copy_case` `:968`)
- Modify: `src-tauri/src/commands/env_copy.rs` (two commands), `src-tauri/src/lib.rs` (register)
- Modify: `src/screens/AutoRun/SetupPanel.tsx` (Environment row `:621-644`; update its "Read-only here" comment)
- Create: `src/screens/AutoRun/ImportScriptsDialog.tsx`, `src/screens/AutoRun/ImportScriptsDialog.test.tsx`
- Modify: `src-tauri/src/autorun/guide.rs` (`:1104`, the sentence that lists what the person can copy)
- Test: `src-tauri/tests/suite/autorun_env_copy.rs`

**Interfaces:**
- Produces: `#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)] pub struct ImportScriptsPlan { pub new: Vec<i32>, pub same: Vec<i32>, pub differ: Vec<i32>, pub unreadable: u32 }` - case ids sorted; `unreadable` counts `case-*.json` files that do not parse as a `CaseScript` or whose name and `case_id` disagree.
- Produces: `pub fn plan_import_scripts(from: &Path, target: &Path) -> Result<ImportScriptsPlan, String>` - `from` is the picked folder, or its `scripts` child when that is a folder; only top-level `case-<digits>.json` regular files are read (a link is skipped and counted unreadable). "same" compares the parsed scripts with `suspected_defect` and `cleared_defect` cleared on both sides.
- Produces: `pub fn import_scripts(from: &Path, target: &Path, replace: bool) -> Result<ImportScriptsPlan, String>` - saves every `new`; with `replace` also every `differ`, each first kept aside with `store::delete_script(target, id, now)`, put back if the save fails; without `replace` the `differ` ones are left. Refuses (nothing written) with `case_copy_replaying(id)` when `replace` and a `differ` case `store::is_replaying(id)`. Marks never travel. Writes through the active root's guard (the target is the active root). Logs counts only.
- Produces: `pub const IMPORT_NO_SCRIPTS: &str = "that folder holds no Auto Run scripts (files named case-<number>.json)";` `pub const IMPORT_FAILED: &str = "the import did not finish - see Settings > Logs";`
- Produces (commands, in `commands/env_copy.rs`): `auto_run_import_scripts_plan(folder: String) -> Result<ImportScriptsPlan, String>` and `auto_run_import_scripts(folder: String, replace: bool) -> Result<ImportScriptsPlan, String>`, both on `spawn_blocking`, both refused with `CASE_COPY_NEEDS_LAYOUT` when the per-environment layout is not in force, both against `store::configured_root()`. The folder is the person's own pick from the native dialog; it is only read.
- UI: "Import scripts…" (`IconImport`, `aria-label="Import scripts into this environment"`, title "Bring Auto Run scripts from a folder into <environment name>") on the Environment row. It opens the folder picker (`open({ directory: true })` from `@tauri-apps/plugin-dialog`, as `PlaywrightExportDialog.tsx:169` does), then `ImportScriptsDialog` with the plan: "<n> new scripts will be added. <m> are already the same. <k> cases already have a different script here and are left as they are." with a checkbox "Replace those <k> scripts (the ones here are kept as deleted scripts)", and Import / Cancel. After import: toast "Imported <n> scripts into <environment>" (+ ", replaced <k>"), and invalidate the scripts queries the case list reads (the existing keys in `src/lib/cache.ts`; no new cache).

- [ ] **Step 1: Write the failing tests** in `autorun_env_copy.rs` (take `serial::autorun()`; set the active root with the existing helpers):
  - `import_plan_sorts_new_same_and_differ`: folder holds case-1 (absent in target), case-2 (identical), case-3 (differs), `notes.txt`, `case-x.json`, `case-4.json` with `case_id: 5`: plan `{ new: [1], same: [2], differ: [3], unreadable: 2 }`.
  - `import_reads_a_scripts_child_folder`: the picked folder holds only `scripts/case-1.json`: plan `new: [1]`.
  - `import_skips_a_case_with_a_different_script_unless_replace`: `import_scripts(.., false)` leaves case-3's target script byte-identical and adds case-1; `import_scripts(.., true)` replaces case-3 and the old one is listed by `store::deleted_scripts` (or the existing deleted-scripts reader).
  - `import_refuses_to_replace_a_case_being_replayed`: mark case 3 replaying (`store::set_replaying` or the existing test hook); `import_scripts(.., true)` is `Err(case_copy_replaying(3))` and case-1 was NOT added.
  - `import_drops_suspected_defect_marks`: a source script with `suspected_defect` set is saved without it.
  - `import_of_an_empty_folder_says_so`: `Err(IMPORT_NO_SCRIPTS)`; the sentence has no `\` or `/`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_env_copy::` - FAIL.
- [ ] **Step 3: Implement** the Rust Interfaces; register the commands; `cd src-tauri && cargo test --test bindings`.
- [ ] **Step 4: Run** `cd src-tauri && cargo test --test suite autorun_env_copy::` - PASS.
- [ ] **Step 5: Frontend tests** in `ImportScriptsDialog.test.tsx`: renders the three counts from a mocked plan; Import without the box calls `autoRunImportScripts(folder, false)`; with the box ticked calls it with `true`; the box is absent when `differ` is empty; a refusal string is shown in the dialog (not a toast) and Import stays enabled. Run `npx vitest run src/screens/AutoRun/ImportScriptsDialog.test.tsx` - FAIL, implement, PASS. Then `npx vitest run src/ui-consistency.test.ts` and `npx tsc --noEmit`.
- [ ] **Step 6: Guide.** In `guide.rs:1104` add "Import scripts… on the Auto Run Setup panel, for a folder of scripts" to the list of what the person can bring in; run `cd src-tauri && cargo test --test suite autorun_guide::`.
- [ ] **Step 7: Commit** (`feat(autorun): import scripts from a folder into the active environment`), heredoc as in Task 1.

---

## Task 3: A case hold ends when its discovery start fails, and the refusal says how long it is held (56)

**Files:**
- Modify: `src-tauri/src/autorun/claims.rs` (`Claim` `:30`, `case_claimed` `:44`, `claim` `:66`)
- Modify: `src-tauri/src/ai_bridge.rs` (`claim_for` `:11919`, `claim_named_case` `:11942`, the route's claim block `:446-454` and the answer after it)
- Test: `src-tauri/tests/suite/autorun_claims.rs`, `src-tauri/tests/suite/autorun_bridge.rs` (or the module that already drives `/autorun-discover-start`)

**Interfaces:**
- `Claim` gains `since_ms: u64` (the first claim; `at_ms` stays the last renewal).
- Changes: `pub fn case_claimed(case_id: i32, label: &str, held_ms: u64) -> String` - `"case {case_id} is being worked on by {label} (held for {held}) - pick another case, or ask the person to release {label} on the Auto Run screen"`, `held` = `"less than a minute"`, `"1 minute"`, `"{n} minutes"`, or `"{h} h {m} min"` from 60 minutes.
- Changes: `pub fn claim(key: &AgentKey, case_id: i32, now_ms: u64) -> Result<bool, String>` - `Ok(true)` when this call made the hold (new, or taken over from a gone holder), `Ok(false)` when it renewed one the key already had.
- Produces: `pub fn release_case(key: &AgentKey, case_id: i32) -> bool`.
- Changes: `claim_for` / `claim_named_case` return `Result<Option<i32>, (u16, String)>` - the case a call newly claimed. In the route, when the path is `/autorun-discover-start`, the caller is an agent, a case was newly claimed, and the answer's status is not 200, `claims::release_case(key, case)` runs and the log says `"Auto Run: case {id}'s hold ended - its discovery did not start"`. Tries and replays are unchanged (R7).

- [ ] **Step 1: Write the failing tests:**
  - In `autorun_claims.rs`: `claim_says_whether_it_made_the_hold` (first `Ok(true)`, renewal `Ok(false)`, take-over of a gone holder `Ok(true)`); `refusal_says_how_long_the_case_is_held` (A claims at T0, A renews at T0+30 min, B is refused at T0+37 min with a sentence containing `"(held for 37 minutes)"` - from the FIRST claim, not the renewal); `held_words` for 0 ms, 61 s, 59 min, 125 min give `less than a minute`, `1 minute`, `59 minutes`, `2 h 5 min`; `release_case_ends_one_hold_only`.
  - In the bridge module: `a_failed_discovery_start_lets_go_of_the_case_it_claimed` - agent A posts `/autorun-discover-start` with `case_id` 135421 and a body the start refuses (no project / no recipe - whatever the existing tests use to get a non-200 without a browser); then agent B's start for 135421 is not refused with `case_claimed`. `a_failed_start_keeps_a_hold_made_earlier` - A first claims 135421 through `/autorun-try` (or `claims::claim` directly), then a failed start: B is still refused. Both take `serial::agents()` then `serial::autorun()`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_claims::` (and the bridge module) - FAIL.
- [ ] **Step 3: Implement**; fix every caller of `case_claimed` and `claim` the compiler names (`grep -n "case_claimed\|claims::claim(" src-tauri/src src-tauri/tests`).
- [ ] **Step 4: Run** both modules, then `cargo test --test suite tcm_mcp::` - PASS.
- [ ] **Step 5: Guide/MCP text.** Where the guide or `mcp.rs`'s `start_autorun_discovery` description quotes the refusal ("is being worked on by"), add "(held for ...)"; run `cargo test --test suite autorun_guide::`.
- [ ] **Step 6: Commit** (`fix(autorun): a failed discovery start lets go of the case it held`).

---

## Task 4: A cold site's first page is waited for, and called slow, not the browser (57)

**Files:**
- Modify: `src-tauri/src/autorun/signin.rs` (`open_start` `:871`, the caller `:1007-1012`, constants near `:291-304`)
- Modify: `src-tauri/src/autorun/patterns.rs` and `src-tauri/src/autorun/transient.rs` (the new sentence is an environment flake, and transient for a run's second try)
- Modify: `src-tauri/src/autorun/guide.rs` (the sign-in failures paragraph, near the item-50 sentence)
- Test: `src-tauri/tests/suite/autorun_signin.rs`, `src-tauri/tests/suite/autorun_transient.rs` (or wherever `is_transient` is tested)

**Interfaces:**
- Produces: `pub const FIRST_PAGE_BUDGET_MS: u64 = 120_000;` `pub const FIRST_PAGE_WAITS_MS: [u64; 3] = [5_000, 15_000, 30_000];`
- Produces: `pub fn slow_start_wait(tries_so_far: u32, elapsed_ms: u64, detail: &str, browser_answers: bool) -> Option<u64>` - the wait before the next try, or `None`: `None` when `!browser_answers`, when `detail` is neither a `Page.navigate` timeout (`"did not answer Page.navigate"`) nor contains `"net::ERR_"`, when `tries_so_far > FIRST_PAGE_WAITS_MS.len()`, or when `elapsed_ms + wait + nav_ms` would pass the budget (pass `nav_ms` in: `slow_start_wait(tries, elapsed, detail, answers, nav_ms)`).
- Produces: `pub const SITE_SLOW_TO_START: &str = "the site is slow to start - its sign-in page did not open within about 2 minutes";` the failure words become `format!("{PAGE_DID_NOT_OPEN}{SITE_SLOW_TO_START} ({tries} tries; last: {why})")`, and the outcome is NOT a harness failure. `why` is the CDP detail with any URL reduced to its path (`save_guard::path_of`).
- Produces: `pub const FIRST_PAGE_RETRY: &str = "the sign-in page did not open; waiting and opening it again (the site may be slow to start)";` logged per retry with the try number and wait (no URL).
- Changes: `open_start` loops: navigate; on failure ask the browser `Runtime.evaluate "1"` (2 s deadline) and call `slow_start_wait`; idle the wait (honouring the run's stop, as `run_steps` does) and navigate again. Only the `Navigate` action is repeated - no step, no fill, no click (item 50's rule: the form is never sent twice).

- [ ] **Step 1: Write the failing tests** in `autorun_signin.rs`:
  - `slow_start_wait_follows_the_schedule`: `(1, 30_000, "the browser did not answer Page.navigate within 30000ms", true, 30_000)` -> `Some(5_000)`; tries 2 -> `Some(15_000)`; tries 3 at elapsed 80_000 -> `None` (80 + 30 + 30 > 120); `net::ERR_ABORTED` at tries 1 -> `Some(5_000)`.
  - `slow_start_wait_none_when_the_browser_stopped_answering`: same detail with `browser_answers: false` -> `None`.
  - `slow_start_wait_none_for_other_failures`: `"net::ERR_NAME_NOT_RESOLVED"` -> `Some` (it is net::ERR_; pin that DNS failures do retry, since a cold VPN does that) and `"refused: session"`, `"password expired"` -> `None`.
  - `first_page_retry_reaches_the_page`: a fake driver whose first two `Page.navigate` calls time out and third succeeds (the suite's `FakePage`/fake driver with a navigate-failure counter; time via the driver's `idle`, which the fake does not really sleep): the sign-in succeeds and the log tail (`serial::log_tail()`) has two `FIRST_PAGE_RETRY` lines.
  - `first_page_retry_never_types`: same fake with every navigate failing: the outcome's words start `PAGE_DID_NOT_OPEN` and contain `SITE_SLOW_TO_START`, `harness` is false, no `Input.insertText` / `Input.dispatchKeyEvent` was sent, and the words hold no `http`.
  - `a_browser_that_stops_answering_is_still_the_browsers_fault`: navigate times out and `Runtime.evaluate` errors: one try only, harness true, words do not contain `SITE_SLOW_TO_START`.
  - In the transient module: `a_slow_site_is_transient` - a case record whose failure holds `SITE_SLOW_TO_START` is `Some(..)` from `is_transient`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_signin::` - FAIL.
- [ ] **Step 3: Implement.** In `patterns.rs` classify `SITE_SLOW_TO_START` with the environment-flake class (the class item 50's `COLD_SITE_RETRY` text uses, or `ErrorClass::Environment` if that is its name), so the healer is not told to repair the script.
- [ ] **Step 4: Run** `autorun_signin::`, the transient module, `autorun_patterns::` (if present) - PASS.
- [ ] **Step 5: Guide.** One sentence beside item 50's: "A site that is slow to wake (the first page does not open) is given about 2 minutes, opened again after 5, 15 and 30 seconds; it then fails as \"the site is slow to start\", which is the site, not the browser and not the script." `cargo test --test suite autorun_guide::`.
- [ ] **Step 6: Commit** (`fix(autorun): wait for a cold site's first page and call it slow, not the browser`).

---

## Task 5: A delete loop whose rows cannot be told apart deletes nothing (58)

**Files:**
- Modify: `src-tauri/src/browser/repeat.rs` (`KEPT_JS` `:72`, `Match` `:97`, `look` `:107`)
- Modify: `src-tauri/src/autorun/runner.rs` (`repeat_while_visible` `:1892`)
- Modify: `src-tauri/src/autorun/discovery_map.rs` (`AreaMap` `:111`), the bridge's try handler in `ai_bridge.rs` (record the verdict), `src-tauri/src/autorun/seen_check.rs` (save refusal and note)
- Modify: `src-tauri/src/autorun/guide.rs` (the `repeat_while_visible` paragraph)
- Test: `src-tauri/tests/suite/autorun_repeat_while_visible.rs`, `src-tauri/tests/suite/seen_check.rs`

**Interfaces:**
- `Match` gains `pub row_desc: Option<String>` - what the row rule resolved to, from `KEPT_JS`: tag name, then `role`, `id` or the first `data-*-id` attribute name when present, then up to 60 characters of its text, e.g. `div[data-node-id] "Delete"`. Never a value of a password field.
- Produces: `pub const EXCEPT_ON_PAGE_JS: &str` - `(except) => except.filter(t => page text or any field value, normalised as KEPT_JS normalises, contains t)`.
- Produces: `pub fn except_guard(found: &[Match], except: &[String], on_page: &[String]) -> Option<String>` - `Some(sentence)` when `except` is non-empty, `found` is non-empty, no match has `kept`, and `on_page` is non-empty: `"{n} match(es) found, but no match's row reads an except text although \"{first on_page}\" is on the page: the first match's row is {row_desc or \"no row\"} ({ROW_RULE}) - point selector at something inside each row, so each row holds its own name"`. `None` otherwise.
- Changes: the runner, before the FIRST pass only, when `except` is non-empty: reads `on_page` with `EXCEPT_ON_PAGE_JS` and fails the action with `except_guard`'s sentence (no pass runs, nothing is clicked). Holds in discovery tries, runs and replays alike.
- Produces (discovery map): `#[derive(...)] pub struct LoopRows { pub selector: Target, pub except: Vec<String>, pub refused: Option<String>, pub at: u64 }` and `AreaMap::loop_rows: Vec<LoopRows>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`); `pub fn record_loop_rows(map: &mut AreaMap, rows: LoopRows)` keeps one entry per (selector, except set), newest wins.
- The bridge's try handler, after a discovery try whose action (or a step it ran) is a `repeat_while_visible` with `except`, records `LoopRows { refused: Some(sentence) }` when the outcome detail contains the guard sentence's fixed part, else `refused: None`.
- Produces (save): in `seen_check`, `pub const LOOP_ROWS_REFUSED: &str = "was refused in discovery because its rows cannot be told apart: ";` and `pub const LOOP_ROWS_UNTRIED: &str = "has except texts but was never tried in discovery, so the save could not check that each row holds its name - try it once before relying on it";` A saved loop whose newest `LoopRows` for the same selector and except set (order-free, folded) has `refused: Some(s)` is refused with `"step {n}: repeat_while_visible {LOOP_ROWS_REFUSED}{s}"`; one with no entry gets the untried line as a note (not a refusal) in the save's notes.

- [ ] **Step 1: Write the failing tests** in `autorun_repeat_while_visible.rs`:
  - `except_guard_refuses_when_no_row_reads_an_except_text_on_the_page`: three matches, none kept, `on_page = ["Core Competencies"]`, first `row_desc = Some("div[data-node-id] \"Delete\"")` -> sentence contains `"Core Competencies"` and `div[data-node-id]`.
  - `except_guard_lets_through`: (a) one match kept; (b) `on_page` empty (the protected rows are simply not there); (c) `except` empty; (d) `found` empty -> all `None`.
  - `a_loop_with_unreadable_rows_deletes_nothing`: the suite's fake page with three matches whose rows read nothing and `EXCEPT_ON_PAGE_JS` answering `["Leadership Competencies"]`: the action fails with the guard sentence and no `Input.dispatchMouseEvent` (click) was sent.
  - `a_loop_whose_rows_read_their_names_still_runs`: same, one row kept: passes run as before.
- [ ] **Step 2: Write the failing save tests** in `seen_check.rs`: `a_loop_refused_in_discovery_is_refused_at_save` (map with `LoopRows { refused: Some(..) }` for the script's selector and `except` in another order -> refusal containing `LOOP_ROWS_REFUSED`); `a_loop_tried_since_and_passed_saves` (newest entry `refused: None` -> no refusal); `an_untried_loop_with_except_saves_with_a_note` (no entry -> saved, notes contain `LOOP_ROWS_UNTRIED`); `a_loop_without_except_needs_no_trial`.
- [ ] **Step 3: Run** `cd src-tauri && cargo test --test suite autorun_repeat_while_visible::` and `cargo test --test suite seen_check::` - FAIL.
- [ ] **Step 4: Implement**; the Playwright export's loop (`pw_export/translate.rs`, `the_export_writes_a_bounded_loop_with_the_same_except_rule`) is unchanged - the guard is the app's.
- [ ] **Step 5: Run** both modules, `discovery_map` tests (`cargo test --test suite autorun_discovery_map::` if present) and `pw_export_translate::` - PASS.
- [ ] **Step 6: Guide.** In the `repeat_while_visible` paragraph: "With `except`, the loop first checks that its rows can be told apart: when the page shows an `except` text but no match's row reads one, it stops before the first pass and says what the row rule found - point `selector` inside each row. The save refuses a loop discovery stopped that way." `cargo test --test suite autorun_guide::`.
- [ ] **Step 7: Commit** (`fix(autorun): a delete loop whose rows cannot be told apart deletes nothing`).

---

## Task 6: A delete loop waits for its own request and ends when its row is already gone (59)

**Files:**
- Modify: `src-tauri/src/browser/actions.rs` (`Action::RepeatWhileVisible` `:260`, `check_repeat` `:940`)
- Modify: `src-tauri/src/autorun/runner.rs` (`repeat_while_visible` `:1892-1983`, `ROW_GONE_WAIT_MS` `:1986`)
- Modify: `src-tauri/src/pw_export/translate.rs` (the loop's row wait uses `settle_ms`), `src-tauri/src/autorun/guide.rs`, `src-tauri/src/autorun/action_help.rs` (the example)
- Test: `src-tauri/tests/suite/autorun_repeat_while_visible.rs`

**Interfaces:**
- `RepeatWhileVisible` gains `#[serde(default, skip_serializing_if = "Option::is_none")] settle_ms: Option<u32>`; every pattern match on it gains `..` where needed.
- `check_repeat` refuses `settle_ms` outside 100..=30000: `"repeat_while_visible settle_ms is 100 to 30000, not {n}"`.
- Runner, after each pass: `actions::settle(d, timing).await`, then the row-gone wait up to `settle_ms.unwrap_or(ROW_GONE_WAIT_MS as u32)` ms (no `expect_ms` cap).
- Produces: `pub fn pass_failure_ends_quietly(row_gone: bool) -> bool` (true when the marked row is gone): when a `then` action fails and `repeat::row_still_there` answers `Ok(false)`, the pass counts as done, its detail says `"pass {n}: the row had already gone ({failure})"`, and the loop looks again.
- Bindings: `cargo test --test bindings`.

- [ ] **Step 1: Write the failing tests:**
  - `settle_ms_is_checked`: 99 and 30001 refused with the sentence; 100, 30000 and absent accepted.
  - `settle_ms_round_trips_and_is_left_out_when_absent`: serialising a loop without it has no `settle_ms` key.
  - `a_slow_row_is_waited_for_up_to_settle_ms`: fake page whose marked row stays 7 s after the click: with `settle_ms: 10000` the loop makes ONE pass and ends "none left"; with no `settle_ms` it makes a second pass (today's behaviour, pinned).
  - `a_pass_whose_row_went_while_it_worked_ends_the_loop`: fake page where pass 2's click on Delete succeeds but its `expect_dialog`/confirm click fails AND the marked row is gone: the loop passes, detail contains `"the row had already gone"`.
  - `a_pass_that_fails_with_its_row_still_there_still_fails`: the confirm fails with the row still present: the loop fails as today.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_repeat_while_visible::` - FAIL.
- [ ] **Step 3: Implement**; the export waits `settle_ms` (or 5000) for the row in its loop body.
- [ ] **Step 4: Run** the module, `pw_export_translate::`, `browser_actions::` - PASS; then `cargo test --test bindings`.
- [ ] **Step 5: Guide and example.** `action_help.rs` loop example unchanged; the guide's loop paragraph: "After each pass the loop waits for the page's requests to finish, then for the row to go, up to `settle_ms` (default 5000, at most 30000) - raise it on a slow site. A pass that fails because its row has already gone ends quietly." `cargo test --test suite autorun_guide::`.
- [ ] **Step 6: Commit** (`fix(autorun): a delete loop waits for its own request and stops when its row is gone`).

---

## Task 7: A loop's `then` may confirm only if shown and wait for its row to go (61, part 1)

**Files:**
- Modify: `src-tauri/src/browser/actions.rs` (`check_repeat` `:959-985`), `src-tauri/src/autorun/runner.rs` (the pass runs a `WhenVisible` through the same helper a step uses)
- Modify: `src-tauri/src/autorun/store.rs` (`:447`, `:473` - what the save allows inside a loop), `src-tauri/src/autorun/seen_check.rs` (`:1590`, `:2176`, `:2288` - `in_loop` reaches one level deeper), `src-tauri/src/pw_export/translate.rs`
- Modify: `src-tauri/src/autorun/guide.rs`
- Test: `src-tauri/tests/suite/autorun_repeat_while_visible.rs`

**Interfaces:**
- `check_repeat` allows `Action::WhenVisible` (its own `then` checked by the existing `when_visible` rules: plain actions only, so no loop and no second `when_visible` inside) and `Action::ExpectHidden`; every other check is still refused with the existing sentence. A row-scoped locator (`[data-autorun-row]`) inside the nested `when_visible` is still resolved in the marked row.
- Produces: `pub const LOOP_THEN_ALLOWS: &str = "plain actions, when_visible (plain actions inside) and expect_hidden";` used in the refusal sentences.

- [ ] **Step 1: Write the failing tests:** `then_may_hold_when_visible_and_expect_hidden` (validate OK); `then_still_refuses_other_checks` (`expect_text` refused, sentence names `LOOP_THEN_ALLOWS`); `when_visible_in_a_loop_holds_no_loop` (a `repeat_while_visible` inside the nested `when_visible` refused with `NESTED_REPEAT`); `a_loop_confirms_only_when_the_dialog_shows` (fake page: pass 1 shows a confirm, pass 2 does not; both pass, one confirm click); `expect_hidden_in_then_fails_the_pass_when_the_row_stays` (row never goes: the pass fails with the expect_hidden detail); the existing `then_and_except_are_checked` updated only where it asserted these two were refused.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_repeat_while_visible::` - FAIL.
- [ ] **Step 3: Implement**, including the export (a `when_visible` inside the loop body is written as the existing `when_visible` translation, indented one level) and the save's seen check of locators inside the nested `when_visible`.
- [ ] **Step 4: Run** the module, `seen_check::`, `autorun_store::`, `pw_export_translate::` - PASS.
- [ ] **Step 5: Guide.** The loop paragraph's "then holds plain actions only" becomes "then holds plain actions, `when_visible` (to confirm a dialog only if it shows) and `expect_hidden` (to wait for the row to go)". `cargo test --test suite autorun_guide::`.
- [ ] **Step 6: Commit** (`feat(autorun): a loop pass may confirm only if shown and wait for its row to go`).

---

## Task 8: A locator can pick a row by the text it holds (60, part 1)

**Files:**
- Modify: `src-tauri/src/browser/locator.rs` (`LocatorStep` `:19`, its validation `:130-165`, `describe` `:180-200`, `walk` `:571-590`)
- Modify: `src-tauri/src/autorun/seen_check.rs` (a `has_text` locator is a narrowed locator, as `nth` is: `:60`, `:93`, `:152`, `:589-594`), `src-tauri/src/pw_export/ts.rs` (`step_expr` `:61`)
- Modify: `src/screens/AutoRun/describeAction.ts` (the locator words), `src-tauri/src/autorun/guide.rs` (the locator section), `src-tauri/src/autorun/roles/writer.md` and `case-rewriter.md` (the "by name, not nth" line)
- Test: `src-tauri/tests/suite/browser_locator.rs`, `src-tauri/tests/suite/seen_check.rs`, `src-tauri/tests/suite/pw_export_translate.rs`, `src/screens/AutoRun/describeAction.test.ts`

**Interfaces:**
- `LocatorStep` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub has_text: Option<String>` - keep only this step's matches that contain the text (case-insensitive, whitespace runs folded; the element's text or a field's value in it). Applied after the step's own matching and before `nth`. Refused when empty or only whitespace (`"has_text is empty - give the text the element holds"`); refused with `input` (a placeholder step stands alone).
- Produces: `pub const HAS_TEXT_JS: &str` - `function(want) { ... }` on a match, true when it holds the text.
- `describe` adds ` holding "<text>"`.
- Export: `.filter({ hasText: <lit> })` before `.nth(...)`.
- Seen check: a step with `has_text` is checked like one with `nth` (`NEVER_COUNTED_NTH` names "its has_text and nth as written"); its `has_text` value, when it holds a value the script typed earlier, needs no sighting of that text (the same typed-value exemption as a name).
- Bindings: `cargo test --test bindings`.

- [ ] **Step 1: Write the failing tests:** in `browser_locator.rs` - `has_text_keeps_the_rows_that_hold_it` (fake page with three `tr` matches reading "Alpha", "AutoTest Grp 7", "Beta": `{ "css": "tr", "has_text": "autotest  grp 7" }` resolves to the second); `has_text_applies_before_nth`; `has_text_reads_a_field_value`; `an_empty_has_text_is_refused`; `has_text_with_input_is_refused`; `describe_names_has_text`. In `seen_check.rs` - `a_has_text_locator_needs_counting_like_nth`; `a_has_text_naming_a_typed_value_needs_no_sighting_of_it`. In `pw_export_translate.rs` - `has_text_becomes_a_filter_before_nth` (`.locator('tr').filter({ hasText: 'AutoTest Grp 7' }).nth(0)`). In `describeAction.test.ts` - a click on that locator reads `row holding "AutoTest Grp 7"` (match the file's existing phrasing).
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite browser_locator::` and `npx vitest run src/screens/AutoRun/describeAction.test.ts` - FAIL.
- [ ] **Step 3: Implement**; regenerate bindings.
- [ ] **Step 4: Run** `browser_locator::`, `seen_check::`, `pw_export_translate::`, `discovery_locators::`, the vitest file - PASS.
- [ ] **Step 5: Guide and roles.** Guide locator section: "`has_text` keeps only the matches that hold this text (`{ \"css\": \"tr\", \"has_text\": \"AutoTest Grp 7\" }`) - the way to reach a row or card by the name it holds, instead of `nth`." Roles: writer.md `:27` and case-rewriter.md `:19` become "Any delete finds its row by the name it holds (has_text), never by nth." (keep the clean-up half for Task 11). Run `cargo test --test suite autorun_roles::` and `autorun_guide::`; if a cap is passed, shorten another line of that file without changing what it says.
- [ ] **Step 6: Commit** (`feat(autorun): pick a row or card by the text it holds (has_text)`).

---

## Task 9: Remember an element's text and check it later (60, part 2)

**Files:**
- Modify: `src-tauri/src/browser/actions.rs` (`Action` beside `RememberUrl` `:67`, `ExpectText` `:83`, validation `:1324-1331`, `:1494`, the run match `:2487`), `src-tauri/src/browser/cdp.rs` (`noted_texts` beside `noted_addresses` `:3041`, `:3280`)
- Modify: `src-tauri/src/autorun/store.rs` (the `same_as` check `:366-420`), `src-tauri/src/autorun/runner.rs` (`:341`), `src-tauri/src/autorun/seen_check.rs` (`:392`), `src-tauri/src/autorun/report.rs` (`:92`), `src-tauri/src/autorun/patterns.rs` (`:641`), `src-tauri/src/pw_export/translate.rs` (`:82`, `:232`, `:1509`)
- Modify: `src-tauri/src/autorun/action_help.rs` (example), `src-tauri/src/autorun/guide.rs` (`:25` kinds list, `:300`), `src/screens/AutoRun/describeAction.ts`
- Test: `src-tauri/tests/suite/browser_actions.rs`, `src-tauri/tests/suite/autorun_store.rs`, `src-tauri/tests/suite/pw_export_translate.rs`, `src/screens/AutoRun/describeAction.test.ts`

**Interfaces:**
- Produces: `Action::RememberText { selector: Target, #[serde(rename = "as")] name: String }` - kind `remember_text`; note the element's text (a field's value for a field), whitespace folded, under `name` (the same 1-30 character rule as `remember_url`, `check_note_name`). Fails like a click when the locator finds nothing or several.
- `ExpectText` gains `#[serde(default, skip_serializing_if = "Option::is_none")] same_as: Option<String>`; exactly one of `equals`, `matches`, `same_as` (`TEXT_NEEDS_ONE` updated to name all three). With `same_as`, the element's text equals the noted one, polled like every `expect_`.
- `Driver::noted_texts(&mut self) -> Option<&mut BTreeMap<String, String>>` (default `None`; the run's drivers keep one per case, as they do `noted_addresses`).
- Produces: `pub const NO_NOTED_TEXT: fn(&str) -> String` style helper `no_noted_text(name)`: `"no text was noted as \"{name}\" in this case: a remember_text with \"as\": \"{name}\" must come before this check"`.
- Save: a `same_as` on `expect_text` with no earlier top-level `remember_text` of that name is refused (the `remember_url` rule's sentence shape, `store.rs:366-420`).
- Report: `note the text of <target> as "<name>"`; describeAction the same.
- Export: `notedTexts[name] = (await <locator>.innerText()).replace(/\s+/g, ' ').trim();` and `await expect(<locator>).toHaveText(notedTexts[name])`.
- Bindings: `cargo test --test bindings`.

- [ ] **Step 1: Write the failing tests:** in `browser_actions.rs` - `remember_text_notes_the_text_for_a_later_same_as` (mirror `remember_url_notes_the_address_for_a_later_same_as` `:1694`: pills read "2 / 4 / 7", remembered, then the check passes; after the page changes them to "3 / 4 / 7" the check fails naming both values); `expect_text_takes_exactly_one_of_equals_matches_same_as`; `remember_text_on_a_driver_without_a_notebook_fails`. In `autorun_store.rs` - `a_text_same_as_needs_an_earlier_remember_text` (mirror `a_same_as_needs_an_earlier_remember_url` `:1629`). In `pw_export_translate.rs` - `remember_text_exports_a_noted_text_and_a_to_have_text`. In `describeAction.test.ts` - the two new phrasings.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite browser_actions::` - FAIL.
- [ ] **Step 3: Implement**; the compiler lists every exhaustive match on `Action` - give `RememberText` the same arm `RememberUrl` has (no check, no target-for-highlight beyond its selector, never in a clean-up's forbidden list). Regenerate bindings.
- [ ] **Step 4: Run** `browser_actions::`, `autorun_store::`, `pw_export_translate::`, `autorun_guide::` (the kinds list test), `seen_check::`, and `npx vitest run src/screens/AutoRun/describeAction.test.ts` - PASS.
- [ ] **Step 5: Guide.** Beside `remember_url` (`:300`): "`{ \"kind\": \"remember_text\", \"selector\": {...}, \"as\": \"counts\" }` - note what an element reads now, under a name, for a later `expect_text` with `\"same_as\": \"counts\"` in the same case (a before/after check: \"the pills keep the counts from step 4\"). The save refuses a `same_as` with no earlier `remember_text`." Add `remember_text` to the kinds list `:25` and an example to `action_help.rs`.
- [ ] **Step 6: Commit** (`feat(autorun): remember an element's text and check it is unchanged later`).

---

## Task 10: A repair can add one step without resending the script (61, part 2)

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (`apply_patch_steps` `:12719-12786`)
- Modify: `src-tauri/src/mcp.rs` (`save_autorun_script`'s `scripts` description `:645`), `src-tauri/src/autorun/guide.rs` (`:2560`, `:2763`, the "Saving it" section)
- Test: `src-tauri/tests/suite/ai_bridge.rs` (beside the existing `apply_patch_steps` test `:4513`)

**Interfaces:**
- `apply_patch_steps` keeps every current refusal except "is not in its saved script": a patch naming a step the saved script lacks is ADDED, as `{ step_number, actions, ... }`, in step-number order among the case steps. A patch with `"cleanup": true` replaces the saved clean-up step whatever its number, or adds one after the last case step when there is none; its `step` must be greater than every case step's number (`"case {id}: a clean-up step comes after the case's last step ({last})"`). A patch with `"cleanup": true` naming a case step's number is refused. The result is still the whole script, so `edits` must still declare the step and every save gate runs.
- Changes the refusal text for a patch with no `actions` only where it mentions adding.

- [ ] **Step 1: Write the failing tests** in `ai_bridge.rs`: `patch_steps_adds_a_missing_step_in_order` (saved steps 1, 2, 4; patch step 3 -> steps 1, 2, 3, 4, the others byte-identical, including a 5,000-character `fill` value in step 2); `patch_steps_adds_a_cleanup_step` (`{ "step": 5, "cleanup": true, "actions": [...] }` on a script with no clean-up -> last step has `cleanup: true`); `patch_steps_replaces_the_cleanup_step` (saved clean-up numbered 5, patch numbered 6 with `cleanup: true` -> one clean-up, numbered 6); `a_cleanup_patch_on_a_case_step_number_is_refused`; and update the existing refusal assertion for an unknown step (it now adds). Then a bridge-level test that a save carrying an added step and an `edits` entry NOT naming it is refused by the edit gate (`check_edits`), and naming it saves.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite ai_bridge::` - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `ai_bridge::`, `autorun_case_values::` (it also calls `apply_patch_steps`), `autorun_edits::`, `tcm_mcp::` - PASS.
- [ ] **Step 5: Texts.** `mcp.rs` `scripts` description: "A repair may send patch_steps in place of steps, each { step, actions } - changing a step, or adding one the script lacks (with \"cleanup\": true, the clean-up step)." Guide "Saving it": the same, with a one-line example adding a clean-up step. `cargo test --test suite autorun_guide::` and `tcm_mcp::`.
- [ ] **Step 6: Commit** (`feat(autorun): patch_steps can add one step, the clean-up included`).

---

## Task 11: A delete the case asks for keeps its check (61, part 3)

**Files:**
- Modify: `src-tauri/src/autorun/cleanup.rs` (a save note beside its existing save warnings)
- Modify: the save path that gathers notes (`ai_bridge.rs`, where the item-47/48 delete notes are added - `grep -n "cleanup::" src-tauri/src/ai_bridge.rs`)
- Modify: `src-tauri/src/autorun/guide.rs` (the clean-up paragraph, `:366-380` in the 44-53 plan's numbering; find "clean-up step"), `src-tauri/src/autorun/roles/writer.md` `:27`, `case-rewriter.md` `:19`, `healer.md` (if it speaks of the clean-up)
- Test: `src-tauri/tests/suite/autorun_cleanup.rs`, `src-tauri/tests/suite/autorun_roles.rs`

**Interfaces:**
- Produces: `pub const KEEP_THE_CASES_DELETE: &str = "a delete the case asks for stays in its step with its check; the clean-up holds a guarded fallback";`
- Produces: `pub fn moved_with_check(old: &CaseScript, new: &CaseScript) -> Vec<String>` - one note per case step N where (a) an action of old step N (compared by `action.kind()` and its target's `describe()`) is absent from new step N and present in the new clean-up step, and (b) new step N has fewer checks (`Action::is_check`) than old step N: `"step {n}: {kind} on {target} moved to the clean-up and the step lost a check - {KEEP_THE_CASES_DELETE}"`. Empty when there is no old script or no new clean-up step.
- The save adds these to its notes (never a refusal, R14).

- [ ] **Step 1: Write the failing tests** in `autorun_cleanup.rs`: `moving_a_delete_and_its_check_is_noted` (old step 6: click Delete in row + `expect_hidden`; new step 6: nothing of the two; clean-up: the click -> one note containing `step 6` and `KEEP_THE_CASES_DELETE`); `a_guarded_fallback_alongside_the_kept_delete_is_not_noted` (the step keeps its delete and check, the clean-up adds a `when_visible` delete -> empty); `a_new_script_is_not_noted`; and a role test in `autorun_roles.rs`: `writer_and_rewriter_keep_the_cases_delete_in_its_step` - both role texts (flattened) contain "stays in its step with its check".
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite autorun_cleanup::` and `autorun_roles::` - FAIL.
- [ ] **Step 3: Implement** the function and wire it into the save's notes.
- [ ] **Step 4: Roles and guide.** writer.md `:27` / case-rewriter.md `:19` (after Task 8's edit) become: "A delete the case asks for stays in its step with its check; the clean-up holds a guarded fallback (when_visible). Any delete finds its row by the name it holds (has_text), never by nth." The guide's clean-up paragraph says the same and that the save notes a delete moved with its check. Make room under the caps by tightening other lines of the same file without changing a rule (the role caps test is the judge).
- [ ] **Step 5: Run** `autorun_cleanup::`, `autorun_roles::`, `autorun_guide::` - PASS.
- [ ] **Step 6: Commit** (`fix(autorun): a delete the case asks for keeps its step and its check`).

---

## Task 12: get_accounts shows no password (62)

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (`accounts_read` `:11717-11805`, the route comment `:807-810`)
- Modify: `src-tauri/src/mcp.rs` (`get_accounts` description `:958-962`, the `get_guide` `quick` description `:1629`), `src-tauri/src/autorun/guide.rs` (`:1106-1109`, `active_environment_section` `:2950-2955` and its doc `:2919-2920`), `src-tauri/src/ai_tools.rs` (any skill line that says get_accounts gives passwords)
- Test: `src-tauri/tests/suite/environments.rs` (the bridge tests at `:1570-1600`), `src-tauri/tests/suite/autorun_guide.rs`

**Interfaces:**
- Each account in the answer: `{ "key", "label", "username", "has_password": bool, "in_use_by"? }` - `has_password` is `!password.trim().is_empty()`. No `password` key in any environment; `test_environment` stays in the answer (other tools read it). The `note` for a non-test environment goes; the log line drops ", passwords included".
- `propose_accounts` (`/accounts-propose`) unchanged: its stored proposals keep passwords (memory: proposed logins carry DB passwords).
- `active_environment_section` says nothing about passwords; it keeps "It is marked as a test environment." / "It is not marked as a test environment." for the other test-environment behaviours.

- [ ] **Step 1: Write the failing tests** in `environments.rs`: rename `get_accounts_shows_them_in_a_test_environment` to `get_accounts_never_shows_a_password` - in a test environment, `v["accounts"][0].get("password").is_none()`, `v["accounts"][0]["has_password"] == true`, and `out` does not contain `"pw-Zq9"`; `get_accounts_says_when_an_account_has_no_password` (empty password -> `has_password: false`); keep `get_accounts_is_not_logged`. In `autorun_guide.rs`: `the_live_environment_section_promises_no_password` - neither form of the section contains "password".
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite environments::` - FAIL.
- [ ] **Step 3: Implement**; update the texts: `get_accounts` description "...its accounts: key, label, username and whether a password is set. No password is ever shown: Auto Run signs in by itself, so a script or template names an account by its key." Remove "whether get_accounts gives passwords" from the `quick` description.
- [ ] **Step 4: Run** `environments::`, `autorun_guide::`, `tcm_mcp::`, `autorun_accounts::` - PASS.
- [ ] **Step 5: Commit** (`fix(autorun): get_accounts shows whether a password is set, never the password`).

---

## Task 13: Backlog status, owed hand checks, changelog proposal

**Files:**
- Modify: `docs/autorun/backlog-2026-10.md` (a "**Done in 2.4.3-beta.10.**" line under each of 55-62, as items 40 and 49 have)
- Modify: `docs/backlog.html` (the owed hand checks; the "Updated" line)
- No change to `src/lib/changelog.ts`, `tauri.conf.json` or `Cargo.toml` (the controller releases)

- [ ] **Step 1: Backlog.** Under each item, one line naming what shipped and any part left: 55 (active-only update, runs by environment, order rule, Import scripts…, "kept but no longer read" notice; already-migrated installs untouched); 56 (hold ends with a failed start; "held for" in the refusal); 57 (2-minute first page, "the site is slow to start"); 58 (row guard at run and save); 59 (`settle_ms`, row-gone pass ends quietly); 60 (`has_text`, `remember_text` + `same_as`); 61 (`when_visible`/`expect_hidden` in `then`, `patch_steps` adds a step, the clean-up note and role rule); 62 (`has_password`, no passwords).
- [ ] **Step 2: Owed hand checks** in `docs/backlog.html` (one `<li>Hand check owed: ...</li>` each, in the Auto Run section): a fresh flat install updated with two environments shows the move toast once, QA starts empty, runs land where they were made (Task 1); Import scripts… from the 582-script archive into QA Automation lists the 31 differing cases and leaves them unless Replace is ticked (Task 2); a subagent whose discovery start fails frees its case for the next (Task 3); the Philippines dev site after an idle night signs in within 2 minutes, or says it is slow to start (Task 4); the competency groups loop with `except` stops before deleting on the Definition Wizard tree (Task 5); 135412/135421 by-name delete loops on the slow site run once (Tasks 6-7); a has_text delete and a remember_text pills check on a Competencies case (Tasks 8-9); 135342 gets its clean-up step through one patch (Task 10); get_accounts on QA Automation lists no password (Task 12).
- [ ] **Step 3: Changelog proposal** - put this text in the hand-back to the controller (not in `changelog.ts`):
  - "Updating to environments no longer copies one environment's scripts into all of them: everything goes to the active environment, and each past run goes to the environment it was run in. The app says once what it moved."
  - "Import scripts… on the Auto Run Setup panel brings scripts from a folder into the environment you are in, and never replaces a case's script unless you tick Replace."
  - "A site that is slow to wake up is given about two minutes to show its first page, and a failure says the site is slow to start."
  - "A case an assistant could not start working on is let go at once, and a refusal says how long a case has been held."
  - "Delete loops in scripts are safer: one that cannot tell its rows apart stops before deleting anything, and each pass waits for the delete to finish."
  - "Scripts can pick a row by the text it holds and check that a value is unchanged from an earlier step; repairs can add one step without resending the script."
  - "Assistants no longer see account passwords."
- [ ] **Step 4: Follow-ups for the controller (not tasks):** How To Use (`docs-site/`) documents the Auto Run Setup panel: the Environment row now has Import scripts…, so its shot and page need retaking (`npm run docs:shots -- --only <setup shot id>`, then `npm run docs:build`); release notes as above.
- [ ] **Step 5: Commit** (`docs(autorun): backlog 55-62 status and owed hand checks`), heredoc as in Task 1.

---

## Self-review notes

- Coverage: 55 (T1, T2), 56 (T3), 57 (T4), 58 (T5), 59 (T6), 60 (T8, T9), 61 (T7, T10, T11), 62 (T12).
- Order: T1 first (the owner's reversal). T5 -> T6 -> T7 all touch `runner::repeat_while_visible` and `check_repeat`: run them in that order. T8 -> T11 both edit the same role lines: T8's wording is the base T11 extends. Bindings change in T1, T2, T6, T8, T9.
- Names used across tasks: `roots::BY_RUN`, `MigrationReport::runs_by_env` (T1), `env_copy::ImportScriptsPlan` (T2), `claims::case_claimed(_, _, held_ms)` (T3), `signin::SITE_SLOW_TO_START` (T4 -> patterns, transient), `repeat::except_guard`, `LoopRows` (T5), `RepeatWhileVisible::settle_ms` (T6), `LOOP_THEN_ALLOWS` (T7), `LocatorStep::has_text` (T8 -> T11 role text), `Action::RememberText`, `ExpectText::same_as` (T9), `cleanup::KEEP_THE_CASES_DELETE` (T11).
- Open for the owner before work starts: R1 (already-migrated untouched), R6 (where the button sits, scripts only), R7 (only a failed discovery start frees the hold), R8 (2 minutes), R9 (save notes, does not refuse, an untried loop), R15 (no role field).
