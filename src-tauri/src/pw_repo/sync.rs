//! Sync: keeping the clone's copies of our Auto Run scripts current.
//!
//! - **The export record.** Every export (`pw_export::export::write_recorded`)
//!   records, for each case it wrote, the script's fingerprint, when, and
//!   the files written. One small JSON store per clone and project at
//!   `<app data>/pw-sync/<clone-hash>-<project>.json`, always replaced
//!   atomically (`ai_tools::atomic_write`). It is not a cache: nothing
//!   expires, and it survives a restart and a sign-in as someone else.
//! - **The states.** Each recorded case against its saved script now and
//!   the clone now (`states`).
//!
//! Update itself is the export: the same all-or-nothing write, for the
//! cases asked for only.

use crate::autorun::{approvals, recipe, store, CaseScript};
use crate::pw_export::clone;
use crate::pw_repo::structure;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The folder under the app's data folder that holds the records.
pub const SYNC_DIR: &str = "pw-sync";

/// What still has to happen in the repo for each updated case.
pub const NEEDS_REPO: &str = "needs /gen-test stages 5 and 6 in the repo";

/// The parts of a script a raw spec is made from, each hashed on its own so
/// an out-of-date case can say which changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parts {
    pub steps: String,
    pub area: String,
    pub account: String,
    pub marks: String,
}

/// A script's fingerprint: SHA-256 (lowercase hex) of the canonical JSON
/// (keys sorted all the way down) of its steps, area, account and marks.
/// Nothing else counts: saving it again, its repair count or when it was
/// saved changes nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub hash: String,
    pub parts: Parts,
}

