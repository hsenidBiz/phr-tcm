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

/// The path's segments, split on either separator (Windows splits a path on
/// both, so an entry written as `autorun/sessions\admin.json` is still the
/// sessions folder), with empty and `.` segments and trailing dots and spaces
/// (which Windows drops) removed. Case is kept.
fn raw_segments(rel: &str) -> Vec<&str> {
    rel.split(['/', '\\'])
        .map(|s| s.trim_end_matches(['.', ' ']))
        .filter(|s| !s.is_empty())
        .collect()
}

/// `rel` is one of the folders a backup never carries, whatever letter case
/// it is spelt in (Windows' file system is case-insensitive, so
/// `AUTORUN/Sessions` and `autorun/sessions` are the same folder):
/// - `EXCLUDED`;
/// - the removed-environments area, and the staging folders an update, a
///   copy and an import leave (`.envs-staging-*`, `envs/.copying-*`,
///   `.import-*`);
/// - every environment's screenshots (`envs/<id>/shots`), as the flat
///   layout's were.
fn excluded(rel: &str) -> bool {
    let seg: Vec<String> = raw_segments(rel).iter().map(|s| s.to_ascii_lowercase()).collect();
    if EXCLUDED.iter().any(|x| {
        let parts: Vec<&str> = x.split('/').collect();
        seg.len() >= parts.len() && seg.iter().zip(&parts).all(|(a, b)| a == b)
    }) {
        return true;
    }
    if seg.first().map(String::as_str) != Some("autorun") {
        return false;
    }
    match seg.get(1).map(String::as_str) {
        Some("removed-envs") => true,
        Some(n) if n.starts_with(".envs-staging-") || n.starts_with(".import-") => true,
        Some("envs") => {
            seg.get(2).is_some_and(|n| n.starts_with(".copying-") || n.starts_with(".seed-replaced-"))
                || seg.get(3).map(String::as_str) == Some("shots")
        }
        _ => false,
    }
}

/// `autorun/<a per-environment item>/...` (or the item itself): a file the
/// flat layout kept at the top of the Auto Run folder. Its second segment.
fn is_flat_original(rel: &str) -> bool {
    let seg = raw_segments(rel);
    seg.first().is_some_and(|s| s.eq_ignore_ascii_case("autorun"))
        && seg.get(1).is_some_and(|n| crate::autorun::roots::PER_ENV_ITEMS.iter().any(|(item, _)| n.eq_ignore_ascii_case(item)))
}

/// `autorun/envs/...`.
fn is_envs_entry(rel: &str) -> bool {
    let seg = raw_segments(rel);
    seg.first().is_some_and(|s| s.eq_ignore_ascii_case("autorun")) && seg.get(1).is_some_and(|s| s.eq_ignore_ascii_case("envs"))
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
///
/// Once the per-environment folders are in use (`autorun/envs` exists), the
/// flat originals the update left at the top of the Auto Run folder are not
/// read by anything and are left out: each environment's own copy is what
/// the backup carries.
pub fn collect_files(data_dir: &Path) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
    let mut kept = vec![];
    let mut skipped = vec![];
    let per_env = crate::autorun::roots::is_per_env_layout(&data_dir.join("autorun"));
    for root in ROOTS {
        walk(data_dir, &data_dir.join(root), per_env, &mut kept, &mut skipped);
    }
    kept.sort_by(|a, b| a.0.cmp(&b.0));
    skipped.sort();
    (kept, skipped)
}

