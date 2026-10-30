//! Importing a backup onto a machine that already has the per-environment
//! layout. A backup from before it has its flat Auto Run files built into one
//! folder per environment (the app-describing items only); a backup from
//! after it brings a folder per environment. Either way each environment
//! folder is replaced whole: the live folder keeps its run history, its
//! screenshots and its Playwright export records (what the backup lacks of
//! them is moved into the new folder, as a copy between environments does),
//! the rest waits in the removed area, never deleted, and the new folder is
//! renamed in.
//!
//! Atomic per environment: the new folders are built whole in a staging
//! folder (`<shared>/.import-<ms>/built/<id>`), a note is written, and only
//! then is each live folder set aside and its replacement renamed in. A
//! start that finds a staging folder with a note finishes the swap
//! ([`recover`]); one without a note holds only copies of the backup's files
//! and is removed.

use super::env_copy::{kept_in_place, log_failure, remove_empty_tree, rename_with, Rename};
use super::roots::{self, CopyTo, ENVS_DIR, PER_ENV_ITEMS, SEED_ID};
use std::path::{Path, PathBuf};

/// A staging folder's name starts with this.
pub const IMPORT_PREFIX: &str = ".import-";
const MARKER: &str = ".importing.json";
const FLAT: &str = "flat";
const BUILT: &str = "built";
const STEP_BUILT: &str = "built";
/// The reason a retired folder carries.
pub const REPLACED_BY_IMPORT: &str = "replaced by an import";

/// Said when the import could not be put in place; the log says why.
pub const IMPORT_FAILED: &str = "the backup's Auto Run data could not be put in place - see Settings > Logs";
/// Said when the import stopped part-way and will be finished at the next start.
pub const IMPORT_UNFINISHED: &str =
    "the import did not finish and will be completed when the app next starts - see Settings > Logs";

#[derive(serde::Serialize, serde::Deserialize)]
struct Marker {
    ids: Vec<String>,
    step: String,
}

/// Where a backup's whole environment folders are staged.
pub fn built_dir(staging: &Path) -> PathBuf {
    staging.join(BUILT)
}

/// `<shared>/.import-<ms>`.
pub fn staging_dir(shared: &Path, now_ms: u64) -> PathBuf {
    shared.join(format!("{IMPORT_PREFIX}{now_ms}"))
}

/// Where the backup's flat per-environment files are written.
pub fn flat_dir(staging: &Path) -> PathBuf {
    staging.join(FLAT)
}

fn name_of(names: &dyn Fn(&str) -> String, id: &str) -> String {
    let n = names(id);
    if n.trim().is_empty() {
        "Environment".to_string()
    } else {
        n
    }
}

/// Sets each environment's live folder aside and renames its built
/// replacement in. Skips an id whose built folder is already gone (done
/// before), so it can be run again after a stop.
#[allow(clippy::too_many_arguments)]
fn swap_in(
    held: &std::sync::RwLockWriteGuard<'static, ()>,
    shared: &Path,
    staging: &Path,
    ids: &[String],
    names: &dyn Fn(&str) -> String,
    now_ms: u64,
    rename: Rename,
    waits_ms: &[u64],
) -> Result<(), String> {
    let envs_dir = shared.join(ENVS_DIR);
    for id in ids {
        if !roots::valid_folder_id(id) {
            return Err(format!("\"{id}\" is not an environment id"));
        }
        let from = staging.join(BUILT).join(id);
        if !roots::exists(&from)? {
            continue;
        }
        let to = roots::env_root(shared, id);
        // The live folder's history and export records join the new folder
        // unless the backup brought its own. Moves, never deletes; run again
        // after a stop it moves only what is still there.
        if roots::exists(&to)? {
            for m in kept_in_place(&to, &from) {
                if std::fs::symlink_metadata(&m.to).is_ok() {
                    continue;
                }
                if let Some(parent) = m.to.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                rename_with(&m.from, &m.to, &mut *rename, waits_ms)?;
            }
        }
        if roots::exists(&to)? && !remove_empty_tree(&to) {
            if id == SEED_ID {
                // The seed has no removed-area key, so it waits beside itself.
                let aside = envs_dir.join(format!("{}{now_ms}", roots::SEED_ASIDE_PREFIX));
                rename_with(&to, &aside, &mut *rename, waits_ms)?;
            } else {
                roots::retire_held(held, shared, id, &name_of(names, id), REPLACED_BY_IMPORT, now_ms, waits_ms)?;
            }
        }
        std::fs::create_dir_all(&envs_dir).map_err(|e| e.to_string())?;
        rename_with(&from, &to, &mut *rename, waits_ms)?;
    }
    Ok(())
}