fn sha(v: &Value) -> String {
    let text = serde_json::to_string(&approvals::canonical(v)).unwrap_or_default();
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn trimmed(s: &Option<String>) -> String {
    s.as_deref().map(str::trim).unwrap_or("").to_string()
}

pub fn fingerprint(script: &CaseScript) -> Fingerprint {
    let steps = serde_json::to_value(&script.steps).unwrap_or(Value::Null);
    let area = Value::String(trimmed(&script.area));
    let account = Value::String(trimmed(&script.account));
    let marks = json!({ "changes": script.changes, "needs_unchanged": script.needs_unchanged });
    let parts = Parts { steps: sha(&steps), area: sha(&area), account: sha(&account), marks: sha(&marks) };
    let hash = sha(&json!({ "steps": steps, "area": area, "account": account, "marks": marks }));
    Fingerprint { hash, parts }
}

/// One exported case, as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exported {
    pub title: String,
    pub fingerprint: Fingerprint,
    /// UTC "YYYY-MM-DDTHH:MM:SSZ".
    pub exported_at: String,
    /// Relative to the clone, forward slashes.
    pub files: Vec<String>,
    /// The case's Module the export placed it by, for an Update when Azure
    /// DevOps gives none.
    #[serde(default)]
    pub module: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRecord {
    #[serde(default)]
    pub cases: BTreeMap<i32, Exported>,
}

/// The clone folder as the record names it: the same spelling rules as
/// `runner::chosen_clone`, hashed.
pub fn clone_key(clone: &str) -> String {
    let p = clone.trim().replace('\\', "/");
    let p = p.trim_end_matches('/');
    let p = if cfg!(windows) { p.to_lowercase() } else { p.to_string() };
    Sha256::digest(p.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// `<dir>/<clone-hash>-<project>.json`.
pub fn record_path(dir: &Path, clone: &str, org: &str, project: &str) -> PathBuf {
    dir.join(format!("{}-{}.json", clone_key(clone), recipe::project_slug(org, project)))
}

/// The record at `path`; none yet is an empty one.
pub fn load(path: &Path) -> Result<ExportRecord, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(s.strip_prefix('\u{feff}').unwrap_or(&s))
            .map_err(|e| format!("the Sync record is not readable: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ExportRecord::default()),
        Err(e) => Err(format!("the Sync record could not be read: {e}")),
    }
}

/// `save` with the writer passed in: the seam a test drives.
pub fn save_with(
    path: &Path,
    record: &ExportRecord,
    write: impl FnOnce(&Path, &str) -> Result<(), String>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("the Sync record could not be saved: {e}"))?;
    }
    let text = serde_json::to_string_pretty(record).map_err(|e| format!("the Sync record could not be saved: {e}"))?;
    write(path, &text)
}

/// Replaces the record at `path` atomically: the old one stays whole until
/// the new one is complete.
pub fn save(path: &Path, record: &ExportRecord) -> Result<(), String> {
    save_with(path, record, crate::ai_tools::atomic_write)
}

/// Records `cases` as just exported, keeping every other case's entry.
pub fn record(path: &Path, cases: Vec<(i32, Exported)>) -> Result<(), String> {
    if cases.is_empty() {
        return Ok(());
    }
    let mut r = load(path)?;
    r.cases.extend(cases);
    save(path, &r)
}

/// Where a case stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind")]
pub enum SyncState {
    /// The script is unchanged since it was exported.
    Current,
    /// The script changed after it was exported; `why` says what.
    OutOfDate { why: String },
    /// Exported, but the clone has no section or no raw spec for it now.
    NotInRepo,
    /// The project has a script for it that was never exported.
    NeverExported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct SyncRow {
    pub case_id: i32,
    pub title: String,
    pub state: SyncState,
    /// When it was last exported, UTC.
    pub exported_at: Option<String>,
    /// Whether Auto Run still has its script, so Update can export it.
    pub has_script: bool,
}

fn changed(was: &Parts, now: &Parts) -> String {
    let mut what = Vec::new();
    if was.steps != now.steps {
        what.push("steps");
    }
    if was.area != now.area {
        what.push("area");
    }
    if was.account != now.account {
        what.push("account");
    }
    if was.marks != now.marks {
        what.push("marks");
    }
    if what.is_empty() {
        "the script changed since the export".to_string()
    } else {
        format!("changed since the export: {}", what.join(", "))
    }
}

fn same(a: &Option<String>, b: &str) -> bool {
    a.as_deref().is_some_and(|a| a.trim().eq_ignore_ascii_case(b.trim()))
}

/// Every case recorded for this clone and project, and every script saved
/// in this project that never was, each with its state, by case id.
/// `root` is Auto Run's store, `dir` the records' folder. Read only.
pub fn states(dir: &Path, root: &Path, clone_path: &str, org: &str, project: &str) -> Result<Vec<SyncRow>, String> {
    let clone = Path::new(clone_path.trim());
    let repo = clone::open(clone)?;
    let sections: BTreeSet<i32> =
        structure::read(clone, true)?.features.iter().flat_map(|f| f.case_ids.iter().copied()).collect();
    let raw: BTreeSet<i32> = repo
        .index
        .iter()
        .filter(|(_, f)| repo.generated_files.iter().any(|g| g == f))
        .filter_map(|(id, _)| id.parse().ok())
        .collect();
    let record = load(&record_path(dir, clone_path, org, project))?;

    let mut scripts: BTreeMap<i32, CaseScript> = store::list_scripts(root)
        .into_iter()
        .filter(|s| same(&s.project, project) && (s.organization.is_none() || same(&s.organization, org)))
        .map(|s| (s.case_id, s))
        .collect();
    for id in record.cases.keys() {
        if !scripts.contains_key(id) {
            if let Ok(Some(s)) = store::load_script(root, *id) {
                scripts.insert(*id, s);
            }
        }
    }

    let ids: BTreeSet<i32> = record.cases.keys().chain(scripts.keys()).copied().collect();
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        let script = scripts.get(&id);
        let rec = record.cases.get(&id);
        let state = match (rec, script) {
            (None, _) => SyncState::NeverExported,
            (Some(_), _) if !(sections.contains(&id) && raw.contains(&id)) => SyncState::NotInRepo,
            (Some(_), None) => SyncState::OutOfDate { why: "its script is no longer in Auto Run".into() },
            (Some(r), Some(s)) => {
                let now = fingerprint(s);
                if now.hash == r.fingerprint.hash {
                    SyncState::Current
                } else {
                    SyncState::OutOfDate { why: changed(&r.fingerprint.parts, &now.parts) }
                }
            }
        };
        rows.push(SyncRow {
            case_id: id,
            title: script.map(|s| s.title.clone()).or_else(|| rec.map(|r| r.title.clone())).unwrap_or_default(),
            state,
            exported_at: rec.map(|r| r.exported_at.clone()),
            has_script: script.is_some(),
        });
    }
    Ok(rows)
}
