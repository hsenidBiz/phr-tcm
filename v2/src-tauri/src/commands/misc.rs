//! Small cross-cutting commands: liveness ping, updater, ambient audio.

use tauri::Manager;

use crate::{audio, updater};

#[tauri::command]
#[specta::specta]
pub fn ping(msg: String) -> String {
    format!("pong: {msg}")
}

/// How hard the app is allowed to hit Azure DevOps: "full" | "balanced" |
/// "gentle". The limit ADO enforces is per USER, so the app shares one
/// budget with the same person's browser - this lets them hand some back.
/// Applied process-wide; the frontend calls it at startup and on change.
#[tauri::command]
#[specta::specta]
/// Returns the resulting gap in ms (u32: specta forbids u64 across IPC).
pub fn set_ado_rate_level(level: String) -> u32 {
    crate::ado::throttle::set_level(&level);
    crate::ado::throttle::current_interval_ms() as u32
}

/// Non-blocking update check; Some(version) when a newer build is published.
#[tauri::command]
#[specta::specta]
pub async fn check_update(app: tauri::AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<updater::UpdateState>();
        updater::check(&state)
    })
    .await
    .ok()
    .flatten()
}

/// Download the pending update and restart into it.
#[tauri::command]
#[specta::specta]
pub async fn apply_update(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<updater::UpdateState>();
        updater::download_and_apply(&state)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Start streaming AudioSpectrum events from system-audio loopback (the
/// flask equalizer rings). No-op if already running; failures are silent by
/// design - the feature is decorative.
#[tauri::command]
#[specta::specta]
pub fn audio_capture_start(app: tauri::AppHandle) -> Result<(), String> {
    audio::start(app)
}

/// Stop the AudioSpectrum stream (last UI subscriber unmounted).
#[tauri::command]
#[specta::specta]
pub fn audio_capture_stop() {
    audio::stop();
}
