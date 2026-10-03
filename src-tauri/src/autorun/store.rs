//! Scripts and runs on disk, under the app's own data directory.

use super::{CaseScript, LocalRun, SuspectedDefect};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A run id is a filename component, never a path: no separators, no
/// traversal, and short enough to be a sane file on any filesystem.
/// `store::new_run_id()` is the only producer today, but this is the rule
/// every id must satisfy before it reaches the filesystem, wherever it
/// came from - an IPC command's argument, or a caller of `load_run`.
pub fn safe_run_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Where scripts and runs live, remembered process-wide.
///
/// Commands get the path from their `AppHandle`, but the AI bridge has no
/// handle - it is a router, not a Tauri command - and still has to write
/// a script an assistant sends. The app sets this once at startup, the
/// same way `/begin` publishes its plan path, and the bridge reads it.
static ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Called once during app setup.
pub fn set_root(root: PathBuf) {
    if let Ok(mut slot) = ROOT.lock() {
        *slot = Some(root);
    }
}

/// `None` before setup has run - a caller with no handle must say so
/// rather than guessing a path and writing somewhere nobody reads.
pub fn configured_root() -> Option<PathBuf> {
    ROOT.lock().ok().and_then(|s| s.clone())
}

fn scripts_dir(root: &Path) -> PathBuf {
    root.join("scripts")
}

fn runs_dir(root: &Path) -> PathBuf {
    root.join("runs")
}

/// Epoch milliseconds, which sorts and reads as a time.
pub fn new_run_id() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("run-{ms}")
}

/// Every write of a script file holds this: a bundle save copies the mark
/// already on disk into what it writes, and `set_suspected_defect` reads,
/// changes and writes one file - either landing between the other's read
/// and write would lose a mark or bring back a cleared one.
fn scripts_lock() -> std::sync::MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

/// The mark on the script already on disk for this case, if any. A file
/// that is missing or unreadable has none: the save about to replace it is
/// the only copy that will be left.
fn mark_on_disk(root: &Path, case_id: i32) -> Option<SuspectedDefect> {
    load_script(root, case_id).ok().flatten().and_then(|s| s.suspected_defect)
}

/// A raw write of one script, exactly as given - the mark included. The
/// app's own save paths go through `save_scripts_atomically`, which keeps
/// the mark on disk; this one is for setting up a store by hand.
pub fn save_script(root: &Path, script: &CaseScript) -> Result<(), String> {
    let dir = scripts_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(script).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("case-{}.json", script.case_id)), json)
        .map_err(|e| e.to_string())
}

/// Why a bundle save failed - so a caller with an HTTP status to pick
/// (the AI bridge) can tell "you sent something invalid" from "the disk
/// said no" without parsing the message text.
#[derive(Debug)]
pub enum SaveScriptsError {
    /// The bundle itself is wrong - a bad id, a duplicate, a script with
    /// no steps. A retry with the same bundle will fail the same way.
    Invalid(String),
    /// The filesystem said no. A retry might succeed.
    Io(String),
}

