//! The Playwright tab: the team's Playwright repo, read from its clone
//! folder. Offered while the Playwright module is on (`modules::on`), and
//! refused while it is off. The clone folder itself is the app setting
//! Export to Playwright uses (`set_playwright_clone`).

use crate::pw_repo::process::{self, Launch, RunEvent};
use crate::pw_repo::runner::{self, BrowserMode, PwReport, RunAction, RunPreview, RunStart, TestTree};
use crate::pw_export::export::{self, ExportResult};
use crate::pw_repo::handoff;
use crate::pw_repo::structure::{self, RepoStructure};
use crate::pw_repo::sync::{self, SyncRow};
use crate::pw_repo::users::{self, AccountChange, AccountRefs, ChangeOutcome, RepoAccount};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri_specta::Event;

/// A sink that emits a run's events to the webview as `PwOutput` / `PwDone`.
fn emitter(app: tauri::AppHandle) -> process::Sink {
    Arc::new(move |event| match event {
        RunEvent::Output { run, line } => {
            let _ = crate::events::PwOutput { run, line }.emit(&app);
        }
        RunEvent::Done { run, code } => {
            let _ = crate::events::PwDone { run, code }.emit(&app);
        }
    })
}

/// Refused where the tab is not offered; else the app's own clone setting,
/// if `clone` is it: the webview's path is never trusted on its own.
fn chosen(clone: &str) -> Result<PathBuf, String> {
    crate::pw_repo::ensure_offered(crate::modules::on(crate::modules::Module::Playwright))?;
    runner::chosen_clone(clone, &crate::app_settings::current().playwright_clone)
}

/// Runs `body` off the async workers with the clone's path and a starter
/// for `npx playwright …` there: it waits on repo commands. With `wait`,
/// each start waits for a short command (another listing, an accounts
/// command) to free the slot instead of being refused at once.
async fn blocking<T: Send + 'static>(
    clone: String,
    wait: bool,
    body: impl FnOnce(&std::path::Path, runner::Starter) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let clone = chosen(&clone)?;
    tauri::async_runtime::spawn_blocking(move || {
        let wait = if wait { process::SLOT_WAIT } else { std::time::Duration::ZERO };
        let start = |args: &[String]| Launch::repo(&clone, "npx", args, process::resolve_here).map(|l| l.waiting(wait));
        body(&clone, &start)
    })
    .await
    .map_err(|_| process::COULD_NOT_START.to_string())?
}

/// What the clone at `clone` holds, and the health line. Read only.
#[tauri::command(async)]
#[specta::specta]
pub fn pw_repo_structure(clone: String) -> Result<RepoStructure, String> {
    let _t = crate::pw_repo::timed("pw_repo_structure");
    let clone = chosen(&clone)?;
    structure::structure_for(true, &clone.to_string_lossy(), structure::node_found())
}

/// Makes the feature folder `path` (under `suites/`) in the clone, with
/// `test-cases/` and `specs/` kept in git by a `.gitkeep` each. Writes
/// nothing when it refuses (`new_folder::create_feature`).
#[tauri::command(async)]
#[specta::specta]
pub fn pw_repo_create_folder(clone: String, path: String) -> Result<crate::pw_repo::new_folder::NewFolder, String> {
    let _t = crate::pw_repo::timed("pw_repo_create_folder");
    let clone = chosen(&clone)?;
    crate::pw_export::clone::open(&clone)?;
    crate::pw_repo::new_folder::create_feature(&clone, &path)
}

/// The repo command going now: its run id, how it is named, and whether it
/// is a short command (a listing, a dry count, an accounts command) rather
/// than a test run.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct PwRunning {
    pub run: u32,
    pub label: String,
    pub brief: bool,
}

/// End run `run` and every process it started. A run already over is fine.
#[tauri::command]
#[specta::specta]
pub fn pw_repo_stop(run: u32) -> Result<(), String> {
    let _t = crate::pw_repo::timed("pw_repo_stop");
    crate::pw_repo::ensure_offered(crate::modules::on(crate::modules::Module::Playwright))?;
    process::stop(run)
}

/// The repo command going now, if any.
#[tauri::command]
#[specta::specta]
pub fn pw_repo_running() -> Result<Option<PwRunning>, String> {
    let _t = crate::pw_repo::timed("pw_repo_running");
    crate::pw_repo::ensure_offered(crate::modules::on(crate::modules::Module::Playwright))?;
    Ok(process::running_now().map(|(run, label, brief)| PwRunning { run, label, brief }))
}

