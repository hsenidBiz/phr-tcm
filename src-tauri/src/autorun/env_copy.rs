//! Copying one environment's Auto Run data into another, the person's own
//! action: from another environment, or from a removed one still kept.
//!
//! Only what describes the app is copied (`PER_ENV_ITEMS` marked
//! `CopyTo::Every`): scripts, areas, components, quirks, API templates. Run
//! history and the Playwright export record are the target's own and stay
//! where they are. What the copy replaces is kept in the removed area for 30
//! days, which also lists it as a source.
//!
//! The copy is built beside the target (`envs/.copying-<target>-<ms>`) and
//! renamed onto it, so a failure never leaves a half-copied environment.

use super::roots::{self, CopyTo, PER_ENV_ITEMS};
use crate::environments::{self, EnvFile};
use std::path::{Path, PathBuf};

/// Said when the target already holds Auto Run data and the person has not
/// yet agreed to replace it.
pub const COPY_NEEDS_CONFIRM: &str = "this environment already has Auto Run data - confirm to replace it";
/// Said when the source is the target.
pub const COPY_ONTO_ITSELF: &str = "an environment cannot be copied onto itself";
/// Said when the source is not one the copy may read from.
pub const COPY_UNKNOWN_SOURCE: &str = "that source is not there any more";
/// Said when the target is not a listed environment.
pub const COPY_UNKNOWN_TARGET: &str = "that environment is not there any more";
/// Said when the copy stopped part-way; the log says why.
pub const COPY_FAILED: &str = "the copy did not finish - see Settings > Logs";

/// The prefix of a source key that names a removed folder.
const REMOVED_PREFIX: &str = "removed:";
/// The folder the copy is built in, inside `envs/`. Not a valid environment
/// folder name, so nothing lists or opens it as an environment.
const STAGING_PREFIX: &str = ".copying-";

/// Somewhere to copy from.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CopySource {
    /// An environment id, or `removed:<folder key>`.
    pub key: String,
    pub name: String,
    pub removed: bool,
    /// Whole days a removed folder is still kept; `None` for an environment.
    pub days_left: Option<u32>,
}

/// What the copy dialog needs to know about a target.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CopyChoices {
    /// The target has no Auto Run data a copy would replace.
    pub target_empty: bool,
    /// Every other environment, then the removed folders still kept.
    pub sources: Vec<CopySource>,
}

/// What a finished copy did.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CopyDone {
    /// Top-level items brought in.
    pub items: u32,
    /// What the copy replaced was kept in the removed area.
    pub kept_aside: bool,
}

/// Whether any file under `dir` (a file that is not a folder: links count)
/// passes `counts`; `rel` is the path of `dir` from the environment root.
fn any_file(dir: &Path, rel: &Path, counts: &dyn Fn(&Path) -> bool) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    for entry in entries.flatten() {
        let rel = rel.join(entry.file_name());
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            if any_file(&entry.path(), &rel, counts) {
                return true;
            }
        } else if counts(&rel) {
            return true;
        }
    }
    false
}

/// No file under any item a copy would replace (export records left out).
pub fn is_empty_env(env_root: &Path) -> bool {
    !PER_ENV_ITEMS
        .iter()
        .filter(|(_, how)| *how == CopyTo::Every)
        .any(|(name, _)| any_file(&env_root.join(name), Path::new(name), &|rel| !roots::is_export_record(rel)))
}

/// The days a retired folder made at `removed_at_ms` is still kept.
fn days_left(removed_at_ms: u64, now_ms: u64) -> u32 {
    let kept = roots::REMOVED_KEPT_DAYS * crate::autorun::store::DAY_MS;
    let left = kept.saturating_sub(now_ms.saturating_sub(removed_at_ms));
    left.div_ceil(crate::autorun::store::DAY_MS) as u32
}

