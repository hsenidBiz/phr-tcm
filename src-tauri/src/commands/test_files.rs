//! IPC surface for a project's Test files (`crate::test_files`): list,
//! add, remove and open the folder. Offered exactly where Auto Run and API
//! Templates are - `ai_tools::autorun_offered`, through the same refusal
//! the API Templates commands use - since only those two upload a test
//! file.
//!
//! Each command has a pure `*_at` half that takes "is it offered here" and
//! the data root, so a test reaches it without an `AppHandle`. Removing a
//! file deletes this machine's copy and nothing else: there is no network
//! call anywhere in this module.

use crate::commands::api_templates::refuse_unless;
use crate::test_files::{self, TestFile};
use std::path::{Path, PathBuf};

/// Said when a command names no organization or project.
pub const NO_PROJECT: &str = "choose an organization and a project first - test files belong to a project";

/// Said when `add` is given a path that is not a full one.
pub const NOT_A_FULL_PATH: &str = "only a file picked with Add files can be added - that is not a full path to one";

/// The project's Test files folder, after the gate and a project check.
fn folder_for(offered: bool, root: &Path, organization: &str, project: &str) -> Result<PathBuf, String> {
    refuse_unless(offered)?;
    if organization.trim().is_empty() || project.trim().is_empty() {
        return Err(NO_PROJECT.to_string());
    }
    Ok(test_files::folder(root, organization, project))
}

pub fn list_at(offered: bool, root: &Path, organization: &str, project: &str) -> Result<Vec<TestFile>, String> {
    test_files::list(&folder_for(offered, root, organization, project)?)
}

pub fn add_at(
    offered: bool,
    root: &Path,
    organization: &str,
    project: &str,
    path: &Path,
    replace: bool,
) -> Result<TestFile, String> {
    let folder = folder_for(offered, root, organization, project)?;
    // Only a file the person picked, which the file dialog always hands
    // over as a full path - never one read against the working directory.
    if !path.is_absolute() {
        return Err(NOT_A_FULL_PATH.to_string());
    }
    test_files::add(&folder, path, replace)
}

pub fn remove_at(offered: bool, root: &Path, organization: &str, project: &str, name: &str) -> Result<(), String> {
    test_files::remove(&folder_for(offered, root, organization, project)?, name)
}

/// The folder, created if it is not there yet, ready to be opened.
pub fn ensure_folder_at(offered: bool, root: &Path, organization: &str, project: &str) -> Result<PathBuf, String> {
    let dir = folder_for(offered, root, organization, project)?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        crate::applog::warn(format!("test files: the folder could not be created: {e}"));
        "the Test files folder could not be created - see Settings, Logs".to_string()
    })?;
    Ok(dir)
}

#[tauri::command]
#[specta::specta]
pub fn test_files_list(app: tauri::AppHandle, organization: String, project: String) -> Result<Vec<TestFile>, String> {
    let root = crate::commands::autorun::root(&app)?;
    list_at(crate::ai_tools::autorun_offered(), &root, &organization, &project)
}

/// Copies the file the person picked at `path` into Test files. `replace`
/// is sent only after the person said yes to replacing a file of that name.
#[tauri::command]
#[specta::specta]
pub fn test_files_add(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    path: String,
    replace: bool,
) -> Result<TestFile, String> {
    let root = crate::commands::autorun::root(&app)?;
    add_at(crate::ai_tools::autorun_offered(), &root, &organization, &project, Path::new(&path), replace)
}

#[tauri::command]
#[specta::specta]
pub fn test_files_remove(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    name: String,
) -> Result<(), String> {
    let root = crate::commands::autorun::root(&app)?;
    remove_at(crate::ai_tools::autorun_offered(), &root, &organization, &project, &name)
}

/// Opens the folder in the file explorer - from Rust, like every other
/// folder the app opens (see `misc::open_app_log_dir`).
#[tauri::command]
#[specta::specta]
pub fn test_files_open_folder(app: tauri::AppHandle, organization: String, project: String) -> Result<(), String> {
    let root = crate::commands::autorun::root(&app)?;
    let dir = ensure_folder_at(crate::ai_tools::autorun_offered(), &root, &organization, &project)?;
    tauri_plugin_opener::open_path(&dir, None::<&str>).map_err(|e| {
        crate::applog::warn(format!("test files: the folder could not be opened: {e}"));
        "Could not open the Test files folder.".to_string()
    })
}
