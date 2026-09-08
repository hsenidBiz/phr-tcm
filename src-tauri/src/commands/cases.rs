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
///
/// `original_tags` is System.Tags from that same TestCaseFull - without
/// it a removed tag cannot actually be removed (ADO merges tag writes
/// made with the plain `add` op; see `tags_write_ops`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn update_test_case(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    tc: model::TestCase,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
    original_steps_xml: Option<String>,
    original_tags: Option<String>,
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
            original_tags.as_deref(),
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
    pbi_id: Option<i32>,
) -> Result<bool, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    // With a PBI in hand, ask about ITS area - the one the cases being
    // shown actually live under. Area permissions are per node, and the
    // root said yes to a user the TCM API then refused; the sign-in-time
    // call passes no PBI and gets the root heuristic.
    let area = match pbi_id {
        Some(id) => match client.get_work_item_paths(&organization, &project, id).await {
            Ok((area, _)) => Some(area),
            // Fail closed: if the area cannot be learned, do not offer a
            // button whose real gate could not be asked.
            Err(_) => return Ok(false),
        },
        None => None,
    };
    Ok(client
        .can_delete_work_items(&organization, &project, area.as_deref())
        .await)
}

/// Move test cases from one PBI's Tested By list to another's - the fix
/// for a case that landed in the wrong PBI. One rev-guarded PATCH per
/// case (remove old link + add new in a single operation), reported per
/// id so a partial move can say exactly which cases went. Reversible:
/// moving them back is the same call with the PBIs swapped.
#[tauri::command]
#[specta::specta]
pub async fn relink_test_cases(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    ids: Vec<i32>,
    from_pbi: i32,
    to_pbi: i32,
) -> Result<Vec<ado::RelinkOutcome>, ado::AdoError> {
    if ids.is_empty() || from_pbi == to_pbi {
        return Ok(vec![]);
    }
    crate::applog::warn(format!(
        "relinking {} test case(s) in {project} from PBI #{from_pbi} to #{to_pbi}: {ids:?}",
        ids.len()
    ));
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        match client
            .relink_test_case(&organization, &project, id, from_pbi, to_pbi)
            .await
        {
            Ok(()) => out.push(ado::RelinkOutcome { id, moved: true, error: None }),
            Err(e) => out.push(ado::RelinkOutcome { id, moved: false, error: Some(e) }),
        }
    }
    Ok(out)
}

/// PERMANENTLY delete test cases through the Test Management API - the
/// only deletion Azure DevOps offers for test artifacts, and irreversible.
/// See `ado/deletion.rs`, the one file allowed to issue a DELETE, and the
/// tests that keep it that way. The confirm dialog carries the warning;
/// this command carries the audit trail.
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
) -> Result<Vec<ado::deletion::DeleteOutcome>, ado::AdoError> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    crate::applog::warn(format!(
        "PERMANENTLY deleting {} test case(s) in {project}: {ids:?}",
        ids.len()
    ));
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .delete_test_cases_permanently(&organization, &project, &ids)
        .await
}
