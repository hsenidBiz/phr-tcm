//! Where API templates and their run history are kept - see design doc
//! "API templates" §4 (storage) and §5.6 (results/run history).
//!
//! Layout, under the Auto Run data root:
//!
//! ```text
//! templates/<project_slug>/<id>.json       the template (see mod.rs)
//! templates/<project_slug>/<id>.runs.json  its run history, newest first
//! ```
//!
//! `<project_slug>` is `autorun::recipe::project_slug`, the same slug the
//! sign-in recipe and quirks files use for this org/project pair.

use super::{valid_id, ApiTemplate};
use crate::autorun::recipe::project_slug;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many runs a template's history keeps. Past this, the oldest runs
/// are no longer worth the read - see constraints.md.
pub(super) const MAX_RUNS: usize = 20;

pub fn templates_dir(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("templates").join(project_slug(org, project))
}

fn template_path(root: &Path, org: &str, project: &str, id: &str) -> PathBuf {
    templates_dir(root, org, project).join(format!("{id}.json"))
}

fn runs_path(root: &Path, org: &str, project: &str, id: &str) -> PathBuf {
    templates_dir(root, org, project).join(format!("{id}.runs.json"))
}

fn invalid_id(id: &str) -> String {
    format!("'{id}' is not a valid template id")
}

/// What a history line was: the prove that saved the template, or a run.
pub const MODE_PROVE: &str = "prove";
pub const MODE_RUN: &str = "run";

fn run_mode() -> String {
    MODE_RUN.to_string()
}

/// One proving or running of a template: when, as who, whether it
/// succeeded, and - on failure - which step and what the run reported.
/// `outputs` carries whatever had actually been captured by the time the
/// run stopped (empty on an early failure).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RunRecord {
    pub at: String,
    /// `"prove"` (a successful prove that saved the template - a failed
    /// one changes nothing and is not kept) or `"run"`. A history written
    /// before this was recorded holds runs only, so it reads as `"run"`.
    #[serde(default = "run_mode")]
    pub mode: String,
    pub account: String,
    pub ok: bool,
    #[serde(default)]
    pub failed_step: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    // specta's built-in `serde_json::Value` mapping pulls in
    // `serde_json::Number`'s i64/u64 variants, which the TypeScript
    // exporter refuses to emit (precision loss) - see the matching note
    // on `Expect::json` in mod.rs. The wire format is still real JSON;
    // only the exported TS type's value side becomes `unknown`.
    #[specta(type = BTreeMap<String, specta_typescript::Unknown>)]
    pub outputs: BTreeMap<String, Value>,
}

/// A template together with its run history, as the tab lists it.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct SavedTemplate {
    pub template: ApiTemplate,
    pub runs: Vec<RunRecord>,
}

/// Reads one template's saved run history, newest first. A missing file
/// reads as no history; a file that fails to parse is a real error (unlike
/// `list`, which is expected to tolerate a broken template file and skip
/// it - a broken *history* file is not silently dropped, since dropping it
/// would truncate `append_run`'s own idea of what already happened).
fn load_runs(root: &Path, org: &str, project: &str, id: &str) -> Result<Vec<RunRecord>, String> {
    match std::fs::read_to_string(runs_path(root, org, project, id)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|e| format!("that template's run history is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

/// Loads one template by id, or `None` if it has never been saved.
pub fn load(root: &Path, org: &str, project: &str, id: &str) -> Result<Option<ApiTemplate>, String> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    match std::fs::read_to_string(template_path(root, org, project, id)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map(Some).map_err(|e| format!("that template is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Writes a template, atomically, replacing whatever was saved under the
/// same id. Refuses before touching the disk if `t.id` is not a valid
/// filename component.
pub fn save(root: &Path, org: &str, project: &str, t: &ApiTemplate) -> Result<(), String> {
    if !valid_id(&t.id) {
        return Err(invalid_id(&t.id));
    }
    let dir = templates_dir(root, org, project);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(t).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&template_path(root, org, project, &t.id), &json)
}

/// Every saved template for this project, with its run history, sorted by
/// `module` then `title`. A template file that no longer parses is skipped
/// and logged - never fatal to the rest of the list, since one bad file
/// must not hide every other template from the tab.
pub fn list(root: &Path, org: &str, project: &str) -> Result<Vec<SavedTemplate>, String> {
    let dir = templates_dir(root, org, project);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !name.ends_with(".json") || name.ends_with(".runs.json") {
            continue;
        }
        let Some(id) = name.strip_suffix(".json") else { continue };

        let contents = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                crate::applog::warn(format!("api template file {name} could not be read, skipped: {e}"));
                continue;
            }
        };
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        let template: ApiTemplate = match serde_json::from_str(contents) {
            Ok(t) => t,
            Err(e) => {
                crate::applog::warn(format!("api template file {name} does not parse, skipped: {e}"));
                continue;
            }
        };
        let runs = load_runs(root, org, project, id).unwrap_or_else(|e| {
            crate::applog::warn(format!("api template {id}'s run history does not parse, treated as empty: {e}"));
            vec![]
        });
        out.push(SavedTemplate { template, runs });
    }

    out.sort_by(|a, b| {
        (a.template.module.as_str(), a.template.title.as_str())
            .cmp(&(b.template.module.as_str(), b.template.title.as_str()))
    });
    Ok(out)
}

/// Deletes a template and its run history. A template that was never
/// saved (or history that was never written) is not an error - `remove`
/// only guarantees neither is there afterwards.
pub fn remove(root: &Path, org: &str, project: &str, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    for path in [template_path(root, org, project, id), runs_path(root, org, project, id)] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

/// Appends one run to a template's history, newest first, keeping the
/// most recent `MAX_RUNS`. The template file itself is never touched -
/// only `save` (a successful prove) writes it.
pub fn append_run(root: &Path, org: &str, project: &str, id: &str, r: RunRecord) -> Result<(), String> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    let mut runs = load_runs(root, org, project, id)?;
    runs.insert(0, r);
    runs.truncate(MAX_RUNS);
    let dir = templates_dir(root, org, project);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&runs).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&runs_path(root, org, project, id), &json)
}
