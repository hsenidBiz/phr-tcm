//! Pull-request panel reads (Work Manager). GET only.

use crate::ado_git;
use crate::state::get_fresh_token;
use crate::ado;

#[tauri::command]
#[specta::specta]
pub async fn list_repos(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<ado_git::RepoRef>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).list_repos(&organization, &project).await
}

#[tauri::command]
#[specta::specta]
pub async fn pr_overview(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<ado_git::PrOverview, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).pr_overview(&organization, &project).await
}

#[tauri::command]
#[specta::specta]
pub async fn repo_pull_requests(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    repo_id: String,
) -> Result<Vec<ado_git::PullRequest>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .repo_pull_requests(&organization, &project, &repo_id)
        .await
}
