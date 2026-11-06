//! What is known about an API template or fixture on THIS environment.
//!
//! A definition is shared by every environment, but whether it has worked
//! here is not. Each environment keeps `api-state/<slug>.json`: per item,
//! the content hash the entry is about, when it was confirmed and the last
//! failure. An entry read against a different current hash says nothing, so
//! a changed definition is unconfirmed everywhere without writing into any
//! other environment's folder.

use super::fixture::Fixture;
use super::flow::Flow;
use super::library::{fixture_hash, state_path, template_hash, ApiKind};
use super::store::SavedTemplate;
use super::ApiTemplate;
use crate::autorun::roots;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct HereFailure {
    pub at: String,
    pub step: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Here {
    Confirmed { last_failure: Option<HereFailure> },
    Unconfirmed,
    Failed { failure: HereFailure },
}

/// Who ran the item: the assistant's retry can flip a confirmed item to
/// Failed here; a plain run only records the strike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrySource {
    Assistant,
    Plain,
}

#[derive(Default, Serialize, Deserialize)]
pub struct EnvApiState {
    #[serde(default)]
    pub templates: BTreeMap<String, ItemState>,
    #[serde(default)]
    pub fixtures: BTreeMap<String, ItemState>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ItemState {
    pub hash: String,
    /// Fixtures: each step template's hash when the fixture last worked.
    #[serde(default)]
    pub uses: BTreeMap<String, String>,
    #[serde(default)]
    pub confirmed_at: Option<String>,
    #[serde(default)]
    pub failure: Option<HereFailure>,
    #[serde(default)]
    pub failed_here: bool,
}

/// Held by every load-modify-save transition, so two records for one
/// project never lose each other.
fn state_lock() -> std::sync::MutexGuard<'static, ()> {
    static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

/// The recorded state; a missing file reads as nothing recorded, and so
/// does an unreadable one (logged, without the path).
pub fn load_state(root: &Path, org: &str, project: &str) -> EnvApiState {
    let text = match std::fs::read_to_string(state_path(root, org, project)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return EnvApiState::default(),
        Err(e) => {
            crate::applog::warn(format!("The API confirmation state could not be read ({e}); treating nothing as recorded"));
            return EnvApiState::default();
        }
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        crate::applog::warn(format!("The API confirmation state is not valid ({e}); treating nothing as recorded"));
        EnvApiState::default()
    })
}

/// Load, change and write back under the state lock and the switch guard.
/// The guard is taken once and kept across the load, so a backup-import
/// fold cannot be overwritten by a stale read.
fn update(root: &Path, org: &str, project: &str, change: impl FnOnce(&mut EnvApiState)) -> Result<(), String> {
    let _lock = state_lock();
    let held = roots::guard(root)?;
    let mut state = load_state(root, org, project);
    change(&mut state);
    let path = state_path(root, org, project);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    roots::write_file_held(&held, &path, &text)
}

/// The derivation both kinds share. `uses_current` is whether every
/// template a fixture builds from is still the version it worked with
/// (always true for a template).
fn derive(entry: Option<&ItemState>, hash: &str, uses_current: bool) -> Here {
    let Some(entry) = entry.filter(|e| e.hash == hash && uses_current) else {
        return Here::Unconfirmed;
    };
    if entry.failed_here {
        return match &entry.failure {
            Some(failure) => Here::Failed { failure: failure.clone() },
            None => Here::Unconfirmed,
        };
    }
    if entry.confirmed_at.is_some() {
        return Here::Confirmed { last_failure: entry.failure.clone() };
    }
    Here::Unconfirmed
}

/// Whether every template the fixture's steps use is, by `templates`, the
/// version the entry recorded (a missing or gone template is not).
fn uses_current(entry: Option<&ItemState>, f: &Fixture, templates: &dyn Fn(&str) -> Option<ApiTemplate>) -> bool {
    entry.is_some_and(|e| {
        f.steps.iter().all(|s| {
            templates(&s.template).is_some_and(|t| e.uses.get(&s.template).is_some_and(|h| *h == template_hash(&t)))
        })
    })
}

pub fn template_here(state: &EnvApiState, t: &ApiTemplate) -> Here {
    derive(state.templates.get(&t.id), &template_hash(t), true)
}

pub fn fixture_here(state: &EnvApiState, f: &Fixture, templates: &dyn Fn(&str) -> Option<ApiTemplate>) -> Here {
    let entry = state.fixtures.get(&f.id);
    derive(entry, &fixture_hash(f), uses_current(entry, f, templates))
}

/// A flow is Confirmed when at least one of the given templates performs
/// one of its stages and every such template is Confirmed here; otherwise
/// Unconfirmed. It is never Failed here.
pub fn flow_here(flow: &Flow, templates: &[SavedTemplate]) -> Here {
    let mut on_flow = templates
        .iter()
        .filter(|t| t.template.stage.as_ref().is_some_and(|r| r.flow == flow.id && flow.stages.iter().any(|s| s.id == r.id)))
        .peekable();
    if on_flow.peek().is_some() && on_flow.all(|t| matches!(t.here, Here::Confirmed { .. })) {
        Here::Confirmed { last_failure: None }
    } else {
        Here::Unconfirmed
    }
}

