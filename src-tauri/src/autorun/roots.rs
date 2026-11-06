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

/// How an environment COPY (`env_copy`, `backup_import`) reads a
/// per-environment item. The one-time update itself puts every item under
/// the ACTIVE environment only, except the [`BY_RUN`] items, which go
/// wherever the run that made them names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyTo {
    /// What an environment copy carries: what describes the app.
    Every,
    /// History: an environment copy leaves it behind.
    ActiveOnly,
}

/// The top-level items that belong to one environment, and how an
/// environment copy treats each.
///
/// The API definitions (templates, fixtures, flows) live in the shared
/// library, not here: `templates` and `fixtures` stay in the list only
/// because they still hold this environment's run histories, which a copy
/// leaves behind and keeps in place. `flows` is not listed
/// ([`LEGACY_API_ITEMS`]). Accounts are not listed either: a copy never
/// touches them.
pub const PER_ENV_ITEMS: [(&str, CopyTo); 14] = [
    ("scripts", CopyTo::Every),
    ("deleted-scripts", CopyTo::Every),
    ("orders", CopyTo::Every),
    ("projects", CopyTo::Every),
    ("templates", CopyTo::ActiveOnly),
    ("fixtures", CopyTo::ActiveOnly),
    ("runs", CopyTo::ActiveOnly),
    ("shots", CopyTo::ActiveOnly),
    ("downloads", CopyTo::ActiveOnly),
    ("reports", CopyTo::ActiveOnly),
    ("replays", CopyTo::ActiveOnly),
    ("explore", CopyTo::ActiveOnly),
    ("captures", CopyTo::ActiveOnly),
    ("api-state", CopyTo::ActiveOnly),
];

/// The API definition folders the flat layout kept at the top of the Auto
/// Run folder. The one-time environment update still carries a never-migrated
/// flat folder's `flows` into the active environment (where the library fold
/// finds them); a copy never does.
pub const LEGACY_API_ITEMS: [&str; 3] = ["templates", "fixtures", "flows"];

/// Every top-level name the flat layout could hold for one environment:
/// [`PER_ENV_ITEMS`] plus the legacy `flows` folder.
pub(crate) fn flat_item_names() -> impl Iterator<Item = &'static str> {
    PER_ENV_ITEMS.iter().map(|(n, _)| *n).chain(LEGACY_API_ITEMS.iter().copied().filter(|n| !PER_ENV_ITEMS.iter().any(|(m, _)| m == n)))
}

/// A file name that is an API definition (`<id>.json`), not the run history
/// beside it (`<id>.runs.json`).
pub(crate) fn is_api_definition_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".json") && !n.ends_with(".runs.json")
}

/// The items the update places by the run that made them: a run, its report,
/// its downloads and the screenshots it names go to the environment the run
/// names (see [`place_runs`]).
pub const BY_RUN: [&str; 4] = ["runs", "reports", "downloads", "shots"];

/// The top-level items every environment shares; they stay in the shared root.
pub const SHARED_ITEMS: [&str; 12] = [
    "environments.json",
    "accounts",
    "sessions",
    "proposals",
    "test-files",
    "test-made.json",
    "allow-written.json",
    "last-browser.txt",
    "accounts.json",
    MIGRATION_NOTE_FILE,
    "api-library",
    "removed-api",
];

/// `projects/<anything>.pwexport.json`: the Playwright export record, which
/// the update copies to the export environment only.
pub fn is_export_record(rel: &Path) -> bool {
    let mut parts = rel.components();
    let first = parts.next().and_then(|c| c.as_os_str().to_str());
    let name = parts.next().and_then(|c| c.as_os_str().to_str());
    first == Some("projects")
        && parts.next().is_none()
        && name.is_some_and(|n| {
            n.ends_with(".pwexport.json") || n.ends_with(EXPORT_MOVING) || n.ends_with(EXPORT_STALE)
        })
}

