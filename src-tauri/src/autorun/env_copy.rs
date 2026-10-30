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
/// Said when the source folder is not there.
pub const COPY_SOURCE_MISSING: &str = "that source has no Auto Run folder to copy from";
/// Said when the source holds nothing a copy would bring: copying it would
/// only empty the target.
pub const COPY_NOTHING_TO_COPY: &str = "that source has no scripts, areas or templates to copy";
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

type Rename<'a> = &'a mut dyn FnMut(&Path, &Path) -> std::io::Result<()>;

fn rename_with(from: &Path, to: &Path, rename: Rename, waits_ms: &[u64]) -> Result<(), String> {
    crate::ai_tools::rename_retrying_waits(from, to, rename, &mut |d| std::thread::sleep(d), waits_ms)
}

fn rename_waiting(from: &Path, to: &Path) -> Result<(), String> {
    rename_with(from, to, &mut |a, b| std::fs::rename(a, b), &roots::MIGRATION_RENAME_WAITS_MS)
}

/// Puts every moved item back where it was (parent made, a held rename
/// retried) and says whether EVERY one went back. Items that did not stay
/// where they are.
fn put_back(moved: &[Moved], rename: Rename, waits_ms: &[u64]) -> bool {
    let mut all = true;
    for m in moved.iter().rev() {
        let put = (|| {
            if let Some(parent) = m.from.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            rename_with(&m.to, &m.from, &mut *rename, waits_ms)
        })();
        if let Err(e) = put {
            all = false;
            log_failure("a kept item could not be put back after a failed copy", &e);
        }
    }
    all
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

/// Whether anything that is not a folder lies anywhere under `dir`.
fn tree_has_file(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return true };
    entries.flatten().any(|e| !e.file_type().is_ok_and(|t| t.is_dir()) || tree_has_file(&e.path()))
}

/// Removes `dir` when no file lies anywhere under it, and only then:
/// a tree that holds a file is left exactly as it is. Says whether it went.
fn remove_empty_tree(dir: &Path) -> bool {
    fn sweep(dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else { return false };
        let mut ok = true;
        for e in entries.flatten() {
            ok &= sweep(&e.path());
        }
        ok && std::fs::remove_dir(dir).is_ok()
    }
    !tree_has_file(dir) && sweep(dir)
}

// ---- the note a copy leaves in its staging folder -------------------------

const MARKER: &str = ".copying.json";
/// The history and export records are in staging and the copy is complete:
/// a start may finish it.
const STEP_KEPT: &str = "kept";
/// The copy was given up with something still in staging: a start only puts
/// the target's history back.
const STEP_ABANDONED: &str = "abandoned";

#[derive(serde::Serialize, serde::Deserialize)]
struct Marker {
    target: String,
    /// The source folder's name (an environment id or a removed folder key).
    source: String,
    step: String,
}

fn write_marker(staging: &Path, target_id: &str, source: &Path, step: &str) -> Result<(), String> {
    let marker = Marker {
        target: target_id.to_string(),
        source: source.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string(),
        step: step.to_string(),
    };
    let written = serde_json::to_string(&marker)
        .map_err(|e| e.to_string())
        .and_then(|text| crate::ai_tools::atomic_write(&staging.join(MARKER), &text));
    if let Err(e) = &written {
        log_failure("a copy could not note how far it got", e);
    }
    written
}

/// Writes the note into a staging folder: (staging, target id, source, step).
type NoteWriter<'a> = &'a dyn Fn(&Path, &str, &Path, &str) -> Result<(), String>;