impl std::fmt::Display for SaveScriptsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveScriptsError::Invalid(s) | SaveScriptsError::Io(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for SaveScriptsError {}

/// Save a whole bundle of scripts as one unit: every entry lands, or none
/// does. Used by both the AI bridge's `save_autorun_script` and the Auto
/// Run screen's file import - the two places a batch of scripts can
/// arrive from outside the app, and the two places a half-applied batch
/// would leave a tester unable to tell which cases are current.
///
/// Three passes, each touching more than the last, so a bad bundle is
/// caught before the disk sees any of it:
///
/// 1. Validate and serialise every entry. Pure in-memory work - a
///    nonsense case id, a duplicate id, or a script with no steps
///    rejects the whole call without a single byte written.
/// 2. Confirm every target path is actually free to become a file. Still
///    nothing written - this is what keeps an EARLIER entry from landing
///    just because it happened to be processed before a later one that
///    cannot be written (e.g. its filename is already a directory).
/// 3. Write each entry to a `.tmp` sibling and `fs::rename` it into
///    place. Each rename is a single filesystem operation - no reader
///    ever sees a half-written case file - and by the time we reach it,
///    pass 2 has already ruled out the one failure mode this bundle
///    format can detect ahead of time.
///
/// Every entry is written with the suspected-defect mark the case's script
/// already has on disk, whatever the entry itself carried: only
/// `set_suspected_defect` changes a mark, so a stale copy from the editor
/// or a file can neither drop one nor bring back one that was cleared.
pub fn save_scripts_atomically(root: &Path, scripts: &[CaseScript]) -> Result<(), SaveScriptsError> {
    let _held = scripts_lock();
    let dir = scripts_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| SaveScriptsError::Io(e.to_string()))?;

    // Pass 1: validate + serialise.
    let mut seen = std::collections::HashSet::new();
    let mut entries: Vec<(PathBuf, String)> = Vec::with_capacity(scripts.len());
    for sc in scripts {
        if sc.case_id <= 0 {
            return Err(SaveScriptsError::Invalid(format!(
                "case id {} is not a valid Azure DevOps work item id",
                sc.case_id
            )));
        }
        if !seen.insert(sc.case_id) {
            return Err(SaveScriptsError::Invalid(format!(
                "case {} appears more than once in this bundle",
                sc.case_id
            )));
        }
        if sc.steps.is_empty() {
            return Err(SaveScriptsError::Invalid(format!(
                "case {} has no steps - a script that runs nothing cannot be saved",
                sc.case_id
            )));
        }
        {
            let mut seen_steps = std::collections::HashSet::new();
            for step in &sc.steps {
                if !seen_steps.insert(step.step_number) {
                    return Err(SaveScriptsError::Invalid(format!(
                        "case {}: step {} appears more than once",
                        sc.case_id, step.step_number
                    )));
                }
            }
        }
        if let Some(key) = &sc.account {
            if !crate::autorun::accounts::valid_key(key) {
                return Err(SaveScriptsError::Invalid(format!(
                    "case {}: \"{key}\" is not a usable account key",
                    sc.case_id
                )));
            }
        }
        for step in &sc.steps {
            if let Some(reason) = &step.unchecked {
                if reason.trim().is_empty() {
                    return Err(SaveScriptsError::Invalid(format!(
                        "case {} step {}: unchecked needs a reason, not an empty string",
                        sc.case_id, step.step_number
                    )));
                }
            }
            for (i, action) in step.actions.iter().enumerate() {
                let text = serde_json::to_string(action).unwrap_or_default();
                if crate::autorun::recipe::has_placeholder(&text) {
                    return Err(SaveScriptsError::Invalid(format!(
                        "case {} step {} action {}: {{{{username}}}} and {{{{password}}}} belong in the project's sign-in recipe, not in a script",
                        sc.case_id, step.step_number, i + 1
                    )));
                }
                if let Err(why) = action.validate() {
                    return Err(SaveScriptsError::Invalid(format!(
                        "case {} step {} action {}: {why}",
                        sc.case_id,
                        step.step_number,
                        i + 1
                    )));
                }
            }
        }
        // The disk's mark stays only while the script still has its step:
        // a mark on a step that is gone could never label a failure or
        // pass, so it would never clear itself.
        let disk_mark = mark_on_disk(root, sc.case_id).filter(|m| {
            let still_there = sc.steps.iter().any(|s| s.step_number == m.step_number);
            if !still_there {
                crate::applog::info(format!(
                    "Auto Run: the suspected defect at step {} of case {} was dropped because a save removed that step",
                    m.step_number, sc.case_id
                ));
            }
            still_there
        });
        let kept = CaseScript { suspected_defect: disk_mark, ..sc.clone() };
        let json = serde_json::to_string_pretty(&kept).map_err(|e| SaveScriptsError::Io(e.to_string()))?;
        entries.push((dir.join(format!("case-{}.json", sc.case_id)), json));
    }

    // Pass 2: every target must be a plain file slot, not something else
    // already occupying that name, checked for the WHOLE bundle before
    // any write.
    for (path, _) in &entries {
        if path.is_dir() {
            return Err(SaveScriptsError::Io(format!(
                "{} exists and is a directory, not a script file",
                path.display()
            )));
        }
    }

    // Pass 3: stage then commit.
    for (path, json) in &entries {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| SaveScriptsError::Io(e.to_string()))?;
        if let Err(e) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(SaveScriptsError::Io(e.to_string()));
        }
    }
    Ok(())
}

