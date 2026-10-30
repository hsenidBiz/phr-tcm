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

/// Said when a write holds the root of an environment that is no longer the
/// active one: the person switched while it was being made.
pub const ENVIRONMENT_SWITCHED: &str = "the environment was switched while this was being saved - nothing was saved; read it again in the environment now active";

/// Held for reading by every write into an environment root (see [`guard`])
/// and for writing by a switch around the move itself, so a switch waits for
/// the writes in flight and a write that starts after it sees the new
/// environment.
///
/// LOCK RULE: never call a store write while holding this lock in any mode.
/// `guard` takes the read lock, and a second read lock taken while a switch
/// waits for the write lock can deadlock.
pub fn switch_lock() -> &'static std::sync::RwLock<()> {
    static LOCK: std::sync::RwLock<()> = std::sync::RwLock::new(());
    &LOCK
}

/// The environment roots other than the active one that may be written to
/// right now (the copy tasks, the export environment's records), one entry
/// per hold.
static ALLOWED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// A hold on one non-active environment root that lets writes through
/// [`guard`] until it is dropped.
#[must_use = "writes into the root are refused again as soon as this is dropped"]
pub struct AllowedTarget(PathBuf);

impl Drop for AllowedTarget {
    fn drop(&mut self) {
        let mut held = ALLOWED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = held.iter().position(|p| *p == self.0) {
            held.swap_remove(at);
        }
    }
}

/// Let writes into `root` through [`guard`] while the returned value lives,
/// though `root` is not the active environment's.
pub fn allow_writes_to(root: &Path) -> AllowedTarget {
    ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).push(root.to_path_buf());
    AllowedTarget(root.to_path_buf())
}

/// Takes the switch lock for reading and checks that `root` may be written:
/// an environment root must be the active one (or allowed); anything else,
/// the flat layout's folder included, is never refused. Keep the guard for
/// the length of the write.
pub fn guard(root: &Path) -> Result<std::sync::RwLockReadGuard<'static, ()>, String> {
    let held = switch_lock().read().unwrap_or_else(|e| e.into_inner());
    if in_removed_area(root) {
        return Err(REMOVED_READ_ONLY.to_string());
    }
    if env_id_of(root).is_none() {
        return Ok(held);
    }
    if crate::autorun::store::configured_root().as_deref() == Some(root) {
        return Ok(held);
    }
    if ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|p| p == root) {
        return Ok(held);
    }
    Err(ENVIRONMENT_SWITCHED.to_string())
}

/// Whether a change announced for `changed` concerns what the person is
/// looking at: anything under the active environment's root, or under the
/// shared root but not inside another environment's folder (`envs/`). A
/// change to another environment's folder (a copy, an edit with a root from
/// before a switch) is not on screen. Judged by position, not equality: the
/// announcers pass nested folders (`<shared>/test-files/<slug>`).
pub fn is_on_screen(changed: &Path) -> bool {
    match crate::autorun::store::configured_root() {
        Some(active) => {
            if changed.starts_with(&active) {
                return true;
            }
            let shared = shared_of(&active);
            changed.starts_with(&shared)
                && !changed.starts_with(shared.join(ENVS_DIR))
                && !changed.starts_with(shared.join(REMOVED_DIR))
        }
        // No environment is chosen yet: nothing is on screen to be wrong.
        None => true,
    }
}

// ---- the removed area -------------------------------------------------

/// Said when a write is aimed at the folder of a removed environment.
pub const REMOVED_READ_ONLY: &str =
    "a removed environment's folder is only kept to be copied from - nothing is saved into it";

/// Whether `root` is a folder under `removed-envs/`.
fn in_removed_area(root: &Path) -> bool {
    root.parent().and_then(Path::file_name).is_some_and(|n| n == REMOVED_DIR)
}

/// How long a removed environment's folder is kept.
pub const REMOVED_KEPT_DAYS: u64 = 30;
/// The record inside a retired folder.
const REMOVED_RECORD: &str = "removed.json";

/// Said when the environment to be removed is the active one.
pub const REMOVE_ACTIVE: &str = "the active environment cannot be removed - switch to another one first";
/// Said when something is writing into the environment's folder right now.
pub const REMOVE_BUSY: &str = "that environment's Auto Run data is being used right now - try again in a moment";
/// Said when the environment is the Playwright export environment.
pub const REMOVE_EXPORT_ENVIRONMENT: &str =
    "that is the Playwright export environment - name another one for the export before removing it";

/// A removed environment's folder, as the webview lists it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RemovedFolder {
    /// The folder name, `<env-id>-<ms>`.
    pub key: String,
    pub env_id: String,
    pub name: String,
    pub removed_at_ms: u64,
    /// `"removed"`, `"replaced"` or `"replaced by an import"`.
    pub reason: String,
}

/// What `removed.json` holds.
#[derive(serde::Serialize, serde::Deserialize)]
struct RemovedRecord {
    env_id: String,
    name: String,
    removed_at_ms: u64,
    reason: String,
}