/// The clone's tests as a tree, from `npx playwright test --list`. The
/// session's listing is reused unless `refresh`; ADO ids and last results
/// are read again either way.
#[tauri::command]
#[specta::specta]
pub async fn pw_list(app: tauri::AppHandle, clone: String, refresh: bool) -> Result<TestTree, String> {
    let _t = crate::pw_repo::timed("pw_list");
    let show = emitter(app);
    blocking(clone, true, move |clone, start| runner::list(clone, refresh, &show, start)).await
}

/// Runs the picked tests, every test, or last time's failures, the way
/// `npm run qa` does: the run list, the dry count, then the run. A count that
/// disagrees refuses with both numbers. A debug run of more than one test
/// answers `ConfirmDebug` until `confirm` is true.
#[tauri::command]
#[specta::specta]
pub async fn pw_run(
    app: tauri::AppHandle,
    clone: String,
    picked: Vec<String>,
    action: RunAction,
    mode: BrowserMode,
    confirm: bool,
) -> Result<RunStart, String> {
    let _t = crate::pw_repo::timed("pw_run");
    let show = emitter(app);
    blocking(clone, false, move |clone, start| runner::run(clone, &picked, action, mode, confirm, &show, start)).await
}

/// The run list and the command a run would use, without running it.
#[tauri::command]
#[specta::specta]
pub async fn pw_preview(
    app: tauri::AppHandle,
    clone: String,
    picked: Vec<String>,
    action: RunAction,
    mode: BrowserMode,
) -> Result<RunPreview, String> {
    let _t = crate::pw_repo::timed("pw_preview");
    let show = emitter(app);
    blocking(clone, false, move |clone, start| runner::preview(clone, &picked, action, mode, &show, start)).await
}

/// The clone's archived reports, newest first.
#[tauri::command]
#[specta::specta]
pub async fn pw_reports(clone: String) -> Result<Vec<PwReport>, String> {
    let _t = crate::pw_repo::timed("pw_reports");
    let clone = chosen(&clone)?;
    // The first read parses every report: on a blocking thread.
    tauri::async_runtime::spawn_blocking(move || runner::reports(&clone)).await.map_err(|e| {
        crate::applog::warn(format!("pw-repo: the reports could not be read: {e}"));
        runner::REPORTS_NOT_READ.to_string()
    })
}

/// Opens one archived report's `index.html` in the default browser.
#[tauri::command(async)]
#[specta::specta]
pub fn pw_open_report(clone: String, run_id: String) -> Result<(), String> {
    let _t = crate::pw_repo::timed("pw_open_report");
    let index = runner::report_index(&chosen(&clone)?, &run_id)?;
    tauri_plugin_opener::open_path(&index, None::<&str>).map_err(|e| {
        crate::applog::warn(format!("pw-repo: a report could not be opened: {e}"));
        runner::REPORT_NOT_OPENED.to_string()
    })
}

/// Runs `body` off the async workers with the clone's path and a starter
/// for `npm run users …` there, which waits for a short command (a
/// listing, another accounts command) to free the slot.
async fn blocking_users<T: Send + 'static>(
    clone: String,
    body: impl FnOnce(&std::path::Path, users::Starter) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let clone = chosen(&clone)?;
    tauri::async_runtime::spawn_blocking(move || {
        let start = |args: &[String]| Launch::repo(&clone, "npm", args, process::resolve_here).map(|l| l.waiting(process::SLOT_WAIT));
        body(&clone, &start)
    })
    .await
    .map_err(|_| process::COULD_NOT_START.to_string())?
}

/// The clone's test accounts, read from its `src/users/users.json`: no
/// repo command, no wait for the run slot. No password: Show asks for one
/// with `pw_users_password`.
#[tauri::command(async)]
#[specta::specta]
pub fn pw_users_list(clone: String) -> Result<Vec<RepoAccount>, String> {
    let _t = crate::pw_repo::timed("pw_users_list");
    users::accounts(&chosen(&clone)?)
}

/// What uses each account, from `npm run users -- list --json`: asked for
/// after the list, as a short repo command. Only the command's end reaches
/// the webview (`PwDone`), never a line of its output.
#[tauri::command]
#[specta::specta]
pub async fn pw_users_refs(app: tauri::AppHandle, clone: String) -> Result<Vec<AccountRefs>, String> {
    let _t = crate::pw_repo::timed("pw_users_refs");
    let show = emitter(app);
    blocking_users(clone, move |clone, start| users::refs(clone, &show, start)).await
}

/// Account `key`'s password, for Show and nothing else. Read from the
/// clone's `src/users/users.json`, never logged.
#[tauri::command(async)]
#[specta::specta]
pub fn pw_users_password(clone: String, key: String) -> Result<String, String> {
    let _t = crate::pw_repo::timed("pw_users_password");
    users::password(&chosen(&clone)?, &key)
}

