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

/// What the one-time copy did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub environments: usize,
    pub items: usize,
}

/// The one sentence start-up logs after the copy. Names no path.
pub fn migration_log_line(r: &MigrationReport) -> String {
    format!("Auto Run: per-environment data set up for {} environment(s), {} item(s) copied", r.environments, r.items)
}

const STAGING_PREFIX: &str = ".envs-staging-";

/// Copies `from` (a file or a folder) to `to`. `link` hard-links each file
/// and falls back to a copy when the file system will not link. Symbolic
/// links and other odd entries are skipped. `skip` is asked about each file
/// with its path relative to the whole Auto Run root (`rel`), so it can tell
/// the export record from other project files.
fn copy_tree(from: &Path, to: &Path, rel: &Path, link: bool, skip: &dyn Fn(&Path) -> bool) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(from).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
            let name = entry.file_name();
            copy_tree(&entry.path(), &to.join(&name), &rel.join(&name), link, skip)?;
        }
    } else if meta.is_file() && !skip(rel) {
        if link && std::fs::hard_link(from, to).is_ok() {
            return Ok(());
        }
        std::fs::copy(from, to).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Copies the flat root's per-environment items into `envs_dir/<id>/` for
/// each id, and returns how many top-level items were copied. `Every`
/// items are real copies into every id; `ActiveOnly` items (history) go to
/// `active` only (the first id when `active` is `None` or not listed) and
/// are hard-linked file by file where the file system allows, so a large
/// `shots/` is not written out again. The export record goes to the active
/// environment only. `flat` is never changed.
pub fn fan_out(flat: &Path, envs_dir: &Path, ids: &[String], active: Option<&str>) -> Result<usize, String> {
    if let Some(bad) = ids.iter().find(|i| !valid_folder_id(i)) {
        return Err(format!("\"{bad}\" is not an environment id"));
    }
    let target = active
        .filter(|a| ids.iter().any(|i| i == a))
        .or_else(|| ids.first().map(String::as_str))
        .map(str::to_string);
    for id in ids {
        std::fs::create_dir_all(envs_dir.join(id)).map_err(|e| e.to_string())?;
    }
    let mut copied = 0;
    for (name, how) in PER_ENV_ITEMS {
        let from = flat.join(name);
        if std::fs::symlink_metadata(&from).is_err() {
            continue;
        }
        match how {
            CopyTo::Every => {
                for id in ids {
                    let keep_export = target.as_deref() == Some(id.as_str());
                    let skip = move |rel: &Path| !keep_export && is_export_record(rel);
                    copy_tree(&from, &envs_dir.join(id).join(name), Path::new(name), false, &skip)?;
                }
            }
            CopyTo::ActiveOnly => {
                if let Some(id) = &target {
                    copy_tree(&from, &envs_dir.join(id).join(name), Path::new(name), true, &|_| false)?;
                }
            }
        }
        copied += 1;
    }
    Ok(copied)
}

/// Gives each environment its own copy of the flat Auto Run data, once.
/// `Ok(None)` when `envs/` already exists. The copy is built in a staging
/// folder and renamed into place, so `envs/` never exists half-built; on
/// any error the staging folder goes and the caller keeps the flat layout
/// for the session. The originals stay as a backup.
pub fn migrate_once(shared: &Path) -> Result<Option<MigrationReport>, String> {
    let envs = shared.join(ENVS_DIR);
    if envs.exists() {
        return Ok(None);
    }
    if let Ok(entries) = std::fs::read_dir(shared) {
        for entry in entries.flatten() {
            if entry.file_name().to_str().is_some_and(|n| n.starts_with(STAGING_PREFIX)) {
                if let Err(e) = std::fs::remove_dir_all(entry.path()) {
                    crate::applog::warn(format!("Auto Run: a leftover half-built copy could not be removed: {e}"));
                }
            }
        }
    }
    if !PER_ENV_ITEMS.iter().any(|(name, _)| std::fs::symlink_metadata(shared.join(name)).is_ok()) {
        std::fs::create_dir_all(&envs).map_err(|e| e.to_string())?;
        return Ok(Some(MigrationReport { environments: 0, items: 0 }));
    }
    let (ids, active) = match crate::environments::read_ids(shared)? {
        Some(found) => found,
        None => (vec![SEED_ID.to_string()], SEED_ID.to_string()),
    };
    let staging = shared.join(format!("{STAGING_PREFIX}{}", crate::cache::now_ms()));
    let built = fan_out(shared, &staging, &ids, Some(&active))
        .and_then(|items| std::fs::rename(&staging, &envs).map(|_| items).map_err(|e| e.to_string()));
    match built {
        Ok(items) => Ok(Some(MigrationReport { environments: ids.len(), items })),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

/// Hands the seed to the first environment made after the update: renames
/// `envs/_seed` to `envs/<env_id>`. Does nothing (false) when there is no
/// seed or the target already holds anything.
pub fn seed_into(shared: &Path, env_id: &str) -> Result<bool, String> {
    if !crate::environments::valid_id(env_id) {
        return Err(format!("\"{env_id}\" is not an environment id"));
    }
    let seed = env_root(shared, SEED_ID);
    if !seed.is_dir() {
        return Ok(false);
    }
    let target = env_root(shared, env_id);
    if target.exists() {
        // An empty folder (made by a selection) gives way; one with
        // anything in it is left alone.
        if std::fs::remove_dir(&target).is_err() {
            return Ok(false);
        }
    }
    std::fs::rename(&seed, &target).map_err(|e| e.to_string())?;
    Ok(true)
}
