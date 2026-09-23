//! Reading and saving a PBI's suggested run order (`tcm-run-order.json`).
//! Shared/suggested orders are written only from Suite Management (task 4)
//! and at upload (task 2/3); these two commands are the low-level read and
//! write both of those, and Run Tests' fallback read, sit on.

use tauri::Manager;

use crate::state::get_fresh_token;
use crate::{ado, run_order};

/// The PBI's suggested run order, or why it could not be read. Never
/// fails for "no file" or "damaged file" - see `RunOrderRead`.
#[tauri::command]
#[specta::specta]
pub async fn get_run_order(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
) -> Result<run_order::RunOrderRead, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .read_run_order(&organization, &project, pbi_id)
        .await
}

/// Save `cases` as the PBI's suggested run order. Fills `format`/`version`,
/// `saved_by` (the signed-in account from `AuthState`, or "unknown" when
/// somehow absent) and `saved_at` (now, UTC), saves, and returns the file
/// that was written so the caller can show who saved it and when without a
/// second read.
#[tauri::command]
#[specta::specta]
pub async fn save_run_order(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    cases: Vec<run_order::RunOrderCase>,
) -> Result<run_order::RunOrderFile, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let saved_by = {
        let state = app.state::<std::sync::Mutex<crate::auth::AuthState>>();
        let s = state.lock().unwrap();
        s.tokens
            .as_ref()
            .and_then(|t| t.account.clone())
            .unwrap_or_else(|| "unknown".to_string())
    };
    let file = run_order::RunOrderFile {
        format: run_order::RUN_ORDER_FORMAT.to_string(),
        version: run_order::RUN_ORDER_VERSION,
        saved_by,
        saved_at: run_order::now_rfc3339(),
        cases,
    };
    crate::applog::warn(format!(
        "saving suggested run order for {project} PBI #{pbi_id}: {} case(s)",
        file.cases.len()
    ));
    ado::AdoClient::new(token)
        .save_run_order(&organization, &project, pbi_id, &file)
        .await?;
    Ok(file)
}