fn confirmed_entry(hash: String, uses: BTreeMap<String, String>) -> ItemState {
    ItemState { hash, uses, confirmed_at: Some(crate::applog::stamp()), failure: None, failed_here: false }
}

/// The failure transition both kinds share, against the state derived
/// before the write.
fn failed_entry(
    entry: Option<&ItemState>,
    before: &Here,
    hash: String,
    uses: BTreeMap<String, String>,
    failure: HereFailure,
    source: TrySource,
) -> ItemState {
    match (before, entry) {
        (Here::Unconfirmed, _) | (_, None) => {
            ItemState { hash, uses, confirmed_at: None, failure: Some(failure), failed_here: true }
        }
        (Here::Failed { .. }, Some(e)) => ItemState { failure: Some(failure), ..e.clone() },
        (Here::Confirmed { last_failure }, Some(e)) => {
            if last_failure.is_some() && source == TrySource::Assistant {
                ItemState { confirmed_at: None, failed_here: true, failure: Some(failure), ..e.clone() }
            } else {
                ItemState { failure: Some(failure), ..e.clone() }
            }
        }
    }
}

fn uses_of(templates: &[ApiTemplate]) -> BTreeMap<String, String> {
    templates.iter().map(|t| (t.id.clone(), template_hash(t))).collect()
}

pub fn confirm_template(root: &Path, org: &str, project: &str, t: &ApiTemplate) -> Result<(), String> {
    update(root, org, project, |s| {
        s.templates.insert(t.id.clone(), confirmed_entry(template_hash(t), BTreeMap::new()));
    })
}

/// A fixture that worked: confirms it with the step templates it ran, and
/// each of those templates too.
pub fn confirm_fixture(root: &Path, org: &str, project: &str, f: &Fixture, ran: &[ApiTemplate]) -> Result<(), String> {
    update(root, org, project, |s| {
        s.fixtures.insert(f.id.clone(), confirmed_entry(fixture_hash(f), uses_of(ran)));
        for t in ran {
            s.templates.insert(t.id.clone(), confirmed_entry(template_hash(t), BTreeMap::new()));
        }
    })
}

pub fn record_template_failure(
    root: &Path,
    org: &str,
    project: &str,
    t: &ApiTemplate,
    failure: HereFailure,
    source: TrySource,
) -> Result<(), String> {
    update(root, org, project, |s| {
        let hash = template_hash(t);
        let entry = s.templates.get(&t.id);
        let before = derive(entry, &hash, true);
        let next = failed_entry(entry, &before, hash, BTreeMap::new(), failure, source);
        s.templates.insert(t.id.clone(), next);
    })
}

pub fn record_fixture_failure(
    root: &Path,
    org: &str,
    project: &str,
    f: &Fixture,
    templates: &[ApiTemplate],
    failure: HereFailure,
    source: TrySource,
) -> Result<(), String> {
    update(root, org, project, |s| {
        let hash = fixture_hash(f);
        let entry = s.fixtures.get(&f.id);
        let lookup = |id: &str| templates.iter().find(|t| t.id == id).cloned();
        let before = derive(entry, &hash, uses_current(entry, f, &lookup));
        let next = failed_entry(entry, &before, hash, uses_of(templates), failure, source);
        s.fixtures.insert(f.id.clone(), next);
    })
}

/// A saved template that carries a proof: confirms it here, and moves every
/// fixture here that was confirmed against the template's previous content
/// on to the new one (the fixture's own definition did not change).
pub fn after_template_saved(
    root: &Path,
    org: &str,
    project: &str,
    before: Option<&ApiTemplate>,
    saved: &ApiTemplate,
) -> Result<(), String> {
    if saved.proven.is_none() {
        return Ok(());
    }
    update(root, org, project, |s| {
        let now = template_hash(saved);
        s.templates.insert(saved.id.clone(), confirmed_entry(now.clone(), BTreeMap::new()));
        let Some(old) = before.map(template_hash) else { return };
        for entry in s.fixtures.values_mut() {
            if entry.uses.get(&saved.id) == Some(&old) {
                entry.uses.insert(saved.id.clone(), now.clone());
            }
        }
    })
}

/// Drops this environment's entry for a removed item. Flows keep none.
pub fn forget(root: &Path, org: &str, project: &str, kind: ApiKind, id: &str) -> Result<(), String> {
    let drop_from = |s: &mut EnvApiState| match kind {
        ApiKind::Templates => {
            s.templates.remove(id);
        }
        ApiKind::Fixtures => {
            s.fixtures.remove(id);
        }
        ApiKind::Flows => {}
    };
    if kind == ApiKind::Flows {
        return Ok(());
    }
    update(root, org, project, drop_from)
}