/// A change to the accounts through `npm run users`. Without `apply` it
/// returns the command's preview and writes nothing; with `apply` it writes
/// and lists the accounts again. A refusal is the command's own words,
/// passwords hidden.
#[tauri::command]
#[specta::specta]
pub async fn pw_users_change(app: tauri::AppHandle, clone: String, change: AccountChange, apply: bool) -> Result<ChangeOutcome, String> {
    let _t = crate::pw_repo::timed("pw_users_change");
    let show = emitter(app);
    blocking_users(clone, move |clone, start| users::change(clone, &change, apply, &show, start)).await
}

/// Every case exported to this clone for this project, and every script of
/// the project never exported, each with its state (`pw_repo::sync`).
/// Read only.
#[tauri::command]
#[specta::specta]
pub async fn pw_sync_states(app: tauri::AppHandle, clone: String, org: String, project: String) -> Result<Vec<SyncRow>, String> {
    let _t = crate::pw_repo::timed("pw_sync_states");
    let root = super::autorun::root(&app)?;
    let dir = super::pw_export::sync_dir(&app)?;
    let configured = crate::app_settings::current().playwright_clone;
    blocking(clone, false, move |_, _| sync::states(&dir, &root, &configured, &org, &project)).await
}

/// Finish in the repo: for `cases` (what an export just wrote), one prompt
/// per test-case file that runs `/gen-test` steps 4, 5, 6 and 7 on them in
/// the clone (`pw_repo::handoff`), into the spec of the same stem. Each case
/// must be one this app exported, with
/// its raw spec and section in the clone. Only text: nothing is started.
#[tauri::command]
#[specta::specta]
pub async fn pw_handoff(
    app: tauri::AppHandle,
    clone: String,
    org: String,
    project: String,
    cases: Vec<i32>,
) -> Result<Vec<handoff::HandoffCard>, String> {
    let _t = crate::pw_repo::timed("pw_handoff");
    let dir = super::pw_export::sync_dir(&app)?;
    let autorun = super::autorun::root(&app)?;
    let configured = crate::app_settings::current().playwright_clone;
    let written = super::pw_export::written(&configured);
    blocking(clone, false, move |root, _| {
        let record = sync::load(&sync::record_path(&dir, &configured, &org, &project))?;
        let runs = crate::autorun::store::list_runs(&autorun);
        let mapped: BTreeMap<i32, String> = cases
            .iter()
            .filter_map(|&id| export::mapped_user_key(&autorun, &org, &project, id, &runs).map(|k| (id, k)))
            .collect();
        handoff::cards_at(root, &record, &written, &mapped, &cases)
    })
    .await
}

/// Exports `cases` again through Export to Playwright's own all-or-nothing
/// write, and records them: each one's section rewritten from Azure DevOps
/// (the repo's Added by engineer, Also and Assertion floor blocks kept),
/// its raw spec replaced, its `index.json` entry kept or added. Every other
/// section and file stays as it is. A refusal writes nothing
/// and leaves the record as it was. Each case is placed by its Module as
/// Azure DevOps has it now, else as it was at its last export.
///
/// A case this app exported may be replaced; one the repo has that this
/// app did not export (InRepoNotFromApp) only when it is in `replace`, the
/// cases the person ticked. Update out of date sends none.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn pw_sync_update(
    app: tauri::AppHandle,
    clone: String,
    org: String,
    project: String,
    cases: Vec<i32>,
    replace: Vec<i32>,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
) -> Result<ExportResult, String> {
    let _t = crate::pw_repo::timed("pw_sync_update");
    update(app, clone, org, project, cases, replace, module_ref, preconditions_ref).await
}

/// Add new: exports every case with a script that the repo has in none of
/// its places (`sync::add_new`), through the same write as Update, and
/// nothing else. Which cases is worked out here, not taken from the
/// webview, and the write refuses any case the repo has. A new case that
/// cannot be exported on its own is left out and listed with why
/// (`export::add_new_with`); the rest go as one all-or-nothing write.
#[tauri::command]
#[specta::specta]
pub async fn pw_sync_add_new(
    app: tauri::AppHandle,
    clone: String,
    org: String,
    project: String,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
) -> Result<ExportResult, String> {
    let _t = crate::pw_repo::timed("pw_sync_add_new");
    let root = super::autorun::root(&app)?;
    let dir = super::pw_export::sync_dir(&app)?;
    let configured = crate::app_settings::current().playwright_clone;
    let (o, p) = (org.clone(), project.clone());
    let (d, r, c) = (dir.clone(), root.clone(), configured.clone());
    let rows = blocking(clone.clone(), false, move |_, _| sync::states(&d, &r, &c, &o, &p)).await?;
    let cases = sync::add_new(&rows);
    if cases.is_empty() {
        return Ok(super::pw_export::nothing_written());
    }
    let record = sync::load(&sync::record_path(&dir, &configured, &org, &project))?;
    // Only the cases that could go are read; a read that fails for one is
    // that case left out, not Add new refused.
    let to_read = export::worth_reading(&root, &cases);
    let (docs, unread) = read_docs_each(&app, &org, &project, &to_read, module_ref.as_deref(), preconditions_ref.as_deref()).await;
    let modules = modules_for(&cases, &docs, &record);
    let result = blocking(clone, false, move |_, _| {
        export::add_new_with(&root, &dir, &org, &project, &cases, &configured, &modules, &docs, &unread)
    })
    .await?;
    super::pw_export::remember_written(&crate::app_settings::current().playwright_clone, &result);
    Ok(result)
}