/// A failed copy's staging folder goes only when everything in it that is
/// not a copy went back; otherwise it stays, marked, for the next start.
fn clear_staging(staging: &Path, target_id: &str, source: &Path, all_back: bool) {
    if all_back {
        let _ = std::fs::remove_dir_all(staging);
    } else {
        // Without a note the next start treats the folder the same way.
        let _ = write_marker(staging, target_id, source, STEP_ABANDONED);
        crate::applog::warn(
            "Environments: a copy that did not finish left the history of an environment aside; it is put back at the next start",
        );
    }
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
    copy_environment_with(
        shared,
        source,
        target_id,
        target_name,
        replace,
        now_ms,
        &mut |a, b| std::fs::rename(a, b),
        &roots::MIGRATION_RENAME_WAITS_MS,
    )
}

/// `copy_environment` with the rename it uses for moving history and for
/// putting the copy in place passed in, so a test can make one fail.
#[allow(clippy::too_many_arguments)]
pub fn copy_environment_with(
    shared: &Path,
    source: &Path,
    target_id: &str,
    target_name: &str,
    replace: bool,
    now_ms: u64,
    rename: Rename,
    waits_ms: &[u64],
) -> Result<CopyDone, String> {
    copy_environment_full(shared, source, target_id, target_name, replace, now_ms, rename, waits_ms, &write_marker)
}

