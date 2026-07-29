//! Work Manager board: columns, items, details, comments, creation.

use crate::state::get_fresh_token;
use crate::{ado, work_board};

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
    let client = ado::AdoClient::new(token);
    let mut fields: Vec<(String, String)> = patches
        .into_iter()
        .map(|p| (p.reference_name, p.value))
        .collect();

    // System.State goes through the VERIFIED path, the same one the board's
    // drag already uses. Azure DevOps can keep or rewrite a state on a 2xx
    // when a rule blocks the transition, and this command threw the
    // response away - so the drawer said "Saved", the item stayed where it
    // was, and the drawer went on showing the state it had asked for.
    // (It does not self-correct: the refetch is deeply equal, so react-query
    // hands back the same object and the effect that reseeds the form never
    // runs. The drawer and the board disagree until it is reopened.)
    //
    // Only this field. The rest cannot be compared: Description and the
    // rich-text pages come back sanitised, System.AssignedTo comes back as
    // an identity object rather than the unique name that was sent, and
    // dates and numbers come back typed.
    let state = fields
        .iter()
        .position(|(r, _)| r == "System.State")
        .map(|i| fields.remove(i).1);

    if !fields.is_empty() {
        client
            .update_work_item_fields(&organization, &project, id, &fields)
            .await?;
    }
    if let Some(target) = state {
        let actual = client
            .set_work_item_state(&organization, &project, id, &target)
            .await?;
        if actual != target {
            return Err(ado::AdoError::Http {
                status: 409,
                body: format!(
                    "Azure DevOps kept #{id} in '{actual}' - moving to '{target}' is blocked by \
                     work item rules (for example required dates). Any other changes were saved. \
                     Fill the required fields, then set the state again."
                ),
            });
        }
    }
    Ok(())
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

/// A work item's revision history, newest first. Read only.
#[tauri::command]
#[specta::specta]
pub async fn work_item_history(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: i32,
) -> Result<Vec<work_board::WorkRevision>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_work_item_history(&organization, &project, id)
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

#[derive(serde::Serialize, specta::Type)]
pub struct CreatedItem {
    pub id: i32,
    pub url: String,
}

/// Everything the New Work Item screen collects. Empty optional fields
/// are skipped; `parent_id` nests the item under its PBI/Feature via a
/// Hierarchy-Reverse relation.
#[derive(serde::Deserialize, specta::Type)]
pub struct NewWorkItem {
    pub wi_type: String,
    pub title: String,
    pub assigned_to: Option<String>,
    pub area_path: Option<String>,
    pub iteration_path: Option<String>,
    pub tags: Option<String>,
    pub priority: Option<i32>,
    pub description: Option<String>,
    pub parent_id: Option<i32>,
}

/// Full-form work item creation (the New Work Item screen). POST only.
#[tauri::command]
#[specta::specta]
pub async fn create_work_item(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    item: NewWorkItem,
) -> Result<CreatedItem, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let mut fields = vec![("System.Title".to_string(), item.title)];
    let mut push = |name: &str, val: Option<String>| {
        if let Some(v) = val.filter(|s| !s.trim().is_empty()) {
            fields.push((name.to_string(), v));
        }
    };
    push("System.AssignedTo", item.assigned_to);
    push("System.AreaPath", item.area_path);
    push("System.IterationPath", item.iteration_path);
    push("System.Tags", item.tags);
    push("Microsoft.VSTS.Common.Priority", item.priority.map(|p| p.to_string()));
    push("System.Description", item.description);
    let (id, url) = ado::AdoClient::new(token)
        .create_work_item(&organization, &project, &item.wi_type, &fields, &[], item.parent_id)
        .await?;
    Ok(CreatedItem { id, url })
}
