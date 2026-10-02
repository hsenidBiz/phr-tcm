//! Export / import of the app's local state, for moving to a new machine.
//!
//! One JSON file carries both halves of what the app remembers:
//! - the webview's `tcm-v2-*` localStorage (settings, theme, drafts, the
//!   local cache db, pins, watch paths) - collected by the frontend and
//!   handed in, since only the webview can read it;
//! - the disk stores under `app_data_dir` (`cache.json` - tags, resolved
//!   suites and the assigned-items baseline - plus Auto Run scripts and
//!   local runs, and materialized shared drafts).
//!
//! What deliberately never travels: credentials. Sign-in tokens live only
//! in memory, the AI-bridge token is re-minted per launch in the OS temp
//! dir, and the app log stays where it is - none of those are under the
//! roots this module reads.

use base64::Engine;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// The disk roots (relative to `app_data_dir`) a backup carries. An
/// allowlist, applied on BOTH sides: export never wanders into logs or
/// strangers' files, and import refuses to write anywhere else - a crafted
/// backup must not be able to drop files outside these folders.
///
/// `reference-cache.json` and `suite-cache.json` are the pre-unified-cache
/// files: kept here so a backup made before that migration still imports -
/// `cache::Store::open` folds them into `cache.json` on next open.
///
/// `databases.json` names the person's own databases - ids and labels,
/// never a login - so environments restored beside it keep meaning the same
/// database. It is restored as a MERGE (`db::catalog::merge_backup`), never
/// written over the list here.
const ROOTS: [&str; 6] =
    ["cache.json", "reference-cache.json", "suite-cache.json", "autorun", "shared-drafts", "databases.json"];

/// A single file per entry is capped so one enormous stray artifact cannot
/// balloon the backup into something no one can email or copy around.
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// Inside an allowed root, but never exported and never imported: session
/// files hold live cookies, and the screenshots folder is evidence for the
/// run in front of the person, up to 200 images of it.
pub const EXCLUDED: [&str; 2] = ["autorun/sessions", "autorun/shots"];

/// `rel` normalised to forward slashes, so an entry written as
/// `autorun/sessions\admin.json` (Windows splits a path on both
/// separators, so it still passes the component check in
/// `safe_relative_path`) is still caught. The comparison is ASCII
/// case-insensitive too - Windows' file system is, so `AUTORUN/Sessions`
/// and `autorun/sessions` are the same folder on disk whatever an entry's
/// path happens to spell it as. `.get()` rather than slicing avoids a
/// panic if `x.len()` does not land on a char boundary of `rel`.
fn excluded(rel: &str) -> bool {
    let rel = rel.replace('\\', "/");
    EXCLUDED.iter().any(|x| {
        rel.eq_ignore_ascii_case(x)
            || rel
                .get(..x.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(x) && rel.as_bytes().get(x.len()) == Some(&b'/'))
    })
}

#[derive(serde::Serialize, serde::Deserialize, specta::Type, Clone, Debug)]
pub struct BackupFile {
    /// Path relative to `app_data_dir`, forward slashes.
    pub path: String,
    /// File content, base64.
    pub b64: String,
}

#[derive(serde::Serialize, serde::Deserialize, specta::Type, Clone, Debug)]
pub struct BackupDoc {
    /// Sentinel + format version, so a random JSON file is refused with a
    /// clear message instead of half-imported.
    pub app: String,
    pub format: u32,
    pub exported_at: String,
    pub app_version: String,
    pub local_storage: BTreeMap<String, String>,
    pub files: Vec<BackupFile>,
}

#[derive(serde::Serialize, specta::Type, Clone, Debug)]
pub struct ExportSummary {
    pub path: String,
    pub keys: u32,
    pub files: u32,
    /// Files left out (too large), named so the export is honest about it.
    pub skipped: Vec<String>,
}

#[derive(serde::Serialize, specta::Type, Clone, Debug)]
pub struct BackupImportResult {
    pub local_storage: BTreeMap<String, String>,
    pub files_restored: u32,
    pub exported_at: String,
    pub app_version: String,
}

pub const APP_SENTINEL: &str = "tcm-v2-backup";
pub const FORMAT: u32 = 1;

/// Walk the allowlisted roots under `data_dir`. Returns kept files as
/// `(relative_path, bytes)` plus the relative paths skipped for size.
pub fn collect_files(data_dir: &Path) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
    let mut kept = vec![];
    let mut skipped = vec![];
    for root in ROOTS {
        walk(data_dir, &data_dir.join(root), &mut kept, &mut skipped);
    }
    kept.sort_by(|a, b| a.0.cmp(&b.0));
    skipped.sort();
    (kept, skipped)
}

fn walk(base: &Path, p: &Path, kept: &mut Vec<(String, Vec<u8>)>, skipped: &mut Vec<String>) {
    let Ok(meta) = std::fs::metadata(p) else { return };
    let rel = match p.strip_prefix(base) {
        Ok(r) => r.to_string_lossy().replace('\\', "/"),
        Err(_) => return,
    };
    if excluded(&rel) {
        return;
    }
    if meta.is_dir() {
        let Ok(entries) = std::fs::read_dir(p) else { return };
        for e in entries.flatten() {
            walk(base, &e.path(), kept, skipped);
        }
        return;
    }
    if meta.len() > MAX_FILE_BYTES {
        skipped.push(rel);
        return;
    }
    if let Ok(bytes) = std::fs::read(p) {
        kept.push((rel, bytes));
    }
}

