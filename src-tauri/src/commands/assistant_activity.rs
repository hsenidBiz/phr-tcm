//! The assistant timeline's command and its change watch.

use crate::assistant_activity::AssistantRecord;

/// What assistants saved, proposed, tried and changed in this organisation
/// and the active environment over the last `days` days, newest first.
#[tauri::command]
#[specta::specta]
pub fn auto_run_assistant_timeline(app: tauri::AppHandle, organization: String, days: u32) -> Result<Vec<AssistantRecord>, String> {
    let env = crate::environments::active(&crate::commands::autorun::root(&app)?)?.id;
    let Some(dir) = crate::activity_log::directory() else { return Ok(Vec::new()) };
    Ok(crate::assistant_activity::timeline_in(&dir, &organization, &env, days, std::time::SystemTime::now()))
}

/// Every second, send `AssistantActivityChanged` when a record was written
/// since the last look. Started once; a second call does nothing.
pub fn spawn_assistant_activity_watch(app: tauri::AppHandle) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        tauri::async_runtime::spawn(async move {
            use tauri_specta::Event as _;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if crate::activity_log::take_assistant_changed() {
                    let _ = crate::events::AssistantActivityChanged {}.emit(&app);
                }
            }
        });
    });
}