/// Whether `id` is the environment the Playwright export writes to.
///
/// HOOK FOR TASK 7: the export environment does not exist yet, so no
/// environment is it. Task 7 makes this read the named export environment,
/// and [`removal_refusal`] then refuses to remove it. It must NOT take the
/// environments lock or resolve/select a root: `remove_env` calls the
/// refusal before taking that lock, but `retire` calls it under the switch
/// lock, and the lock order is switch, then environments.
pub fn is_export_environment(_shared: &Path, _id: &str) -> bool {
    false
}

/// Whether a copy or an export is writing into `root` right now.
pub(crate) fn is_held_for_writes(root: &Path) -> bool {
    ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|p| p == root)
}

/// The sentence that says why environment `id` cannot be removed now (it is
/// active, something is writing into its folder, or it is the export
/// environment), or `None`.
pub fn removal_refusal(shared: &Path, id: &str) -> Option<&'static str> {
    if !valid_folder_id(id) || id == SEED_ID {
        return None;
    }
    let folder = env_root(shared, id);
    // `selected_root`, never `configured_root`: that one reads the
    // environments file, and a caller may hold its lock.
    if crate::autorun::store::selected_root().as_deref() == Some(folder.as_path()) {
        return Some(REMOVE_ACTIVE);
    }
    if ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|p| *p == folder) {
        return Some(REMOVE_BUSY);
    }
    if is_export_environment(shared, id) {
        return Some(REMOVE_EXPORT_ENVIRONMENT);
    }
    None
}

/// The environment id and the millisecond stamp of a retired folder's name.
fn parse_key(key: &str) -> Option<(String, u64)> {
    let (id, ms) = key.rsplit_once('-')?;
    if !crate::environments::valid_id(id) {
        return None;
    }
    Some((id.to_string(), ms.parse().ok()?))
}

/// Moves `envs/<id>` to `removed-envs/<id>-<now_ms>` and records who it
/// was. `Ok(None)` when there was no folder. Refused (and nothing moved)
/// while that environment is active, busy or the export environment: the
/// move waits for the writes in flight (the switch lock) and, once it has
/// started, a write aimed at the old folder is refused because that folder
/// is no longer the active one.
pub fn retire(shared: &Path, env_id: &str, name: &str, reason: &str, now_ms: u64) -> Result<Option<String>, String> {
    if !valid_folder_id(env_id) || env_id == SEED_ID {
        return Err(format!("\"{env_id}\" is not an environment id"));
    }
    let from = env_root(shared, env_id);
    match std::fs::symlink_metadata(&from) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    let switching = switch_lock().write().unwrap_or_else(|e| e.into_inner());
    if let Some(why) = removal_refusal(shared, env_id) {
        return Err(why.to_string());
    }
    retire_held(&switching, shared, env_id, name, reason, now_ms, &[50, 100, 200])
}

/// [`retire`] for a caller that already holds the switch lock for writing
/// (`held` is the proof) and has decided the folder may go: it does not
/// look at whether the environment is active, so a copy can set aside what
/// it replaces. `waits_ms` are the pauses between tries of a rename another
/// process holds.
pub(crate) fn retire_held(
    _held: &std::sync::RwLockWriteGuard<'static, ()>,
    shared: &Path,
    env_id: &str,
    name: &str,
    reason: &str,
    now_ms: u64,
    waits_ms: &[u64],
) -> Result<Option<String>, String> {
    let from = env_root(shared, env_id);
    match std::fs::symlink_metadata(&from) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    let key = format!("{env_id}-{now_ms}");
    let area = shared.join(REMOVED_DIR);
    let to = area.join(&key);
    if std::fs::symlink_metadata(&to).is_ok() {
        return Err("a folder for that removal is already there".to_string());
    }
    std::fs::create_dir_all(&area).map_err(|e| e.to_string())?;
    let record = RemovedRecord {
        env_id: env_id.to_string(),
        name: name.to_string(),
        removed_at_ms: now_ms,
        reason: reason.to_string(),
    };
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    let record_path = from.join(REMOVED_RECORD);
    crate::ai_tools::atomic_write(&record_path, &text)?;
    // Short waits: this runs on the command thread. A rename that still
    // fails is retried by `retire_unlisted` at the next start.
    if let Err(e) = crate::ai_tools::rename_retrying_waits(
        &from,
        &to,
        &mut |a, b| std::fs::rename(a, b),
        &mut |d| std::thread::sleep(d),
        waits_ms,
    ) {
        let _ = std::fs::remove_file(&record_path);
        return Err(e);
    }
    Ok(Some(key))
}

/// Retires every `envs/<id>` folder whose id is not in `environments.json`
/// (a removal whose move failed, or a folder brought back by a late write)
/// under the usual naming and 30-day rules, and returns how many. The seed
/// and anything that is not an environment id are skipped. Nothing happens
/// when the environments file cannot be read or is not there yet.
pub fn retire_unlisted(shared: &Path, now_ms: u64) -> usize {
    let Some(listed) = crate::environments::listed_ids(shared) else { return 0 };
    let mut retired = 0;
    for folder in env_roots(shared) {
        let Some(id) = env_id_of(&folder) else { continue };
        if id == SEED_ID || listed.iter().any(|l| *l == id) {
            continue;
        }
        match retire(shared, &id, "Removed environment", "removed", now_ms) {
            Ok(Some(_)) => retired += 1,
            Ok(None) => {}
            Err(e) => crate::applog::warn(format!("Auto Run: a removed environment's folder stayed in place: {e}")),
        }
    }
    retired
}

