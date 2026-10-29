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

/// Said when an Auto Run item is a link or shortcut: the update will not
/// follow it, and will not pretend it copied it.
pub const ITEM_IS_A_LINK: &str = "an Auto Run folder or file is a link or shortcut, which the update does not follow";

/// Only these hold files that are written once and never changed in place
/// (a screenshot has a time-stamped name; a download is finished before it
/// is renamed), so only these are hard-linked. Everything else is copied: a
/// file rewritten in place would otherwise change the flat original too.
const LINKED_ITEMS: [&str; 2] = ["shots", "downloads"];

const STAGING_PREFIX: &str = ".envs-staging-";

/// How long the one-time folder rename waits for another program (often
/// antivirus, still scanning the files just written) to let go: about 15
/// seconds in all, once per install, and only when a hold is actually seen.
pub const MIGRATION_RENAME_WAITS_MS: [u64; 11] = [50, 100, 200, 400, 800, 1000, 1500, 2000, 3000, 3000, 3000];

/// A small file in the shared root counting copy attempts that failed one
/// after another; gone once a start succeeds.
const FAILED_STARTS_FILE: &str = "migration-failed-starts.txt";

fn failed_starts(shared: &Path) -> u32 {
    std::fs::read_to_string(shared.join(FAILED_STARTS_FILE)).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0)
}

fn record_failed_start(shared: &Path) -> u32 {
    let n = failed_starts(shared) + 1;
    let _ = std::fs::create_dir_all(shared);
    if let Err(e) = std::fs::write(shared.join(FAILED_STARTS_FILE), n.to_string()) {
        crate::applog::debug(format!("Auto Run: the failed-start count could not be saved: {e}"));
    }
    n
}

fn clear_failed_starts(shared: &Path) {
    let _ = std::fs::remove_file(shared.join(FAILED_STARTS_FILE));
}

/// The start-up warning for a copy that did not finish. From the third
/// failed start in a row it says so and what usually causes it. Names no
/// path (`why` is empty when the reason would have).
pub fn failure_warning(why: &str, starts_in_a_row: u32) -> String {
    let mut line = String::from("Auto Run: the per-environment copy did not finish and will be tried again at the next start");
    if !why.is_empty() {
        line.push_str(&format!(" ({why})"));
    }
    if starts_in_a_row >= 3 {
        line.push_str(&format!(
            ". It has failed {starts_in_a_row} starts in a row; another program holding the files (often antivirus) is the usual cause"
        ));
    }
    line
}

/// Whether an error string is safe to log: it names no path.
pub fn path_free(message: &str) -> bool {
    !message.contains(['\\', '/'])
}

/// `Ok(true)` when `path` exists. Only "not found" is absence: any other
/// error (a permission problem, a broken link) must stop the update rather
/// than read as "nothing there".
fn exists(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

/// Copies `from` (a file or a folder) to `to`. `link` hard-links each file
/// and falls back to a copy when the file system will not link. A link, a
/// junction or any entry that cannot be read fails the copy. `skip` is
/// asked about each file with its path relative to the whole Auto Run root
/// (`rel`), so it can tell the export record from other project files.
fn copy_tree(from: &Path, to: &Path, rel: &Path, link: bool, skip: &dyn Fn(&Path) -> bool) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(from).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            copy_tree(&entry.path(), &to.join(&name), &rel.join(&name), link, skip)?;
        }
    } else if meta.is_file() {
        if skip(rel) {
            return Ok(());
        }
        if link && std::fs::hard_link(from, to).is_ok() {
            return Ok(());
        }
        std::fs::copy(from, to).map_err(|e| e.to_string())?;
    } else {
        return Err(ITEM_IS_A_LINK.to_string());
    }
    Ok(())
}

