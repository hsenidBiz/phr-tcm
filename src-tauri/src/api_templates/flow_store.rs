//! Where API template flows are kept - see design doc "API template flows".
//!
//! Layout, under the Auto Run data root:
//!
//! ```text
//! flows/<project_slug>/<id>.json    the flow (see flow.rs)
//! ```
//!
//! `<project_slug>` is the same `autorun::recipe::project_slug` that
//! `store::templates_dir` uses, so a project's flows sit beside its
//! templates under the same name. `save` does not set `Flow::saved`: the
//! caller does, once the flow has been proven.

use super::flow::Flow;
use super::valid_id;
use crate::autorun::recipe::project_slug;
use std::path::{Path, PathBuf};

pub fn flows_dir(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("flows").join(project_slug(org, project))
}

fn flow_path(root: &Path, org: &str, project: &str, id: &str) -> PathBuf {
    flows_dir(root, org, project).join(format!("{id}.json"))
}

fn invalid_id(id: &str) -> String {
    format!("'{id}' is not a valid flow id")
}

/// Loads one flow by id, or `None` if it has never been saved.
pub fn load(root: &Path, org: &str, project: &str, id: &str) -> Result<Option<Flow>, String> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    match std::fs::read_to_string(flow_path(root, org, project, id)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map(Some).map_err(|e| format!("that flow is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Writes a flow, atomically, replacing whatever was saved under the same
/// id. Refuses before touching the disk if `f.id` is not a valid filename
/// component.
pub fn save(root: &Path, org: &str, project: &str, f: &Flow) -> Result<(), String> {
    let _held = flows_lock();
    write(root, org, project, f)
}

/// Why `save_if` did not write.
#[derive(Debug, Clone, PartialEq)]
pub enum SaveIfError {
    /// A flow of that id was there at the moment of writing (a file that no
    /// longer reads counts, as it does for the early check). Nothing was written.
    Appeared,
    /// The write itself failed.
    Failed(String),
}

/// `save`, but when `expect_absent` and a flow of that id is there now,
/// writes nothing and says `Appeared`. The check and the write happen under
/// one lock, so two calls that each found no flow cannot both save: the
/// later one finds the earlier one's. With `expect_absent` false it is `save`.
pub fn save_if(root: &Path, org: &str, project: &str, f: &Flow, expect_absent: bool) -> Result<(), SaveIfError> {
    if !valid_id(&f.id) {
        return Err(SaveIfError::Failed(invalid_id(&f.id)));
    }
    let _held = flows_lock();
    if expect_absent && !matches!(load(root, org, project, &f.id), Ok(None)) {
        return Err(SaveIfError::Appeared);
    }
    write(root, org, project, f).map_err(SaveIfError::Failed)
}

/// Every check-then-write and every delete of a flow file holds this: two
/// assistants' saves of one flow id, the person's import and a removal can
/// land at once. It is coordination, not a cache, and is never held across
/// an await (every function that takes it is synchronous).
fn flows_lock() -> std::sync::MutexGuard<'static, ()> {
    static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

/// The write both savers share; the caller holds the lock.
fn write(root: &Path, org: &str, project: &str, f: &Flow) -> Result<(), String> {
    if !valid_id(&f.id) {
        return Err(invalid_id(&f.id));
    }
    crate::autorun::roots::create_dir_all(root, flows_dir(root, org, project)).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(f).map_err(|e| e.to_string())?;
    crate::autorun::roots::write_file(root, &flow_path(root, org, project, &f.id), &json)
}

/// Every saved flow for this project, sorted by `title`. A flow file that
/// no longer reads or parses is skipped and logged - never fatal to the
/// rest of the list, since one bad file must not hide every other flow.
pub fn list(root: &Path, org: &str, project: &str) -> Result<Vec<Flow>, String> {
    let entries = match std::fs::read_dir(flows_dir(root, org, project)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !name.ends_with(".json") {
            continue;
        }
        let contents = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                crate::applog::warn(format!("api template flow file {name} could not be read, skipped: {e}"));
                continue;
            }
        };
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        match serde_json::from_str::<Flow>(contents) {
            Ok(f) => out.push(f),
            Err(e) => crate::applog::warn(format!("api template flow file {name} does not parse, skipped: {e}")),
        }
    }

    out.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(out)
}

/// Deletes a flow. A flow that was never saved is not an error - `remove`
/// only guarantees it is not there afterwards.
pub fn remove(root: &Path, org: &str, project: &str, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    let _held = flows_lock();
    match crate::autorun::roots::remove_file(root, &flow_path(root, org, project, id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