/// The retired folders still within 30 days that have a readable record,
/// newest first.
pub fn removed_folders(shared: &Path, now_ms: u64) -> Vec<RemovedFolder> {
    let kept = REMOVED_KEPT_DAYS * crate::autorun::store::DAY_MS;
    let Ok(entries) = std::fs::read_dir(shared.join(REMOVED_DIR)) else { return vec![] };
    let mut out: Vec<RemovedFolder> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let key = e.file_name().to_str()?.to_string();
            let (env_id, ms) = parse_key(&key)?;
            if now_ms.saturating_sub(ms) > kept {
                return None;
            }
            let text = std::fs::read_to_string(e.path().join(REMOVED_RECORD)).ok()?;
            let rec: RemovedRecord = serde_json::from_str(&text).ok()?;
            if rec.env_id != env_id {
                return None;
            }
            Some(RemovedFolder { key, env_id, name: rec.name, removed_at_ms: ms, reason: rec.reason })
        })
        .collect();
    out.sort_by(|a, b| b.removed_at_ms.cmp(&a.removed_at_ms).then_with(|| a.key.cmp(&b.key)));
    out
}

/// The folder of a retired environment, only for a key [`removed_folders`]
/// would list now. A key never becomes a path otherwise. The folder is for
/// reading and copying from; [`guard`] refuses every write into it.
pub fn removed_root(shared: &Path, key: &str, now_ms: u64) -> Option<PathBuf> {
    removed_folders(shared, now_ms).iter().find(|f| f.key == key)?;
    Some(shared.join(REMOVED_DIR).join(key))
}

/// Deletes the retired folders older than 30 days and returns how many.
/// A folder whose name is not `<env id>-<ms>` is left alone.
pub fn purge_removed(shared: &Path, now_ms: u64) -> usize {
    let kept = REMOVED_KEPT_DAYS * crate::autorun::store::DAY_MS;
    let Ok(entries) = std::fs::read_dir(shared.join(REMOVED_DIR)) else { return 0 };
    let mut purged = 0;
    for e in entries.flatten() {
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Some((_, ms)) = e.file_name().to_str().and_then(parse_key) else { continue };
        if now_ms.saturating_sub(ms) > kept {
            match std::fs::remove_dir_all(e.path()) {
                Ok(()) => purged += 1,
                Err(err) => {
                    crate::applog::warn(format!("Auto Run: a removed environment's folder stayed behind: {err}"))
                }
            }
        }
    }
    purged
}

/// The refusal as an I/O error, for the helpers that stand in for `std::fs`
/// calls: its text is exactly [`ENVIRONMENT_SWITCHED`].
fn refused(sentence: String) -> std::io::Error {
    std::io::Error::other(sentence)
}

/// `std::fs::create_dir_all(dir)` for a folder under `root`, refused (and
/// nothing made) when `root` stopped being the active environment.
pub fn create_dir_all(root: &Path, dir: impl AsRef<Path>) -> std::io::Result<()> {
    let _held = guard(root).map_err(refused)?;
    std::fs::create_dir_all(dir)
}

/// `std::fs::remove_file(path)` for a file under `root`, refused when `root`
/// stopped being the active environment.
pub fn remove_file(root: &Path, path: &Path) -> std::io::Result<()> {
    let _held = guard(root).map_err(refused)?;
    std::fs::remove_file(path)
}

/// [`write_file`] for a caller that already holds [`guard`] for `root`
/// (a bundle of files that must land as one): the guard is the proof.
pub fn write_file_held(_held: &std::sync::RwLockReadGuard<'static, ()>, path: &Path, text: &str) -> Result<(), String> {
    crate::ai_tools::atomic_write(path, text)
}

/// Writes `text` to `path` inside `root` (atomically), refused when `root`
/// stopped being the active environment.
pub fn write_file(root: &Path, path: &Path, text: &str) -> Result<(), String> {
    let _held = guard(root)?;
    crate::ai_tools::atomic_write(path, text)
}

/// Writes `bytes` to `path` inside `root`, refused when `root` stopped
/// being the active environment.
pub fn write_bytes(root: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let _held = guard(root)?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

/// Whether an error string is safe to log: it names no path.
pub fn path_free(message: &str) -> bool {
    !message.contains(['\\', '/'])
}

/// `Ok(true)` when `path` exists. Only "not found" is absence: any other
/// error (a permission problem, a broken link) must stop the update rather
/// than read as "nothing there".
pub(crate) fn exists(path: &Path) -> Result<bool, String> {
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
pub(crate) fn copy_tree(from: &Path, to: &Path, rel: &Path, link: bool, skip: &dyn Fn(&Path) -> bool) -> Result<(), String> {
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
