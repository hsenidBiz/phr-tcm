//! Discovery recordings: a video of an assistant's own exploration of the
//! live app, kept apart from the failed-case videos. They live in
//! `recordings/` beside an index (`index.json`), are never pruned with the
//! failed-case pictures and videos, and are not swept when the runs are
//! cleared - only `remove` deletes one.

use crate::events::AutorunDataKind;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The longest a recording runs before it is stopped by itself.
pub const CAP_MS: u64 = 600_000;
/// `CAP_MS` as a duration.
pub const CAP: std::time::Duration = std::time::Duration::from_millis(CAP_MS);
/// The longest a recording's title is kept, in characters.
pub const MAX_TITLE: usize = 80;

/// One kept recording.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct RecordingEntry {
    pub id: String,
    pub title: String,
    pub created: String,
    pub duration_ms: u32,
    pub file: String,
}

/// A recording that is running: the browser's recorder, what it is called
/// and when it started.
#[derive(Debug)]
pub struct Running {
    pub recording: crate::browser::recording::Recording,
    pub title: String,
    pub started_ms: u64,
    /// The same moment on the monotonic clock: the cap is measured on it, so
    /// a wall clock that jumps never lengthens or shortens a recording.
    pub started: std::time::Instant,
    pub root: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct Index {
    #[serde(default)]
    recordings: Vec<RecordingEntry>,
}

static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn recordings_dir(root: &Path) -> PathBuf {
    root.join("recordings")
}

fn index_path(root: &Path) -> PathBuf {
    recordings_dir(root).join("index.json")
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

/// `recording-<digits>-<digits>.mp4` and nothing else. The name comes back
/// from the webview, so it is checked, not trusted; the prefix keeps it
/// apart from `store::safe_video_name`.
pub fn safe_recording_name(name: &str) -> bool {
    let Some(middle) = name.strip_prefix("recording-").and_then(|n| n.strip_suffix(".mp4")) else {
        return false;
    };
    let mut parts = middle.split('-');
    let ok = |p: Option<&str>| p.is_some_and(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()));
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

/// Unicode format characters (category Cf): zero-width and bidi controls,
/// which would hide an address from the `://` check.
fn is_format_char(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}'
        | '\u{E0001}' | '\u{E0020}'..='\u{E007F}')
}

/// A title for the list: whitespace and control characters folded to one
/// space, trimmed, cut to `MAX_TITLE` characters. A blank title and one
/// that names an address are refused.
pub fn clean_title(raw: &str) -> Result<String, String> {
    let stripped: String = raw.chars().filter(|c| !is_format_char(*c)).collect();
    let folded = stripped
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let title: String = folded.chars().take(MAX_TITLE).collect();
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("a recording needs a short title, e.g. \"Bug 4521: leave balance fix\"".to_string());
    }
    let lower = title.to_lowercase();
    if lower.contains("://") || lower.contains("www.") {
        return Err("a recording's title names no address".to_string());
    }
    Ok(title)
}

/// One writer of the index at a time: `roots::guard` is a shared lock, so two
/// calls could otherwise read, change and write at once and the later write
/// would drop the earlier one's entry. Taken after the guard.
static INDEX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn index_lock() -> std::sync::MutexGuard<'static, ()> {
    INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Read the index. Only "not found" is empty. Text that is not valid UTF-8
/// or JSON is set aside as `index.unreadable-<ms>.json` (when `set_aside`,
/// which needs the write guard held), logged, and read as empty. Any other
/// I/O error is an error: nothing is overwritten.
fn read_index(root: &Path, set_aside: bool) -> Result<Index, String> {
    let path = index_path(root);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Index::default()),
        Err(e) => {
            crate::applog::warn(format!("the recordings index could not be read: {}", e.kind()));
            return Err("the list of recordings could not be read; see Settings > Logs".to_string());
        }
    };
    match std::str::from_utf8(&bytes).ok().and_then(|t| serde_json::from_str::<Index>(t).ok()) {
        Some(index) => Ok(index),
        None => {
            if set_aside {
                let aside = recordings_dir(root).join(format!("index.unreadable-{}.json", now_ms()));
                let _ = std::fs::rename(&path, &aside);
            }
            crate::applog::warn("the recordings index could not be understood; it was set aside and the list starts empty");
            Ok(Index::default())
        }
    }
}

