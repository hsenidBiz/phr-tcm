//! What the Auto Run screen needs to know about the Playwright repo's rules.

/// Whether the Playwright tab has a clone set, for the Auto Run screen.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct PwRulesStatus {
    pub clone_set: bool,
}

/// `clone_set`: `clone` trimmed is non-empty and is a folder.
pub fn pw_rules_status_for(clone: &str) -> PwRulesStatus {
    let clone = clone.trim();
    PwRulesStatus { clone_set: !clone.is_empty() && std::path::Path::new(clone).is_dir() }
}

/// Called when the Auto Run screen opens: the generated agent files catch up
/// with the repo's rules, then the status. Never an error.
#[tauri::command]
#[specta::specta]
pub fn pw_rules_status(app: tauri::AppHandle) -> PwRulesStatus {
    crate::commands::modules::resync_agents(&app);
    pw_rules_status_for(&crate::app_settings::current().playwright_clone)
}
