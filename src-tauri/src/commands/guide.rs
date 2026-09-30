//! The How To Use guide's commands: is it on disk and current, and download
//! it. Ungated - the guide is for everyone. Opening it is `misc::open_help`.
//! The logic is in `crate::guide`; a development build downloads nothing
//! and always reads Ready (it opens the repository's copy).

use std::path::PathBuf;

use tauri::Manager;
use tauri_specta::Event as _;

use crate::guide::{self, GithubGuide, GuideState, GuideStatus};

/// `help/` under the app's local data dir - where the guide lives. None
/// (with the reason logged) when the OS will not say where that is.
pub(crate) fn help_root(app: &tauri::AppHandle) -> Option<PathBuf> {
    match app.path().app_local_data_dir() {
        Ok(dir) => Some(dir.join("help")),
        Err(e) => {
            crate::applog::warn(format!("could not resolve the help folder: {e}"));
            None
        }
    }
}

/// Whether How To Use is on disk and current, and the download's size.
#[tauri::command]
#[specta::specta]
pub async fn guide_status(app: tauri::AppHandle) -> GuideStatus {
    if crate::ai_tools::dev_build() {
        return GuideStatus { state: GuideState::Ready, size: None };
    }
    match help_root(&app) {
        Some(root) => guide::status(&root, &GithubGuide::this_version()).await,
        None => GuideStatus { state: GuideState::NotDownloaded, size: None },
    }
}

/// How often progress is sent: every this many bytes, and at the end - a
/// 31 MB zip arrives in thousands of chunks.
const PROGRESS_STEP: u64 = 256 * 1024;

/// Download this version's How To Use, install it, and open it (as the
/// button did when the guide shipped inside the app). Streams
/// `GuideProgress`. Errors are the guide's sentences; the raw reason is in
/// the log.
#[tauri::command]
#[specta::specta]
pub async fn guide_download(app: tauri::AppHandle) -> Result<(), String> {
    if crate::ai_tools::dev_build() {
        return crate::help::open_index(std::path::Path::new(crate::help::DEV_INDEX));
    }
    let root = help_root(&app).ok_or_else(|| guide::DOWNLOAD_FAILED.to_string())?;
    let emitter = app.clone();
    let mut sent: Option<u64> = None;
    let index = guide::download(&root, &GithubGuide::this_version(), move |received, total| {
        let due = match sent {
            None => true,
            Some(last) => received >= last + PROGRESS_STEP || received >= total,
        };
        if due {
            sent = Some(received);
            let _ = crate::events::GuideProgress {
                received: u32::try_from(received).unwrap_or(u32::MAX),
                total: u32::try_from(total).unwrap_or(u32::MAX),
            }
            .emit(&emitter);
        }
    })
    .await?;
    crate::help::open_index(&index)
}