/// Where `target_id` can copy from: every other environment, then the
/// removed folders still kept (which include what an earlier copy replaced,
/// the target's own included).
pub fn copy_choices(shared: &Path, envs: &EnvFile, target_id: &str, now_ms: u64) -> Result<CopyChoices, String> {
    if !envs.environments.iter().any(|e| e.id == target_id) || !environments::valid_id(target_id) {
        return Err(COPY_UNKNOWN_TARGET.to_string());
    }
    let mut sources: Vec<CopySource> = envs
        .environments
        .iter()
        .filter(|e| e.id != target_id)
        .map(|e| CopySource { key: e.id.clone(), name: e.name.clone(), removed: false, days_left: None })
        .collect();
    sources.extend(roots::removed_folders(shared, now_ms).into_iter().map(|f| CopySource {
        key: format!("{REMOVED_PREFIX}{}", f.key),
        name: f.name,
        removed: true,
        days_left: Some(days_left(f.removed_at_ms, now_ms)),
    }));
    Ok(CopyChoices { target_empty: is_empty_env(&roots::env_root(shared, target_id)), sources })
}

/// The folder a source key names, only for a listed environment or a removed
/// folder still kept. A key never becomes a path otherwise.
pub fn source_root(shared: &Path, envs: &EnvFile, key: &str, now_ms: u64) -> Result<PathBuf, String> {
    if let Some(folder) = key.strip_prefix(REMOVED_PREFIX) {
        return roots::removed_root(shared, folder, now_ms).ok_or_else(|| COPY_UNKNOWN_SOURCE.to_string());
    }
    if environments::valid_id(key) && envs.environments.iter().any(|e| e.id == key) {
        return Ok(roots::env_root(shared, key));
    }
    Err(COPY_UNKNOWN_SOURCE.to_string())
}

/// Logs a failure that must not reach the person's screen: only text that
/// names no path.
fn log_failure(what: &str, error: &str) {
    if roots::path_free(error) {
        crate::applog::warn(format!("Environments: {what}: {error}"));
    } else {
        crate::applog::warn(format!("Environments: {what}"));
    }
}

/// One item moved out of the target into the staging folder.
struct Moved {
    from: PathBuf,
    to: PathBuf,
}

/// Puts every moved item back where it was.
fn put_back(moved: &[Moved]) {
    for m in moved.iter().rev() {
        if let Err(e) = std::fs::rename(&m.to, &m.from) {
            log_failure("a kept item could not be put back after a failed copy", &e.to_string());
        }
    }
}

/// The target's history items and export records, as `(from, to)` pairs
/// into `staging`.
fn kept_in_place(target: &Path, staging: &Path) -> Vec<Moved> {
    let mut out = Vec::new();
    for (name, how) in PER_ENV_ITEMS {
        if how == CopyTo::ActiveOnly && std::fs::symlink_metadata(target.join(name)).is_ok() {
            out.push(Moved { from: target.join(name), to: staging.join(name) });
        }
    }
    if let Ok(entries) = std::fs::read_dir(target.join("projects")) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if roots::is_export_record(&Path::new("projects").join(&name)) {
                out.push(Moved { from: entry.path(), to: staging.join("projects").join(&name) });
            }
        }
    }
    out
}

/// Removes the folders of a tree that holds no file at all. Anything else
/// stays and is reported as not empty.
fn remove_empty_tree(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    let mut empty = true;
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            empty &= remove_empty_tree(&entry.path());
        } else {
            empty = false;
        }
    }
    empty && std::fs::remove_dir(dir).is_ok()
}

fn rename_waiting(from: &Path, to: &Path) -> Result<(), String> {
    crate::ai_tools::rename_retrying_waits(
        from,
        to,
        &mut |a, b| std::fs::rename(a, b),
        &mut |d| std::thread::sleep(d),
        &roots::MIGRATION_RENAME_WAITS_MS,
    )
}

