//! The app's own settings (`crate::app_settings`), for the Settings screen.

use crate::app_settings::{self, AppSettings};

/// The settings as the app is using them now.
#[tauri::command]
#[specta::specta]
pub fn get_app_settings() -> AppSettings {
    app_settings::current()
}

/// Whether closing the main window keeps the app running in the tray.
#[tauri::command]
#[specta::specta]
pub fn set_close_to_tray(on: bool) -> Result<AppSettings, String> {
    app_settings::update(|s| s.close_to_tray = on).map_err(|e| {
        crate::applog::warn(format!("saving the close-to-tray setting failed: {e}"));
        "The setting could not be saved. Settings → Logs has the details.".to_string()
    })
}
