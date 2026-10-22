//! What the Auto Run screen needs to know about the Playwright repo's rules.

/// Whether the Playwright tab has a clone set, for the Auto Run screen.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct PwRulesStatus {
    pub clone_set: bool,
    /// A path is set but is no longer a folder (so `clone_set` is false too).
    pub clone_gone: bool,
}

/// `clone_set`: `clone` trimmed is non-empty and is a folder. `clone_gone`:
/// it is non-empty and is not a folder.
pub fn pw_rules_status_for(clone: &str) -> PwRulesStatus {
    let clone = clone.trim();
    let is_folder = !clone.is_empty() && std::path::Path::new(clone).is_dir();
    PwRulesStatus { clone_set: is_folder, clone_gone: !clone.is_empty() && !is_folder }
}

/// Called when the Auto Run screen opens: the generated agent files catch up
/// with the repo's rules, then the status. Never an error. The files are read
/// and written off the main thread.
#[tauri::command]
#[specta::specta]
pub async fn pw_rules_status(app: tauri::AppHandle) -> PwRulesStatus {
    let _ = tauri::async_runtime::spawn_blocking(move || crate::commands::modules::resync_agents(&app)).await;
    pw_rules_status_for(&crate::app_settings::current().playwright_clone)
}
