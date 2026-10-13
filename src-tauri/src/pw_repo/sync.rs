//! Sync: keeping the clone's copies of our Auto Run scripts current.
//!
//! - **The export record.** Every export (`pw_export::export::write_recorded`)
//!   records, for each case it wrote, the fingerprint of what it read, when,
//!   and the files written. One small JSON store per clone and project at
//!   `<app data>/pw-sync/<clone-hash>-<project>.json`, always replaced
//!   atomically (`ai_tools::atomic_write`). It is not a cache: nothing
//!   expires, and it survives a restart and a sign-in as someone else.
//! - **The states.** Each recorded case against what an export would read
//!   of it now, and the clone now (`states`).
//!
//! Update itself is the export: the same all-or-nothing write, for the
//! cases asked for only. Add new is the export of every case with a script
//! the repo has in none of its places (`pw_repo::presence`, `add_new`).

use crate::autorun::{approvals, recipe, store, CaseScript};
use crate::pw_export::clone;
use crate::pw_export::export::Fingerprinter;
use crate::pw_repo::{presence, structure};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The folder under the app's data folder that holds the records.
pub const SYNC_DIR: &str = "pw-sync";

/// The parts of what an export reads of a case, each hashed on its own so
/// an out-of-date case can say which changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parts {
    pub title: String,
    pub steps: String,
    pub area: String,
    pub account: String,
}

/// A case's fingerprint: SHA-256 (lowercase hex) of the canonical JSON
/// (keys sorted all the way down) of exactly what the export reads of it
/// (`FingerprintInput`). Nothing else counts: saving the script again, its
/// repair count, when it was saved or its marks (which the export never
/// reads) change nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub hash: String,
    pub parts: Parts,
}

/// What the export reads of a case (`export::Fingerprinter` gathers it).
pub struct FingerprintInput {
    pub title: String,
    /// The steps with every component expanded, as JSON.
    pub steps: Value,
    /// The area the case starts in (or the Module it is placed by).
    pub area: String,
    /// That area's menu clicks, as JSON.
    pub area_clicks: Value,
    /// The script's account key.
    pub account: String,
    /// The repo user that account maps to, if it is mapped.
    pub user_key: Option<String>,
}

fn sha(v: &Value) -> String {
    let text = serde_json::to_string(&approvals::canonical(v)).unwrap_or_default();
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn fingerprint(input: &FingerprintInput) -> Fingerprint {
    let title = Value::String(input.title.clone());
    let area = json!({ "name": input.area, "clicks": input.area_clicks });
    let account = json!({ "account": input.account, "user_key": input.user_key });
    let parts = Parts { title: sha(&title), steps: sha(&input.steps), area: sha(&area), account: sha(&account) };
    let hash = sha(&json!({ "title": title, "steps": input.steps, "area": area, "account": account }));
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
    /// The case's Module the export placed it by: the states read the case
    /// by it, and an Update falls back on it when Azure DevOps gives none.
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

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// The record at `path`; none yet is an empty one. One that cannot be
/// parsed is moved aside as `<name>.corrupt-<ms>.json` beside it, logged,
/// and read as empty: Sync starts again from scratch, and the next export
/// records as usual.
pub fn load(path: &Path) -> Result<ExportRecord, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ExportRecord::default()),
        Err(e) => return Err(format!("the Sync record could not be read: {e}")),
    };
    let e = match serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text)) {
        Ok(r) => return Ok(r),
        Err(e) => e,
    };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let backup = path.with_file_name(format!("{stem}.corrupt-{ms}.json"));
    match std::fs::rename(path, &backup) {
        Ok(()) => crate::applog::warn(format!(
            "pw-sync: the Sync record {} could not be read ({e}), so it was kept as {} and Sync starts again",
            name_of(path),
            name_of(&backup)
        )),
        Err(m) => crate::applog::warn(format!(
            "pw-sync: the Sync record {} could not be read ({e}) nor moved aside ({m}), so it is read as empty",
            name_of(path)
        )),
    }
    Ok(ExportRecord::default())
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