/// `copy_environment_with`, and the writer of the notes passed in too.
#[allow(clippy::too_many_arguments)]
pub fn copy_environment_full(
    shared: &Path,
    source: &Path,
    target_id: &str,
    target_name: &str,
    replace: bool,
    now_ms: u64,
    rename: Rename,
    waits_ms: &[u64],
    note: NoteWriter,
) -> Result<CopyDone, String> {
    if !environments::valid_id(target_id) {
        return Err(COPY_UNKNOWN_TARGET.to_string());
    }
    let target = roots::env_root(shared, target_id);
    if source == target {
        return Err(COPY_ONTO_ITSELF.to_string());
    }
    // Held for writing to the end (about 15 s at most when a folder is held
    // by another process): every guarded write waits behind it.
    let switching = roots::switch_lock().write().unwrap_or_else(|e| e.into_inner());
    if roots::is_held_for_writes(&target) {
        return Err(roots::REMOVE_BUSY.to_string());
    }
    if !std::fs::metadata(source).is_ok_and(|m| m.is_dir()) {
        return Err(COPY_SOURCE_MISSING.to_string());
    }
    if is_empty_env(source) {
        return Err(COPY_NOTHING_TO_COPY.to_string());
    }
    if !is_empty_env(&target) && !replace {
        return Err(COPY_NEEDS_CONFIRM.to_string());
    }
    let staging = shared.join(roots::ENVS_DIR).join(format!("{STAGING_PREFIX}{target_id}-{now_ms}"));
    let built = (|| {
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
    let items = match built {
        Ok(items) => items,
        Err(e) => {
            log_failure("a copy could not be built", &e);
            // Only a copy is in there: nothing of the target's yet.
            let _ = std::fs::remove_dir_all(&staging);
            return Err(COPY_FAILED.to_string());
        }
    };

    // Step 2: the target's history and export record join the copy.
    let mut done_moves: Vec<Moved> = Vec::new();
    for m in kept_in_place(&target, &staging) {
        let step = (|| {
            if let Some(parent) = m.to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            rename_with(&m.from, &m.to, &mut *rename, waits_ms)
        })();
        match step {
            Ok(()) => done_moves.push(m),
            Err(e) => {
                log_failure("a copy could not keep the history of the environment it replaces", &e);
                let back = put_back(&done_moves, &mut *rename, waits_ms);
                clear_staging(&staging, target_id, source, back);
                return Err(COPY_FAILED.to_string());
            }
        }
    }
    // The note is what lets a start finish this copy; without it the copy
    // does not go on (the history goes back like after a step 2 failure).
    if note(&staging, target_id, source, STEP_KEPT).is_err() {
        let back = put_back(&done_moves, &mut *rename, waits_ms);
        clear_staging(&staging, target_id, source, back);
        return Err(COPY_FAILED.to_string());
    }

    // Step 3: what is left of the target waits in the removed area. A target
    // with nothing but empty folders left has nothing worth keeping.
    let retired = if std::fs::symlink_metadata(&target).is_err() || remove_empty_tree(&target) {
        Ok(None)
    } else {
        roots::retire_held(&switching, shared, target_id, target_name, "replaced", now_ms, waits_ms)
    };
    let retired = match retired {
        Ok(key) => key,
        Err(e) => {
            log_failure("a copy could not set the replaced data aside", &e);
            let back = put_back(&done_moves, &mut *rename, waits_ms);
            clear_staging(&staging, target_id, source, back);
            return Err(COPY_FAILED.to_string());
        }
    };

    // Step 4: the finished copy takes the target's place.
    if let Err(e) = rename_with(&staging, &target, &mut *rename, waits_ms) {
        log_failure("a copy could not take its place", &e);
        // Best effort to leave things as they were: the old folder back, the
        // history back in it, and the copy (only a copy) removed. Whatever
        // of that fails leaves staging, marked, and the next start finishes
        // the copy.
        let back = match &retired {
            Some(key) => rename_with(&shared.join(roots::REMOVED_DIR).join(key), &target, &mut *rename, waits_ms)
                .map(|_| {
                    let _ = std::fs::remove_file(target.join("removed.json"));
                })
                .is_ok(),
            None => std::fs::create_dir_all(&target).is_ok(),
        };
        if back && put_back(&done_moves, &mut *rename, waits_ms) {
            let _ = std::fs::remove_dir_all(&staging);
        } else {
            crate::applog::warn("Environments: a copy did not finish and will be completed at the next start");
        }
        return Err(COPY_FAILED.to_string());
    }
    if let Err(e) = std::fs::remove_file(target.join(MARKER)) {
        log_failure("a copy's note stayed behind", &e.to_string());
    }
    crate::applog::info(format!("Environments: copied Auto Run data into one environment ({items} item(s))"));
    Ok(CopyDone { items, kept_aside: retired.is_some() })
}

/// The target id of a `.copying-<id>-<ms>` folder name.
fn staging_target(name: &str) -> Option<String> {
    let rest = name.strip_prefix(STAGING_PREFIX)?;
    let (id, ms) = rest.rsplit_once('-')?;
    ms.parse::<u64>().ok()?;
    environments::valid_id(id).then(|| id.to_string())
}

/// Moves the history and export records found in `staging` back into
/// `target` where the target has none of its own. Says whether every one
/// that was there is now in the target.
fn restore_kept(staging: &Path, target: &Path) -> (bool, usize) {
    let mut items: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (name, how) in PER_ENV_ITEMS {
        if how == CopyTo::ActiveOnly && std::fs::symlink_metadata(staging.join(name)).is_ok() {
            items.push((staging.join(name), target.join(name)));
        }
    }
    if let Ok(entries) = std::fs::read_dir(staging.join("projects")) {
        for e in entries.flatten() {
            if roots::is_export_record(&Path::new("projects").join(e.file_name())) {
                items.push((e.path(), target.join("projects").join(e.file_name())));
            }
        }
    }
    let mut all = true;
    let mut moved_count = 0;
    for (from, to) in items {
        if std::fs::symlink_metadata(&to).is_ok() {
            continue;
        }
        let moved = (|| {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            rename_waiting(&from, &to)
        })();
        match moved {
            Ok(()) => moved_count += 1,
            Err(e) => {
                all = false;
                log_failure("an environment's history could not be put back", &e);
            }
        }
    }
    (all, moved_count)
}

/// Settles every copy a crash or a failed restore left half-way
/// (`envs/.copying-*`), before any environment folder is chosen:
/// - no note: left as it is, and said so;
/// - finished copy and the target gone or holding no file: the copy is put
///   in the target's place;
/// - otherwise: the target's history and export records in staging are put
///   back where the target has none, and staging is left aside.
///
/// Nothing is ever deleted. Returns how many folders it settled.
pub fn recover(shared: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(shared.join(roots::ENVS_DIR)) else { return 0 };
    // (target, stamp, folder), newest first within a target: only the newest
    // copy for a target may be finished; the others only give history back.
    let mut found: Vec<(String, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(target_id) = staging_target(name) else { continue };
        let Some(ms) = name.rsplit_once('-').and_then(|(_, ms)| ms.parse::<u64>().ok()) else { continue };
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            found.push((target_id, ms, entry.path()));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)).then_with(|| a.2.cmp(&b.2)));
    let mut settled = 0;
    let mut previous: Option<&str> = None;
    for (target_id, _, staging) in &found {
        let newest = previous != Some(target_id.as_str());
        previous = Some(target_id.as_str());
        let target = roots::env_root(shared, target_id);
        let marker: Option<Marker> = std::fs::read_to_string(staging.join(MARKER))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .filter(|m: &Marker| m.target == *target_id);
        // No readable note is the same as an abandoned copy: whatever of the
        // target's history is in there is the target's, and goes back.
        let kept = marker.as_ref().is_some_and(|m| m.step == STEP_KEPT);
        if kept && newest && (std::fs::symlink_metadata(&target).is_err() || remove_empty_tree(&target)) {
            match rename_waiting(staging, &target) {
                Ok(()) => {
                    let _ = std::fs::remove_file(target.join(MARKER));
                    crate::applog::info("Environments: a copy that was cut short was finished");
                    settled += 1;
                }
                Err(e) => log_failure("a copy that was cut short could not be finished", &e),
            }
            continue;
        }
        let (all, moved) = restore_kept(staging, &target);
        if moved > 0 {
            crate::applog::warn(
                "Environments: a copy that did not finish was set aside; its environment has its history back",
            );
            settled += 1;
        }
        if !all {
            crate::applog::warn("Environments: a copy that did not finish could not give an environment its history back");
        }
    }
    settled
}

