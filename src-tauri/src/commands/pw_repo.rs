//! The Playwright tab: the team's Playwright repo, read from its clone
//! folder. Offered exactly where Auto Run is (`ai_tools::autorun_offered`),
//! and refused everywhere else. The clone folder itself is the app setting
//! Export to Playwright uses (`set_playwright_clone`).

use crate::pw_repo::process::{self, Launch, RunEvent};
use crate::pw_repo::runner::{self, BrowserMode, PwReport, RunAction, RunPreview, RunStart, TestTree};
use crate::pw_repo::structure::{self, RepoStructure};
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

/// Runs `body` off the async workers with the clone's path and a starter
/// for `npx playwright …` there: it waits on repo commands.
async fn blocking<T: Send + 'static>(
    clone: String,
    body: impl FnOnce(&std::path::Path, runner::Starter) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    crate::pw_repo::ensure_offered(crate::ai_tools::autorun_offered())?;
    tauri::async_runtime::spawn_blocking(move || {
        let clone = PathBuf::from(clone.trim());
        let start = |args: &[String]| Launch::repo(&clone, "npx", args, process::resolve_here);
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

/// Start `npx playwright …` or `npm run users …` in the clone, in a job of
/// its own. Its output comes as `PwOutput` events and its end as `PwDone`.
/// Any other program or command is refused, and so is a second start while
/// one runs, with a sentence naming it.
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
    blocking(clone, move |clone, start| runner::list(clone, refresh, &show, start)).await
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
    blocking(clone, move |clone, start| runner::run(clone, &picked, action, mode, confirm, &show, start)).await
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
    blocking(clone, move |clone, start| runner::preview(clone, &picked, action, mode, &show, start)).await
}

/// The clone's archived reports, newest first.
#[tauri::command]
#[specta::specta]
pub fn pw_reports(clone: String) -> Result<Vec<PwReport>, String> {
    crate::pw_repo::ensure_offered(crate::ai_tools::autorun_offered())?;
    Ok(runner::reports(&PathBuf::from(clone.trim())))
}

/// Opens one archived report's `index.html` in the default browser.
#[tauri::command]
#[specta::specta]
pub fn pw_open_report(clone: String, run_id: String) -> Result<(), String> {
    crate::pw_repo::ensure_offered(crate::ai_tools::autorun_offered())?;
    let index = runner::report_index(&PathBuf::from(clone.trim()), &run_id)?;
    tauri_plugin_opener::open_path(&index, None::<&str>).map_err(|e| {
        crate::applog::warn(format!("pw-repo: a report could not be opened: {e}"));
        runner::REPORT_NOT_OPENED.to_string()
    })
}
