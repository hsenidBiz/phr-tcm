//! AI Findings: the notes an assistant leaves when something it read is
//! wrong - a test case that contradicts its spec, a spec that contradicts
//! itself, code that does not do what either says.
//!
//! Local app data, never Azure DevOps. One JSON file under `app_data_dir`,
//! newest first, bounded. The AI bridge writes through the process-wide
//! root set at app setup (it has no `AppHandle`), the same arrangement as
//! the Auto Run store - two derivations of "where findings live" is how a
//! recorded finding ends up somewhere the AI Bridge tab never looks.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tauri_specta::Event;

pub const KINDS: [&str; 3] = ["test_case", "spec", "code"];
pub const STATUSES: [&str; 2] = ["open", "resolved"];
/// Newest kept; a store that grows without bound is a store nobody reads.
pub const CAP: usize = 500;
const FILE: &str = "findings.json";
const MAX_TEXT: usize = 8_000;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Finding {
    pub id: String,
    pub org: String,
    pub project: String,
    /// One of `KINDS`.
    pub kind: String,
    /// What it is about: a work item id, a spec path and section, a file path.
    pub subject: String,
    pub title: String,
    /// Markdown, rendered in the app and in the browser report.
    pub detail: String,
    /// RFC 3339, UTC.
    pub created_at: String,
    /// One of `STATUSES`.
    pub status: String,
}

pub struct NewFinding {
    pub org: String,
    pub project: String,
    pub kind: String,
    pub subject: String,
    pub title: String,
    pub detail: String,
}

static ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Serializes every load-mutate-save cycle across the whole process. The
/// bridge serves connections concurrently and the Tauri commands run on
/// other threads, so without this two writers can interleave: both load
/// the same snapshot, both write, and the loser's `save_all` clobbers the
/// winner's. Held across load *and* save in `record`, `set_status` and
/// `remove` - never just around the save.
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// Called once during app setup.
pub fn set_root(root: PathBuf) {
    if let Ok(mut slot) = ROOT.lock() {
        *slot = Some(root);
    }
}

/// `None` before setup has run - a caller with no handle must say so
/// rather than guess a path and write where nobody reads.
pub fn configured_root() -> Option<PathBuf> {
    ROOT.lock().ok().and_then(|s| s.clone())
}

/// The handle `record` emits `FindingRecorded` through. Absent in tests
/// and before setup; the write still happens, only the event is skipped.
pub fn set_app_handle(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app);
}

fn file(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// Every finding in the store, newest first. A missing or unreadable file
/// is an empty store: the next write replaces it.
fn load_all(root: &Path) -> Vec<Finding> {
    let Ok(raw) = std::fs::read_to_string(file(root)) else {
        return vec![];
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Temp file then rename, so a crash mid-write leaves the old file whole.
/// The temp name is unique per write (pid plus a process-wide counter),
/// not just per process: the lock already serializes the load-mutate-save
/// cycle, but a shared name would still let a leftover temp file from a
/// prior crash collide with a fresh write.
fn save_all(root: &Path, all: &[Finding]) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let target = file(root);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = root.join(format!("{FILE}.{}.{n}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(all).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
}

pub fn list(root: &Path, org: &str, project: &str) -> Vec<Finding> {
    load_all(root)
        .into_iter()
        .filter(|f| f.org == org && f.project == project)
        .collect()
}

/// Only the open ones - what the card, the bell and the report show first.
pub fn list_open(root: &Path, org: &str, project: &str) -> Vec<Finding> {
    list(root, org, project).into_iter().filter(|f| f.status == "open").collect()
}

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-date arithmetic (Howard Hinnant's algorithm), so no clock
    // crate is pulled in for one timestamp.
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{millis}-{}", N.fetch_add(1, Ordering::SeqCst))
}

fn clip(s: &str) -> String {
    s.trim().chars().take(MAX_TEXT).collect()
}

/// Validate, prepend, cap, write, announce.
pub fn record(root: &Path, new: NewFinding) -> Result<Finding, String> {
    if !KINDS.contains(&new.kind.as_str()) {
        return Err(format!("kind must be one of test_case, spec or code, not \"{}\"", new.kind));
    }
    if new.title.trim().is_empty() {
        return Err("a finding needs a title".into());
    }
    if new.org.trim().is_empty() || new.project.trim().is_empty() {
        return Err("a finding belongs to an organization and project".into());
    }
    let finding = Finding {
        id: new_id(),
        org: new.org,
        project: new.project,
        kind: new.kind,
        subject: clip(&new.subject),
        title: clip(&new.title),
        detail: clip(&new.detail),
        created_at: now_rfc3339(),
        status: "open".into(),
    };
    {
        let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = load_all(root);
        all.insert(0, finding.clone());
        all.truncate(CAP);
        save_all(root, &all)?;
    }
    if let Some(app) = APP_HANDLE.get() {
        let _ = crate::events::FindingRecorded {
            id: finding.id.clone(),
            org: finding.org.clone(),
            project: finding.project.clone(),
            kind: finding.kind.clone(),
            title: finding.title.clone(),
        }
        .emit(app);
    }
    Ok(finding)
}

pub fn set_status(root: &Path, id: &str, status: &str) -> Result<Finding, String> {
    if !STATUSES.contains(&status) {
        return Err(format!("status must be open or resolved, not \"{status}\""));
    }
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = load_all(root);
    let Some(f) = all.iter_mut().find(|f| f.id == id) else {
        return Err(format!("no finding with id {id}"));
    };
    f.status = status.into();
    let out = f.clone();
    save_all(root, &all)?;
    Ok(out)
}

pub fn remove(root: &Path, id: &str) -> Result<(), String> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = load_all(root);
    let before = all.len();
    all.retain(|f| f.id != id);
    if all.len() == before {
        return Err(format!("no finding with id {id}"));
    }
    save_all(root, &all)
}