/// Copies the flat root's per-environment items into `envs_dir/<id>/` for
/// each id, and returns how many top-level items were copied. `Every`
/// items are real copies into every id; `ActiveOnly` items (history) go to
/// `active` only (the first id when `active` is `None` or not listed). Of
/// those only the write-once ones (`LINKED_ITEMS`) are hard-linked, so a
/// large `shots/` is not written out again. The export record goes to the
/// active environment only. `flat` is never changed. Anything unreadable,
/// and any link, fails the copy.
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
        if !exists(&from)? {
            continue;
        }
        match how {
            CopyTo::Every => {
                for id in ids {
                    let keep_export = target.as_deref() == Some(id.as_str());
                    let skip = move |rel: &Path| !keep_export && is_export_record(rel);
                    copy_tree(&from, &envs_dir.join(id).join(name), Path::new(name), false, &skip)?;
                }
                copied += 1;
            }
            CopyTo::ActiveOnly => {
                if let Some(id) = &target {
                    let link = LINKED_ITEMS.contains(&name);
                    copy_tree(&from, &envs_dir.join(id).join(name), Path::new(name), link, &|_| false)?;
                    copied += 1;
                }
            }
        }
    }
    Ok(copied)
}

/// The file operations `migrate_with` does, passed in so a test can make
/// one of them fail.
pub struct MigrationOps<'a> {
    pub fan_out: &'a dyn Fn(&Path, &Path, &[String], Option<&str>) -> Result<usize, String>,
    pub rename: &'a mut dyn FnMut(&Path, &Path) -> std::io::Result<()>,
    pub sleep: &'a mut dyn FnMut(std::time::Duration),
}

/// Gives each environment its own copy of the flat Auto Run data, once.
/// `Ok(None)` when `envs/` already exists. The copy is built in a staging
/// folder and renamed into place, so `envs/` never exists half-built; on
/// any error the staging folder goes and the caller keeps the flat layout
/// for the session. The originals stay as a backup.
pub fn migrate_once(shared: &Path) -> Result<Option<MigrationReport>, String> {
    migrate_with(
        shared,
        MigrationOps { fan_out: &fan_out, rename: &mut |a, b| std::fs::rename(a, b), sleep: &mut std::thread::sleep },
    )
}