/// `read_docs` for `cases` together, and when that fails, for each case on
/// its own: the docs read, and the cases that could not be. Each failure is
/// logged.
async fn read_docs_each(
    app: &tauri::AppHandle,
    org: &str,
    project: &str,
    cases: &[i32],
    module_ref: Option<&str>,
    preconditions_ref: Option<&str>,
) -> (BTreeMap<i32, crate::pw_export::test_case::CaseDoc>, Vec<i32>) {
    if cases.is_empty() {
        return (BTreeMap::new(), vec![]);
    }
    match super::pw_export::read_docs(app, org, project, cases, module_ref, preconditions_ref).await {
        Ok(docs) => return (docs, vec![]),
        Err(e) => crate::applog::warn(format!("pw-sync: Add new could not read its cases together ({e}), so each is read on its own")),
    }
    let (mut docs, mut unread) = (BTreeMap::new(), Vec::new());
    for &id in cases {
        match super::pw_export::read_docs(app, org, project, &[id], module_ref, preconditions_ref).await {
            Ok(one) => docs.extend(one),
            Err(e) => {
                crate::applog::warn(format!("pw-sync: Add new could not read case {id}: {e}"));
                unread.push(id);
            }
        }
    }
    (docs, unread)
}

/// Each case's Module as Azure DevOps has it now, else as it was at its
/// last export.
fn modules_for(
    cases: &[i32],
    docs: &BTreeMap<i32, crate::pw_export::test_case::CaseDoc>,
    record: &sync::ExportRecord,
) -> BTreeMap<i32, String> {
    cases
        .iter()
        .map(|id| {
            let now = docs.get(id).map(|d| d.module.trim().to_string()).filter(|m| !m.is_empty());
            let then = record.cases.get(id).map(|e| e.module.clone()).unwrap_or_default();
            (*id, now.unwrap_or(then))
        })
        .collect()
}

/// The body of Update and Add new.
#[allow(clippy::too_many_arguments)]
async fn update(
    app: tauri::AppHandle,
    clone: String,
    org: String,
    project: String,
    cases: Vec<i32>,
    replace: Vec<i32>,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
) -> Result<ExportResult, String> {
    chosen(&clone)?;
    if cases.is_empty() {
        return Ok(super::pw_export::nothing_written());
    }
    let root = super::autorun::root(&app)?;
    let dir = super::pw_export::sync_dir(&app)?;
    let configured = crate::app_settings::current().playwright_clone;
    let record = sync::load(&sync::record_path(&dir, &configured, &org, &project))?;
    let replace = sync::update_replace(&record, &cases, &replace);
    // Refuse an unexportable selection before any request is made, as
    // Export does, placing each case by the Module it was exported by. A
    // case never exported has no Module yet: the write itself checks it,
    // once Azure DevOps has given its Module. A case the repo has that may
    // not be replaced is refused here for all of them.
    let recorded: BTreeMap<i32, String> =
        cases.iter().filter_map(|id| record.cases.get(id).map(|e| (*id, e.module.clone()))).collect();
    {
        let known: Vec<i32> = recorded.keys().copied().collect();
        let (root, org, project, configured) = (root.clone(), org.clone(), project.clone(), configured.clone());
        let (all, replace) = (cases.clone(), replace.clone());
        blocking(clone.clone(), false, move |c, _| {
            export::refuse_present(&crate::pw_repo::presence::scan(c)?, &all, &replace)?;
            export::ensure_exportable(&root, &org, &project, &known, &configured, &recorded, &replace)
        })
        .await?;
    }
    let docs = super::pw_export::read_docs(&app, &org, &project, &cases, module_ref.as_deref(), preconditions_ref.as_deref()).await?;
    let modules = modules_for(&cases, &docs, &record);
    let written_to = configured.clone();
    let result = blocking(clone, false, move |_, _| {
        export::write_recorded(&root, &dir, &org, &project, &cases, &configured, &modules, &docs, &replace)
    })
    .await?;
    super::pw_export::remember_written(&written_to, &result);
    Ok(result)
}
