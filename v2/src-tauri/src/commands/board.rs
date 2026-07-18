//! Work Manager board: columns, items, details, comments, quick create.

use std::sync::Mutex;
use tauri::Manager;

use crate::state::get_fresh_token;
use crate::{ado, auth, work_board};

#[tauri::command]
#[specta::specta]
pub async fn fetch_board(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    area: Option<String>,
    pbi_id: Option<i32>,
    current_sprint: bool,
) -> Result<work_board::BoardData, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .fetch_board(&organization, &project, area.as_deref(), pbi_id, current_sprint)
        .await
}

/// Move a board item into a column: resolves the target state exactly like
/// v1 (_state_for_column) and PATCHes System.State. Returns the state set.
#[tauri::command]
#[specta::specta]
pub async fn move_board_item(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    item_id: i32,
    work_item_type: String,
    column: String,
) -> Result<String, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let states = client
        .get_work_item_states(&organization, &project, &work_item_type)
        .await?;
    let mut by_type = std::collections::HashMap::new();
    by_type.insert(work_item_type.clone(), states);
    let target = work_board::state_for_column(&work_item_type, &column, &by_type).ok_or(
        ado::AdoError::Http {
            status: 0,
            body: format!("no state maps to column '{column}' for {work_item_type}"),
        },
    )?;
    // Verified move: ADO rules (e.g. required dates) can reject or rewrite
    // the transition even on a 2xx, so trust only the persisted state.
    let actual = client
        .set_work_item_state(&organization, &project, item_id, &target)
        .await?;
    if actual != target {
        return Err(ado::AdoError::Http {
            status: 409,
            body: format!(
                "Azure DevOps kept #{item_id} in '{actual}' — moving to '{target}' is blocked by \
                 work item rules (for example required dates). Open the item, fill the required \
                 fields, then try again."
            ),
        });
    }
    Ok(target)
}

#[tauri::command]
#[specta::specta]
pub async fn list_teams(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<work_board::TeamRef>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).list_teams(&organization, &project).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_team_members(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<work_board::Member>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .list_team_members(&organization, &project)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn work_item_detail(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: i32,
) -> Result<work_board::WorkItemDetail, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_work_item_detail(&organization, &project, id)
        .await
}

/// PATCH a work item's fields (create-or-replace 'add' ops, only the
/// changed refs). ADO 4xx (invalid transition / required field) surfaces
/// verbatim for the drawer to show.
#[tauri::command]
#[specta::specta]
pub async fn update_work_item(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: i32,
    patches: Vec<work_board::FieldPatch>,
) -> Result<(), ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let fields: Vec<(String, String)> = patches
        .into_iter()
        .map(|p| (p.reference_name, p.value))
        .collect();
    ado::AdoClient::new(token)
        .update_work_item_fields(&organization, &project, id, &fields)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn activity_values(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    wi_type: String,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_field_allowed_values(
            &organization,
            &project,
            &wi_type,
            "Microsoft.VSTS.Common.Activity",
        )
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn work_item_comments(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: i32,
) -> Result<Vec<work_board::WorkComment>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_work_item_comments(&organization, &project, id)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn add_comment(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: i32,
    text: String,
) -> Result<(), ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .add_work_item_comment(&organization, &project, id, &text)
        .await
}

/// Best-effort avatar fetch (None -> initials disc in the UI).
#[tauri::command]
#[specta::specta]
pub async fn avatar_b64(app: tauri::AppHandle, url: String) -> Option<String> {
    let token = get_fresh_token(&app).await.ok()?;
    ado::AdoClient::new(token).get_avatar_b64(&url).await
}

/// Quick create a Task/Bug from the board, optionally assigned to me.
#[tauri::command]
#[specta::specta]
pub async fn quick_create_item(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    wi_type: String,
    title: String,
    assign_to_me: bool,
) -> Result<i32, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let mut fields = vec![("System.Title".to_string(), title)];
    if assign_to_me {
        let account = {
            let state = app.state::<Mutex<auth::AuthState>>();
            let s = state.lock().unwrap();
            s.tokens.as_ref().and_then(|t| t.account.clone())
        };
        if let Some(upn) = account {
            fields.push(("System.AssignedTo".to_string(), upn));
        }
    }
    let (id, _url) = ado::AdoClient::new(token)
        .create_work_item(&organization, &project, &wi_type, &fields, &[])
        .await?;
    Ok(id)
}
