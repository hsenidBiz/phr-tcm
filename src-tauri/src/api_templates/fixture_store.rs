//! Where fixtures and their run history are kept, beside the templates:
//!
//! ```text
//! fixtures/<project_slug>/<id>.json       the fixture (see fixture.rs)
//! fixtures/<project_slug>/<id>.runs.json  its run history, newest first
//! ```
//!
//! `<project_slug>` is the slug templates use (`autorun::recipe::project_slug`).

use super::fixture::{validate, Fixture, FixtureRun};
use super::store::MAX_RUNS;
use super::{store as template_store, valid_id};
use crate::autorun::recipe::project_slug;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn fixtures_dir(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("fixtures").join(project_slug(org, project))
}

fn fixture_path(root: &Path, org: &str, project: &str, id: &str) -> PathBuf {
    fixtures_dir(root, org, project).join(format!("{id}.json"))
}

fn runs_path(root: &Path, org: &str, project: &str, id: &str) -> PathBuf {
    fixtures_dir(root, org, project).join(format!("{id}.runs.json"))
}

fn invalid_id(id: &str) -> String {
    format!("'{id}' is not a valid fixture id")
}

/// A fixture together with its run history, as the tab lists it.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct SavedFixture {
    pub fixture: Fixture,
    pub runs: Vec<FixtureRun>,
}

fn load_runs(root: &Path, org: &str, project: &str, id: &str) -> Result<Vec<FixtureRun>, String> {
    match std::fs::read_to_string(runs_path(root, org, project, id)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|e| format!("that fixture's run history is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

/// Loads one fixture by id, or `None` if it has never been saved.
pub fn load(root: &Path, org: &str, project: &str, id: &str) -> Result<Option<Fixture>, String> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    match std::fs::read_to_string(fixture_path(root, org, project, id)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map(Some).map_err(|e| format!("that fixture is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Validates `f` against the project's saved templates and writes it,
/// atomically. Nothing is written when anything is refused.
pub fn save(root: &Path, org: &str, project: &str, f: &Fixture) -> Result<(), Vec<String>> {
    if !valid_id(&f.id) {
        return Err(vec![invalid_id(&f.id)]);
    }
    let lookup = |id: &str| template_store::load(root, org, project, id).ok().flatten();
    let flows = |id: &str| super::flow_store::load(root, org, project, id).ok().flatten();
    validate(f, &lookup, &flows)?;
    let dir = fixtures_dir(root, org, project);
    std::fs::create_dir_all(&dir).map_err(|e| vec![e.to_string()])?;
    let json = serde_json::to_string_pretty(f).map_err(|e| vec![e.to_string()])?;
    crate::ai_tools::atomic_write(&fixture_path(root, org, project, &f.id), &json).map_err(|e| vec![e])
}

/// Every saved fixture for this project with its history, sorted by name.
/// A file that does not parse is skipped and logged.
pub fn list(root: &Path, org: &str, project: &str) -> Result<Vec<SavedFixture>, String> {
    let entries = match std::fs::read_dir(fixtures_dir(root, org, project)) {
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
        let text = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                crate::applog::warn(format!("fixture file {name} could not be read, skipped: {e}"));
                continue;
            }
        };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let fixture: Fixture = match serde_json::from_str(text) {
            Ok(f) => f,
            Err(e) => {
                crate::applog::warn(format!("fixture file {name} does not parse, skipped: {e}"));
                continue;
            }
        };
        let runs = load_runs(root, org, project, id).unwrap_or_else(|e| {
            crate::applog::warn(format!("fixture {id}'s run history does not parse, treated as empty: {e}"));
            vec![]
        });
        out.push(SavedFixture { fixture, runs });
    }
    out.sort_by(|a, b| {
        (a.fixture.name.as_str(), a.fixture.id.as_str()).cmp(&(b.fixture.name.as_str(), b.fixture.id.as_str()))
    });
    Ok(out)
}

/// Appends one run to a fixture's history, newest first, keeping the most
/// recent `MAX_RUNS`. The fixture file itself is never touched.
pub fn append_run(root: &Path, org: &str, project: &str, id: &str, r: FixtureRun) -> Result<(), String> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    let mut runs = load_runs(root, org, project, id)?;
    runs.insert(0, r);
    runs.truncate(MAX_RUNS);
    std::fs::create_dir_all(fixtures_dir(root, org, project)).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&runs).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&runs_path(root, org, project, id), &json)
}

/// The outputs of the newest successful run, or `None` when the fixture
/// has never run successfully (failed runs are skipped).
pub fn current_outputs(root: &Path, org: &str, project: &str, id: &str) -> Option<BTreeMap<String, Value>> {
    load_runs(root, org, project, id).ok()?.into_iter().find(|r| r.ok).map(|r| r.outputs)
}

/// Removes a fixture and its run history: the person's, from the Fixtures
/// tab, as removing a template is. What its runs made stays in the record
/// of test-made drafts, for Clean up. A fixture that is not there is
/// already removed.
pub fn remove(root: &Path, org: &str, project: &str, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    for path in [fixture_path(root, org, project, id), runs_path(root, org, project, id)] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