/// Set (`Some`) or remove (`None`) one case's suspected-defect mark, and
/// nothing else: the script's steps, repairs and last repair are written
/// back exactly as they were read. The one way a mark changes - the
/// assistant's `mark_autorun_suspected_defect`, its repair of the marked
/// step, and a person's Clear all come through here. Written to a `.tmp`
/// sibling and renamed into place, like a bundle save. Refused for a case
/// with no script on this machine.
pub fn set_suspected_defect(root: &Path, case_id: i32, mark: Option<SuspectedDefect>) -> Result<(), String> {
    let _held = scripts_lock();
    let Some(mut script) = load_script(root, case_id)? else {
        return Err(format!("case {case_id} has no script on this machine"));
    };
    if script.suspected_defect == mark {
        return Ok(());
    }
    script.suspected_defect = mark;
    write_script(root, &script)
}

/// Remove a case's mark, but only when the mark on disk is on
/// `step_number`, and say whether it was removed. The check and the write
/// happen together under the scripts lock, so a mark that moved to another
/// step since the caller read it is left alone. `Ok(false)` when there is
/// no mark or it is on another step; an error for a case with no script.
pub fn clear_suspected_defect_at(root: &Path, case_id: i32, step_number: i32) -> Result<bool, String> {
    clear_mark(root, case_id, Some(step_number))
}

/// A person's Clear: remove the case's mark whichever step it is on, and
/// say whether there was one to remove.
pub fn clear_suspected_defect_any(root: &Path, case_id: i32) -> Result<bool, String> {
    clear_mark(root, case_id, None)
}

fn clear_mark(root: &Path, case_id: i32, step_number: Option<i32>) -> Result<bool, String> {
    let _held = scripts_lock();
    let Some(mut script) = load_script(root, case_id)? else {
        return Err(format!("case {case_id} has no script on this machine"));
    };
    match &script.suspected_defect {
        Some(d) if step_number.is_none_or(|n| n == d.step_number) => {}
        _ => return Ok(false),
    }
    script.suspected_defect = None;
    write_script(root, &script)?;
    Ok(true)
}

/// Write one script by a `.tmp` sibling and a rename. The caller holds the
/// scripts lock.
fn write_script(root: &Path, script: &CaseScript) -> Result<(), String> {
    let json = serde_json::to_string_pretty(script).map_err(|e| e.to_string())?;
    let path = scripts_dir(root).join(format!("case-{}.json", script.case_id));
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}

