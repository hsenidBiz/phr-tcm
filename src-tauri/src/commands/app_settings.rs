//! The app's own settings (`crate::app_settings`), for the Settings screen.

use crate::app_settings::{self, AppSettings};
use crate::browser::viewport::{self, PageSize, DEFAULT_PAGE};

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

/// The folder of the Playwright automation clone scripts export into.
#[tauri::command]
#[specta::specta]
pub async fn set_playwright_clone(app: tauri::AppHandle, path: String) -> Result<AppSettings, String> {
    // The agent files carry the repo's rules: a different clone gives different
    // text. Reading the repo and writing those files stays off the main thread.
    let job = tauri::async_runtime::spawn_blocking(move || {
        let saved = app_settings::update(|s| s.playwright_clone = path.trim().to_string()).map_err(|e| {
            crate::applog::warn(format!("saving the Playwright clone setting failed: {e}"));
            "The setting could not be saved. Settings → Logs has the details.".to_string()
        })?;
        crate::commands::modules::resync_agents(&app);
        Ok::<AppSettings, String>(saved)
    });
    job.await.unwrap_or_else(|e| {
        crate::applog::warn(format!("saving the Playwright clone setting stopped early: {e}"));
        Err("The setting could not be saved. Settings → Logs has the details.".to_string())
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

/// Whether Auto Run outlines and pauses before each action, in a watched run
/// and in the supervised browser. Read when a run or a supervised step starts.
#[tauri::command]
#[specta::specta]
pub fn set_autorun_highlight(on: bool) -> Result<AppSettings, String> {
    app_settings::update(|s| s.autorun_highlight = on).map_err(|e| {
        crate::applog::warn(format!("saving the highlight setting failed: {e}"));
        "The setting could not be saved. Settings → Logs has the details.".to_string()
    })
}

/// The page size every browser the app drives shows.
#[tauri::command]
#[specta::specta]
pub fn get_page_size() -> PageSize {
    viewport::current()
}

/// Save the page size. A size outside `viewport::MIN_PAGE..=MAX_PAGE` is
/// refused and nothing is saved. A browser already open keeps its size; the
/// next one the app opens uses this one.
#[tauri::command]
#[specta::specta]
pub fn set_page_size(size: PageSize) -> Result<PageSize, String> {
    let size = viewport::validate(size)?;
    app_settings::update(|s| s.page_size = size).map_err(|e| {
        crate::applog::warn(format!("saving the page size failed: {e}"));
        "The setting could not be saved. Settings → Logs has the details.".to_string()
    })?;
    Ok(size)
}

/// Put the page size back to what the app ships with. A save that fails says
/// so, in the sentence a failed `set_page_size` uses.
#[tauri::command]
#[specta::specta]
pub fn reset_page_size() -> Result<PageSize, String> {
    app_settings::update(|s| s.page_size = DEFAULT_PAGE).map_err(|e| {
        crate::applog::warn(format!("resetting the page size failed: {e}"));
        "The setting could not be saved. Settings → Logs has the details.".to_string()
    })?;
    Ok(viewport::current())
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
