//! Reading and saving a PBI's suggested run order (`tcm-run-order.json`).
//! Shared/suggested orders are written only from Suite Management (task 6)
//! and at upload (task 2); these two commands are the low-level read and
//! write both of those, and Run Tests' fallback read, sit on.

use tauri::Manager;

use crate::state::get_fresh_token;
use crate::{ado, run_order};

/// Who a saved run order is "saved by": the signed-in account from
/// `AuthState`, as the context bar shows it, or "unknown" when somehow
/// absent. Shared by the save command and the upload so both name the
/// saver the same way.
pub(crate) fn saved_by(app: &tauri::AppHandle) -> String {
    let state = app.state::<std::sync::Mutex<crate::auth::AuthState>>();
    let s = state.lock().unwrap();
    s.tokens
        .as_ref()
        .and_then(|t| t.account.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

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
    let file = run_order::new_file(saved_by(&app), cases);
    crate::applog::warn(format!(
        "saving suggested run order for {project} PBI #{pbi_id}: {} case(s)",
        file.cases.len()
    ));
    ado::AdoClient::new(token)
        .save_run_order(&organization, &project, pbi_id, &file)
        .await?;
    Ok(file)
}
