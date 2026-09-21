//! Session files: `sessions/<account key>.json`. They hold live cookies,
//! so they are left out of backups (`backup::EXCLUDED`) and are dropped
//! when the login behind them changes (`accounts::save_accounts`).

use super::accounts::{session_path, valid_key};
use crate::browser::session::SavedSession;
use std::path::Path;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

pub fn save_session(root: &Path, key: &str, s: &SavedSession) -> Result<(), String> {
    if !valid_key(key) {
        return Err(format!("\"{key}\" is not a usable account key"));
    }
    let path = session_path(root, key);
    std::fs::create_dir_all(path.parent().expect("sessions folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(s).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}

/// The saved session, if there is one and it is younger than
/// `max_age_minutes`. A file stamped in the future (a clock that moved
/// back) is not trusted either.
pub fn load_fresh_session(root: &Path, key: &str, max_age_minutes: u32, now_ms: u64) -> Option<SavedSession> {
    if !valid_key(key) {
        return None;
    }
    let text = std::fs::read_to_string(session_path(root, key)).ok()?;
    let s: SavedSession = serde_json::from_str(&text).ok()?;
    let age = now_ms.checked_sub(s.saved_at_ms)?;
    (age <= u64::from(max_age_minutes) * 60_000).then_some(s)
}

pub fn forget_session(root: &Path, key: &str) {
    if valid_key(key) {
        let _ = std::fs::remove_file(session_path(root, key));
    }
}
