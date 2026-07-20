//! AI-guide generation commands. Generation is pure/local-file-write only;
//! `list_repo_folders` is the wizard's one ADO read (GET only). No DELETE
//! surface anywhere.

use crate::ado;
use crate::ai_guide::{build_guide_body, flavor_files, GuideOptions};
use crate::state::get_fresh_token;

/// Immediate subfolders of `path` in a project repo - drives the wizard's
/// docs-folder drill-down picker.
#[tauri::command]
#[specta::specta]
pub async fn list_repo_folders(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    repo_id: String,
    path: String,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .repo_folders(&organization, &project, &repo_id, &path)
        .await
}

#[tauri::command]
#[specta::specta]
pub fn preview_ai_guide(options: GuideOptions) -> String {
    build_guide_body(&options)
}

#[tauri::command]
#[specta::specta]
pub fn write_ai_guide(dir: String, options: GuideOptions) -> Result<Vec<String>, String> {
    let body = build_guide_body(&options);
    let files = flavor_files(&body, &options.flavors);
    if files.is_empty() {
        return Err("No output flavor selected.".into());
    }
    let base = std::path::Path::new(&dir);
    let mut written = Vec::with_capacity(files.len());
    for (rel, content) in files {
        let target = base.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, content)
            .map_err(|e| format!("Could not write {}: {e}", target.display()))?;
        written.push(rel);
    }
    Ok(written)
}
