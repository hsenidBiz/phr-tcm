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

/// Whether the app is registered to start at sign-in. Read from the
/// registry each time, so the switch always shows the truth.
#[tauri::command]
#[specta::specta]
pub fn get_autostart(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or_else(|e| {
        crate::applog::warn(format!("reading Start with Windows failed, showing it as off: {e}"));
        false
    })
}

/// Register or unregister the start at sign-in. Answers the state after.
#[tauri::command]
#[specta::specta]
pub fn set_autostart(app: tauri::AppHandle, on: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let done = if on { launcher.enable() } else { launcher.disable() };
    done.map_err(|e| {
        crate::applog::warn(format!("changing Start with Windows failed: {e}"));
        "Start with Windows could not be changed. Settings → Logs has the details.".to_string()
    })?;
    Ok(launcher.is_enabled().unwrap_or(on))
}

/// Whether update checks include beta builds. Turning it off on a beta
/// build keeps that build until a newer stable one ships - the updater
/// never downgrades.
#[tauri::command]
#[specta::specta]
pub fn set_beta_updates(on: bool) -> Result<AppSettings, String> {
    app_settings::update(|s| s.beta_updates = on).map_err(|e| {
        crate::applog::warn(format!("saving the beta updates setting failed: {e}"));
        "The setting could not be saved. Settings → Logs has the details.".to_string()
    })
}

/// Whether a start at sign-in stays hidden in the tray (on) or opens the
/// window (off). Read at start-up, so the Windows startup entry itself never
/// changes with it.
#[tauri::command]
#[specta::specta]
pub fn set_start_minimized(on: bool) -> Result<AppSettings, String> {
    app_settings::update(|s| s.start_minimized = on).map_err(|e| {
        crate::applog::warn(format!("saving the start minimized setting failed: {e}"));
        "The setting could not be saved. Settings → Logs has the details.".to_string()
    })
}

/// Stay signed in. Off removes the kept sign-in from Credential Manager at
/// once; on keeps the current one, so the very next launch goes straight in.
#[tauri::command]
#[specta::specta]
pub fn set_stay_signed_in(app: tauri::AppHandle, on: bool) -> Result<AppSettings, String> {
    let settings = app_settings::update(|s| s.stay_signed_in = on).map_err(|e| {
        crate::applog::warn(format!("saving the stay signed in setting failed: {e}"));
        "The setting could not be saved. Settings → Logs has the details.".to_string()
    })?;
    crate::saved_session::keep_for_next_launch(&app);
    Ok(settings)
}
