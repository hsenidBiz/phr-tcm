//! The app's own settings: whether closing the main window keeps it running
//! in the tray, whether the one-time notice about that has been shown,
//! whether this install takes beta builds, and whether the sign-in is kept
//! between launches.
//!
//! Rust owns them, not the webview's storage, because they are needed
//! before any page loads (the close handler, the update check at launch)
//! and by code that has no page at all. One small file in the app data dir,
//! beside `extras.json` - and like that one, not `crate::cache`, which is
//! wiped when a different account signs in: these belong to the machine.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const FILE: &str = "app-settings.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct AppSettings {
    /// Closing the main window hides it to the tray instead of quitting.
    pub close_to_tray: bool,
    /// The "still running in the tray" notice has been shown once.
    pub close_notice_shown: bool,
    /// Update checks include beta (prerelease) builds.
    pub beta_updates: bool,
    /// A start at sign-in (Start with Windows) stays hidden in the tray
    /// instead of opening the window.
    pub start_minimized: bool,
    /// AI tools run `db_query` without asking the person first: the app
    /// writes each registered tool's own "always allow" for it, and tells
    /// the assistant it need not ask. Off unless turned on.
    pub db_auto_approve: bool,
    /// Stay signed in: the sign-in is kept in Windows Credential Manager
    /// and a launch goes straight in while Microsoft still accepts it
    /// (`crate::saved_session`). Off: every launch signs in in the browser.
    pub stay_signed_in: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            close_notice_shown: false,
            beta_updates: false,
            start_minimized: true,
            db_auto_approve: false,
            stay_signed_in: true,
        }
    }
}

static DIR: OnceLock<PathBuf> = OnceLock::new();
static CURRENT: Mutex<Option<AppSettings>> = Mutex::new(None);

/// What `dir` holds. Missing, unreadable or the wrong shape all read as the
/// defaults - never an error. Unknown fields are ignored and missing ones
/// take their default (`#[serde(default)]`).
pub fn load(dir: &Path) -> AppSettings {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
        .unwrap_or_default()
}

/// Write `s` into `dir` (created if missing), atomically.
pub fn save(dir: &Path, s: &AppSettings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let body = serde_json::to_string(s).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&dir.join(FILE), &body)
}

/// Called once from setup with the app data dir.
pub fn init(dir: PathBuf) {
    *CURRENT.lock().unwrap() = Some(load(&dir));
    let _ = DIR.set(dir);
}

/// The settings as this process last loaded or saved them. The defaults
/// before `init` (the `--mcp` proxy never runs setup).
pub fn current() -> AppSettings {
    CURRENT.lock().unwrap().clone().unwrap_or_default()
}

/// Change the settings and save them. On a failed save the in-memory copy
/// is left as it was, so the app never acts on a choice that was not kept.
pub fn update(f: impl FnOnce(&mut AppSettings)) -> Result<AppSettings, String> {
    let dir = DIR.get().ok_or_else(|| "settings are not available yet".to_string())?;
    let mut next = current();
    f(&mut next);
    save(dir, &next)?;
    *CURRENT.lock().unwrap() = Some(next.clone());
    Ok(next)
}
