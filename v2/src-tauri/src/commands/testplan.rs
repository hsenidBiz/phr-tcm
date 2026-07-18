//! Test plan / suite discovery and the PBI's requirement suite.

use tauri_specta::Event;

use crate::events::SuiteScanProgress;
use crate::state::get_fresh_token;
use crate::{ado, ado_testplan};

#[tauri::command]
#[specta::specta]
pub async fn list_plans_with_suites(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<ado_testplan::PlanWithSuites>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let emitter = app.clone();
    ado::AdoClient::new(token)
        .list_plans_with_suites_cb(&organization, &project, move |done, total| {
            let _ = SuiteScanProgress { done, total }.emit(&emitter);
        })
        .await
}

/// Find-or-create the PBI's requirement suite and return it with its plan.
#[tauri::command]
#[specta::specta]
pub async fn ensure_pbi_suite(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
) -> Result<ado_testplan::EnsuredSuite, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let (area, iteration) = client
        .get_work_item_paths(&organization, &project, pbi_id)
        .await?;
    let emitter = app.clone();
    client
        .ensure_requirement_suite_cb(&organization, &project, pbi_id, &area, &iteration, move |done, total| {
            let _ = SuiteScanProgress { done, total }.emit(&emitter);
        })
        .await
}

/// Read-only suite lookup for background prefetch: finds the PBI's
/// requirement suite if one exists anywhere, but NEVER creates a plan or
/// suite (creation stays on the Run Tests screen where the user asked).
#[tauri::command]
#[specta::specta]
pub async fn find_pbi_suite(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
) -> Result<Option<ado_testplan::EnsuredSuite>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let (area, _iteration) = client
        .get_work_item_paths(&organization, &project, pbi_id)
        .await?;
    client
        .find_pbi_requirement_suite(&organization, &project, pbi_id, &area)
        .await
}
