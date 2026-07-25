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
pub async fn pr_work_items(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    repo: String,
    pr_id: i32,
) -> Result<Vec<ado_git::PrWorkItem>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .pr_work_items(&organization, &project, &repo, pr_id)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn board_pr_links(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<ado_git::PrLink>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .board_pr_links(&organization, &project)
        .await
}

/// Build runs for one PR - validation + post-merge CI - each with its
/// stages and the environments a release carried it to.
#[tauri::command]
#[specta::specta]
pub async fn pr_pipeline(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    repo_id: String,
    pr_id: i32,
    merge_commit: String,
) -> Result<Vec<crate::pipelines::PrBuild>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .pr_builds(&organization, &project, &repo_id, pr_id, &merge_commit)
        .await
}

/// Plain-text output for one build step - the same content ADO's log pane
/// shows. Polled by the dialog while a step is running.
#[tauri::command]
#[specta::specta]
pub async fn build_log(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    build_id: i32,
    log_id: i32,
) -> Result<String, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .build_log(&organization, &project, build_id, log_id)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn repo_pull_requests(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    repo_id: String,
    status: String,
) -> Result<Vec<ado_git::PullRequest>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .repo_pull_requests(&organization, &project, &repo_id, &status)
        .await
}