/// `migrate_once` with its file operations passed in.
pub fn migrate_with(shared: &Path, ops: MigrationOps) -> Result<Option<MigrationReport>, String> {
    let envs = shared.join(ENVS_DIR);
    if exists(&envs)? {
        if !envs.is_dir() {
            return Err("the per-environment folder name is taken by a file".to_string());
        }
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
    let mut any = false;
    for (name, _) in PER_ENV_ITEMS {
        any |= exists(&shared.join(name))?;
    }
    if !any {
        std::fs::create_dir_all(&envs).map_err(|e| e.to_string())?;
        return Ok(Some(MigrationReport { environments: 0, items: 0 }));
    }
    let (ids, active) = match crate::environments::read_ids(shared)? {
        Some(found) => found,
        None => (vec![SEED_ID.to_string()], SEED_ID.to_string()),
    };
    let staging = shared.join(format!("{STAGING_PREFIX}{}", crate::cache::now_ms()));
    let built = (ops.fan_out)(shared, &staging, &ids, Some(&active))
        .and_then(|items| {
            crate::ai_tools::rename_retrying_waits(&staging, &envs, ops.rename, ops.sleep, &MIGRATION_RENAME_WAITS_MS)
                .map(|_| items)
        });
    match built {
        Ok(items) => Ok(Some(MigrationReport { environments: ids.len(), items })),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

/// What start-up found, and which layout is in charge.
#[derive(Debug, Default)]
pub struct StartUp {
    /// The per-environment layout is in charge (`envs/` exists).
    pub per_env: bool,
    pub report: Option<MigrationReport>,
    /// The copy failed; the sentence names no path (it is left out when it would).
    pub failed: Option<String>,
    /// A development build left the copy alone.
    pub skipped: bool,
    /// How many starts in a row the copy has now failed (0 when it did not).
    pub failed_starts: u32,
}

/// Whether this build may run the one-time copy. A development build
/// shares the installed app's data folder, so it copies only when asked
/// (`TCM_MIGRATE_ENVS=1`); a release build always does.
pub fn migration_allowed(debug_build: bool, flag: Option<&str>) -> bool {
    !debug_build || flag == Some("1")
}

/// The copy (when `allowed`), then the layout choice: per-environment when
/// `envs/` exists, flat when it does not (a failed or skipped copy).
pub fn start_up(shared: &Path, allowed: bool) -> StartUp {
    let mut out = StartUp::default();
    if allowed {
        match migrate_once(shared) {
            Ok(r) => {
                out.report = r;
                clear_failed_starts(shared);
            }
            Err(e) => {
                out.failed = Some(if path_free(&e) { e } else { String::new() });
                out.failed_starts = record_failed_start(shared);
            }
        }
    } else {
        out.skipped = true;
    }
    out.per_env = is_per_env_layout(shared);
    if out.per_env {
        retry_seed(shared);
    }
    out
}

/// Hands the seed to the first environment made after the update: renames
/// `envs/_seed` to `envs/<env_id>`. False when there is no seed. An
/// existing EMPTY target gives way; one holding anything is an error, so
/// the seed stays for a later try.
pub fn seed_into(shared: &Path, env_id: &str) -> Result<bool, String> {
    seed_into_with(shared, env_id, &mut |a, b| std::fs::rename(a, b), &mut std::thread::sleep)
}

/// `seed_into` with the rename and the wait passed in.
pub fn seed_into_with(
    shared: &Path,
    env_id: &str,
    rename: &mut dyn FnMut(&Path, &Path) -> std::io::Result<()>,
    sleep: &mut dyn FnMut(std::time::Duration),
) -> Result<bool, String> {
    if !crate::environments::valid_id(env_id) {
        return Err(format!("\"{env_id}\" is not an environment id"));
    }
    let seed = env_root(shared, SEED_ID);
    if !seed.is_dir() {
        return Ok(false);
    }
    let target = env_root(shared, env_id);
    if target.exists() {
        std::fs::remove_dir(&target).map_err(|_| "the new environment's folder already holds data".to_string())?;
    }
    crate::ai_tools::rename_retrying(&seed, &target, rename, sleep)?;
    Ok(true)
}

/// What `retry_seed` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SeedHandover {
    /// Top-level items moved into the first environment.
    pub moved: usize,
    /// Items left in `_seed` because the environment already had one.
    pub kept: usize,
}

/// A seed a failed handover left in `_seed` reaches the first environment:
/// each top-level item it LACKS is moved over (one it has is never
/// overwritten), and `_seed` is removed once empty. Run at start-up; logs
/// (no path) and never fails.
pub fn retry_seed(shared: &Path) -> SeedHandover {
    let mut done = SeedHandover::default();
    let seed = env_root(shared, SEED_ID);
    if !seed.is_dir() {
        return done;
    }
    let Ok(Some((ids, _))) = crate::environments::read_ids(shared) else { return done };
    let Some(first) = ids.first() else { return done };
    let target = env_root(shared, first);
    if let Err(e) = std::fs::create_dir_all(&target) {
        crate::applog::warn(format!("Auto Run: the data the update copied could not be handed over yet: {e}"));
        return done;
    }
    let Ok(entries) = std::fs::read_dir(&seed) else { return done };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let to = target.join(&name);
        if std::fs::symlink_metadata(&to).is_ok() {
            done.kept += 1;
            continue;
        }
        let moved = crate::ai_tools::rename_retrying(
            &entry.path(),
            &to,
            &mut |a, b| std::fs::rename(a, b),
            &mut std::thread::sleep,
        );
        match moved {
            Ok(()) => done.moved += 1,
            Err(e) => {
                done.kept += 1;
                crate::applog::warn(format!("Auto Run: an item the update copied could not be handed over yet: {e}"));
            }
        }
    }
    // Only an empty seed goes.
    let _ = std::fs::remove_dir(&seed);
    if done.moved > 0 {
        crate::applog::info(format!("Auto Run: {} item(s) the update copied were handed to the first environment", done.moved));
    }
    if done.kept > 0 {
        crate::applog::warn(format!(
            "Auto Run: {} item(s) the update copied were kept aside because the first environment already had its own",
            done.kept
        ));
    }
    done
}