/// `Ok(None)` for a case nobody has scripted yet - that is the normal
/// state of most cases, not an error.
pub fn load_script(root: &Path, case_id: i32) -> Result<Option<CaseScript>, String> {
    let path = scripts_dir(root).join(format!("case-{case_id}.json"));
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| format!("{} is not a readable script: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_run(root: &Path, run: &LocalRun) -> Result<(), String> {
    let dir = runs_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(run).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{}.json", run.id)), json).map_err(|e| e.to_string())
}

/// `save_run`, but refuses to overwrite a run that has already been sent
/// to Azure DevOps (`published` set on disk) with a copy that has lost
/// that fact (`published` unset). A stale review screen - one opened
/// before a send and saved after - must never erase the record that the
/// send happened. Saving the run back WITH its `published` block intact
/// (a note edited after sending) is still allowed, and so is a run that
/// was never published in the first place.
///
/// A run file that exists but cannot be READ (corrupt JSON, a partial
/// write) must refuse too, rather than being treated as "nothing to
/// guard against" - failing open here would let exactly the corruption
/// this guard exists for slip an unpublished copy over a published run
/// whose file merely could not be parsed this time.
pub fn save_run_guarded(root: &Path, run: &LocalRun) -> Result<(), String> {
    if !safe_run_id(&run.id) {
        return Err(format!("run id {:?} is not a safe filename", run.id));
    }
    if run.published.is_none() {
        match load_run(root, &run.id) {
            Ok(Some(existing)) if existing.published.is_some() => {
                return Err(
                    "this run has already been sent to Azure DevOps and can no longer be changed"
                        .to_string(),
                );
            }
            Ok(_) => {}
            Err(e) => {
                return Err(format!(
                    "this run's file on disk could not be read, so it was not overwritten: {e}"
                ));
            }
        }
    }
    save_run(root, run)
}

/// `Ok(None)` for a run id nobody has saved yet. The id is checked with
/// the same rule as a save - this is the id's read-side entry point, and
/// a hostile or buggy value must be rejected before it ever reaches the
/// filesystem, not just when writing.
pub fn load_run(root: &Path, id: &str) -> Result<Option<LocalRun>, String> {
    if !safe_run_id(id) {
        return Err(format!("run id {id:?} is not a safe filename"));
    }
    let path = runs_dir(root).join(format!("{id}.json"));
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| format!("{} is not a readable run: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Newest first. An unreadable file is skipped: one corrupt run must
/// never hide every other run from the results view.
pub fn list_runs(root: &Path) -> Vec<LocalRun> {
    let Ok(entries) = std::fs::read_dir(runs_dir(root)) else {
        return vec![];
    };
    let mut out: Vec<LocalRun> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<LocalRun>(&s).ok())
        .collect();
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    out
}

/// How many failure screenshots are kept. They are evidence for the run in
/// front of the person, not an archive - but an unattended run of thirty
/// cases with a picture per step passes 200 on its own, and at JPEG
/// quality 60 a thousand pictures is on the order of 100 MB.
const MAX_SHOTS: usize = 1000;

static SHOT_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn shots_dir(root: &Path) -> PathBuf {
    root.join("shots")
}

/// Where a run's report page is written - beside the shots it links.
pub fn reports_dir(root: &Path) -> PathBuf {
    root.join("reports")
}

/// `shot-<digits>-<digits>.jpg` and nothing else. The name comes back from
/// the webview, so it is checked, not trusted.
pub fn safe_shot_name(name: &str) -> bool {
    let Some(middle) = name.strip_prefix("shot-").and_then(|n| n.strip_suffix(".jpg")) else {
        return false;
    };
    let mut parts = middle.split('-');
    let ok = |p: Option<&str>| p.is_some_and(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()));
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

/// Whether a screenshot of that name is in the shots folder. Only a name
/// `safe_shot_name` accepts is looked up at all.
/// It must be a regular file: a name that is a link to somewhere else is not
/// a picture of the run and is never linked.
pub fn shot_exists(root: &Path, name: &str) -> bool {
    safe_shot_name(name)
        && std::fs::symlink_metadata(shots_dir(root).join(name)).is_ok_and(|m| m.file_type().is_file())
}

pub fn save_shot(root: &Path, bytes: &[u8]) -> Result<String, String> {
    save_shot_keeping(root, bytes, MAX_SHOTS)
}

/// Save, then drop the oldest beyond `keep`. Names sort by time: epoch
/// milliseconds, then a zero-padded counter for shots in the same
/// millisecond.
///
/// Once the write above has landed, the name it returns is a screenshot
/// that genuinely exists on disk - losing track of it because pruning
/// afterwards hit trouble (antivirus holding a lock on a just-written file
/// is a real event on this machine) would be worse than a folder that grows
/// a little past `keep` until the next successful prune.
pub fn save_shot_keeping(root: &Path, bytes: &[u8], keep: usize) -> Result<String, String> {
    let dir = shots_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let seq = SHOT_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let name = format!("shot-{ms}-{seq:06}.jpg");
    std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;

    prune_shots(root, &dir, keep);
    Ok(name)
}

/// Every shot name referenced by a run that has NOT been sent to Azure
/// DevOps yet - pictures a person has not had the chance to review and
/// confirm are evidence, not disposable, no matter how old. A run once it
/// is `published` no longer protects its shots: they already made it into
/// the record Azure DevOps holds (or the problem list says they did not),
/// so keeping them here is no longer the only copy that matters.
fn shots_of_unpublished_runs(root: &Path) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for run in list_runs(root) {
        if run.published.is_some() {
            continue;
        }
        for case in &run.cases {
            for step in &case.steps {
                if let Some(name) = &step.screenshot {
                    out.insert(name.clone());
                }
                for outcome in &step.outcomes {
                    if let Some(name) = &outcome.screenshot {
                        out.insert(name.clone());
                    }
                }
            }
        }
    }
    out
}

/// Drop the oldest shots beyond `keep`, never one still referenced by an
/// unpublished run's own steps - `keep` is the budget for everything
/// else. Entirely best effort: a listing or delete failure here must
/// never lose track of a screenshot that already made it to disk.
fn prune_shots(root: &Path, dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let protected = shots_of_unpublished_runs(root);
    let mut all: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| safe_shot_name(n))
        .filter(|n| !protected.contains(n))
        .collect();
    all.sort();
    if all.len() > keep {
        for old in &all[..all.len() - keep] {
            let _ = std::fs::remove_file(dir.join(old));
        }
    }
}

