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
/// Said when an earlier import has not finished: a new one would be replayed
/// over by it at the next start.
pub const IMPORT_PENDING: &str =
    "an earlier import has not finished - restart the app to finish it first, then import again";
/// Said when the import stopped part-way and will be finished at the next start.
pub const IMPORT_UNFINISHED: &str =
    "the import did not finish - restart the app now and it will be completed; until then do not save changes in Auto Run (see Settings > Logs)";

#[derive(serde::Serialize, serde::Deserialize)]
struct Marker {
    ids: Vec<String>,
    step: String,
    /// The text of the `environments.json` the import brings: written by a
    /// start that finishes the import before it swaps, so a crash between the
    /// note and the list never replays the folders under the old list.
    #[serde(default)]
    list: Option<String>,
}

/// Whether an earlier import is unfinished: a staging folder with a note
/// that can be read. One whose note cannot be read is left alone (never
/// deleted), said once in the log, and does not hold up a new import: a
/// restart would not change it.
pub fn pending(shared: &Path) -> bool {
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Ok(entries) = std::fs::read_dir(shared) else { return false };
    entries.flatten().any(|e| {
        let staging = e.file_name().to_str().and_then(|n| n.strip_prefix(IMPORT_PREFIX)).is_some_and(|m| m.parse::<u64>().is_ok());
        if !staging {
            return false;
        }
        match std::fs::read_to_string(e.path().join(MARKER)) {
            Ok(_) => true,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => {
                if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    crate::applog::warn("Backup import: an unfinished import has a note that could not be read and was left as it is");
                }
                false
            }
        }
    })
}

/// Whether one of `ids` is being written to by a copy or an export right now.
pub fn busy(shared: &Path, ids: &[String]) -> bool {
    ids.iter().any(|i| roots::valid_folder_id(i) && roots::is_held_for_writes(&roots::env_root(shared, i)))
}