// ---- copying one case ------------------------------------------------------

/// Said when the target already has a script for the case and the person has
/// not yet agreed to replace it.
pub const CASE_COPY_NEEDS_CONFIRM: &str = "that environment already has a script for this case - confirm to replace it";
/// Said when the case has no script in the source.
pub const NO_SCRIPT_TO_COPY: &str = "this case has no script to copy";
/// Said when the target is the environment the person is in.
pub const COPY_TO_ACTIVE: &str = "that is the environment you are in - pick another one";
/// Said when the target is not an environment folder this copy may write to.
pub const CASE_COPY_BAD_TARGET: &str = "that environment cannot be copied to";

/// What copying one case would do, or did. Names only: nothing here holds a
/// path, a password or a script body.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CaseCopyPlan {
    /// The target already has a script for the case (copying replaces it).
    pub target_has_script: bool,
    /// Components the target lacks, brought across.
    pub components_added: Vec<String>,
    /// Components the target has in a different version; its version is used.
    pub components_kept: Vec<String>,
    /// The case's area, when the target lacks it and it is brought across.
    pub area_added: Option<String>,
    /// The case's area, when the target has it in a different version.
    pub area_kept: Option<String>,
    /// The case names no area: it goes where its Module goes, nothing to copy.
    pub uses_default_area: bool,
    /// The target had no sign-in recipe and the source has one: it is brought across.
    pub recipe_added: bool,
    /// The script's account key, when the target has no account of that key.
    pub missing_account: Option<String>,
    /// Fixture and flow ids the script names that the target lacks. Not copied.
    pub missing_api: Vec<String>,
}

/// What a case copy would write, besides the script itself.
struct CaseGather {
    script: crate::autorun::CaseScript,
    target_script: Option<crate::autorun::CaseScript>,
    components: Vec<super::components::Component>,
    area: Option<super::nav::ModulePath>,
    recipe: Option<super::recipe::SignInRecipe>,
    plan: CaseCopyPlan,
}