fn walk(base: &Path, p: &Path, per_env: bool, kept: &mut Vec<(String, Vec<u8>)>, skipped: &mut Vec<String>) {
    let Ok(meta) = std::fs::metadata(p) else { return };
    let rel = match p.strip_prefix(base) {
        Ok(r) => r.to_string_lossy().replace('\\', "/"),
        Err(_) => return,
    };
    if excluded(&rel) || (per_env && is_flat_original(&rel)) {
        return;
    }
    if meta.is_dir() {
        let Ok(entries) = std::fs::read_dir(p) else { return };
        for e in entries.flatten() {
            walk(base, &e.path(), per_env, kept, skipped);
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

/// Is `seg` a name that is only what it says? Not empty or `.`, no `:` (an
/// NTFS stream or `::$INDEX_ALLOCATION` names another folder), no trailing
/// dot or space (Windows drops them, so the folder written is not the one
/// checked), no device name (`CON`, `NUL`, `COM1`... with any extension), no
/// 8.3 short-name form (`~1`). `..` is refused by the component check.
fn plain_name(seg: &str) -> bool {
    if seg.is_empty() || seg.contains(':') || seg.ends_with(['.', ' ']) {
        return false;
    }
    let stem = seg.split('.').next().unwrap_or("").to_ascii_lowercase();
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0")
    };
    if ["con", "prn", "aux", "nul"].contains(&stem.as_str()) || numbered("com") || numbered("lpt") {
        return false;
    }
    !seg.as_bytes().windows(2).any(|w| w[0] == b'~' && w[1].is_ascii_digit())
}

/// Is `rel` a path import may write? Relative, inside one of the roots,
/// and free of `..`/absolute/drive components (the zip-slip family).
pub fn safe_relative_path(rel: &str) -> bool {
    if rel.is_empty() || rel.len() > 500 {
        return false;
    }
    if rel.split(['/', '\\']).any(|seg| !plain_name(seg)) {
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
    // An environment's folder is `autorun/envs/<id>/<something>`: the id is a
    // real environment id (or the seed), so a name from a backup never
    // becomes a folder of its own.
    if is_envs_entry(rel) {
        let seg = raw_segments(rel);
        if seg.len() < 4 || !crate::autorun::roots::valid_folder_id(seg[2]) {
            return false;
        }
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

/// Is this backup from before the per-environment layout? No entry under
/// `autorun/envs/`, and at least one under a per-environment item at the top
/// of the Auto Run folder (`autorun/scripts/...`, `autorun/projects/...`).
/// Only entries an import would accept count.
pub fn is_pre_update(files: &[BackupFile]) -> bool {
    let mut flat = false;
    for f in files.iter().filter(|f| safe_relative_path(&f.path)) {
        if is_envs_entry(&f.path) {
            return false;
        }
        flat |= is_flat_original(&f.path) && raw_segments(&f.path).len() >= 3;
    }
    flat
}

/// Said when the backup's list of environments cannot be read.
pub const BAD_ENV_LIST: &str = "the list of environments in this backup cannot be read - nothing was imported";

/// An entry of a pre-update backup in an item the update copies into every
/// environment (scripts, areas, templates and so on), not an export record.
fn is_app_item(rel: &str) -> bool {
    use crate::autorun::roots::{self, CopyTo, PER_ENV_ITEMS};
    let seg = raw_segments(rel);
    seg.len() >= 3
        && seg[0].eq_ignore_ascii_case("autorun")
        && PER_ENV_ITEMS.iter().any(|(item, how)| *how == CopyTo::Every && seg[1].eq_ignore_ascii_case(item))
        && !roots::is_export_record(&seg[1..].iter().collect::<PathBuf>())
}

/// What an import does to the environments' folders. The one place that
/// decides it: the import and the preview both ask.
struct Plan {
    pre_update: bool,
    post_update: bool,
    /// The folders it replaces, as `(id, name)`; the name is empty for the seed.
    replaces: Vec<(String, String)>,
    /// The backup's own list of environments.
    list: Option<crate::environments::EnvFile>,
}

fn plan(autorun: &Path, files: &[BackupFile]) -> Result<Plan, String> {
    use crate::autorun::roots;
    let list_entry = files
        .iter()
        .find(|f| raw_segments(&f.path).iter().map(|s| s.to_ascii_lowercase()).eq(["autorun", "environments.json"]));
    let list = match list_entry {
        Some(f) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&f.b64)
                .map_err(|_| format!("corrupt backup: {} is not valid base64", f.path))?;
            Some(crate::environments::parse_file(&bytes).map_err(|_| BAD_ENV_LIST.to_string())?)
        }
        None => None,
    };
    let per_env = roots::is_per_env_layout(autorun);
    let pre_update = per_env && is_pre_update(files);
    let mut brought: Vec<String> = files
        .iter()
        .filter(|f| safe_relative_path(&f.path) && is_envs_entry(&f.path))
        .map(|f| raw_segments(&f.path)[2].to_string())
        .collect();
    brought.sort();
    brought.dedup();
    let post_update = per_env && !pre_update && !brought.is_empty();
    let machine = crate::environments::listed_names(autorun).unwrap_or_default();
    let name_of = |id: &str| -> String {
        if id == roots::SEED_ID {
            return String::new();
        }
        list.as_ref()
            .and_then(|l| l.environments.iter().find(|e| e.id == id).map(|e| e.name.clone()))
            .or_else(|| machine.iter().find(|(i, _)| i == id).map(|(_, n)| n.clone()))
            .unwrap_or_else(|| id.to_string())
    };
    let ids: Vec<String> = if pre_update && files.iter().filter(|f| safe_relative_path(&f.path)).any(|f| is_app_item(&f.path)) {
        match &list {
            Some(l) => l.environments.iter().map(|e| e.id.clone()).collect(),
            None if !machine.is_empty() => machine.iter().map(|(i, _)| i.clone()).collect(),
            None => vec![roots::SEED_ID.to_string()],
        }
    } else if post_update {
        brought
    } else {
        vec![]
    };
    let replaces = ids
        .into_iter()
        .map(|id| {
            let n = name_of(&id);
            (id, n)
        })
        .collect();
    Ok(Plan { pre_update, post_update, replaces, list })
}

/// What importing a backup would do, for the confirmation.
#[derive(serde::Serialize, specta::Type, Clone, Debug)]
pub struct BackupPreview {
    pub exported_at: String,
    pub app_version: String,
    /// The backup is from before each environment had its own Auto Run data.
    pub pre_update: bool,
    /// The backup brings a folder per environment.
    pub post_update: bool,
    /// The environments whose Auto Run data the import replaces, by name:
    /// exactly the ones the import will replace. Each replaced folder keeps
    /// its run history, screenshots and Playwright records, and the rest
    /// waits 30 days. Empty when nothing is replaced (a backup of history
    /// alone, or a machine without per-environment folders, where the next
    /// start does the update's copy).
    pub replaces: Vec<String>,
}

/// What importing `doc` onto `data_dir`'s machine would do. Writes nothing.
/// Refuses what the import would refuse.
pub fn preview(data_dir: &Path, doc: &BackupDoc) -> Result<BackupPreview, String> {
    let plan = plan(&data_dir.join("autorun"), &doc.files)?;
    Ok(BackupPreview {
        exported_at: doc.exported_at.clone(),
        app_version: doc.app_version.clone(),
        pre_update: plan.pre_update,
        post_update: plan.post_update,
        replaces: plan.replaces.into_iter().map(|(_, n)| n).filter(|n| !n.is_empty()).collect(),
    })
}

/// Where a restored entry is written, and whether that is a staging folder.
/// A backup from before the update (`flat`: the import's staging folder)
/// keeps its per-environment files out of the live folders until they are
/// built into one folder per environment; `envs` is a staging folder for
/// the environment folders a backup brings (built whole, then swapped in, or
/// renamed to `envs/` on a machine without one), so nothing live is written.
fn destination(data_dir: &Path, rel: &str, flat: Option<&Path>, envs: Option<&Path>) -> (PathBuf, bool) {
    let seg = raw_segments(rel);
    if let (Some(flat), true) = (flat, is_flat_original(rel)) {
        return (seg[1..].iter().fold(flat.to_path_buf(), |p, s| p.join(s)), true);
    }
    if let (Some(envs), true) = (envs, is_envs_entry(rel)) {
        return (seg[2..].iter().fold(envs.to_path_buf(), |p, s| p.join(s)), true);
    }
    (data_dir.join(rel), false)
}

/// Restore the disk half of a backup, holding the locks a switch holds (the
/// Playwright records, then the switch lock) so nothing writes an environment
/// meanwhile. Unsafe paths are refused one by one (the rest still land);
/// returns how many files were written. See [`restore_files_held`].
pub fn restore_files(data_dir: &Path, files: &[BackupFile]) -> Result<u32, String> {
    let _records = crate::pw_export::mapping::records_lock().lock().unwrap_or_else(|e| e.into_inner());
    let held = crate::autorun::roots::switch_lock().write().unwrap_or_else(|e| e.into_inner());
    restore_files_held(&held, data_dir, files)
}

/// [`restore_files`] for a caller that already holds the switch lock for
/// writing (`held` is the proof; the Playwright records lock is taken
/// before it).
///
/// Nothing is written to a live path until the import can finish: first
/// every entry is decoded, the list of environments is read, the folders to
/// replace are chosen and checked for use, and the new folders are built in
/// a staging folder; only then are the shared files written and the
/// environment folders swapped in, each whole (see `autorun::backup_import`).
/// On a machine without per-environment folders a backup's `envs/` is built
/// in a staging folder and renamed in, and a pre-update backup's files land
/// flat for the next start's copy.
pub fn restore_files_held(
    held: &std::sync::RwLockWriteGuard<'static, ()>,
    data_dir: &Path,
    files: &[BackupFile],
) -> Result<u32, String> {
    use crate::autorun::{backup_import, roots};
    let b64 = base64::engine::general_purpose::STANDARD;
    let autorun = data_dir.join("autorun");
    let now = crate::cache::now_ms();
    // An earlier import that did not finish would be replayed over this one.
    if backup_import::pending(&autorun) {
        return Err(backup_import::IMPORT_PENDING.to_string());
    }

    // ---- Phase 1: look, write nothing ---------------------------------
    let mut entries: Vec<(&BackupFile, Vec<u8>)> = Vec::new();
    for f in files {
        if !safe_relative_path(&f.path) {
            crate::applog::warn(format!("Backup import skipped an unsafe path: {}", f.path));
            continue;
        }
        let bytes = b64
            .decode(&f.b64)
            .map_err(|_| format!("corrupt backup: {} is not valid base64", f.path))?;
        entries.push((f, bytes));
    }
    let plan = plan(&autorun, files)?;
    let ids: Vec<String> = plan.replaces.iter().map(|(i, _)| i.clone()).collect();
    if backup_import::busy(&autorun, &ids) {
        return Err(roots::REMOVE_BUSY.to_string());
    }
    let per_env_machine = roots::is_per_env_layout(&autorun);
    let has_envs_entries = entries.iter().any(|(f, _)| is_envs_entry(&f.path));
    // The Playwright export environment before the list is replaced.
    let previous_export = crate::environments::export_id_unlocked(&autorun);

    // ---- Phase 2: stage, writing only inside staging folders ----------
    let import_staging = (!ids.is_empty()).then(|| backup_import::staging_dir(&autorun, now));
    let flat_stage = import_staging.as_deref().filter(|_| plan.pre_update).map(backup_import::flat_dir);
    let envs_stage = if plan.post_update {
        import_staging.as_deref().map(backup_import::built_dir)
    } else {
        (!per_env_machine && has_envs_entries).then(|| autorun.join(format!(".envs-staging-{now}")))
    };
    let clean = || {
        if let Some(st) = &import_staging {
            let _ = std::fs::remove_dir_all(st);
        }
        if let Some(st) = &envs_stage {
            let _ = std::fs::remove_dir_all(st);
        }
    };
    // A pre-update backup that replaces nothing (history alone) is not applied.
    let drop_flat = plan.pre_update && ids.is_empty();
    let mut restored = 0u32;
    let mut staged_applied = 0u32;
    let mut shared_entries: Vec<(&BackupFile, Vec<u8>, PathBuf)> = Vec::new();
    for (f, bytes) in entries {
        if drop_flat && is_flat_original(&f.path) {
            continue;
        }
        let (target, staged) = destination(data_dir, &f.path, flat_stage.as_deref(), envs_stage.as_deref());
        if !staged {
            shared_entries.push((f, bytes, target));
            continue;
        }
        let wrote = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&target, bytes))
            .map_err(|e| format!("could not restore {}: {e}", f.path));
        if let Err(e) = wrote {
            clean();
            return Err(e);
        }
        staged_applied += u32::from(!plan.pre_update || is_app_item(&f.path));
    }
    let replacing = match &import_staging {
        Some(st) => match backup_import::prepare(st, &ids, plan.pre_update) {
            Ok(replacing) => replacing,
            Err(e) => {
                clean();
                return Err(e);
            }
        },
        None => false,
    };

    // ---- Phase 3: write the shared files, then swap the folders in ----
    // The list the app will read: the Playwright export environment stays
    // where its records are when it is still listed, else the backup's own
    // choice, else the active environment (the records go with it).
    let mut list_text: Option<String> = None;
    let mut move_records: Option<(String, String)> = None;
    if let Some(list) = &plan.list {
        let listed = |id: &str| list.environments.iter().any(|e| e.id == id);
        let mut out = list.clone();
        let chosen = match &previous_export {
            Some(prev) if listed(prev) => prev.clone(),
            _ => list.playwright_export.clone().filter(|e| listed(e)).unwrap_or_else(|| list.active.clone()),
        };
        if let Some(prev) = previous_export.as_ref().filter(|p| !listed(p) && **p != chosen) {
            move_records = Some((prev.clone(), chosen.clone()));
        }
        out.playwright_export = Some(chosen);
        list_text = crate::environments::file_text(&out).ok();
    }
    let shared_phase = (|| -> Result<(), String> {
        // A backup from before environments: the machine-wide list, and no
        // environments file to say whose it is. Kept until the loop is done.
        let mut legacy_accounts: Option<Vec<u8>> = None;
        let mut has_environments = false;
        // The environments file last: nothing else depends on it, and it is
        // the one that makes the new folders the listed ones.
        shared_entries.sort_by_key(|(f, _, _)| f.path.replace('\\', "/").to_ascii_lowercase() == "autorun/environments.json");
        for (f, bytes, target) in &shared_entries {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let norm = f.path.replace('\\', "/").to_ascii_lowercase();
            if norm == "autorun/accounts.json" {
                legacy_accounts = Some(bytes.clone());
            }
            if norm == "autorun/environments.json" {
                has_environments = true;
                let text = list_text.as_ref().map_or_else(|| bytes.clone(), |t| t.clone().into_bytes());
                std::fs::write(target, text).map_err(|e| format!("could not restore {}: {e}", f.path))?;
                restored += 1;
                continue;
            }
            if norm == "databases.json" {
                // This machine's own list unreadable (or unsaveable) skips the
                // merge, as an unreadable list in the backup does: the rest of
                // the restore - and the settings handed back after it - must not
                // fail halfway over the one file it would never overwrite.
                match crate::db::catalog::merge_backup(target, bytes) {
                    Ok(_) => restored += 1,
                    Err(e) => crate::applog::warn(format!(
                        "Backup import: the list of databases was not merged, and this machine's list was left as it was: {e}"
                    )),
                }
                continue;
            }
            std::fs::write(target, bytes).map_err(|e| format!("could not restore {}: {e}", f.path))?;
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
                // failure is not the restore's failure.
                let _ = std::fs::remove_dir_all(autorun.join("sessions"));
            }
        }
        if let Some(list) = legacy_accounts {
            restore_legacy_accounts(&autorun, &list, has_environments)?;
        }
        Ok(())
    })();
    if let Err(e) = shared_phase {
        // The swap has not started: only copies of the backup's files are
        // staged, and the note goes first so no start replays this import.
        if let Some(st) = &import_staging {
            backup_import::abandon(st);
        }
        if let (Some(st), false) = (&envs_stage, plan.post_update) {
            let _ = std::fs::remove_dir_all(st);
        }
        return Err(e);
    }
    if let (Some(staging), true) = (&import_staging, replacing) {
        let names = |id: &str| {
            plan.replaces
                .iter()
                .find(|(i, _)| i == id)
                .map(|(_, n)| n.clone())
                .filter(|n| !n.is_empty())
                .or_else(|| crate::environments::listed_name(&autorun, id))
                .unwrap_or_default()
        };
        backup_import::commit(
            held,
            &autorun,
            staging,
            &ids,
            &names,
            now,
            &mut |a, b| std::fs::rename(a, b),
            &roots::MIGRATION_RENAME_WAITS_MS,
        )?;
        restored += staged_applied;
    }
    if let (Some(staging), false) = (&envs_stage, plan.post_update) {
        let put = if staging.is_dir() {
            crate::ai_tools::rename_retrying_waits(
                staging,
                &autorun.join(roots::ENVS_DIR),
                &mut |a, b| std::fs::rename(a, b),
                &mut std::thread::sleep,
                &roots::MIGRATION_RENAME_WAITS_MS,
            )
        } else {
            Ok(())
        };
        if let Err(e) = put {
            crate::applog::warn(format!("Backup import: the environments' folders could not be put in place: {e}"));
            let _ = std::fs::remove_dir_all(staging);
            return Err(backup_import::IMPORT_FAILED.to_string());
        }
        restored += staged_applied;
    }
    // The export environment fell to another one: its records go with it.
    if let Some((from, to)) = move_records {
        if roots::valid_folder_id(&from) && roots::valid_folder_id(&to) {
            let moved = crate::environments::copy_export_records(&roots::env_root(&autorun, &from), &roots::env_root(&autorun, &to));
            if let Err(e) = moved {
                crate::applog::warn(format!("Backup import: the Playwright export records could not follow the export environment: {e}"));
            }
        }
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
