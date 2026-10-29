//! Where Auto Run keeps its files, now that each environment has its own.
//!
//! `<appdata>/autorun/` is the SHARED root and
//! `<appdata>/autorun/envs/<env-id>/` is an ENVIRONMENT root. The stores
//! that belong to one environment take an environment root and are
//! unchanged; the stores that every environment shares (environments,
//! accounts, sessions, proposals, test files and a few machine files)
//! resolve the shared root from whatever root they are given
//! ([`shared_of`]), so a caller holding an environment root still reads and
//! writes shared data in the right place.

use std::path::{Path, PathBuf};

/// The folder under the shared root that holds one folder per environment.
pub const ENVS_DIR: &str = "envs";
/// The folder an update builds first and every new environment starts from.
pub const SEED_ID: &str = "_seed";
/// Where a removed environment's folder waits to be purged.
pub const REMOVED_DIR: &str = "removed-envs";

/// How an update carries a per-environment item into the environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyTo {
    /// Into every environment.
    Every,
    /// Into the active environment only (history is big and is that
    /// environment's own).
    ActiveOnly,
}

/// The top-level items that belong to one environment, and how the update
/// copies each.
pub const PER_ENV_ITEMS: [(&str, CopyTo); 14] = [
    ("scripts", CopyTo::Every),
    ("deleted-scripts", CopyTo::Every),
    ("orders", CopyTo::Every),
    ("projects", CopyTo::Every),
    ("templates", CopyTo::Every),
    ("fixtures", CopyTo::Every),
    ("flows", CopyTo::Every),
    ("runs", CopyTo::ActiveOnly),
    ("shots", CopyTo::ActiveOnly),
    ("downloads", CopyTo::ActiveOnly),
    ("reports", CopyTo::ActiveOnly),
    ("replays", CopyTo::ActiveOnly),
    ("explore", CopyTo::ActiveOnly),
    ("captures", CopyTo::ActiveOnly),
];

/// The top-level items every environment shares; they stay in the shared root.
pub const SHARED_ITEMS: [&str; 9] = [
    "environments.json",
    "accounts",
    "sessions",
    "proposals",
    "test-files",
    "test-made.json",
    "allow-written.json",
    "last-browser.txt",
    "accounts.json",
];

/// `projects/<anything>.pwexport.json`: the Playwright export record, which
/// the update copies to the export environment only.
pub fn is_export_record(rel: &Path) -> bool {
    let mut parts = rel.components();
    let first = parts.next().and_then(|c| c.as_os_str().to_str());
    let name = parts.next().and_then(|c| c.as_os_str().to_str());
    first == Some("projects")
        && parts.next().is_none()
        && name.is_some_and(|n| n.ends_with(".pwexport.json"))
}

/// A folder name under `envs/`: an environment id, or the seed.
pub fn valid_folder_id(id: &str) -> bool {
    id == SEED_ID || crate::environments::valid_id(id)
}

/// `Some(<shared>)` when `root` is `<shared>/envs/<valid folder id>`.
fn split_env_root(root: &Path) -> Option<(&Path, &str)> {
    let id = root.file_name()?.to_str()?;
    if !valid_folder_id(id) {
        return None;
    }
    let envs = root.parent()?;
    if envs.file_name()?.to_str()? != ENVS_DIR {
        return None;
    }
    Some((envs.parent()?, id))
}

/// The shared root for any root a store is handed: the shared root itself,
/// or the one an environment root sits under.
pub fn shared_of(root: &Path) -> PathBuf {
    match split_env_root(root) {
        Some((shared, _)) => shared.to_path_buf(),
        None => root.to_path_buf(),
    }
}

/// The folder id of an environment root (the seed included); `None` for the
/// shared root and for anything else.
pub fn env_id_of(root: &Path) -> Option<String> {
    split_env_root(root).map(|(_, id)| id.to_string())
}

/// `<shared>/envs/<env id>`. The id must already be a valid folder id.
pub fn env_root(shared: &Path, env_id: &str) -> PathBuf {
    shared.join(ENVS_DIR).join(env_id)
}

/// The sibling environment root when `root` is an environment root, and
/// `root` itself in the flat layout.
pub fn env_root_beside(root: &Path, env_id: &str) -> PathBuf {
    match split_env_root(root) {
        Some((shared, _)) if valid_folder_id(env_id) => env_root(shared, env_id),
        _ => root.to_path_buf(),
    }
}

/// Whether the per-environment layout is in force under `shared`.
pub fn is_per_env_layout(shared: &Path) -> bool {
    shared.join(ENVS_DIR).is_dir()
}

/// Every `envs/<valid folder id>` that exists, sorted.
pub fn env_roots(shared: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(shared.join(ENVS_DIR)) else { return vec![] };
    let mut roots: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| e.file_name().to_str().is_some_and(valid_folder_id))
        .map(|e| e.path())
        .collect();
    roots.sort();
    roots
}