/// Copies `source`'s scripts, areas, components, quirks and API templates
/// into environment `target_id`, keeping the target's run history and
/// export record where they are. A target that already holds data needs
/// `replace`; what it held is kept in the removed area as `target_name`.
/// Waits for the writes in flight (the switch lock) and refuses while
/// something is writing into the target.
pub fn copy_environment(
    shared: &Path,
    source: &Path,
    target_id: &str,
    target_name: &str,
    replace: bool,
    now_ms: u64,
) -> Result<CopyDone, String> {
    if !environments::valid_id(target_id) {
        return Err(COPY_UNKNOWN_TARGET.to_string());
    }
    let target = roots::env_root(shared, target_id);
    if source == target {
        return Err(COPY_ONTO_ITSELF.to_string());
    }
    let switching = roots::switch_lock().write().unwrap_or_else(|e| e.into_inner());
    if roots::is_held_for_writes(&target) {
        return Err(roots::REMOVE_BUSY.to_string());
    }
    let target_empty = is_empty_env(&target);
    if !target_empty && !replace {
        return Err(COPY_NEEDS_CONFIRM.to_string());
    }
    let staging = shared.join(roots::ENVS_DIR).join(format!("{STAGING_PREFIX}{target_id}-{now_ms}"));
    let done = (|| {
        if std::fs::symlink_metadata(&staging).is_ok() {
            return Err("a copy is already under way".to_string());
        }
        std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        // Step 1: the source's describing items, never the export record.
        let mut items = 0u32;
        for (name, how) in PER_ENV_ITEMS {
            if how != CopyTo::Every || !roots::exists(&source.join(name))? {
                continue;
            }
            let skip = |rel: &Path| roots::is_export_record(rel);
            roots::copy_tree(&source.join(name), &staging.join(name), Path::new(name), false, &skip)?;
            items += 1;
        }
        Ok(items)
    })();
    let items = match done {
        Ok(items) => items,
        Err(e) => {
            log_failure("a copy could not be built", &e);
            let _ = std::fs::remove_dir_all(&staging);
            return Err(COPY_FAILED.to_string());
        }
    };

    // Step 2: the target's history and export record join the copy.
    let moved = kept_in_place(&target, &staging);
    let mut done_moves: Vec<Moved> = Vec::new();
    for m in moved {
        let step = (|| {
            if let Some(parent) = m.to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            rename_waiting(&m.from, &m.to)
        })();
        match step {
            Ok(()) => done_moves.push(m),
            Err(e) => {
                log_failure("a copy could not keep the history of the environment it replaces", &e);
                put_back(&done_moves);
                let _ = std::fs::remove_dir_all(&staging);
                return Err(COPY_FAILED.to_string());
            }
        }
    }

    // Step 3: what is left of the target waits in the removed area. A target
    // with nothing but empty folders left has nothing worth keeping.
    let retired = if std::fs::symlink_metadata(&target).is_err() || remove_empty_tree(&target) {
        Ok(None)
    } else {
        roots::retire_held(&switching, shared, target_id, target_name, "replaced", now_ms, &roots::MIGRATION_RENAME_WAITS_MS)
    };
    let retired = match retired {
        Ok(key) => key,
        Err(e) => {
            log_failure("a copy could not set the replaced data aside", &e);
            put_back(&done_moves);
            let _ = std::fs::remove_dir_all(&staging);
            return Err(COPY_FAILED.to_string());
        }
    };

    // Step 4: the finished copy takes the target's place.
    if let Err(e) = rename_waiting(&staging, &target) {
        log_failure("a copy could not take its place", &e);
        // Best effort to leave things as they were: the old folder back, the
        // history back in it, and the copy (only a copy) removed.
        let back = match &retired {
            Some(key) => rename_waiting(&shared.join(roots::REMOVED_DIR).join(key), &target).map(|_| {
                let _ = std::fs::remove_file(target.join("removed.json"));
            }),
            None => std::fs::create_dir_all(&target).map_err(|e| e.to_string()),
        };
        if back.is_ok() {
            put_back(&done_moves);
            let _ = std::fs::remove_dir_all(&staging);
        }
        return Err(COPY_FAILED.to_string());
    }
    crate::applog::info(format!("Environments: copied Auto Run data into one environment ({items} item(s))"));
    Ok(CopyDone { items, kept_aside: retired.is_some() })
}