/// Is `rel` a path import may write? Relative, inside one of the roots,
/// and free of `..`/absolute/drive components (the zip-slip family).
pub fn safe_relative_path(rel: &str) -> bool {
    if rel.is_empty() || rel.len() > 500 {
        return false;
    }
    let p = Path::new(rel);
    if !p
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return false;
    }
    if excluded(rel) {
        return false;
    }
    ROOTS
        .iter()
        .any(|root| rel == *root || rel.starts_with(&format!("{root}/")))
}

pub fn build_doc(
    app_version: &str,
    exported_at: &str,
    local_storage: BTreeMap<String, String>,
    files: Vec<(String, Vec<u8>)>,
) -> BackupDoc {
    let b64 = base64::engine::general_purpose::STANDARD;
    BackupDoc {
        app: APP_SENTINEL.into(),
        format: FORMAT,
        exported_at: exported_at.into(),
        app_version: app_version.into(),
        local_storage,
        files: files
            .into_iter()
            .map(|(path, bytes)| BackupFile { path, b64: b64.encode(bytes) })
            .collect(),
    }
}

/// Atomic write: temp sibling then rename, so a failed export never leaves
/// a truncated file that LOOKS like a backup.
pub fn write_doc(doc: &BackupDoc, path: &Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
    let tmp: PathBuf = path.with_extension("tmp-export");
    std::fs::write(&tmp, json).map_err(|e| format!("could not write the backup: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not write the backup: {e}")
    })
}

pub fn read_doc(path: &Path) -> Result<BackupDoc, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not read the file: {e}"))?;
    let doc: BackupDoc =
        serde_json::from_str(&text).map_err(|_| "this is not a Test Case Manager backup file".to_string())?;
    if doc.app != APP_SENTINEL {
        return Err("this is not a Test Case Manager backup file".into());
    }
    if doc.format > FORMAT {
        return Err(format!(
            "this backup was made by a newer version of the app (format {}) - update this copy first",
            doc.format
        ));
    }
    Ok(doc)
}

/// Restore the disk half of a backup. Unsafe paths are refused one by one
/// (the rest still land); returns how many files were written.
pub fn restore_files(data_dir: &Path, files: &[BackupFile]) -> Result<u32, String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut restored = 0u32;
    // A backup from before environments: the machine-wide list, and no
    // environments file to say whose it is. Kept until the loop is done.
    let mut legacy_accounts: Option<Vec<u8>> = None;
    let mut has_environments = false;
    for f in files {
        if !safe_relative_path(&f.path) {
            crate::applog::warn(format!("Backup import skipped an unsafe path: {}", f.path));
            continue;
        }
        let bytes = b64
            .decode(&f.b64)
            .map_err(|_| format!("corrupt backup: {} is not valid base64", f.path))?;
        let target = data_dir.join(&f.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let norm = f.path.replace('\\', "/").to_ascii_lowercase();
        if norm == "autorun/accounts.json" {
            legacy_accounts = Some(bytes.clone());
        }
        has_environments |= norm == "autorun/environments.json";
        if norm == "databases.json" {
            crate::db::catalog::merge_backup(&target, &bytes)?;
            restored += 1;
            continue;
        }
        std::fs::write(&target, bytes).map_err(|e| format!("could not restore {}: {e}", f.path))?;
        restored += 1;
        // The old single list, or any one environment's list
        // (`autorun/accounts/<env id>.json`).
        let is_accounts_file = norm == "autorun/accounts.json"
            || norm.strip_prefix("autorun/accounts/").is_some_and(|rest| rest.ends_with(".json") && !rest.contains('/'));
        if is_accounts_file {
            // `accounts::save_accounts` drops a saved session the moment the
            // login behind it changes, so a saved session never outlives the
            // login it was made with - through the app. A restore writes
            // accounts.json straight to disk instead, bypassing that, so it
            // has to make the same guarantee itself or the next run restores
            // the OLD person's cookies under whatever account the backup's
            // admin now is. Sessions are a cache: the cost of wiping all of
            // them is one real sign-in, so this is best effort and its own
            // failure is not the restore's failure. Done here, inside the
            // loop, rather than once after it: a later entry can still fail
            // the whole restore (the `?` above returns early), and
            // yesterday's cookies must not survive under today's names even
            // then.
            let _ = std::fs::remove_dir_all(data_dir.join("autorun").join("sessions"));
        }
    }
    if let Some(list) = legacy_accounts {
        restore_legacy_accounts(&data_dir.join("autorun"), &list, has_environments)?;
    }
    Ok(restored)
}

/// The machine-wide `accounts.json` a backup restored. Nothing reads that
/// file since environments: from a backup made before them (no
/// `environments.json` in it) the list becomes the ACTIVE environment's -
/// Default is made from it if there is no environment yet. A backup that
/// carries its own environments carries their lists too, and the stray
/// machine-wide one is only a frozen copy. Either way it is not left on
/// disk: it holds passwords and nothing would ever read it.
fn restore_legacy_accounts(root: &Path, list: &[u8], has_environments: bool) -> Result<(), String> {
    if !has_environments {
        let failed = |e: String| format!("the Auto Run accounts were restored but could not be given to the active environment: {e}");
        // Makes Default from the restored file when there is no
        // environments file yet (and then removes it).
        let id = crate::environments::active_id(root).map_err(failed)?;
        let to = crate::autorun::accounts::accounts_path_for(root, &id);
        std::fs::create_dir_all(to.parent().unwrap_or(root)).map_err(|e| failed(e.to_string()))?;
        let text = String::from_utf8_lossy(list);
        crate::ai_tools::atomic_write(&to, &text).map_err(failed)?;
        crate::applog::info("Backup import: the Auto Run accounts went to the active environment");
    }
    match std::fs::remove_file(root.join("accounts.json")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            crate::applog::warn(format!("Backup import: the old accounts file stayed behind: {e}"))
        }
        _ => {}
    }
    Ok(())
}
