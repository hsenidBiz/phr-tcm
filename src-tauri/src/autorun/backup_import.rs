//! Importing a backup made before the per-environment layout onto a machine
//! that already has it. The backup's flat Auto Run files fan out into every
//! environment, as the update does; each environment folder they replace is
//! kept in the removed area, never deleted.
//!
//! Atomic per environment: the new folders are built whole in a staging
//! folder (`<shared>/.import-<ms>/built/<id>`), a note is written, and only
//! then is each live folder set aside and its replacement renamed in. A
//! start that finds a staging folder with a note finishes the swap
//! ([`recover`]); one without a note holds only copies of the backup's files
//! and is removed.

use super::env_copy::{log_failure, remove_empty_tree, rename_with, Rename};
use super::roots::{self, ENVS_DIR, SEED_ID};
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
        if roots::exists(&to)? && !remove_empty_tree(&to) {
            if id == SEED_ID {
                // The seed has no removed-area key, so it waits beside itself.
                let aside = envs_dir.join(format!(".seed-replaced-{now_ms}"));
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

/// Builds the backup's per-environment files (already written under
/// `flat_dir(staging)`) into one folder per environment in `ids`, then swaps
/// them in. `active` receives the history, as the update gives it. `held` is
/// the proof that the switch lock is held for writing. Nothing live changes
/// until every folder is built and the note is written. Answers how many
/// environments were put in place.
#[allow(clippy::too_many_arguments)]
pub fn replace_environments(
    held: &std::sync::RwLockWriteGuard<'static, ()>,
    shared: &Path,
    staging: &Path,
    ids: &[String],
    active: &str,
    names: &dyn Fn(&str) -> String,
    now_ms: u64,
    rename: Rename,
    waits_ms: &[u64],
) -> Result<usize, String> {
    if ids.iter().any(|i| roots::valid_folder_id(i) && roots::is_held_for_writes(&roots::env_root(shared, i))) {
        let _ = std::fs::remove_dir_all(staging);
        return Err(roots::REMOVE_BUSY.to_string());
    }
    let built = staging.join(BUILT);
    let made = std::fs::create_dir_all(&built).map_err(|e| e.to_string()).and_then(|_| {
        roots::fan_out(&flat_dir(staging), &built, ids, Some(active))?;
        let note = Marker { ids: ids.to_vec(), step: STEP_BUILT.to_string() };
        let text = serde_json::to_string(&note).map_err(|e| e.to_string())?;
        crate::ai_tools::atomic_write(&staging.join(MARKER), &text)
    });
    if let Err(e) = made {
        log_failure("an import could not build the environments' data", &e);
        // Nothing live was touched: only copies of the backup's files are here.
        let _ = std::fs::remove_dir_all(staging);
        return Err(IMPORT_FAILED.to_string());
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