/// `base`, or the next stamp up that names nothing yet in `taken`.
fn fresh_stamp(base: u64, taken: &dyn Fn(u64) -> bool) -> u64 {
    (base..base + 1000).find(|s| !taken(*s)).unwrap_or(base)
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
    // Every id is judged before anything moves.
    if let Some(bad) = ids.iter().find(|i| !roots::valid_folder_id(i)) {
        return Err(format!("\"{bad}\" is not an environment id"));
    }
    for id in ids {
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
                let stamp = fresh_stamp(now_ms, &|t| envs_dir.join(format!("{}{t}", roots::SEED_ASIDE_PREFIX)).exists());
                let aside = envs_dir.join(format!("{}{stamp}", roots::SEED_ASIDE_PREFIX));
                rename_with(&to, &aside, &mut *rename, waits_ms)?;
            } else {
                let area = shared.join(roots::REMOVED_DIR);
                let stamp = fresh_stamp(now_ms, &|t| area.join(format!("{id}-{t}")).exists());
                roots::retire_held(held, shared, id, &name_of(names, id), REPLACED_BY_IMPORT, stamp, waits_ms)?;
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

/// Gets the staged folders ready for `ids`, writing only inside `staging`:
/// with `from_flat` the folders are first built from the backup's flat files,
/// without it they were staged whole (`built/<id>`) as the backup brought
/// them. Then the note is written, which commits the import. `Ok(false)`
/// when nothing in the backup describes an app (the staging folder is
/// cleared, nothing is replaced). A failure clears the staging folder: it
/// holds only copies of the backup's files until the swap starts.
///
/// `records` is the Playwright export records the import carries along when
/// the export environment moves: `(the old export environment's folder, the
/// new one's id)`. They are copied into the new one's BUILT folder before
/// the note is written, so the swap (or a start that finishes it) brings
/// them in and a failed swap can never strand them in a folder that
/// `retire_unlisted` later moves aside. Nothing happens when the new
/// export environment is not one of `ids` (the caller copies them then).
pub fn prepare(
    staging: &Path,
    ids: &[String],
    from_flat: bool,
    list: Option<&str>,
    records: Option<(&Path, &str)>,
) -> Result<bool, String> {
    let made = (|| {
        if let Some(bad) = ids.iter().find(|i| !roots::valid_folder_id(i)) {
            return Err(format!("\"{bad}\" is not an environment id"));
        }
        if from_flat && build_from_flat(staging, ids)? == 0 {
            return Ok(false);
        }
        if let Some((from, to)) = records.filter(|(_, to)| ids.iter().any(|i| i == to)) {
            let dir = staging.join(BUILT).join(to);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            crate::environments::copy_export_records(from, &dir)?;
        }
        let note = Marker { ids: ids.to_vec(), step: STEP_BUILT.to_string(), list: list.map(str::to_string) };
        let text = serde_json::to_string(&note).map_err(|e| e.to_string())?;
        crate::ai_tools::atomic_write(&staging.join(MARKER), &text).map(|_| true)
    })();
    match made {
        Ok(true) => Ok(true),
        Ok(false) => {
            let _ = std::fs::remove_dir_all(staging);
            Ok(false)
        }
        Err(e) => {
            log_failure("an import could not build the environments' data", &e);
            let _ = std::fs::remove_dir_all(staging);
            Err(IMPORT_FAILED.to_string())
        }
    }
}

/// Drops a prepared import before its swap has started: its note goes first,
/// so a start never finds a note for an import that was given up.
pub fn abandon(staging: &Path) {
    let _ = std::fs::remove_file(staging.join(MARKER));
    let _ = std::fs::remove_dir_all(staging);
}

/// Puts the prepared folders in place of the live ones (`held` is the proof
/// that the switch lock is held for writing). A stop leaves the note and the
/// folders for the next start ([`recover`]).
#[allow(clippy::too_many_arguments)]
pub fn commit(
    held: &std::sync::RwLockWriteGuard<'static, ()>,
    shared: &Path,
    staging: &Path,
    ids: &[String],
    names: &dyn Fn(&str) -> String,
    now_ms: u64,
    rename: Rename,
    waits_ms: &[u64],
) -> Result<usize, String> {
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

/// [`prepare`] and [`commit`] for `ids`, after the busy check. Answers how
/// many environments were put in place.
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
    if busy(shared, ids) {
        let _ = std::fs::remove_dir_all(staging);
        return Err(roots::REMOVE_BUSY.to_string());
    }
    if !prepare(staging, ids, from_flat, None, None)? {
        return Ok(0);
    }
    commit(held, shared, staging, ids, names, now_ms, rename, waits_ms)
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
        // Only a note that is NOT THERE proves the swap never started (the
        // note is written before anything live moves). One that cannot be
        // read, or understood, may belong to a swap that carried a live
        // folder's history into this staging folder: it is left as it is.
        let note: Marker = match std::fs::read_to_string(staging.join(MARKER)) {
            Ok(text) => match serde_json::from_str::<Marker>(&text) {
                Ok(m) if m.step == STEP_BUILT => m,
                _ => {
                    crate::applog::warn("Backup import: an unfinished import has a note that could not be understood and was left as it is");
                    continue;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                crate::applog::warn("Backup import: an import that never started putting data in place was cleared");
                let _ = std::fs::remove_dir_all(&staging);
                continue;
            }
            Err(_) => {
                crate::applog::warn("Backup import: an unfinished import has a note that could not be read and was left as it is");
                continue;
            }
        };
        let held = roots::switch_lock().write().unwrap_or_else(|e| e.into_inner());
        // The list the import brings goes in first, so the folders are never
        // swapped in under the list that came before.
        if let Some(list) = &note.list {
            if let Err(e) = crate::ai_tools::atomic_write(&crate::environments::file_path(shared), list) {
                log_failure("an import that was cut short could not write its list of environments", &e);
                continue;
            }
        }
        let names = |id: &str| crate::environments::listed_name(shared, id).unwrap_or_default();
        let done = swap_in(
            &held,
            shared,
            &staging,
            &note.ids,
            &names,
            // Fresh: the stamp the import began with may name a folder already.
            crate::cache::now_ms().max(ms),
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
