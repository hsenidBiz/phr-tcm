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

/// The project's tag names, served from the shared reference cache.
///
/// Serve-then-revalidate: a cached list comes back immediately - instantly
/// on the second launch, since the cache is on disk - and a stale one is
/// refreshed in the background for next time. Only a completely cold cache
/// waits on Azure DevOps. The AI bridge reads the same cache, so an
/// assistant asking for tags costs nothing extra (see cache/mod.rs).
#[tauri::command]
#[specta::specta]
pub async fn list_project_tags(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<String>, ado::AdoError> {
    let key = crate::cache::keys::tags(&organization, &project);
    if let Some(v) = crate::cache::fresh::<Vec<String>>(&key, crate::cache::keys::TAGS_TTL_MS) {
        return Ok(v);
    }
    if let Some(stale) = crate::cache::get::<Vec<String>>(&key) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            // Best effort: a failed background refresh just leaves the
            // stale list in place until the next attempt.
            if let Ok(token) = get_fresh_token(&app).await {
                if let Ok(fresh) = ado::AdoClient::new(token)
                    .get_tags(&organization, &project)
                    .await
                {
                    crate::cache::put(&key, &fresh);
                }
            }
        });
        return Ok(stale);
    }
    let token = get_fresh_token(&app).await?;
    let tags = ado::AdoClient::new(token).get_tags(&organization, &project).await?;
    crate::cache::put(&key, &tags);
    Ok(tags)
}

/// Fold tags the app just learned about locally - carried by test cases it
/// created, so they provably exist now - into a cached tag list.
/// Case-insensitively deduplicated and sorted; returns whether anything was
/// added. Applied through `cache::update`, which leaves the entry's age
/// alone and never seeds a cold key with a partial list.
pub fn add_new_tags(tags: &mut Vec<String>, extra: &[String]) -> bool {
    let mut added = false;
    for v in extra {
        let v = v.trim();
        if v.is_empty() || tags.iter().any(|e| e.eq_ignore_ascii_case(v)) {
            continue;
        }
        tags.push(v.to_string());
        added = true;
    }
    if added {
        tags.sort_by_key(|v| v.to_lowercase());
    }
    added
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
