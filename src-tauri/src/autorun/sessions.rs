//! Session files: `sessions/<env id>/<account key>.json`, one folder per
//! environment. They hold live cookies,
//! so they are left out of backups (`backup::EXCLUDED`) and are dropped
//! when the login behind them changes (`accounts::save_accounts`).

use super::accounts::{session_path, valid_key};
use crate::browser::session::SavedSession;
use std::path::Path;

/// Epoch milliseconds - the one clock reading the cache uses too.
pub use crate::cache::now_ms;

pub fn save_session(root: &Path, key: &str, s: &SavedSession) -> Result<(), String> {
    if !valid_key(key) {
        return Err(format!("\"{key}\" is not a usable account key"));
    }
    let path = session_path(root, key)?;
    std::fs::create_dir_all(path.parent().expect("sessions folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(s).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path, &json)
}

/// The saved session file for `key`, read as it is, whatever its age.
fn read_saved(root: &Path, key: &str) -> Option<SavedSession> {
    if !valid_key(key) {
        return None;
    }
    let text = std::fs::read_to_string(session_path(root, key).ok()?).ok()?;
    serde_json::from_str(&text).ok()
}

/// The saved session, if there is one and it is younger than
/// `max_age_minutes`. A file stamped in the future (a clock that moved
/// back) is not trusted either.
pub fn load_fresh_session(root: &Path, key: &str, max_age_minutes: u32, now_ms: u64) -> Option<SavedSession> {
    let s = read_saved(root, key)?;
    let age = now_ms.checked_sub(s.saved_at_ms)?;
    (age <= u64::from(max_age_minutes) * 60_000).then_some(s)
}

/// How old the saved session for `key` is, in whole minutes, whether or not
/// it is still fresh enough to use; `None` when there is none to read. For
/// the log, which says how old a refused session was.
pub fn saved_session_age_minutes(root: &Path, key: &str) -> Option<u64> {
    let s = read_saved(root, key)?;
    Some(now_ms().saturating_sub(s.saved_at_ms) / 60_000)
}

pub fn forget_session(root: &Path, key: &str) {
    if valid_key(key) {
        if let Ok(p) = session_path(root, key) {
            let _ = std::fs::remove_file(p);
        }
    }
}
