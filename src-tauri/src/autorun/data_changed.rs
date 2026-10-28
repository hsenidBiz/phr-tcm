//! Told of every successful write to the data an assistant adds through the
//! AI bridge (areas, components, the discovery map and its summary, quirks,
//! test files, proposed accounts), whoever made it. The app registers one
//! listener at startup that says so to the window (`AutorunDataChanged`), so
//! the Auto Run screen reads that data again without a reload.
//!
//! Same shape as the scripts listeners in `store`: process-wide, copied out
//! of the lock before they run, and a store calls `notify` only after the
//! write landed - a refused or no-op write says nothing.

use crate::events::AutorunDataKind;
use std::path::Path;
use std::sync::Mutex;

/// The store root (the test-files folder for test files) and what changed.
type DataListener = std::sync::Arc<dyn Fn(&Path, &[AutorunDataKind]) + Send + Sync>;

static LISTENERS: Mutex<Vec<DataListener>> = Mutex::new(Vec::new());

/// Be told of every successful write from now on.
pub fn on_data_changed(f: impl Fn(&Path, &[AutorunDataKind]) + Send + Sync + 'static) {
    LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).push(std::sync::Arc::new(f));
}

/// Tell every listener that `kind` changed under `at`. A listener runs with
/// no lock of this module held and no store lock held either (callers
/// release theirs first), and one that panics cannot break the write
/// that announced it.
pub fn notify(at: &Path, kind: AutorunDataKind) {
    let listeners: Vec<DataListener> = LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for listener in listeners {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener(at, &[kind])));
    }
}
