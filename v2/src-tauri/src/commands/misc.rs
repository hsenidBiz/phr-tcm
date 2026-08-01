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
    let ms = crate::ado::throttle::current_interval_ms();
    crate::applog::info(format!("Azure DevOps request rate set to '{level}' ({ms} ms gap)"));
    ms as u32
}

/// The app's own recent log lines, newest last - shown in Settings so a
/// bug report can carry what the app actually did.
#[tauri::command]
#[specta::specta]
pub fn app_logs(limit: u32) -> Vec<crate::applog::LogLine> {
    crate::applog::recent(limit.clamp(1, 2000) as usize)
}

/// Folder holding the daily log files, for "Open log folder".
#[tauri::command]
#[specta::specta]
pub fn app_log_dir() -> String {
    crate::applog::directory()
}

/// Non-blocking update check; Some(version) when a newer build is published.
#[tauri::command]
#[specta::specta]
pub async fn check_update(app: tauri::AppHandle) -> updater::UpdateStatus {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<updater::UpdateState>();
        updater::check(&state)
    })
    .await
    .unwrap_or_else(|e| updater::UpdateStatus {
        available: None,
        blocked: Some(format!("The update check did not run: {e}")),
    })
}

/// Download the pending update and restart into it, streaming
/// `UpdateProgress` so the banner can show how much is left.
#[tauri::command]
#[specta::specta]
pub async fn apply_update(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let emitter = app.clone();
        let state = app.state::<updater::UpdateState>();
        updater::download_and_apply(&state, move |p| {
            use tauri_specta::Event as _;
            let _ = crate::events::UpdateProgress {
                percent: p.percent as i32,
                downloaded: p.downloaded as f64,
                total: p.total as f64,
            }
            .emit(&emitter);
        })
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

/// Start the background check for newly assigned work items. Idempotent:
/// the first call arms the loop, later calls only update the scope it
/// polls, so switching project doesn't spawn a second task.
#[tauri::command]
#[specta::specta]
pub fn watch_assigned_work(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<(), String> {
    use std::sync::Mutex;
    use tauri_specta::Event;

    static SCOPE: Mutex<Option<(String, String)>> = Mutex::new(None);
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    *SCOPE.lock().unwrap() = Some((organization, project));
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok(()); // already running against the new scope
    }

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(
                crate::assigned_watch::POLL_SECS,
            ))
            .await;
            let Some((org, project)) = SCOPE.lock().unwrap().clone() else {
                continue;
            };
            // Not signed in yet, or the token expired - skip this round
            // rather than nagging; the next one will pick it up.
            let Ok(token) = crate::state::get_fresh_token(&app).await else {
                continue;
            };
            let client = crate::ado::AdoClient::new(token);
            match crate::assigned_watch::check_once(&client, &org, &project).await {
                Ok(items) if !items.is_empty() => {
                    crate::applog::info(format!(
                        "{} work item(s) newly assigned",
                        items.len()
                    ));
                    let _ = crate::events::WorkAssigned { items }.emit(&app);
                }
                Ok(_) => {}
                Err(e) => crate::applog::error(format!("assigned-work check failed: {e}")),
            }
        }
    });
    Ok(())
}

/// Prepare a bug report about THIS app: scrub the log, write it out, and
/// build a prefilled GitHub issue for the reporter to review and submit.
///
/// Nothing is posted here. The reporter sees the body first, which is the
/// point - the log is theirs to check before it goes anywhere public - and
/// it means the app needs no GitHub credential of any kind.
#[tauri::command]
#[specta::specta]
pub fn prepare_bug_report(
    app: tauri::AppHandle,
    description: String,
    organization: String,
    project: String,
) -> Result<crate::bugreport::BugReport, String> {
    let raw = crate::applog::recent(6000)
        .iter()
        .map(|l| format!("{} [{}] {}", l.at, l.level.to_uppercase(), l.message))
        .collect::<Vec<_>>()
        .join("\n");
    let scrubbed = crate::bugreport::scrub(&raw, &organization, &project);

    // Written beside the daily logs, so "Open log folder" reaches it too.
    let dir = if crate::applog::directory().is_empty() {
        std::env::temp_dir()
    } else {
        std::path::PathBuf::from(crate::applog::directory())
    };
    let name = format!("tcm-bug-report-{}.log", std::process::id());
    let path = dir.join(&name);
    std::fs::write(&path, &scrubbed).map_err(|e| format!("could not write the log: {e}"))?;

    let (excerpt, truncated) = crate::bugreport::excerpt(&scrubbed);
    let version = app.package_info().version.to_string();
    let os = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let body = crate::bugreport::body(&description, &version, &os, &excerpt, truncated, &name);
    crate::applog::info("Prepared a bug report");
    Ok(crate::bugreport::BugReport {
        url: crate::bugreport::issue_url(&crate::bugreport::title(&description), &body),
        log_path: path.to_string_lossy().to_string(),
        truncated,
    })
}