/// A record being moved to a new export environment (a staging copy).
pub const EXPORT_MOVING: &str = ".pwexport.json.moving";
/// A record set aside because it was left in a folder by an earlier stint
/// as the export environment, for a project the current one does not have.
pub const EXPORT_STALE: &str = ".pwexport.json.stale";

/// Removes the staging copies (`*.pwexport.json.moving`) a move to a new
/// export environment left behind, in every environment folder. A staging
/// copy is only ever a copy of a record that still exists in the old folder
/// until the move is complete, so discarding it loses nothing. Answers how
/// many went.
pub fn sweep_export_staging(shared: &Path) -> usize {
    let mut gone = 0;
    for root in env_roots(shared) {
        let Ok(entries) = std::fs::read_dir(root.join("projects")) else { continue };
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().ends_with(EXPORT_MOVING) && std::fs::remove_file(e.path()).is_ok() {
                gone += 1;
            }
        }
    }
    gone
}

/// A live record (`*.pwexport.json`), not a staging or set-aside copy.
fn is_live_record(rel: &Path) -> bool {
    rel.file_name().is_some_and(|n| n.to_string_lossy().ends_with(".pwexport.json"))
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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MigrationReport {
    pub environments: usize,
    pub items: usize,
    /// Scripts now under the active environment.
    pub scripts: usize,
    /// The name of the environment that took the old data.
    pub active_name: String,
    /// Environment name and how many runs it took, most first.
    pub runs_by_env: Vec<(String, usize)>,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What the person is told, once, after the copy. Names no path.
pub fn migration_notice(r: &MigrationReport) -> String {
    let mut text = format!(
        "Auto Run moved {} to {}; other environments start empty. ",
        plural(r.scripts, "script", "scripts"),
        r.active_name
    );
    let runs: Vec<&(String, usize)> = r.runs_by_env.iter().filter(|(_, n)| *n > 0).collect();
    if !runs.is_empty() {
        let mut parts: Vec<String> = vec![];
        for (i, (name, n)) in runs.iter().enumerate() {
            parts.push(if i == 0 {
                format!("{} went to {name}", plural(*n, "run", "runs"))
            } else {
                format!("{n} to {name}")
            });
        }
        let last = parts.pop().unwrap_or_default();
        let joined = if parts.is_empty() { last } else { format!("{} and {last}", parts.join(", ")) };
        text.push_str(&format!("{joined}, as each run names. "));
    }
    text.push_str("The old Auto Run folders are kept as a backup and are no longer read.");
    text
}

/// The one line start-up logs after the copy. Names no path.
pub fn migration_log_line(r: &MigrationReport) -> String {
    format!("Auto Run: {}", migration_notice(r))
}

/// The small file in the shared root that holds the notice until the
/// Auto Run screen has shown it.
pub const MIGRATION_NOTE_FILE: &str = "migration-note.txt";

/// Keeps `text` for the screen to say once. Logs a failure (no path) and
/// never fails the caller.
pub fn write_migration_note(shared: &Path, text: &str) {
    let _ = std::fs::create_dir_all(shared);
    if let Err(e) = std::fs::write(shared.join(MIGRATION_NOTE_FILE), text) {
        crate::applog::debug(format!("Auto Run: the update notice could not be kept: {e}"));
    }
}

/// The kept notice, removed as it is read, so it is said once.
pub fn take_migration_note(shared: &Path) -> Option<String> {
    let path = shared.join(MIGRATION_NOTE_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
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
    let active = crate::autorun::store::resolve_root();
    if active.as_ref().is_ok_and(|a| a == root) {
        return Ok(held);
    }
    if ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|p| p == root) {
        return Ok(held);
    }
    // No environment is selected for a reason that is not a switch (the
    // list cannot be read, or there is none yet): say that.
    match active {
        Err(why) => Err(why),
        Ok(_) => Err(ENVIRONMENT_SWITCHED.to_string()),
    }
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
                && !changed.starts_with(shared.join(crate::api_templates::library::REMOVED_API_DIR))
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
pub const REMOVE_EXPORT_ENVIRONMENT: &str = crate::environments::REMOVE_EXPORT_ENV;

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

/// Whether `id` is the environment the Playwright export writes from, as
/// `environments.json` names it. An unset or unlisted name is nobody (the
/// active environment adopts it at the next start, and the active one
/// cannot be removed anyway).
///
/// It must NOT take the environments lock or resolve/select a root:
/// `remove_env` calls the refusal before taking that lock, but `retire`
/// calls it under the switch lock, and the lock order is switch, then
/// environments.
pub fn is_export_environment(shared: &Path, id: &str) -> bool {
    crate::environments::export_id_unlocked(shared).as_deref() == Some(id)
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
        // Dated, so several of these in the Copy dialog can be told apart.
        let (y, m, d) = crate::applog::civil((now_ms / 86_400_000) as i64);
        let name = format!("Removed environment ({y:04}-{m:02}-{d:02})");
        match retire(shared, &id, &name, "removed", now_ms) {
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

/// A seed an import replaced waits beside the environments under this name
/// plus the stamp, and is purged with the removed folders.
pub const SEED_ASIDE_PREFIX: &str = ".seed-replaced-";

/// Deletes the retired folders older than 30 days, and the replaced seeds
/// the same age, and returns how many. A folder whose name is not
/// `<env id>-<ms>` is left alone.
pub fn purge_removed(shared: &Path, now_ms: u64) -> usize {
    let kept = REMOVED_KEPT_DAYS * crate::autorun::store::DAY_MS;
    let mut purged = 0;
    let mut purge = |path: PathBuf, ms: u64| {
        if now_ms.saturating_sub(ms) > kept {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => purged += 1,
                Err(err) => {
                    crate::applog::warn(format!("Auto Run: a removed environment's folder stayed behind: {err}"))
                }
            }
        }
    };
    if let Ok(entries) = std::fs::read_dir(shared.join(REMOVED_DIR)) {
        for e in entries.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if let Some((_, ms)) = e.file_name().to_str().and_then(parse_key) {
                purge(e.path(), ms);
            }
        }
    }
    // Versions the API library fold kept aside: `removed-api/<ms>`.
    if let Ok(entries) = std::fs::read_dir(shared.join(crate::api_templates::library::REMOVED_API_DIR)) {
        for e in entries.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if let Some(ms) = e.file_name().to_str().and_then(|n| n.parse::<u64>().ok()) {
                purge(e.path(), ms);
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(shared.join(ENVS_DIR)) {
        for e in entries.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let ms = e.file_name().to_str().and_then(|n| n.strip_prefix(SEED_ASIDE_PREFIX)).and_then(|m| m.parse::<u64>().ok());
            if let Some(ms) = ms {
                purge(e.path(), ms);
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

/// The environment id a run belongs to: its environment's name matched
/// trimmed and case-insensitively against the listed environments (the first
/// listed wins), else the active one (a run with no name, a blank one, or
/// one no environment has).
pub fn run_target(run_env: Option<&str>, envs: &[(String, String)], active: &str) -> String {
    let wanted = run_env.map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty());
    wanted
        .and_then(|w| envs.iter().find(|(_, name)| name.trim().to_lowercase() == w))
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| active.to_string())
}

/// Where the runs, and what they name, go.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Placement {
    /// Run file name (without `.json`) to environment id.
    pub runs: std::collections::BTreeMap<String, String>,
    /// Screenshot name to the environments of the runs that name it (one
    /// each; a shot two runs in different environments name goes to both).
    pub shots: std::collections::BTreeMap<String, Vec<String>>,
}

/// A name as a run records it, with one kind of separator.
fn shot_key(name: &str) -> String {
    name.replace('\\', "/")
}

/// Reads each `runs/*.json` under `flat` and says which environment it, and
/// the screenshots it names, belong to. A run that cannot be read goes to
/// the active environment and is logged without its path.
pub fn place_runs(flat: &Path, envs: &[(String, String)], active: &str) -> Result<Placement, String> {
    let mut placed = Placement::default();
    let dir = flat.join("runs");
    if !exists(&dir)? {
        return Ok(placed);
    }
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") || !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
        let run = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| {
            serde_json::from_str::<super::LocalRun>(t.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())
        });
        let run = match run {
            Ok(r) => r,
            Err(_) => {
                crate::applog::warn("Auto Run: a saved run could not be read for the update and goes to the active environment");
                placed.runs.insert(stem, active.to_string());
                continue;
            }
        };
        let target = run_target(run.environment.as_deref(), envs, active);
        for case in &run.cases {
            for step in case.steps.iter().chain(case.cleanup.iter()) {
                // A failure picture lives on the action's outcome, not the step.
                let named = step
                    .screenshot
                    .iter()
                    .chain(step.outcomes.iter().filter_map(|o| o.screenshot.as_ref()));
                for shot in named.map(String::as_str).filter(|s| !s.is_empty()) {
                    let at = placed.shots.entry(shot_key(shot)).or_default();
                    if !at.contains(&target) {
                        at.push(target.clone());
                    }
                }
            }
        }
        placed.runs.insert(stem, target);
    }
    Ok(placed)
}

/// Every file under `dir` with its path below it (forward slashes). A link
/// or anything else that is not a plain file or folder fails the copy.
fn files_below(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let rel = format!("{prefix}{}", entry.file_name().to_string_lossy());
        let meta = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            files_below(&entry.path(), &format!("{rel}/"), out)?;
        } else if meta.is_file() {
            out.push((rel, entry.path()));
        } else {
            return Err(ITEM_IS_A_LINK.to_string());
        }
    }
    Ok(())
}

/// Whether any id in an order file has a script in `flat`. The order is
/// stored as `{"case_ids": [...]}`; a bare list of ids reads the same.
fn order_has_a_script(flat: &Path, text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else {
        return false;
    };
    let ids = value.get("case_ids").unwrap_or(&value);
    ids.as_array().is_some_and(|a| {
        a.iter()
            .filter_map(|v| v.as_i64())
            .any(|id| flat.join("scripts").join(format!("case-{id}.json")).is_file())
    })
}

/// Counts for the report, read from what was built under `built`.
fn describe(
    built: &Path,
    envs: &[(String, String)],
    active: &str,
    environments: usize,
    items: usize,
) -> MigrationReport {
    let count = |id: &str, dir: &str, keep: &dyn Fn(&str) -> bool| -> usize {
        std::fs::read_dir(built.join(id).join(dir))
            .map(|d| {
                d.flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                    .filter(|e| keep(&e.file_name().to_string_lossy()))
                    .count()
            })
            .unwrap_or(0)
    };
    let scripts = count(active, "scripts", &|n| n.starts_with("case-") && n.ends_with(".json"));
    let mut runs_by_env: Vec<(String, usize)> = envs
        .iter()
        .map(|(id, name)| (name.clone(), count(id, "runs", &|n| n.ends_with(".json"))))
        .filter(|(_, n)| *n > 0)
        .collect();
    runs_by_env.sort_by(|a, b| b.1.cmp(&a.1));
    let active_name = envs.iter().find(|(id, _)| id == active).map(|(_, n)| n.clone()).unwrap_or_default();
    MigrationReport { environments, items, scripts, active_name, runs_by_env }
}

/// Copies the flat root's per-environment items into `envs_dir/<id>/` and
/// reports what it did. Every item goes under `active` only, except the
/// [`BY_RUN`] items, which go to the environment the run that made them
/// names ([`place_runs`]; a screenshot two such runs name goes to each, and
/// what no run names goes to `active`). An order is carried only when one
/// of its cases has a script. Only the write-once items (`LINKED_ITEMS`)
/// are hard-linked, so a large `shots/` is not written out again. The
/// Playwright export record also reaches the export environment. `flat` is
/// never changed. Anything unreadable, and any link, fails the copy.
pub fn fan_out(
    flat: &Path,
    envs_dir: &Path,
    envs: &[(String, String)],
    active: &str,
) -> Result<MigrationReport, String> {
    if let Some((bad, _)) = envs.iter().find(|(i, _)| !valid_folder_id(i)) {
        return Err(format!("\"{bad}\" is not an environment id"));
    }
    let active = if envs.iter().any(|(i, _)| i == active) {
        active.to_string()
    } else {
        envs.first().map(|(i, _)| i.clone()).ok_or_else(|| "there is no environment to copy into".to_string())?
    };
    // The export records go to the environment already named as the export
    // environment (when listed), else to the active one.
    let export_target = crate::environments::export_id_unlocked(flat)
        .filter(|e| envs.iter().any(|(i, _)| i == e))
        .unwrap_or_else(|| active.clone());
    for (id, _) in envs {
        std::fs::create_dir_all(envs_dir.join(id)).map_err(|e| e.to_string())?;
    }
    let placement = place_runs(flat, envs, &active)?;
    let into = |id: &str, name: &str| envs_dir.join(id).join(name);
    let mut items = 0;
    for name in flat_item_names() {
        let from = flat.join(name);
        if !exists(&from)? {
            continue;
        }
        items += 1;
        let link = LINKED_ITEMS.contains(&name);
        if name == "shots" {
            let mut files = vec![];
            files_below(&from, "", &mut files)?;
            for (rel, path) in files {
                let at = placement.shots.get(&rel).cloned().unwrap_or_else(|| vec![active.clone()]);
                for id in at {
                    let to = into(&id, name).join(&rel);
                    std::fs::create_dir_all(to.parent().unwrap_or(envs_dir)).map_err(|e| e.to_string())?;
                    copy_tree(&path, &to, Path::new(name), link, &|_| false)?;
                }
            }
        } else if BY_RUN.contains(&name) || name == "orders" {
            for entry in std::fs::read_dir(&from).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let file = entry.file_name();
                let target = if name == "orders" {
                    let json = entry.path().extension().and_then(|e| e.to_str()) == Some("json");
                    if json {
                        match std::fs::read_to_string(entry.path()) {
                            Ok(text) if !order_has_a_script(flat, &text) => continue,
                            Ok(_) => {}
                            // Unreadable is not empty: keep it where it can be tried again.
                            Err(_) => crate::applog::warn("Auto Run: a saved run order could not be read for the update and goes to the active environment"),
                        }
                    }
                    active.clone()
                } else {
                    let key = if name == "downloads" {
                        file.to_string_lossy().to_string()
                    } else {
                        Path::new(&file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
                    };
                    placement.runs.get(&key).cloned().unwrap_or_else(|| active.clone())
                };
                let to = into(&target, name);
                std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
                copy_tree(&entry.path(), &to.join(&file), &Path::new(name).join(&file), link, &|_| false)?;
            }
        } else if name == "projects" {
            let mut targets = vec![active.clone()];
            if export_target != active {
                targets.push(export_target.clone());
            }
            for id in targets {
                let is_active = id == active;
                let keep_export = export_target == id;
                let skip = move |rel: &Path| {
                    if is_export_record(rel) {
                        !keep_export || !is_live_record(rel)
                    } else {
                        !is_active
                    }
                };
                copy_tree(&from, &into(&id, name), Path::new(name), false, &skip)?;
            }
        } else {
            copy_tree(&from, &into(&active, name), Path::new(name), link, &|_| false)?;
        }
    }
    Ok(describe(envs_dir, envs, &active, envs.len(), items))
}

/// The file operations `migrate_with` does, passed in so a test can make
/// one of them fail.
pub struct MigrationOps<'a> {
    pub fan_out: &'a dyn Fn(&Path, &Path, &[(String, String)], &str) -> Result<MigrationReport, String>,
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
    // A copy that was built whole and failed only at the final rename is
    // kept (see below): the next start retries just that rename. Every other
    // staging folder is half-built and goes.
    let mut finished = sweep_stagings(shared);
    if let Some((staging, _)) = &finished {
        // The app ran on the flat folder since the copy was made: the copy
        // is stale, so it is made again.
        let made = std::fs::metadata(complete_marker(staging)).and_then(|m| m.modified()).ok();
        if made.is_none_or(|made| newest_flat_change(shared).is_some_and(|n| n > made)) {
            crate::applog::info("Auto Run: a kept per-environment copy is rebuilt because the app was used before it could be put in place");
            let _ = std::fs::remove_dir_all(staging);
            let _ = std::fs::remove_file(complete_marker(staging));
            finished = None;
        }
    }
    if let Some((staging, base)) = finished {
        let (listed, active) = listed_environments(shared);
        let report = describe(&staging, &listed, &active, base.environments, base.items);
        crate::ai_tools::rename_retrying_waits(&staging, &envs, ops.rename, ops.sleep, &MIGRATION_RENAME_WAITS_MS)?;
        let _ = std::fs::remove_file(complete_marker(&staging));
        return Ok(Some(report));
    }
    let mut any = false;
    for name in flat_item_names() {
        any |= exists(&shared.join(name))?;
    }
    if !any {
        std::fs::create_dir_all(&envs).map_err(|e| e.to_string())?;
        return Ok(Some(MigrationReport::default()));
    }
    let (listed, active) = match crate::environments::read_names(shared)? {
        Some(found) => found,
        None => (vec![(SEED_ID.to_string(), DEFAULT_ENV_NAME.to_string())], SEED_ID.to_string()),
    };
    let staging = shared.join(format!("{STAGING_PREFIX}{}", crate::cache::now_ms()));
    let built = (ops.fan_out)(shared, &staging, &listed, &active);
    let report = match built {
        Ok(report) => report,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    // The copy is whole: say so beside it, so a rename that fails (another
    // program holding a file it just scanned) does not cost a second copy.
    // The layout version tells a copy built by an older build from this one.
    if let Err(e) =
        std::fs::write(complete_marker(&staging), format!("{} {} {LAYOUT_VERSION}", report.environments, report.items))
    {
        crate::applog::debug(format!("Auto Run: the finished-copy mark could not be written: {e}"));
    }
    match crate::ai_tools::rename_retrying_waits(&staging, &envs, ops.rename, ops.sleep, &MIGRATION_RENAME_WAITS_MS) {
        Ok(_) => {
            let _ = std::fs::remove_file(complete_marker(&staging));
            Ok(Some(report))
        }
        Err(e) => {
            // Kept for the next start, which only renames it.
            crate::applog::warn("Auto Run: the finished per-environment copy is kept and will be put in place at the next start");
            Err(e)
        }
    }
}

/// What a kept staging folder's marker must carry as its third word: the
/// layout the copy inside it has (old data under the active environment,
/// runs by the environment they name). A copy without it was built by an
/// older build, with a copy in every environment, and is rebuilt.
const LAYOUT_VERSION: &str = "by-run-1";

/// What the first environment is called when no environment file exists yet.
const DEFAULT_ENV_NAME: &str = "Default";

/// The listed environments (id and name) and the active id; the seed when
/// the file is missing or unreadable.
fn listed_environments(shared: &Path) -> (Vec<(String, String)>, String) {
    match crate::environments::read_names(shared) {
        Ok(Some(found)) => found,
        _ => (vec![(SEED_ID.to_string(), DEFAULT_ENV_NAME.to_string())], SEED_ID.to_string()),
    }
}

/// The newest modified time of anything inside the flat originals of the
/// per-environment items (the folders' own times included).
fn newest_flat_change(shared: &Path) -> Option<std::time::SystemTime> {
    fn walk(p: &Path, newest: &mut Option<std::time::SystemTime>) {
        let Ok(meta) = std::fs::symlink_metadata(p) else { return };
        if let Ok(t) = meta.modified() {
            if newest.is_none_or(|n| t > n) {
                *newest = Some(t);
            }
        }
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(p) {
                for e in entries.flatten() {
                    walk(&e.path(), newest);
                }
            }
        }
    }
    let mut newest = None;
    for name in flat_item_names() {
        walk(&shared.join(name), &mut newest);
    }
    newest
}

