//! The record of test-made drafts: one entry per thing a fixture (or, later,
//! a script's setup) made in the application under test, kept in
//! `test-made.json` in the Auto Run store. It is what Clean up test-made
//! drafts works from, so it is the one list of things the tests may delete.
//! See the design doc, "The record of test-made drafts" (section 3).
//!
//! Only the fixture runner adds entries (`record`), and only Clean up
//! changes a status (`set_status`). Both writers are `pub(crate)`: no tool
//! and no IPC command can add, edit or remove an entry. `list` is the one
//! public reader.
//!
//! An entry holds ids and names only - never a password, a cookie, a host
//! or a query string.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What a made thing's `status` is while it is still there.
pub const PRESENT: &str = "present";
/// What Clean up sets once it deleted the thing.
pub const DELETED: &str = "deleted";

/// What Clean up sets when its delete did not work.
pub fn delete_failed(reason: &str) -> String {
    format!("delete failed: {reason}")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct TestMade {
    /// The id of the environment it was made in.
    pub environment: String,
    /// What it is (`cycle`, `suite` and so on), as the fixture's `creates`
    /// names it - the kind a delete template deletes.
    pub kind: String,
    /// Its id in the application.
    pub id: String,
    pub name: String,
    /// When it was made, ISO 8601 UTC.
    pub created_at: String,
    /// The id of the fixture that made it.
    pub fixture: String,
    /// The fixture run that made it.
    pub run_id: String,
    /// The case whose setup made it, when a setup did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_id: Option<i32>,
    /// `present`, `deleted` or `delete failed: <the reason>`.
    pub status: String,
}

fn path(root: &Path) -> PathBuf {
    root.join("test-made.json")
}

/// Every read-change-write of the file holds this, so a record and a status
/// change landing together cannot lose one another's entries.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

/// The record as it is on disk. A file that is not there is an empty
/// record; one that does not read is an error, so a writer never replaces
/// entries it could not read with only its own.
fn load(root: &Path) -> Result<Vec<TestMade>, String> {
    match std::fs::read_to_string(path(root)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|e| format!("the record of test-made drafts is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

fn write(root: &Path, entries: &[TestMade]) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path(root), &json)
}

/// Every entry, oldest first. A record that does not read is logged and
/// shown as empty.
pub fn list(root: &Path) -> Vec<TestMade> {
    let _held = lock();
    load(root).unwrap_or_else(|e| {
        crate::applog::warn(format!("the record of test-made drafts could not be read: {e}"));
        vec![]
    })
}

/// Adds `entries` after the ones already recorded. Only the fixture runner
/// calls this.
pub(crate) fn record(root: &Path, entries: &[TestMade]) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    let _held = lock();
    let mut all = load(root)?;
    all.extend(entries.iter().cloned());
    write(root, &all)
}

/// Sets the status of the entry for `kind` `id` in `environment`. `Ok(false)`
/// when there is no such entry. Only Clean up calls this.
// Clean up of test-made drafts is its caller, and is not built yet.
#[allow(dead_code)]
pub(crate) fn set_status(root: &Path, environment: &str, kind: &str, id: &str, status: &str) -> Result<bool, String> {
    let _held = lock();
    let mut all = load(root)?;
    let mut found = false;
    for e in all.iter_mut().filter(|e| e.environment == environment && e.kind == kind && e.id == id) {
        e.status = status.to_string();
        found = true;
    }
    if found {
        write(root, &all)?;
    }
    Ok(found)
}
