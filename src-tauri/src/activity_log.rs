//! Detailed records of what the assistant's tools actually did against a
//! database (and, later, an API template) - the full statement or request,
//! never the credentials - kept apart from `applog`.
//!
//! `applog` exists for bug reports (see its own doc comment), and a bug
//! report is exactly the wrong place for a full trail of SQL: it can be a
//! page of text per SELECT, and unlike `applog::recent` it is never scrubbed
//! or shipped anywhere. So this is its own store: one JSONL file per day per
//! kind - `db-YYYY-MM-DD.jsonl`, `api-YYYY-MM-DD.jsonl` - under an
//! `activity` folder `applog`'s own pruning never looks inside. Callers
//! still write ONE short summary line to `applog` alongside every record
//! here (see `db/query.rs`), so Settings -> Logs still shows that something
//! happened, just not what it said.
//!
//! Dependency-free like `applog`, and reuses its clock (`applog::stamp`)
//! rather than keeping a second one.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// Which daily file a record belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Db,
    Api,
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Kind::Db => "db",
            Kind::Api => "api",
        }
    }
}

/// How long activity files are kept - see the plan's global constraints.
pub const KEEP_DAYS: u64 = 30;

fn dir_cell() -> &'static Mutex<Option<PathBuf>> {
    static D: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(None))
}

/// Points the recorder at a directory and prunes old files there. Unlike
/// most `init` functions in this crate, this is meant to be called again -
/// once from the real Tauri setup, and once per test with a fresh tempdir.
pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut cell) = dir_cell().lock() {
        *cell = Some(dir.clone());
    }
    prune(&dir, SystemTime::now(), KEEP_DAYS);
}

/// The directory records are written to, once `init` has run.
pub fn directory() -> Option<PathBuf> {
    dir_cell().lock().ok().and_then(|d| d.clone())
}

/// `"db-2026-09-28.jsonl"` / `"api-2026-09-28.jsonl"`.
pub fn file_name(kind: Kind, date: &str) -> String {
    format!("{}-{date}.jsonl", kind.prefix())
}

/// Stamps `entry` with `"at"` and appends it as one line to today's file
/// for `kind`. Best-effort and silent like `applog::log`: a record that
/// cannot be written must never interrupt the caller, and there is nowhere
/// else to report the failure to. Dropped entirely if `init` was never
/// called - the same "logging before setup is a no-op" rule `applog` uses,
/// except `applog` still has its in-memory tail to fall back on and this
/// has no equivalent, by design: it is not meant to be shown live.
pub fn record(kind: Kind, mut entry: serde_json::Value) {
    let Some(dir) = directory() else { return };
    let at = crate::applog::stamp();
    let date = at[..10].to_string();
    if let serde_json::Value::Object(map) = &mut entry {
        map.insert("at".to_string(), serde_json::Value::String(at));
    }
    let path = dir.join(file_name(kind, &date));
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = writeln!(f, "{line}");
    }
}

/// Removes only this app's `db-*.jsonl` / `api-*.jsonl` files under `dir`
/// older than `keep_days` by mtime - never a file this module did not
/// write, whatever else lives beside them.
pub fn prune(dir: &Path, now: SystemTime, keep_days: u64) {
    let cutoff = now
        .checked_sub(std::time::Duration::from_secs(keep_days * 86_400))
        .unwrap_or(std::time::UNIX_EPOCH);
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let ours = (name.starts_with("db-") || name.starts_with("api-")) && name.ends_with(".jsonl");
        if !ours {
            continue;
        }
        if e.metadata().and_then(|m| m.modified()).map(|m| m < cutoff).unwrap_or(false) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
