//! A note, written before an upload sends its first batch, of what the
//! upload is about to create - so that if the app dies mid-upload, the next
//! start can tell the person which cases may already be in Azure DevOps.
//!
//! One small JSON file per (organization, PBI) under the app's local data
//! folder. The webview clears it once it has reconciled the upload; Rust
//! never clears it (see `commands/queue.rs`). Each file records which
//! account wrote it, and another account never sees or removes it.
//!
//! The organization is hashed into the file name, so no name of a client
//! ever sits in a folder listing.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const DIR_NAME: &str = "upload-journal";

fn dir_cell() -> &'static Mutex<Option<PathBuf>> {
    static D: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(None))
}

/// Points the journal at its folder. Re-callable (once from the Tauri
/// setup, once per test with a fresh tempdir).
pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut cell) = dir_cell().lock() {
        *cell = Some(dir);
    }
}

/// The journal folder, once `init` has run.
pub fn directory() -> Option<PathBuf> {
    dir_cell().lock().ok().and_then(|d| d.clone())
}

/// What an upload meant to create when it started.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UploadJournal {
    pub organization: String,
    pub project: String,
    pub pbi_id: i32,
    /// ISO-8601 lower bound for "created by this upload".
    pub since: String,
    /// Titles of the rows that create (no `update_id`), in queue order.
    pub titles: Vec<String>,
    /// The webview's hold signature of each of those rows (empty when the
    /// webview sent none that lined up with the queue).
    pub sigs: Vec<String>,
    /// Ids of the PBI's linked cases that existed before the upload.
    pub ids: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalRead {
    None,
    Running,
    Found { journal: UploadJournal },
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OnDisk {
    owner: String,
    journal: UploadJournal,
}

/// `"{hash of org}-{pbi}.json"`: no organization text in the name.
pub fn file_name(organization: &str, pbi_id: i32) -> String {
    format!("{:08x}-{pbi_id}.json", crate::autorun::recipe::fnv1a(organization))
}

/// The journal for an upload about to start; `None` when no row creates.
pub fn journal_for_submit(
    organization: &str,
    project: &str,
    pbi_id: i32,
    since: &str,
    queue: &[crate::model::TestCase],
    pre_existing: &[i32],
    hold_sigs: &[String],
) -> Option<UploadJournal> {
    let aligned = hold_sigs.len() == queue.len();
    let mut titles = Vec::new();
    let mut sigs = Vec::new();
    for (i, tc) in queue.iter().enumerate() {
        if tc.update_id.is_none() {
            titles.push(tc.title.clone());
            if aligned {
                sigs.push(hold_sigs[i].clone());
            }
        }
    }
    if titles.is_empty() {
        return None;
    }
    let mut ids: Vec<i32> = Vec::new();
    for id in pre_existing {
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    Some(UploadJournal {
        organization: organization.to_string(),
        project: project.to_string(),
        pbi_id,
        since: since.to_string(),
        titles,
        sigs,
        ids,
    })
}

pub fn write_in(dir: &Path, owner: &str, j: &UploadJournal) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("could not create the journal folder: {e}"))?;
    let body = serde_json::to_string(&OnDisk { owner: owner.to_string(), journal: j.clone() })
        .map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&dir.join(file_name(&j.organization, j.pbi_id)), &body)
}

fn load(path: &Path) -> Option<OnDisk> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn read_in(dir: &Path, owner: &str, organization: &str, pbi_id: i32) -> JournalRead {
    if is_running(organization, pbi_id) {
        return JournalRead::Running;
    }
    match load(&dir.join(file_name(organization, pbi_id))) {
        Some(d) if d.owner == owner => JournalRead::Found { journal: d.journal },
        _ => JournalRead::None,
    }
}

pub fn clear_in(dir: &Path, owner: &str, organization: &str, pbi_id: i32) -> Result<(), String> {
    let path = dir.join(file_name(organization, pbi_id));
    match load(&path) {
        Some(d) if d.owner == owner => std::fs::remove_file(&path).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(format!("could not remove the upload journal: {e}"))
            }
        }),
        _ => Ok(()),
    }
}

pub fn list_in(dir: &Path, owner: &str, organization: &str) -> Vec<UploadJournal> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(d) = load(&path) else {
            crate::applog::warn("skipped an upload journal file that could not be read");
            continue;
        };
        if d.owner != owner || d.journal.organization != organization || is_running(organization, d.journal.pbi_id) {
            continue;
        }
        out.push(d.journal);
    }
    out.sort_by_key(|j| j.pbi_id);
    out
}

fn running() -> &'static Mutex<Vec<(String, i32)>> {
    static R: OnceLock<Mutex<Vec<(String, i32)>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Vec::new()))
}

/// Held while an upload for (organization, PBI) runs; dropping it clears the mark.
pub struct RunningMark {
    key: (String, i32),
}

pub fn mark_running(organization: &str, pbi_id: i32) -> RunningMark {
    let key = (organization.to_string(), pbi_id);
    if let Ok(mut v) = running().lock() {
        v.push(key.clone());
    }
    RunningMark { key }
}

impl Drop for RunningMark {
    fn drop(&mut self) {
        if let Ok(mut v) = running().lock() {
            if let Some(i) = v.iter().position(|k| *k == self.key) {
                v.remove(i);
            }
        }
    }
}

pub fn is_running(organization: &str, pbi_id: i32) -> bool {
    running()
        .lock()
        .map(|v| v.iter().any(|(o, p)| o == organization && *p == pbi_id))
        .unwrap_or(false)
}
