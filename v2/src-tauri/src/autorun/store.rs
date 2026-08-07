//! Scripts and runs on disk, under the app's own data directory.

use super::{CaseScript, LocalRun};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
pub fn save_scripts_atomically(root: &Path, scripts: &[CaseScript]) -> Result<(), SaveScriptsError> {
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
        let json = serde_json::to_string_pretty(sc).map_err(|e| SaveScriptsError::Io(e.to_string()))?;
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