/// Every component name `actions` use, a `when_visible` body's included, at
/// any depth.
fn used_components(actions: &[crate::browser::actions::Action], out: &mut Vec<String>) {
    use crate::browser::actions::Action;
    for a in actions {
        match a {
            Action::UseComponent { component, .. } => out.push(component.trim().to_string()),
            Action::WhenVisible { then, .. } | Action::RepeatWhileVisible { then, .. } => used_components(then, out),
            _ => {}
        }
    }
}

fn same<T: serde::Serialize>(a: &T, b: &T) -> bool {
    match (serde_json::to_value(a), serde_json::to_value(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Reads both sides and decides; writes nothing.
fn gather_case(
    source: &Path,
    target: &Path,
    target_env_id: &str,
    org: &str,
    project: &str,
    case_id: i32,
) -> Result<CaseGather, String> {
    use super::{accounts, components, nav, recipe, store};
    if !environments::valid_id(target_env_id) || roots::env_id_of(target).as_deref() != Some(target_env_id) {
        return Err(CASE_COPY_BAD_TARGET.to_string());
    }
    if roots::env_id_of(source).is_some() && roots::env_id_of(source) == roots::env_id_of(target) {
        return Err(COPY_ONTO_ITSELF.to_string());
    }
    // A script that cannot be read is logged; the sentence names no path.
    let load = |root: &Path| {
        store::load_script(root, case_id).map_err(|e| {
            crate::applog::warn(format!("Auto Run: a script to copy could not be read: {e}"));
            COPY_FAILED.to_string()
        })
    };
    let script = load(source)?.ok_or_else(|| NO_SCRIPT_TO_COPY.to_string())?;
    let target_script = load(target)?;

    // Components: the ones the script uses, then the ones those use.
    let src_c = components::load_components(source, org, project)?;
    let tgt_c = components::load_components(target, org, project)?;
    let mut todo: Vec<String> = Vec::new();
    for s in &script.steps {
        used_components(&s.actions, &mut todo);
    }
    todo.reverse();
    let mut seen: Vec<String> = Vec::new();
    let mut added: Vec<components::Component> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    while let Some(name) = todo.pop() {
        let k = nav::module_key(&name);
        if seen.contains(&k) {
            continue;
        }
        seen.push(k);
        let walk = match (components::find(&src_c, &name), components::find(&tgt_c, &name)) {
            (Some(s), None) => {
                added.push(s.clone());
                s
            }
            (Some(s), Some(t)) => {
                if !same(s, t) {
                    kept.push(t.name.clone());
                }
                // The target's version is the one that runs, so its own
                // uses are the ones that must exist.
                t
            }
            (None, Some(t)) => t,
            (None, None) => continue,
        };
        let mut inner = Vec::new();
        used_components(&walk.actions, &mut inner);
        inner.reverse();
        todo.extend(inner);
    }

    // The area.
    let mut area = None;
    let mut area_added = None;
    let mut area_kept = None;
    let mut uses_default_area = false;
    match script.area.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        None => uses_default_area = true,
        Some(name) => {
            let src_n = nav::load_nav(source, org, project)?;
            let tgt_n = nav::load_nav(target, org, project)?;
            if let Some(s) = nav::find_area(&src_n, name) {
                match nav::find_area(&tgt_n, name) {
                    None => {
                        area_added = Some(s.name().to_string());
                        area = Some(s.clone());
                    }
                    Some(t) if !same(s, t) => area_kept = Some(t.name().to_string()),
                    Some(_) => {}
                }
            }
        }
    }

    // The sign-in recipe.
    let mut recipe_out = None;
    if recipe::load_recipe(target, org, project)?.is_none() {
        if let Some(r) = recipe::load_recipe(source, org, project)? {
            r.validate()?;
            recipe_out = Some(r);
        }
    }

    // The account key, and the API data that is named but not copied.
    let missing_account = script
        .account
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .filter(|k| !accounts::load_accounts_for(target, target_env_id).is_ok_and(|a| a.iter().any(|x| x.key == *k)))
        .map(str::to_string);
    let mut missing_api: Vec<String> = Vec::new();
    let mut note = |id: &str, absent: bool| {
        if absent && !id.trim().is_empty() && !missing_api.iter().any(|m| m == id) {
            missing_api.push(id.to_string());
        }
    };
    if let Some(s) = &script.setup {
        let there = crate::api_templates::fixture_store::load(target, org, project, &s.fixture).is_ok_and(|f| f.is_some());
        note(&s.fixture, !there);
    }
    for p in &script.preconditions {
        let there = crate::api_templates::flow_store::load(target, org, project, &p.flow).is_ok_and(|f| f.is_some());
        note(&p.flow, !there);
    }

    let plan = CaseCopyPlan {
        target_has_script: target_script.is_some(),
        components_added: added.iter().map(|c| c.name.clone()).collect(),
        components_kept: kept,
        area_added,
        area_kept,
        uses_default_area,
        recipe_added: recipe_out.is_some(),
        missing_account,
        missing_api,
    };
    Ok(CaseGather { script, target_script, components: added, area, recipe: recipe_out, plan })
}

/// What copying case `case_id`'s script from `source` into `target` would
/// bring and keep. Writes nothing.
pub fn plan_case_copy(
    source: &Path,
    target: &Path,
    target_env_id: &str,
    org: &str,
    project: &str,
    case_id: i32,
) -> Result<CaseCopyPlan, String> {
    gather_case(source, target, target_env_id, org, project, case_id).map(|g| g.plan)
}

/// Copies case `case_id` into `target` (an environment root other than the
/// active one): what its script needs and the target lacks first - components,
/// area, recipe - then the script, last, so a failure never leaves a script
/// whose area or component is missing. Nothing the target already has is
/// overwritten except the script, and only with `replace`; the script it
/// replaces is first kept aside like a deleted one, and put back if the new
/// one cannot be written. The suspected-defect marks do not travel.
pub fn copy_case(
    source: &Path,
    target: &Path,
    target_env_id: &str,
    org: &str,
    project: &str,
    case_id: i32,
    replace: bool,
) -> Result<CaseCopyPlan, String> {
    use super::{components, nav, recipe, store};
    let g = gather_case(source, target, target_env_id, org, project, case_id)?;
    if g.plan.target_has_script && !replace {
        return Err(CASE_COPY_NEEDS_CONFIRM.to_string());
    }
    let failed = |what: &str, e: String| {
        crate::applog::warn(format!("Auto Run: copying case {case_id} to another environment stopped at its {what}: {e}"));
        COPY_FAILED.to_string()
    };
    // Writes into a root that is not the active one are let through the
    // guard only while this is held.
    let _allowed = roots::allow_writes_to(target);
    for c in g.components {
        components::put(target, org, project, c).map_err(|e| failed("components", e))?;
    }
    if let Some(a) = g.area {
        nav::put_path_if(target, org, project, a, nav::AreaExpect::Any).map_err(|e| failed("area", e))?;
    }
    if let Some(r) = &g.recipe {
        recipe::save_recipe(target, org, project, r).map_err(|e| failed("sign-in recipe", e))?;
    }
    if g.target_script.is_some() {
        store::delete_script(target, case_id, super::sessions::now_ms()).map_err(|e| failed("old script", e))?;
    }
    let mut script = g.script;
    script.suspected_defect = None;
    script.cleared_defect = None;
    if let Err(e) = store::save_script(target, &script) {
        if let Some(old) = &g.target_script {
            let _ = store::save_script(target, old);
        }
        return Err(failed("script", e));
    }
    crate::applog::info(format!("Auto Run: copied case {case_id} to another environment"));
    Ok(g.plan)
}