fn write_index(held: &std::sync::RwLockReadGuard<'static, ()>, root: &Path, index: &Index) -> Result<(), String> {
    let text = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    super::roots::write_file_held(held, &index_path(root), &text)
}

/// Keep one recording: the file (created new), then the index entry. If the
/// index cannot be written the file is removed again.
pub fn add(root: &Path, title: &str, duration_ms: u32, bytes: &[u8]) -> Result<RecordingEntry, String> {
    if bytes.is_empty() {
        return Err("the browser gave no video".to_string());
    }
    let title = clean_title(title)?;
    let entry = {
        let held = super::roots::guard(root)?;
        let dir = recordings_dir(root);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let ms = now_ms();
        let (id, file) = loop {
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let id = format!("recording-{ms}-{seq:06}");
            let file = format!("{id}.mp4");
            match std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&file)) {
                Ok(mut f) => {
                    if let Err(e) = std::io::Write::write_all(&mut f, bytes) {
                        drop(f);
                        let _ = std::fs::remove_file(dir.join(&file));
                        return Err(e.to_string());
                    }
                    break (id, file);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        };
        let entry = RecordingEntry { id, title, created: crate::applog::iso_stamp(), duration_ms, file };
        let _index = index_lock();
        let mut index = match read_index(root, true) {
            Ok(i) => i,
            Err(e) => {
                let _ = std::fs::remove_file(dir.join(&entry.file));
                return Err(e);
            }
        };
        index.recordings.push(entry.clone());
        if let Err(e) = write_index(&held, root, &index) {
            let _ = std::fs::remove_file(dir.join(&entry.file));
            return Err(e);
        }
        entry
    };
    super::data_changed::notify(root, AutorunDataKind::Recordings);
    Ok(entry)
}

/// Every kept recording, newest first, leaving out an entry whose file is
/// not a regular file.
pub fn list(root: &Path) -> Vec<RecordingEntry> {
    let dir = recordings_dir(root);
    let mut entries: Vec<RecordingEntry> = read_index(root, false)
        .unwrap_or_default()
        .recordings
        .into_iter()
        .filter(|e| {
            safe_recording_name(&e.file)
                && std::fs::symlink_metadata(dir.join(&e.file)).is_ok_and(|m| m.file_type().is_file())
        })
        .collect();
    entries.reverse();
    entries
}

/// Delete a recording's file and its entry.
pub fn remove(root: &Path, id: &str) -> Result<(), String> {
    {
        let held = super::roots::guard(root)?;
        let _index = index_lock();
        let mut index = read_index(root, true)?;
        let Some(at) = index.recordings.iter().position(|e| e.id == id) else {
            return Err("that recording is gone".to_string());
        };
        let entry = index.recordings.remove(at);
        write_index(&held, root, &index)?;
        if safe_recording_name(&entry.file) {
            if let Err(e) = std::fs::remove_file(recordings_dir(root).join(&entry.file)) {
                crate::applog::warn(format!("a removed recording's file could not be deleted: {}", e.kind()));
            }
        }
    }
    super::data_changed::notify(root, AutorunDataKind::Recordings);
    Ok(())
}

/// A recording's bytes, for playing or saving it.
pub fn load(root: &Path, file: &str) -> Result<Vec<u8>, String> {
    if !safe_recording_name(file) {
        return Err("that is not a video name".to_string());
    }
    let path = recordings_dir(root).join(file);
    let meta = std::fs::symlink_metadata(&path).map_err(|_| "that video is gone".to_string())?;
    if !meta.file_type().is_file() {
        return Err("that video is gone".to_string());
    }
    if meta.len() > super::store::MAX_VIDEO_BYTES {
        return Err("that video is too large to open here".to_string());
    }
    std::fs::read(&path).map_err(|_| "that video is gone".to_string())
}

/// A recording's size in bytes, or `None` when it is not on this machine.
pub fn size(root: &Path, file: &str) -> Option<u64> {
    if !safe_recording_name(file) {
        return None;
    }
    let meta = std::fs::symlink_metadata(recordings_dir(root).join(file)).ok()?;
    meta.file_type().is_file().then_some(meta.len())
}
