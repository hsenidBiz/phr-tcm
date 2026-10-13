//! The Playwright tab: the team's Playwright repo, read from its clone
//! folder. Offered exactly where Auto Run is (`ai_tools::autorun_offered`),
//! and refused everywhere else. The clone folder itself is the app setting
//! Export to Playwright uses (`set_playwright_clone`).

use crate::pw_repo::process::{self, Launch, RunEvent};
use crate::pw_repo::runner::{self, BrowserMode, PwReport, RunAction, RunPreview, RunStart, TestTree};
use crate::pw_export::export::{self, ExportResult};
use crate::pw_repo::structure::{self, RepoStructure};
use crate::pw_repo::sync::{self, SyncRow};
use crate::pw_repo::users::{self, AccountChange, ChangeOutcome, RepoAccount};
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
    crate::pw_repo::ensure_offered(crate::ai_tools::autorun_offered())?;
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
#[tauri::command]
#[specta::specta]
pub fn pw_repo_structure(clone: String) -> Result<RepoStructure, String> {
    structure::structure_for(crate::ai_tools::autorun_offered(), &clone, structure::node_found())
}

/// The repo command going now: its run id and how it is named.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct PwRunning {
    pub run: u32,
    pub label: String,
}

/// Start `npx playwright …` in the clone, in a job of its own. Its output
/// comes as `PwOutput` events and its end as `PwDone`. Any other program or
/// command is refused (`npm run users` goes through `pw_users_*`, which keep
/// passwords out of the output), and so is a second start while one runs,
/// with a sentence naming it.
#[tauri::command]
#[specta::specta]
pub async fn pw_repo_start(app: tauri::AppHandle, clone: String, program: String, args: Vec<String>) -> Result<u32, String> {
    let sink = emitter(app);
    // Off the async workers: it looks at the clone and PATH, and spawns.
    let offered = crate::ai_tools::autorun_offered();
    tauri::async_runtime::spawn_blocking(move || process::start_repo(offered, &clone, &program, &args, sink))
        .await
        .map_err(|_| process::COULD_NOT_START.to_string())?
}

/// End run `run` and every process it started. A run already over is fine.
#[tauri::command]
#[specta::specta]
pub fn pw_repo_stop(run: u32) -> Result<(), String> {
    crate::pw_repo::ensure_offered(crate::ai_tools::autorun_offered())?;
    process::stop(run)
}

/// The repo command going now, if any.
#[tauri::command]
#[specta::specta]
pub fn pw_repo_running() -> Result<Option<PwRunning>, String> {
    crate::pw_repo::ensure_offered(crate::ai_tools::autorun_offered())?;
    Ok(process::running().map(|(run, label)| PwRunning { run, label }))
}

/// The clone's tests as a tree, from `npx playwright test --list`. The
/// session's listing is reused unless `refresh`; ADO ids and last results
/// are read again either way.
#[tauri::command]
#[specta::specta]
pub async fn pw_list(app: tauri::AppHandle, clone: String, refresh: bool) -> Result<TestTree, String> {
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
    let show = emitter(app);
    blocking(clone, false, move |clone, start| runner::preview(clone, &picked, action, mode, &show, start)).await
}

/// The clone's archived reports, newest first.
#[tauri::command]
#[specta::specta]
pub fn pw_reports(clone: String) -> Result<Vec<PwReport>, String> {
    Ok(runner::reports(&chosen(&clone)?))
}

/// Opens one archived report's `index.html` in the default browser.
#[tauri::command]
#[specta::specta]
pub fn pw_open_report(clone: String, run_id: String) -> Result<(), String> {
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

/// The clone's test accounts, from `npm run users -- list --json`. No
/// password: Show asks for one with `pw_users_password`. Only the command's
/// end reaches the webview (`PwDone`), never a line of its output.
#[tauri::command]
#[specta::specta]
pub async fn pw_users_list(app: tauri::AppHandle, clone: String) -> Result<Vec<RepoAccount>, String> {
    let show = emitter(app);
    blocking_users(clone, move |clone, start| users::list(clone, &show, start)).await
}

/// Account `key`'s password, for Show and nothing else. Read from the
/// clone's `src/users/users.json`, never logged.
#[tauri::command]
#[specta::specta]
pub fn pw_users_password(clone: String, key: String) -> Result<String, String> {
    users::password(&chosen(&clone)?, &key)
}

/// A change to the accounts through `npm run users`. Without `apply` it
/// returns the command's preview and writes nothing; with `apply` it writes
/// and lists the accounts again. A refusal is the command's own words,
/// passwords hidden.
#[tauri::command]
#[specta::specta]
pub async fn pw_users_change(app: tauri::AppHandle, clone: String, change: AccountChange, apply: bool) -> Result<ChangeOutcome, String> {
    let show = emitter(app);
    blocking_users(clone, move |clone, start| users::change(clone, &change, apply, &show, start)).await
}

/// Every case exported to this clone for this project, and every script of
/// the project never exported, each with its state (`pw_repo::sync`).
/// Read only.
#[tauri::command]
#[specta::specta]
pub async fn pw_sync_states(app: tauri::AppHandle, clone: String, org: String, project: String) -> Result<Vec<SyncRow>, String> {
    let root = super::autorun::root(&app)?;
    let dir = super::pw_export::sync_dir(&app)?;
    let configured = crate::app_settings::current().playwright_clone;
    blocking(clone, false, move |_, _| sync::states(&dir, &root, &configured, &org, &project)).await
}

/// Exports `cases` again through Export to Playwright's own all-or-nothing
/// write, and records them: each one's section rewritten from Azure DevOps
/// (the repo's Added by engineer, Also and Assertion floor blocks kept),
/// its raw spec replaced, its `index.json` entry kept or added. Every other
/// section and file stays as it is. A refusal writes nothing
/// and leaves the record as it was. Each case is placed by its Module as
/// Azure DevOps has it now, else as it was at its last export.
#[tauri::command]
#[specta::specta]
pub async fn pw_sync_update(
    app: tauri::AppHandle,
    clone: String,
    org: String,
    project: String,
    cases: Vec<i32>,
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
    // Refuse an unexportable selection before any request is made, as
    // Export does, placing each case by the Module it was exported by. A
    // case never exported has no Module yet: the write itself checks it,
    // once Azure DevOps has given its Module.
    let recorded: BTreeMap<i32, String> =
        cases.iter().filter_map(|id| record.cases.get(id).map(|e| (*id, e.module.clone()))).collect();
    {
        let known: Vec<i32> = recorded.keys().copied().collect();
        let (root, org, project, configured) = (root.clone(), org.clone(), project.clone(), configured.clone());
        blocking(clone.clone(), false, move |_, _| export::ensure_exportable(&root, &org, &project, &known, &configured, &recorded))
            .await?;
    }
    let docs = super::pw_export::read_docs(&app, &org, &project, &cases, module_ref.as_deref(), preconditions_ref.as_deref()).await?;
    let modules: BTreeMap<i32, String> = cases
        .iter()
        .map(|id| {
            let now = docs.get(id).map(|d| d.module.trim().to_string()).filter(|m| !m.is_empty());
            let then = record.cases.get(id).map(|e| e.module.clone()).unwrap_or_default();
            (*id, now.unwrap_or(then))
        })
        .collect();
    blocking(clone, false, move |_, _| export::write_recorded(&root, &dir, &org, &project, &cases, &configured, &modules, &docs)).await
}