/// Held across a record's load, extend and save, so two exports at once
/// cannot each keep only their own cases.
static RECORDING: Mutex<()> = Mutex::new(());

/// Records `cases` as just exported, keeping every other case's entry.
pub fn record(path: &Path, cases: Vec<(i32, Exported)>) -> Result<(), String> {
    if cases.is_empty() {
        return Ok(());
    }
    let _held = RECORDING.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = load(path)?;
    r.cases.extend(cases);
    save(path, &r)
}

/// Where a case stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind")]
pub enum SyncState {
    /// Nothing the export reads of it changed since it was exported.
    Current,
    /// Something the export reads of it changed; `why` says what.
    OutOfDate { why: String },
    /// Exported, but the clone has no section or no raw spec for it now.
    NotInRepo,
    /// The project has a script for it that was never exported, and the
    /// repo has it nowhere.
    NeverExported,
    /// The repo has it, but this app has no export of it on record: someone
    /// wrote it by hand, or exported it from another machine. Update out of
    /// date and Add new never touch it; Update ticked replaces it only when
    /// it is ticked.
    InRepoNotFromApp,
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
    /// The repo has it in at least one place (`pw_repo::presence`).
    pub in_repo: bool,
    /// Where, short: "in competencies.md and specs/competencies.spec.ts".
    pub repo_note: Option<String>,
}

fn changed(was: &Parts, now: &Parts) -> String {
    let mut what = Vec::new();
    if was.title != now.title {
        what.push("title");
    }
    if was.steps != now.steps {
        what.push("steps");
    }
    if was.area != now.area {
        what.push("area");
    }
    if was.account != now.account {
        what.push("account");
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
/// `root` is Auto Run's store, `dir` the records' folder. Read only, but
/// for a record that cannot be parsed (`load`).
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
    let found = presence::scan_repo(&repo);
    let record = load(&record_path(dir, clone_path, org, project))?;
    let fingerprints = Fingerprinter::load(root, org, project)?;

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
            (None, _) if found.has(id) => SyncState::InRepoNotFromApp,
            (None, _) => SyncState::NeverExported,
            (Some(_), _) if !(sections.contains(&id) && raw.contains(&id)) => SyncState::NotInRepo,
            (Some(_), None) => SyncState::OutOfDate { why: "its script is no longer in Auto Run".into() },
            (Some(r), Some(_)) => match fingerprints.of(root, org, project, id, &r.module)? {
                Some(now) if now.hash == r.fingerprint.hash => SyncState::Current,
                Some(now) => SyncState::OutOfDate { why: changed(&r.fingerprint.parts, &now.parts) },
                None => SyncState::OutOfDate { why: "its script is no longer in Auto Run".into() },
            },
        };
        rows.push(SyncRow {
            case_id: id,
            title: script.map(|s| s.title.clone()).or_else(|| rec.map(|r| r.title.clone())).unwrap_or_default(),
            state,
            exported_at: rec.map(|r| r.exported_at.clone()),
            has_script: script.is_some(),
            in_repo: found.has(id),
            repo_note: found.note(id),
        });
    }
    Ok(rows)
}

/// The cases Add new exports: every one with a script that the repo has in
/// none of its places. That is each never-exported case, and each NotInRepo
/// case whose section, index entry and spec marker are all gone: it is not in
/// the repo, so adding it back is adding a new case. A NotInRepo case the
/// repo still has somewhere (its section, say) is left alone, as is every
/// InRepoNotFromApp, Current and OutOfDate case.
pub fn add_new(rows: &[SyncRow]) -> Vec<i32> {
    rows.iter().filter(|r| r.has_script && !r.in_repo).map(|r| r.case_id).collect()
}

/// The cases of an Update the write may replace although the repo has them
/// (`export::refuse_present`): the ones this app exported (on `record`),
/// which is what Sync is for, and the ones the person ticked (`ticked`).
/// An InRepoNotFromApp case is replaced only when it is ticked.
pub fn update_replace(record: &ExportRecord, cases: &[i32], ticked: &[i32]) -> Vec<i32> {
    cases.iter().copied().filter(|id| record.cases.contains_key(id) || ticked.contains(id)).collect()
}
