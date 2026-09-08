//! OS-level change notification for the JSON files an import is following,
//! so the UI never polls: idle costs nothing, and a save is noticed in
//! milliseconds rather than on the next tick. Several files can be watched
//! at once - a queue is often fed by more than one.
//!
//! Two things here are deliberate, and both come from how assistants and
//! editors actually save:
//!
//! 1. **The PARENT DIRECTORY is watched, not the file.** A save is very
//!    often "write a temp file, rename it over the target", which replaces
//!    the file a direct watch is bound to - the watch would go deaf after
//!    the first edit. A directory watch survives it; events are filtered
//!    back down to our file name.
//! 2. **A change is confirmed by hashing before anything is emitted.** One
//!    save raises several filesystem events, and plenty of events touch a
//!    file without changing its bytes. The frontend hears one event per
//!    real content change.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tauri::AppHandle;
use tauri_specta::Event;

use crate::events::WatchedFileChanged;

/// A single save raises a burst of events; wait this long for the burst to
/// finish before reading, so the file isn't hashed half-written.
const COALESCE: Duration = Duration::from_millis(200);

/// A hard ceiling on how long one burst may be coalesced.
///
/// The watch is on the DIRECTORY, not the file, so every event beside our
/// file also landed in this channel and reset the 200 ms window. A folder
/// with any background traffic - a log being appended, OneDrive or Dropbox
/// syncing, a download target - never went quiet for 200 ms, so the drain
/// never ended and the change was never emitted: the followed file simply
/// stopped updating the queue, with nothing said and nothing to see.
const MAX_COALESCE: Duration = Duration::from_secs(2);

/// Content fingerprint of a file, or `None` when it can't be read right
/// now - deleted, or momentarily absent mid-rename. Not an error: the
/// watcher simply has nothing to report yet.
pub fn stamp(path: &Path) -> Option<String> {
    Some(fingerprint(&std::fs::read(path).ok()?))
}

/// The fingerprint of some bytes. Shared with `write_watched` so a write
/// the app makes hashes identically to the same bytes read back - if these
/// two ever drifted, self-writes would stop being recognised.
pub fn fingerprint(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Dropping this stops the watch: the watcher's sender goes with it, the
/// worker thread's `recv` fails, and the thread returns.
pub struct FileWatch {
    _watcher: RecommendedWatcher,
    /// A fingerprint the app itself is about to write. Seeing it is not
    /// news - reporting it would show the user a change report for their
    /// own typing - so it is absorbed once and forgotten.
    expected: std::sync::Arc<Mutex<Option<String>>>,
}

/// Every file currently being followed, keyed by path. A queue can be fed
/// by several JSON files at once (one per feature, say), and each needs
/// its own watch so one can be dropped without disturbing the others.
#[derive(Default)]
pub struct FileWatchState(pub Mutex<std::collections::HashMap<String, FileWatch>>);

/// Watch `path` for content changes, emitting `WatchedFileChanged` on each
/// one. Watching a path already watched replaces that one watch and leaves
/// the others alone.
pub fn start(app: &AppHandle, state: &FileWatchState, path: &str) -> Result<(), String> {
    let app = app.clone();
    let emit_path = path.to_string();
    start_with(state, path, move |stamp| {
        let _ = WatchedFileChanged {
            path: emit_path.clone(),
            stamp,
        }
        .emit(&app);
    })
}

/// The watch itself, with the notification decoupled from Tauri so the
/// burst/rename/no-op handling can be tested without an app handle.
/// `on_change` receives the new fingerprint, once per real change.
pub fn start_with<F>(state: &FileWatchState, path: &str, mut on_change: F) -> Result<(), String>
where
    F: FnMut(String) + Send + 'static,
{
    let target = PathBuf::from(path);
    let dir = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| format!("{path} has no folder to watch"))?
        .to_path_buf();
    let name = target
        .file_name()
        .ok_or_else(|| format!("{path} is not a file"))?
        .to_os_string();

    let (tx, rx) = mpsc::channel();
    let mut watcher =
        notify::recommended_watcher(tx).map_err(|e| format!("Could not start watching: {e}"))?;
    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Could not watch {}: {e}", dir.display()))?;

    let emit_for = target.clone();
    let mut last = stamp(&target);
    let expected: std::sync::Arc<Mutex<Option<String>>> = Default::default();
    let mine = expected.clone();
    std::thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let touched_us = |ev: &notify::Result<notify::Event>| {
                ev.as_ref()
                    .map(|e| e.paths.iter().any(|p| p.file_name() == Some(&name)))
                    .unwrap_or(false)
            };
            let mut ours = touched_us(&first);
            // Drain the rest of the burst (and let a rename settle) before
            // reading, so a half-written file is never hashed - but never
            // for longer than MAX_COALESCE, however busy the folder is.
            let deadline = std::time::Instant::now() + MAX_COALESCE;
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    break;
                }
                match rx.recv_timeout(COALESCE.min(left)) {
                    Ok(next) => ours |= touched_us(&next),
                    // Quiet for the window, or the watcher is gone. Either
                    // way this burst is over.
                    Err(_) => break,
                }
            }
            if !ours {
                continue;
            }
            let now = stamp(&emit_for);
            // `None` here means gone or mid-rename - keep the previous
            // fingerprint so the next real write still reads as a change.
            let Some(now) = now else { continue };
            if Some(&now) == last.as_ref() {
                continue;
            }
            // Our own write (a comment saved from the report page). Take it
            // as the new baseline so the NEXT outside edit still reads as a
            // change, and say nothing.
            let ours = {
                let mut slot = mine.lock().unwrap();
                if slot.as_ref() == Some(&now) {
                    *slot = None;
                    true
                } else {
                    false
                }
            };
            last = Some(now.clone());
            if ours {
                continue;
            }
            on_change(now);
        }
    });

    // Replacing an existing watch on the same path drops the old one here,
    // which ends its thread - re-arming is never a leak.
    state
        .0
        .lock()
        .unwrap()
        .insert(path.to_string(), FileWatch { _watcher: watcher, expected });
    Ok(())
}

/// Write `text` to a watched file without the watch reporting it back.
///
/// The app writes into these files itself (a comment typed in the report
/// page), and that write is not news to the app that made it. Registering
/// the fingerprint BEFORE the write closes the race where the watcher
/// notices the new bytes before we get a chance to claim them.
///
/// A path that is not being watched is written normally - Manual Entry
/// drafts and one-off exports have no watch to confuse.
pub fn write_watched(state: &FileWatchState, path: &str, text: &str) -> Result<String, String> {
    let stamp = fingerprint(text.as_bytes());
    if let Some(watch) = state.0.lock().unwrap().get(path) {
        *watch.expected.lock().unwrap() = Some(stamp.clone());
    }
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(stamp)
}

/// Stop following one file. Unknown paths are a no-op: the UI may drop a
/// watch the backend already lost (app restart, file deleted).
pub fn stop(state: &FileWatchState, path: &str) {
    state.0.lock().unwrap().remove(path);
}

/// Stop following everything - used when the PBI scope changes.
pub fn stop_all(state: &FileWatchState) {
    state.0.lock().unwrap().clear();
}

/// The files currently being watched.
pub fn watched_paths(state: &FileWatchState) -> Vec<String> {
    let mut out: Vec<String> = state.0.lock().unwrap().keys().cloned().collect();
    out.sort();
    out
}
