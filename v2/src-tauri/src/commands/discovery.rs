//! Org / project / PBI discovery and project-level lookups.

use crate::ado;
use crate::state::get_fresh_token;
use crate::work_board;

#[tauri::command]
#[specta::specta]
pub async fn list_orgs(app: tauri::AppHandle) -> Result<Vec<ado::Org>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).list_orgs().await
}

#[tauri::command]
#[specta::specta]
pub async fn list_projects(
    app: tauri::AppHandle,
    organization: String,
) -> Result<Vec<ado::Project>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).get_projects(&organization).await
}

#[tauri::command]
#[specta::specta]
pub async fn search_pbis(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    query: String,
) -> Result<Vec<ado::PbiHit>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .search_pbis(&organization, &project, &query, 20)
        .await
}

/// The project's Area or Iteration paths for the create pickers.
#[tauri::command]
#[specta::specta]
pub async fn classification_paths(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    structure: String,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_classification_paths(&organization, &project, &structure)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn list_project_tags(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).get_tags(&organization, &project).await
}

/// Iteration paths with sprint dates, for DevOps-style iteration pickers.
#[tauri::command]
#[specta::specta]
pub async fn list_iterations(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<work_board::IterationRef>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_iterations_dated(&organization, &project)
        .await
}
