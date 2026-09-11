//! AI Findings for the AI Bridge tab: list, resolve or reopen, dismiss.
//! Local data only; nothing here reaches Azure DevOps.

use std::path::{Path, PathBuf};

use crate::findings::{self, Finding};

/// The store root: what setup published, else derived from the handle.
fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Some(r) = findings::configured_root() {
        return Ok(r);
    }
    use tauri::Manager;
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// The listing against an explicit root, so a test can hold it to account
/// without an `AppHandle`.
pub fn list_findings_at(root: &Path, organization: &str, project: &str) -> Vec<Finding> {
    findings::list(root, organization, project)
}

#[tauri::command]
#[specta::specta]
pub fn list_findings(app: tauri::AppHandle, organization: String, project: String) -> Vec<Finding> {
    match root(&app) {
        Ok(r) => list_findings_at(&r, &organization, &project),
        Err(_) => vec![],
    }
}

#[tauri::command]
#[specta::specta]
pub fn set_finding_status(app: tauri::AppHandle, id: String, status: String) -> Result<Finding, String> {
    findings::set_status(&root(&app)?, &id, &status)
}

#[tauri::command]
#[specta::specta]
pub fn remove_finding(app: tauri::AppHandle, id: String) -> Result<(), String> {
    findings::remove(&root(&app)?, &id)
}