/// The small file beside a staging folder that says the copy in it is whole.
fn complete_marker(staging: &Path) -> PathBuf {
    let mut name = staging.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".complete");
    staging.with_file_name(name)
}

/// Removes every half-built staging folder (and stray marker) under
/// `shared`, and returns the newest folder that has a readable marker with
/// what it holds. Only that folder is kept.
fn sweep_stagings(shared: &Path) -> Option<(PathBuf, MigrationReport)> {
    let entries = std::fs::read_dir(shared).ok()?;
    let mut whole: Vec<(u64, PathBuf, MigrationReport)> = vec![];
    let mut doomed: Vec<PathBuf> = vec![];
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
        let Some(rest) = name.strip_prefix(STAGING_PREFIX) else { continue };
        if rest.ends_with(".complete") {
            // A mark whose folder is gone means nothing.
            if !path.with_file_name(name.trim_end_matches(".complete")).is_dir() {
                doomed.push(path);
            }
            continue;
        }
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            doomed.push(path);
            continue;
        }
        let report = std::fs::read_to_string(complete_marker(&path)).ok().and_then(|t| {
            let mut it = t.split_whitespace();
            let report = MigrationReport { environments: it.next()?.parse().ok()?, items: it.next()?.parse().ok()?, ..Default::default() };
            (it.next()? == LAYOUT_VERSION).then_some(report)
        });
        match (report, rest.parse::<u64>()) {
            (Some(r), Ok(ms)) => whole.push((ms, path, r)),
            _ => doomed.push(path),
        }
    }
    whole.sort_by_key(|(ms, _, _)| *ms);
    let keep = whole.pop();
    doomed.extend(whole.into_iter().map(|(_, p, _)| p));
    for path in doomed {
        let gone = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if let Err(e) = gone {
            crate::applog::warn(format!("Auto Run: a leftover half-built copy could not be removed: {e}"));
        }
        let _ = std::fs::remove_file(complete_marker(&path));
    }
    keep.map(|(_, p, r)| (p, r))
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
    /// Unfinished environment copies `env_copy::recover` settled.
    pub settled_copies: usize,
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
        // First of all: before the seed handover, and before the caller
        // sets the shared root, so nothing can choose a folder a cut-short
        // copy still owes data.
        out.settled_copies = super::backup_import::recover(shared) + super::env_copy::recover(shared);
        retry_seed(shared);
        if allowed {
            fold_api_library(shared);
        }
    }
    out
}

/// The API definitions every environment held move into the shared library
/// (once). A failure is logged without a path and never fails start-up: the
/// next start tries again.
fn fold_api_library(shared: &Path) {
    fold_api_library_logged(None, shared);
}

/// Folds (taking the switch lock for writing unless the caller `held` it)
/// and logs the outcome. A failure never reaches the caller: the definitions
/// stay where they were and the next start tries again. The log names no
/// path.
pub(crate) fn fold_api_library_logged(held: Option<&std::sync::RwLockWriteGuard<'static, ()>>, shared: &Path) {
    use crate::api_templates::library_move as lm;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let folded = match held {
        Some(h) => lm::fold_in_held(h, shared, now_ms),
        None => lm::fold_in(shared, now_ms),
    };
    match folded {
        Ok(Some(report)) => crate::applog::info(lm::move_log_line(&report)),
        Ok(None) => {}
        Err(e) => crate::applog::warn(format!(
            "Auto Run: the API templates, flows and fixtures could not be moved into the shared library yet ({}); the next start tries again",
            if path_free(&e) { e.as_str() } else { "a file could not be moved" }
        )),
    }
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