pub fn load_shot(root: &Path, name: &str) -> Result<Vec<u8>, String> {
    if !safe_shot_name(name) {
        return Err(format!("{name:?} is not a screenshot name"));
    }
    std::fs::read(shots_dir(root).join(name)).map_err(|e| format!("that screenshot is gone: {e}"))
}

/// Delete the named cases' scripts from this machine. A case with no
/// script file is not an error - the whole point of "clear scripts" is
/// that a case nobody has scripted yet is left exactly as it was. Returns
/// how many files were actually removed, so the caller can report a
/// truthful count rather than just "done".
///
/// Any other I/O error stops the sweep and is returned as-is, naming the
/// first case that failed - the same shape every other store function
/// here uses, and there is no atomicity to preserve across independent
/// per-case files the way there is for a script bundle.
pub fn clear_scripts(root: &Path, case_ids: &[i32]) -> Result<usize, String> {
    // Under the scripts lock: a mark being set loads, then renames a new
    // file into place, and a delete landing between would be undone.
    let _held = scripts_lock();
    let dir = scripts_dir(root);
    let mut removed = 0usize;
    for id in case_ids {
        let path = dir.join(format!("case-{id}.json"));
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("could not remove {}: {e}", path.display())),
        }
    }
    Ok(removed)
}

/// Delete every saved run, and every screenshot with it - including runs
/// that were already sent to Azure DevOps. The record Azure DevOps holds
/// is the durable one; the confirm the screen shows before calling this
/// says so. Returns the number of RUN files removed (not shots - a single
/// run's evidence can be many pictures, and that count would not mean
/// anything to the person reading the toast).
///
/// An empty or missing root removes nothing and returns 0, the same as
/// `list_runs` reads nothing back from one - but a directory that exists
/// and cannot be listed (permissions, or a file sitting where `runs/`
/// should be) is a real failure, reported rather than swallowed as zero.
pub fn clear_runs(root: &Path) -> Result<usize, String> {
    // Every pass runs even if an earlier one failed, so a partial clear is
    // finished as far as it can be and then reported - never half done with
    // no sign of it.
    let mut problems: Vec<String> = Vec::new();
    let removed = sweep_files(&runs_dir(root), Some("json"), &mut problems);
    sweep_files(&shots_dir(root), None, &mut problems);
    // The reports opened from those runs go too: they name the cases and
    // link pictures that are now gone.
    sweep_files(&reports_dir(root), None, &mut problems);
    if problems.is_empty() {
        Ok(removed)
    } else {
        Err(problems.join("; "))
    }
}

/// Whether `path` is itself a link (a symlink or, on Windows, a junction or
/// any other reparse point) - never followed.
fn is_link(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Remove the regular files directly in `dir` (only those with extension
/// `only_ext`, when given) and return how many went. Nothing else is
/// touched: a folder in it, a link (never followed, never removed) and
/// anything outside it stay, and a `dir` that is itself a link is skipped
/// whole - what it points at is not ours to clear. A missing `dir` is
/// nothing to do; one that cannot be read, or a file that cannot be
/// removed, is added to `problems` and the sweep goes on.
fn sweep_files(dir: &Path, only_ext: Option<&str>, problems: &mut Vec<String>) -> usize {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if is_link(&meta) => return 0,
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return 0,
        Err(e) => {
            problems.push(format!("could not read {}: {e}", dir.display()));
            return 0;
        }
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            problems.push(format!("could not read {}: {e}", dir.display()));
            return 0;
        }
    };
    let mut removed = 0usize;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                problems.push(format!("could not read an entry of {}: {e}", dir.display()));
                continue;
            }
        };
        let path = entry.path();
        // `DirEntry::file_type` does not follow links.
        match entry.file_type() {
            Ok(t) if t.is_file() => {}
            Ok(_) => continue,
            Err(e) => {
                problems.push(format!("could not read {}: {e}", path.display()));
                continue;
            }
        }
        if let Some(ext) = only_ext {
            if !path.extension().is_some_and(|x| x == ext) {
                continue;
            }
        }
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => problems.push(format!("could not remove {}: {e}", path.display())),
        }
    }
    removed
}
