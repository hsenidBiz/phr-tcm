//! The Playwright tab: the team's Playwright repo, read from its clone
//! folder. Offered exactly where Auto Run is (`ai_tools::autorun_offered`),
//! and refused everywhere else. The clone folder itself is the app setting
//! Export to Playwright uses (`set_playwright_clone`).

use crate::pw_repo::process::{self, RunEvent};
use crate::pw_repo::structure::{self, RepoStructure};
use std::sync::Arc;
use tauri_specta::Event;

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
    let sink: process::Sink = Arc::new(move |event| match event {
        RunEvent::Output { run, line } => {
            let _ = crate::events::PwOutput { run, line }.emit(&app);
        }
        RunEvent::Done { run, code } => {
            let _ = crate::events::PwDone { run, code }.emit(&app);
        }
    });
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
