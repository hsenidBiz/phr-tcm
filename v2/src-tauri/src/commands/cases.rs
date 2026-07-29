//! Reading and editing individual Test Case work items.

use crate::ado;
use crate::model;
use crate::state::get_fresh_token;

#[tauri::command]
#[specta::specta]
pub async fn pbi_test_cases(
    app: tauri::AppHandle,
    organization: String,
    pbi_id: i32,
) -> Result<Vec<ado::TestCaseSummary>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_pbi_test_cases(&organization, pbi_id)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn pbi_test_cases_full(
    app: tauri::AppHandle,
    organization: String,
    pbi_id: i32,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
) -> Result<Vec<ado::TestCaseFull>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_pbi_test_cases_full(
            &organization,
            pbi_id,
            module_ref.as_deref(),
            preconditions_ref.as_deref(),
        )
        .await
}

/// Test cases for arbitrary ids (suite browser handoffs).
#[tauri::command]
#[specta::specta]
pub async fn test_cases_by_ids(
    app: tauri::AppHandle,
    organization: String,
    ids: Vec<i32>,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
) -> Result<Vec<ado::TestCaseFull>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_test_cases_by_ids(
            &organization,
            &ids,
            module_ref.as_deref(),
            preconditions_ref.as_deref(),
        )
        .await
}

/// Save one existing case from the editor (no suite-ensure, no pacing).
/// The case must carry update_id; blank-skip semantics apply as always.
///
/// `original_steps_xml` is the Steps field as Azure DevOps currently holds
/// it, taken from the TestCaseFull this edit started from. Without it a
/// title-only save rewrites the steps from a plain-text read and strips
/// their formatting and embedded images - see `steps_patch`.
#[tauri::command]
#[specta::specta]
pub async fn update_test_case(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    tc: model::TestCase,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
    original_steps_xml: Option<String>,
) -> Result<(), String> {
    let id = tc.update_id.ok_or("update_test_case requires update_id")?;
    tc.is_valid()?;
    let token = get_fresh_token(&app).await.map_err(|e| e.to_string())?;
    ado::AdoClient::new(token)
        .update_test_case_from_model(
            &organization,
            &project,
            id,
            &tc,
            module_ref.as_deref(),
            preconditions_ref.as_deref(),
            original_steps_xml.as_deref(),
            // This command IS the editor. A field the user emptied is meant
            // to be emptied in Azure DevOps - skipping it left the old value
            // there while the app reported the save as done.
            ado::BlankPolicy::Clear,
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn list_test_case_fields(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<ado::FieldRef>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_test_case_fields(&organization, &project)
        .await
}

/// Values for ANY Test Case field: the definition's picklist when one
/// exists, otherwise the distinct values in use on the project's Test
/// Cases (many orgs keep Modules as plain values, not allowedValues).
#[tauri::command]
#[specta::specta]
pub async fn test_case_field_values(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    field_ref: String,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let picklist = client
        .get_field_allowed_values(&organization, &project, "Test Case", &field_ref)
        .await?;
    if !picklist.is_empty() {
        return Ok(picklist);
    }
    client
        .field_values_in_use(&organization, &project, &field_ref)
        .await
}

/// Whether this user may delete work items here, which is what decides
/// whether the app offers to at all.
///
/// Fails closed inside the client: anything short of an explicit yes from
/// Azure DevOps is a no. See `ado/recycle.rs` for why that asymmetry is
/// deliberate.
#[tauri::command]
#[specta::specta]
pub async fn can_delete_test_cases(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<bool, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    Ok(ado::AdoClient::new(token)
        .can_delete_work_items(&organization, &project)
        .await)
}

/// Move test cases to the project's RECYCLE BIN, where Azure DevOps can
/// restore them. This app has no permanent delete and issues no other
/// DELETE anywhere - see `ado/recycle.rs`, which is the only file allowed
/// to, and the tests that keep it that way.
///
/// Every id is reported individually: a partly-completed delete has to be
/// able to say which ones survived.
#[tauri::command]
#[specta::specta]
pub async fn delete_test_cases(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    ids: Vec<i32>,
) -> Result<Vec<ado::recycle::DeleteOutcome>, ado::AdoError> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    crate::applog::warn(format!(
        "deleting {} test case(s) to the recycle bin in {project}: {ids:?}",
        ids.len()
    ));
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .delete_test_cases_to_recycle_bin(&organization, &project, &ids)
        .await
}
