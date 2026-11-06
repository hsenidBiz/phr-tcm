//! Where the shared API library lives and how its items are told apart.
//!
//! Definitions (templates, fixtures, flows) are shared by every Auto Run
//! environment and live under `<shared>/api-library/<kind>/<slug>/`. What
//! stays in an environment root is only its run histories
//! (`<env>/templates|fixtures/<slug>/`) and its confirmation state
//! (`<env>/api-state/<slug>.json`, see `here`).
//!
//! The library folder is deliberately not `templates/`: the flat-layout
//! originals that the environment migration left in `<shared>/templates`
//! are a backup that is no longer read.

use super::fixture::Fixture;
use super::flow::Flow;
use super::ApiTemplate;
use crate::autorun::recipe::project_slug;
use crate::autorun::roots::shared_of;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const LIBRARY_DIR: &str = "api-library";
pub const STATE_DIR: &str = "api-state";
pub const REMOVED_API_DIR: &str = "removed-api";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiKind {
    Templates,
    Fixtures,
    Flows,
}

impl ApiKind {
    pub fn folder(self) -> &'static str {
        match self {
            ApiKind::Templates => "templates",
            ApiKind::Fixtures => "fixtures",
            ApiKind::Flows => "flows",
        }
    }
}

/// The shared folder holding this project's definitions of `kind`.
pub fn library_dir(root: &Path, kind: ApiKind, org: &str, project: &str) -> PathBuf {
    shared_of(root).join(LIBRARY_DIR).join(kind.folder()).join(project_slug(org, project))
}

/// This environment's run histories of `kind` (templates and fixtures only).
pub fn history_dir(root: &Path, kind: ApiKind, org: &str, project: &str) -> PathBuf {
    root.join(kind.folder()).join(project_slug(org, project))
}

/// This environment's confirmation state for the project.
pub fn state_path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join(STATE_DIR).join(format!("{}.json", project_slug(org, project)))
}

/// SHA-256 hex of the definition as JSON.
fn digest<T: serde::Serialize>(definition: &T) -> String {
    let text = serde_json::to_string(definition).unwrap_or_default();
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A template's content identity: everything but the proof the app writes.
pub fn template_hash(t: &ApiTemplate) -> String {
    let mut bare = t.clone();
    bare.proven = None;
    digest(&bare)
}

pub fn fixture_hash(f: &Fixture) -> String {
    digest(f)
}

/// A flow's content identity: everything but the sample the app writes.
pub fn flow_hash(f: &Flow) -> String {
    let mut bare = f.clone();
    bare.saved = None;
    digest(&bare)
}
