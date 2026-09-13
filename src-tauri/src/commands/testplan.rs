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
    let ensured = client
        .ensure_requirement_suite_cb(&organization, &project, pbi_id, &area, &iteration, move |done, total| {
            let _ = SuiteScanProgress { done, total }.emit(&emitter);
        })
        .await?;
    // Shared with the upload and the AI bridge, so a PBI Run Tests has
    // resolved is never scanned for again in this session. A Shift-click
    // re-detect lands here too and overwrites the entry.
    ado_testplan::remember_suite(&client.base_url, &organization, &project, pbi_id, &ensured);
    Ok(ensured)
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
    let found = client
        .find_pbi_requirement_suite(&organization, &project, pbi_id, &area)
        .await?;
    if let Some(s) = &found {
        ado_testplan::remember_suite(&client.base_url, &organization, &project, pbi_id, s);
    }
    Ok(found)
}

/// The order of a suite's entries (child suites first, then test cases),
/// for the Manage Test Cases list. Read only.
#[tauri::command]
#[specta::specta]
pub async fn list_suite_entries(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    suite_id: i32,
) -> Result<Vec<ado_testplan::SuiteEntry>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_suite_entries(&organization, &project, suite_id)
        .await
}

/// Put a suite's test cases in the given order. Cases not named keep
/// their place after the named ones; child suites are not touched.
/// Returns the order the server reports back.
#[tauri::command]
#[specta::specta]
pub async fn reorder_suite_cases(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    suite_id: i32,
    case_ids: Vec<i32>,
) -> Result<Vec<i32>, ado::AdoError> {
    crate::applog::warn(format!(
        "reordering {} test case(s) in {project} suite #{suite_id}: {case_ids:?}",
        case_ids.len()
    ));
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .reorder_suite_cases(&organization, &project, suite_id, &case_ids)
        .await
}

/// A static child suite (a "folder") under a static parent or the plan
/// root. The name is trimmed; an empty one is refused here rather than
/// sent, so the message names the real problem.
#[tauri::command]
#[specta::specta]
pub async fn create_static_suite(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
    parent_suite_id: i32,
    name: String,
) -> Result<ado_testplan::SuiteRef, ado::AdoError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(ado::AdoError::Http {
            status: 0,
            body: "A folder needs a name.".to_string(),
        });
    }
    crate::applog::warn(format!(
        "creating static suite \"{name}\" in {project} plan #{plan_id} under suite #{parent_suite_id}"
    ));
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .create_static_suite(&organization, &project, plan_id, parent_suite_id, &name)
        .await
}

/// Copy existing test cases into a suite: they stay wherever they already
/// were. Returns the ids the server reports as now in the suite.
#[tauri::command]
#[specta::specta]
pub async fn add_cases_to_suite(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
    suite_id: i32,
    case_ids: Vec<i32>,
) -> Result<Vec<i32>, ado::AdoError> {
    if case_ids.is_empty() {
        return Ok(vec![]);
    }
    crate::applog::warn(format!(
        "adding {} test case(s) to {project} plan #{plan_id} suite #{suite_id}: {case_ids:?}",
        case_ids.len()
    ));
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .add_test_cases_to_suite(&organization, &project, plan_id, suite_id, &case_ids)
        .await
}