/// Builds the per-environment folders from the backup's flat files (under
/// `flat_dir(staging)`): the app-describing items, every environment alike,
/// never the history or the export records.
fn build_from_flat(staging: &Path, ids: &[String]) -> Result<usize, String> {
    let flat = flat_dir(staging);
    let built = staging.join(BUILT);
    let mut items = 0;
    for id in ids {
        std::fs::create_dir_all(built.join(id)).map_err(|e| e.to_string())?;
    }
    for (name, how) in PER_ENV_ITEMS {
        if how != CopyTo::Every || !roots::exists(&flat.join(name))? {
            continue;
        }
        items += 1;
        for id in ids {
            roots::copy_tree(&flat.join(name), &built.join(id).join(name), Path::new(name), false, &|rel| {
                roots::is_export_record(rel)
            })?;
        }
    }
    Ok(items)
}

/// Puts the staged folders in place of the live ones for `ids`. With
/// `from_flat` the folders are first built from the backup's flat files;
/// without it they were staged whole (`built/<id>`) as the backup brought
/// them. `held` is the proof that the switch lock is held for writing.
/// Nothing live changes until every folder is ready and the note is written.
/// Answers how many environments were put in place.
#[allow(clippy::too_many_arguments)]
pub fn replace_environments(
    held: &std::sync::RwLockWriteGuard<'static, ()>,
    shared: &Path,
    staging: &Path,
    ids: &[String],
    from_flat: bool,
    names: &dyn Fn(&str) -> String,
    now_ms: u64,
    rename: Rename,
    waits_ms: &[u64],
) -> Result<usize, String> {
    if ids.iter().any(|i| roots::valid_folder_id(i) && roots::is_held_for_writes(&roots::env_root(shared, i))) {
        let _ = std::fs::remove_dir_all(staging);
        return Err(roots::REMOVE_BUSY.to_string());
    }
    let made = (|| {
        if from_flat && build_from_flat(staging, ids)? == 0 {
            // Nothing in the backup describes an app: nothing to replace.
            return Ok(false);
        }
        let note = Marker { ids: ids.to_vec(), step: STEP_BUILT.to_string() };
        let text = serde_json::to_string(&note).map_err(|e| e.to_string())?;
        crate::ai_tools::atomic_write(&staging.join(MARKER), &text).map(|_| true)
    })();
    match made {
        Ok(true) => {}
        Ok(false) => {
            let _ = std::fs::remove_dir_all(staging);
            return Ok(0);
        }
        Err(e) => {
            log_failure("an import could not build the environments' data", &e);
            // Nothing live was touched: only copies of the backup's files are here.
            let _ = std::fs::remove_dir_all(staging);
            return Err(IMPORT_FAILED.to_string());
        }
    }
    match swap_in(held, shared, staging, ids, names, now_ms, rename, waits_ms) {
        Ok(()) => {
            // Only copies of the backup's files are left in there.
            let _ = std::fs::remove_dir_all(staging);
            Ok(ids.len())
        }
        Err(e) => {
            log_failure("an import stopped while putting the environments in place", &e);
            Err(IMPORT_UNFINISHED.to_string())
        }
    }
}

/// Settles every import a crash or a failed swap left (`.import-*` in the
/// shared root), before any environment folder is chosen. A staging folder
/// with a note is finished (the import was committed when the note was
/// written); one without holds only copies of the backup's files and goes.
/// A folder that is replaced goes to the removed area, never away. Returns
/// how many it finished.
pub fn recover(shared: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(shared) else { return 0 };
    let mut found: Vec<(u64, PathBuf)> = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name();
        let stamp = name.to_str().and_then(|n| n.strip_prefix(IMPORT_PREFIX)).and_then(|m| m.parse::<u64>().ok());
        if let (Some(ms), true) = (stamp, e.file_type().is_ok_and(|t| t.is_dir())) {
            found.push((ms, e.path()));
        }
    }
    // Oldest first: a later import is the person's latest word.
    found.sort();
    let mut finished = 0;
    for (ms, staging) in found {
        let note: Option<Marker> = std::fs::read_to_string(staging.join(MARKER))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .filter(|m: &Marker| m.step == STEP_BUILT);
        let Some(note) = note else {
            crate::applog::warn("Backup import: an import that never started putting data in place was cleared");
            let _ = std::fs::remove_dir_all(&staging);
            continue;
        };
        let held = roots::switch_lock().write().unwrap_or_else(|e| e.into_inner());
        let names = |id: &str| crate::environments::listed_name(shared, id).unwrap_or_default();
        let done = swap_in(
            &held,
            shared,
            &staging,
            &note.ids,
            &names,
            ms,
            &mut |a, b| std::fs::rename(a, b),
            &roots::MIGRATION_RENAME_WAITS_MS,
        );
        match done {
            Ok(()) => {
                let _ = std::fs::remove_dir_all(&staging);
                crate::applog::info("Backup import: an import that was cut short was finished");
                finished += 1;
            }
            Err(e) => log_failure("an import that was cut short could not be finished", &e),
        }
    }
    finished
}
