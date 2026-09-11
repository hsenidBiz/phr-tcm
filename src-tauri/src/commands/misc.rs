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

/// Open the log folder in the file explorer. From Rust, like every other
/// path the app opens: the webview's opener permission is `opener:default`,
/// which covers URLs and "reveal", not `open_path` - so the frontend
/// calling the plugin directly was refused, and the button did nothing.
#[tauri::command]
#[specta::specta]
pub fn open_app_log_dir() -> Result<(), String> {
    let dir = crate::applog::directory();
    if dir.is_empty() {
        return Err("The log folder is not set up yet.".into());
    }
    tauri_plugin_opener::open_path(&dir, None::<&str>).map_err(|e| {
        crate::applog::warn(format!("open log folder failed: {e}"));
        "Could not open the log folder.".to_string()
    })
}

/// Non-blocking update check; Some(version) when a newer build is published.
#[tauri::command]
#[specta::specta]
pub async fn check_update(app: tauri::AppHandle) -> updater::UpdateStatus {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        let running = app.package_info().version.to_string();
        // Did the LAST "Restart to update" actually land? The apply runs
        // after that process died, so this launch is the first thing that
        // can compare where it aimed against where we are.
        let failed_attempt = app
            .path()
            .app_data_dir()
            .ok()
            .and_then(|dir| updater::failed_attempt(&dir, &running));
        let state = app.state::<updater::UpdateState>();
        updater::UpdateStatus { failed_attempt, ..updater::check(&state) }
    })
    .await
    .unwrap_or_else(|e| updater::UpdateStatus {
        blocked: Some(format!("The update check did not run: {e}")),
        ..Default::default()
    })
}

/// Download the pending update and restart into it, streaming
/// `UpdateProgress` so the banner can show how much is left.
#[tauri::command]
#[specta::specta]
pub async fn apply_update(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        let emitter = app.clone();
        let data_dir = app.path().app_data_dir().ok();
        let state = app.state::<updater::UpdateState>();
        updater::download_and_apply(&state, data_dir, move |p| {
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

/// Frontend UI breadcrumbs - navigation, button clicks - into the same
/// rolling app log the bug report ships, so "what did you do before it
/// broke" answers itself from the report. Length-capped: the log is a
/// diagnostic trail, not a keylogger, and callers only send control names.
#[tauri::command]
#[specta::specta]
pub fn log_ui(message: String) {
    let msg: String = message.chars().take(200).collect();
    crate::applog::info(format!("[ui] {msg}"));
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
    title: String,
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
        url: crate::bugreport::issue_url(&crate::bugreport::effective_title(&title, &description), &body),
        log_path: path.to_string_lossy().to_string(),
        truncated,
    })
}

/// Export everything the app remembers on this machine - the webview's
/// `tcm-v2-*` localStorage (handed in by the frontend, which is the only
/// side that can read it) plus the disk stores under `app_data_dir` - into
/// one JSON file the user can carry to another laptop. Credentials never
/// travel: sign-in tokens are memory-only and outside the exported roots.
#[tauri::command]
#[specta::specta]
pub fn export_app_backup(
    app: tauri::AppHandle,
    local_storage: std::collections::BTreeMap<String, String>,
    path: String,
) -> Result<crate::backup::ExportSummary, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let (files, skipped) = crate::backup::collect_files(&data_dir);
    let exported_at = crate::applog::stamp();
    let version = app.package_info().version.to_string();
    let keys = local_storage.len() as u32;
    let n_files = files.len() as u32;
    let doc = crate::backup::build_doc(&version, &exported_at, local_storage, files);
    crate::backup::write_doc(&doc, std::path::Path::new(&path))?;
    crate::applog::info(format!(
        "Exported a backup to {path} ({keys} setting(s), {n_files} file(s))"
    ));
    Ok(crate::backup::ExportSummary { path, keys, files: n_files, skipped })
}

/// Read a backup file, restore its disk half, and hand the localStorage
/// half back to the frontend to apply (only the webview can write it).
/// The frontend reloads afterwards so every screen re-reads its state.
#[tauri::command]
#[specta::specta]
pub fn import_app_backup(
    app: tauri::AppHandle,
    path: String,
) -> Result<crate::backup::BackupImportResult, String> {
    let doc = crate::backup::read_doc(std::path::Path::new(&path))?;
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let files_restored = crate::backup::restore_files(&data_dir, &doc.files)?;
    crate::applog::info(format!(
        "Imported a backup from {path} (made {} by version {}; {} setting(s), {files_restored} file(s))",
        doc.exported_at, doc.app_version, doc.local_storage.len()
    ));
    Ok(crate::backup::BackupImportResult {
        local_storage: doc.local_storage,
        files_restored,
        exported_at: doc.exported_at,
        app_version: doc.app_version,
    })
}
