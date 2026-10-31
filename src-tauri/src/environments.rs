//! Environments: a named website address, database and default password,
//! exactly one of them active for the whole app.
//!
//! An environment is identified by its id and never by its address - two
//! environments may share one (local and hosted dev can both be served at
//! the same URL). Every per-environment file is keyed on the id:
//! `accounts/<id>.json`, `sessions/<id>/`, `proposals/<id>.json`.
//!
//! The list lives in `environments.json` under the Auto Run root, written
//! atomically. The default password is NOT in it: it lives in the secret
//! store under `password_target(id)`, and the webview only ever learns
//! whether one is set (see `commands::environments`).
//!
//! Nothing here calls Azure DevOps; removal deletes local files only.

use crate::autorun::accounts::{accounts_path_for, sessions_dir_for};
use crate::autorun::recipe::{check_start_url, is_bare_origin};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Environment {
    /// `env-<8 lowercase hex>`. Generated, stable, never reused.
    pub id: String,
    /// What a person reads. Unique, ignoring case.
    pub name: String,
    /// A full http(s) address, or empty: empty means "use the sign-in
    /// recipe's own address".
    pub start_url: String,
    /// Other origins `navigate` may go to, as the recipe's.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// A database id from `db::credentials::databases`.
    pub db_id: String,
    /// On lets the assistant read this environment's full logins.
    #[serde(default)]
    pub test_environment: bool,
    /// What the names of things the tests make start with, so Clean up can
    /// tell them from a person's own. See `validate_test_prefix`.
    #[serde(default = "default_test_prefix")]
    pub test_prefix: String,
    /// "Run database changes without asking" for this environment: the
    /// assistant's tools get their own "always allow" for `db_query` while
    /// this is the active environment, and the bridge tells the assistant it
    /// need not ask. Off unless turned on, and changed only by
    /// `set_db_auto_approve` - a save keeps what the file holds, so an edit
    /// made from an older copy can never turn it back on.
    #[serde(default)]
    pub db_auto_approve: bool,
    /// One SELECT that says when each login's password falls due (see
    /// `autorun::password_expiry`). Empty means not checked.
    #[serde(default)]
    pub password_expiry_query: String,
    /// One SELECT that says whether a login is signed in somewhere else
    /// right now (see `autorun::active_login`). Empty means not checked.
    #[serde(default)]
    pub active_session_query: String,
    /// "Reset expired passwords": on a test environment, a sign-in the site
    /// sends to its password reset page has the app set a new simple
    /// password and save it on the account (see `autorun::signin`). Off
    /// unless turned on, and changed only by `set_reset_expired_passwords` -
    /// a save keeps what the file holds, as `db_auto_approve` does. It does
    /// nothing on an environment that is not marked a test environment.
    #[serde(default)]
    pub reset_expired_passwords: bool,
}

/// What a new environment, and one saved before prefixes existed, uses.
pub const DEFAULT_TEST_PREFIX: &str = "AUTOTEST";

pub fn default_test_prefix() -> String {
    DEFAULT_TEST_PREFIX.to_string()
}

/// Said when a test name prefix is outside its limits.
pub const TEST_PREFIX_RULE: &str = "the test name prefix is 3 to 20 letters, digits or -";

/// A prefix is 3 to 20 characters of letters, digits or `-`.
pub fn validate_test_prefix(prefix: &str) -> Result<(), String> {
    let n = prefix.chars().count();
    if (3..=20).contains(&n) && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        Ok(())
    } else {
        Err(TEST_PREFIX_RULE.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EnvFile {
    /// The active environment's id.
    pub active: String,
    pub environments: Vec<Environment>,
    /// The machine-wide "Run database changes without asking" of older
    /// versions has been given to the environment active at the time
    /// (`adopt_legacy_auto_approve`). Set in the same write, so it moves
    /// exactly once.
    #[serde(default)]
    pub auto_approve_adopted: bool,
    /// The environment the Playwright export writes from: its scripts are
    /// the ones exported, and its folder holds the export records. Unset
    /// (a file from before) or naming an environment that is not listed, it
    /// is the active one at the next `export_env` / `adopt_export_env`.
    /// Changed only by `set_export_env`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playwright_export: Option<String>,
}

const DEFAULT_NAME: &str = "Default";

/// Said when the only environment is asked to be removed.
const LAST_ENV: &str = "the last environment cannot be removed";

/// Said for allowed sites saved on an environment with no address: they
/// would never be used, since an empty address means the recipe's own.
pub const ALLOWED_NEEDS_ADDRESS: &str =
    "Also allowed needs a website address - leave both empty to use the sign-in recipe's";

/// Every read-modify-write of the file holds this, so two commands at once
/// cannot each make a "Default" or lose the other's change.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn file_path(root: &Path) -> PathBuf {
    crate::autorun::roots::shared_of(root).join("environments.json")
}

/// The assistant's proposed accounts for one environment.
pub fn proposals_path_for(root: &Path, env_id: &str) -> PathBuf {
    crate::autorun::roots::shared_of(root).join("proposals").join(format!("{env_id}.json"))
}

/// Where an environment's default password is kept in the secret store.
pub fn password_target(id: &str) -> String {
    format!("env-default-password:{id}")
}

/// `env-` and eight lowercase hex digits - the only shape `new_id` makes,
/// and the only one allowed into a file path.
pub fn valid_id(id: &str) -> bool {
    id.strip_prefix("env-")
        .is_some_and(|hex| hex.len() == 8 && hex.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)))
}

/// A fresh id: a hash of the name and the time, re-drawn until it matches
/// no id in the file. Ids are never handed out again on purpose - a
/// removed environment's id is not reused, since nothing would ever need
/// it to be.
fn new_id(name: &str, taken: &[Environment]) -> String {
    use sha2::{Digest, Sha256};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let mut salt = 0u32;
    loop {
        let digest = Sha256::digest(format!("{name}\u{0}{nanos}\u{0}{salt}").as_bytes());
        let id = format!("env-{:02x}{:02x}{:02x}{:02x}", digest[0], digest[1], digest[2], digest[3]);
        if !taken.iter().any(|e| e.id == id) {
            return id;
        }
        salt += 1;
    }
}

/// What every file must hold before anything trusts it: at least one
/// environment, well-formed unique ids (they become file names), and an
/// active id that is one of them. No database check - that is `validate`'s,
/// and a database that went away must not lock a person out of the list.
fn check_structure(file: &EnvFile) -> Result<(), String> {
    if file.environments.is_empty() {
        return Err("there are no environments".to_string());
    }
    let mut ids = std::collections::HashSet::new();
    for e in &file.environments {
        if !valid_id(&e.id) {
            return Err(format!("\"{}\" is not a usable environment id", e.id));
        }
        if !ids.insert(e.id.as_str()) {
            return Err(format!("the environment id \"{}\" appears more than once", e.id));
        }
    }
    if !ids.contains(file.active.as_str()) {
        return Err("the active environment is not in the list".to_string());
    }
    Ok(())
}

/// The whole file, as a save must leave it: structure, plus each
/// environment's name, address, allowed sites and database.
pub fn validate(file: &EnvFile, known_db_ids: &[String]) -> Result<(), String> {
    check_structure(file)?;
    let mut names: Vec<String> = vec![];
    for e in &file.environments {
        let name = e.name.trim();
        if name.is_empty() {
            return Err("an environment needs a name".to_string());
        }
        let folded = name.to_lowercase();
        if names.contains(&folded) {
            return Err(format!("an environment named \"{name}\" already exists"));
        }
        names.push(folded);
        if !e.start_url.trim().is_empty() {
            check_start_url(e.start_url.trim()).map_err(|_| {
                format!("the website address of \"{name}\" must be a full http or https address, or empty")
            })?;
        } else if e.allowed_origins.iter().any(|o| !o.trim().is_empty()) {
            // Without an address the recipe's own address and allowed sites
            // are used (`recipe::effective_recipe`), so these would be saved
            // and then never read.
            return Err(ALLOWED_NEEDS_ADDRESS.to_string());
        }
        for o in &e.allowed_origins {
            if !is_bare_origin(o) {
                return Err(format!(
                    "\"{o}\" is not an origin - write it as https://host or https://host:port, with no path"
                ));
            }
        }
        validate_test_prefix(&e.test_prefix)?;
        crate::autorun::password_expiry::check_query(&e.password_expiry_query)
            .map_err(|why| format!("the password expiry query of \"{name}\": {why}"))?;
        crate::autorun::active_login::check_query(&e.active_session_query)
            .map_err(|why| format!("the active session query of \"{name}\": {why}"))?;
        if !known_db_ids.iter().any(|k| *k == e.db_id) {
            return Err(format!("\"{name}\" uses a database that is not set up - pick one"));
        }
    }
    Ok(())
}

fn read(root: &Path) -> Result<Option<EnvFile>, String> {
    let text = match std::fs::read_to_string(file_path(root)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("the environments file could not be read: {e}")),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let file: EnvFile =
        serde_json::from_str(text).map_err(|e| format!("the environments file is not readable: {e}"))?;
    check_structure(&file).map_err(|e| format!("the environments file is not readable: {e}"))?;
    Ok(Some(file))
}

/// An `environments.json` as the app reads it (a byte-order mark allowed),
/// checked the same way: for a file that did not come from this machine.
pub fn parse_file(bytes: &[u8]) -> Result<EnvFile, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "the environments file is not readable".to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let file: EnvFile =
        serde_json::from_str(text).map_err(|e| format!("the environments file is not readable: {e}"))?;
    check_structure(&file).map_err(|e| format!("the environments file is not readable: {e}"))?;
    Ok(file)
}

/// The text of `file` as `write` saves it.
pub fn file_text(file: &EnvFile) -> Result<String, String> {
    serde_json::to_string_pretty(file).map_err(|e| e.to_string())
}

/// Copies the Playwright export records of environment folder `from` into
/// `to` (staging names first, then renamed into place), leaving `from` as it
/// is. Answers how many were copied. The caller holds the records lock and
/// the switch lock.
pub fn copy_export_records(from: &Path, to: &Path) -> Result<usize, String> {
    move_export_records(from, to).map(|m| m.old.len())
}

fn write(root: &Path, file: &EnvFile) -> Result<(), String> {
    std::fs::create_dir_all(crate::autorun::roots::shared_of(root)).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&file_path(root), &json)
}

/// The environment ids and the active id, or `None` while there is no file.
/// Never makes one. An unreadable file is an error, left as it is.
pub fn read_ids(root: &Path) -> Result<Option<(Vec<String>, String)>, String> {
    let _held = lock();
    Ok(read(root)?.map(|f| (f.environments.into_iter().map(|e| e.id).collect(), f.active)))
}

/// Hands `envs/_seed` (what the update left for the first environment) to
/// `id`, when there is one. Run AFTER the file that names `id` is saved, so
/// a failed save never strands the data under an id nobody has. A failed
/// handover is logged (no path) and the data stays in `_seed` for the next
/// environment made; it never fails the caller.
fn take_seed(root: &Path, id: &str) {
    let shared = crate::autorun::roots::shared_of(root);
    match crate::autorun::roots::seed_into(&shared, id) {
        Ok(true) => crate::applog::info("Environments: the first environment took over the data the update copied"),
        Ok(false) => {}
        Err(e) => crate::applog::warn(format!(
            "Environments: the data the update copied could not be handed to the new environment yet: {e}"
        )),
    }
}

/// The file, made on first use. A file that exists but cannot be read is an
/// error and is left exactly as it is - never replaced with a fresh Default.
fn load_or_init_locked(root: &Path, current_db: Option<&str>) -> Result<EnvFile, String> {
    if let Some(file) = read(root)? {
        return Ok(file);
    }
    let db_id = current_db
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(crate::db_defaults::DB_PRESETS[0].id)
        .to_string();
    let id = new_id(DEFAULT_NAME, &[]);
    // The machine's one accounts list becomes Default's. Copied before the
    // file is written: if the copy fails nothing is saved, and the next
    // call tries again from scratch.
    let legacy = crate::autorun::roots::shared_of(root).join("accounts.json");
    if legacy.is_file() {
        let to = accounts_path_for(root, &id);
        std::fs::create_dir_all(to.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
        std::fs::copy(&legacy, &to).map_err(|e| format!("the accounts could not be moved to Default: {e}"))?;
    }
    let first_id = id.clone();
    let file = EnvFile {
        active: id.clone(),
        environments: vec![Environment {
            id,
            name: DEFAULT_NAME.to_string(),
            start_url: String::new(),
            allowed_origins: vec![],
            db_id,
            test_environment: false,
            test_prefix: default_test_prefix(),
            db_auto_approve: false,
            password_expiry_query: String::new(),
            active_session_query: String::new(),
            reset_expired_passwords: false,
        }],
        auto_approve_adopted: false,
        playwright_export: Some(first_id.clone()),
    };
    write(root, &file)?;
    take_seed(root, &file.active);
    crate::applog::info("Environments: made Default from this machine's settings");
    // Only now - the copy is written and the file that points at it is
    // saved - are the old single list and its sessions dead weight. The
    // list holds passwords and the sessions live cookies, so neither is
    // left lying about (or carried into every backup).
    remove_legacy_files(root);
    Ok(file)
}

/// The machine-wide `accounts.json` and the session FILES directly under
/// `sessions/` from before environments. The per-environment folders
/// (`sessions/<env id>/`) are not touched. Best effort: a file that will
/// not go is logged, and nothing reads it any more.
pub fn remove_legacy_files(root: &Path) {
    let root = &crate::autorun::roots::shared_of(root);
    let warn = |e: std::io::Error| {
        crate::applog::warn(format!("Environments: an old accounts or session file stayed behind: {e}"))
    };
    match std::fs::remove_file(root.join("accounts.json")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => warn(e),
        _ => {}
    }
    let Ok(entries) = std::fs::read_dir(root.join("sessions")) else { return };
    for entry in entries.flatten() {
        let is_file = entry.file_type().is_ok_and(|t| t.is_file());
        let is_json = entry.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("json"));
        if is_file && is_json {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                warn(e);
            }
        }
    }
}

/// The environments, creating `Default` on first use: an empty address (the
/// recipe's keeps working), the database `current_db` names (or the first
/// shipped one), and this machine's accounts list copied to it.
pub fn load_or_init(root: &Path, current_db: Option<&str>) -> Result<EnvFile, String> {
    let _held = lock();
    load_or_init_locked(root, current_db)
}

pub fn active(root: &Path) -> Result<Environment, String> {
    let file = load_or_init(root, None)?;
    file.environments
        .into_iter()
        .find(|e| e.id == file.active)
        .ok_or_else(|| "the active environment is not in the list".to_string())
}

pub fn active_id(root: &Path) -> Result<String, String> {
    Ok(load_or_init(root, None)?.active)
}

/// The active environment, or None while there is no file yet. Never makes
/// one: the bridge asks this on every push, and a Default made there would
/// not get the database the Company database card has chosen.
pub fn read_active(root: &Path) -> Result<Option<Environment>, String> {
    let _held = lock();
    Ok(read(root)?.and_then(|file| file.environments.into_iter().find(|e| e.id == file.active)))
}

/// Switch "Run database changes without asking" on or off for one
/// environment. Nothing else in the file changes.
pub fn set_db_auto_approve(root: &Path, id: &str, on: bool) -> Result<EnvFile, String> {
    let _held = lock();
    let mut file = load_or_init_locked(root, None)?;
    let env = file
        .environments
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| "that environment is not there any more".to_string())?;
    env.db_auto_approve = on;
    write(root, &file)?;
    Ok(file)
}

/// Switch "Reset expired passwords" on or off for one environment. Nothing
/// else in the file changes.
pub fn set_reset_expired_passwords(root: &Path, id: &str, on: bool) -> Result<EnvFile, String> {
    let _held = lock();
    let mut file = load_or_init_locked(root, None)?;
    let env = file
        .environments
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| "that environment is not there any more".to_string())?;
    env.reset_expired_passwords = on;
    write(root, &file)?;
    Ok(file)
}

/// Give older versions' machine-wide "Run database changes without asking"
/// to the environment active now, and to no other - once. `legacy_on` is
/// that old setting. Answers whether anything was turned on. No file yet
/// means nothing to give it to, and none is made.
pub fn adopt_legacy_auto_approve(root: &Path, legacy_on: bool) -> Result<bool, String> {
    let _held = lock();
    let Some(mut file) = read(root)? else { return Ok(false) };
    if file.auto_approve_adopted {
        return Ok(false);
    }
    let active = file.active.clone();
    if legacy_on {
        if let Some(env) = file.environments.iter_mut().find(|e| e.id == active) {
            env.db_auto_approve = true;
        }
    }
    file.auto_approve_adopted = true;
    write(root, &file)?;
    if legacy_on {
        crate::applog::info("Environments: running database changes without asking now belongs to the active environment");
    }
    Ok(legacy_on)
}

// ---- the Playwright export environment ------------------------------------

/// Said when the export environment is the one to be removed.
pub const REMOVE_EXPORT_ENV: &str = "this is the Playwright export environment - name another one on the Playwright tab first";

/// Said when Export, Sync or Finish in the repo is asked in any environment
/// but the export one.
pub fn not_export_env(name: &str) -> String {
    format!("Export and Sync run only in {name}, the Playwright export environment - switch to {name} on the AI Bridge tab first")
}

/// Said when no environment exists yet to be the export environment.
pub const NO_ENVIRONMENTS: &str = "no environment has been set up yet";
/// Said when the environment to be named export environment is not listed.
pub const EXPORT_ENV_UNKNOWN: &str = "that environment is not there any more";
/// Said when the export records could not be moved to the new export
/// environment; nothing was changed.
pub const EXPORT_ENV_NOT_MOVED: &str = "the Playwright export settings could not be moved to that environment - nothing was changed; close anything using the Auto Run files and try again";

/// The id the file names as the export environment, when it is listed.
fn export_id_of(file: &EnvFile) -> Option<&str> {
    file.playwright_export.as_deref().filter(|id| file.environments.iter().any(|e| e.id == *id))
}

/// The export environment's id: the one the file names when it is listed,
/// else the active one.
pub fn export_id(file: &EnvFile) -> &str {
    export_id_of(file).unwrap_or(&file.active)
}

/// The id of the export environment as the file names it right now, read
/// WITHOUT the environments lock and without making anything: `None` while
/// the file is missing, unreadable, or names none that is listed. This is
/// what `roots::is_export_environment` calls, from under the switch lock.
pub fn export_id_unlocked(root: &Path) -> Option<String> {
    let file = read(root).ok().flatten()?;
    export_id_of(&file).map(str::to_string)
}

/// Gives the export environment to the active one when the file names none
/// that is listed. Under the lock; the file must exist.
fn adopt_locked(root: &Path, file: &mut EnvFile) -> Result<bool, String> {
    if export_id_of(file).is_some() {
        return Ok(false);
    }
    file.playwright_export = Some(file.active.clone());
    write(root, file)?;
    Ok(true)
}

/// The Playwright export environment. One that is unset, or names an
/// environment that is gone, becomes the active one, saved.
pub fn export_env(root: &Path) -> Result<Environment, String> {
    let _held = lock();
    // Never makes the first environment: only `env_list` does (it knows the
    // database the Company database card chose).
    let mut file = read(root)?.ok_or_else(|| NO_ENVIRONMENTS.to_string())?;
    adopt_locked(root, &mut file)?;
    let id = export_id_of(&file).unwrap_or(&file.active).to_string();
    file.environments.into_iter().find(|e| e.id == id).ok_or_else(|| EXPORT_ENV_UNKNOWN.to_string())
}

/// Gives the export environment to the active one when none is named. Only
/// when `environments.json` exists - it never makes one. Answers whether it
/// changed the file.
pub fn adopt_export_env(root: &Path) -> Result<bool, String> {
    let _held = lock();
    let Some(mut file) = read(root)? else { return Ok(false) };
    let changed = adopt_locked(root, &mut file)?;
    if changed {
        crate::applog::info("Environments: the active environment is now the Playwright export environment");
    }
    Ok(changed)
}

/// `Ok` when work on `root` may export: `root` is the export environment's
/// root (or the shared root of the flat layout, judged by the active
/// environment). Otherwise the one sentence that names the export
/// environment. Makes no file; with no file there is nothing to refuse.
pub fn check_export_env(root: &Path) -> Result<(), String> {
    let _held = lock();
    let Some(file) = read(root)? else { return Ok(()) };
    let here = crate::autorun::roots::env_id_of(root).unwrap_or_else(|| file.active.clone());
    let export = export_id_of(&file).unwrap_or(&file.active);
    if here == export {
        return Ok(());
    }
    let name = file.environments.iter().find(|e| e.id == export).map(|e| e.name.clone()).unwrap_or_default();
    Err(not_export_env(&name))
}

/// Makes `id` the export environment and moves every
/// `projects/*.pwexport.json` from the old export environment's folder to
/// its. The new folder's own file of the same name is replaced (logged).
///
/// Held under the switch lock for writing (so no guarded write is in
/// flight and none starts), then the environments lock. Each record is
/// first copied beside its destination under a staging name; only when
/// every copy exists are they renamed into place (a held rename retried),
/// the file written, and the old records removed. A failure before the file
/// is written changes nothing that matters: the old records are still
/// there and still the export environment's. A flat layout has one folder
/// for all, so only the name changes.
pub fn set_export_env(root: &Path, id: &str) -> Result<EnvFile, String> {
    use crate::autorun::roots;
    let shared = roots::shared_of(root);
    // First: never interleaves with Save map or a folder rename's placements.
    let _records = crate::pw_export::mapping::records_lock().lock().unwrap_or_else(|e| e.into_inner());
    let _switching = roots::switch_lock().write().unwrap_or_else(|e| e.into_inner());
    let _held = lock();
    let mut file = load_or_init_locked(root, None)?;
    if !file.environments.iter().any(|e| e.id == id) {
        return Err(EXPORT_ENV_UNKNOWN.to_string());
    }
    let old = export_id_of(&file).unwrap_or(&file.active).to_string();
    if old != id && roots::is_per_env_layout(&shared) {
        let (from, to) = (roots::env_root(&shared, &old), roots::env_root(&shared, id));
        let moved = move_export_records(&from, &to).map_err(|e| {
            crate::applog::warn(format!("Environments: the Playwright export settings could not be moved: {e}"));
            EXPORT_ENV_NOT_MOVED.to_string()
        })?;
        file.playwright_export = Some(id.to_string());
        write(root, &file)?;
        for p in &moved.old {
            if let Err(e) = std::fs::remove_file(p) {
                crate::applog::warn(format!("Environments: an old Playwright export record stayed behind: {e}"));
            }
        }
        // A record the new folder held for a project the old one did not is
        // from an earlier stint: set aside, so it never becomes current.
        for p in &moved.leftovers {
            let mut stale = p.clone().into_os_string();
            stale.push(".stale");
            let stale = PathBuf::from(stale);
            let _ = std::fs::remove_file(&stale);
            if let Err(e) = std::fs::rename(p, &stale) {
                crate::applog::warn(format!("Environments: an old Playwright export record could not be set aside: {e}"));
            }
        }
        crate::applog::info(format!(
            "Environments: the Playwright export environment changed ({} record(s) moved, {} replaced)",
            moved.old.len(),
            moved.replaced
        ));
        return Ok(file);
    }
    file.playwright_export = Some(id.to_string());
    write(root, &file)?;
    Ok(file)
}

struct MovedRecords {
    /// The records in the old folder, to remove once the file names the new one.
    old: Vec<PathBuf>,
    /// The new folder's own records for projects the old one has none of.
    leftovers: Vec<PathBuf>,
    replaced: usize,
}

/// Short on purpose: the switch lock is held while a rename waits.
const EXPORT_MOVE_WAITS_MS: [u64; 3] = [50, 100, 200];

/// Copies each record to `to` (staging names first, then renamed into
/// place). Nothing in `from` is touched. On failure the staging copies are
/// removed.
fn move_export_records(from: &Path, to: &Path) -> Result<MovedRecords, String> {
    let records = crate::pw_export::mapping::files(from);
    if records.is_empty() {
        let leftovers = crate::pw_export::mapping::files(to);
        return Ok(MovedRecords { old: vec![], leftovers, replaced: 0 });
    }
    let dir = to.join("projects");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Staging copies a cut-short move left: copies only, safe to discard.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().ends_with(crate::autorun::roots::EXPORT_MOVING) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    let cleanup = |staged: &[(PathBuf, PathBuf)]| {
        for (tmp, _) in staged {
            let _ = std::fs::remove_file(tmp);
        }
    };
    for rec in &records {
        let Some(name) = rec.file_name() else { continue };
        let dest = dir.join(name);
        let tmp = dir.join(format!("{}.moving", name.to_string_lossy()));
        if let Err(e) = std::fs::copy(rec, &tmp) {
            cleanup(&staged);
            let _ = std::fs::remove_file(&tmp);
            return Err(e.to_string());
        }
        staged.push((tmp, dest));
    }
    let mut replaced = 0;
    for (i, (tmp, dest)) in staged.iter().enumerate() {
        if dest.exists() {
            replaced += 1;
        }
        let r = crate::ai_tools::rename_retrying_waits(
            tmp,
            dest,
            &mut |a, b| std::fs::rename(a, b),
            &mut |d| std::thread::sleep(d),
            &EXPORT_MOVE_WAITS_MS,
        );
        if let Err(e) = r {
            // Those already in place are copies of records the old folder
            // still holds; the rest are removed.
            cleanup(&staged[i..]);
            return Err(e);
        }
    }
    let moved_names: Vec<_> = records.iter().filter_map(|r| r.file_name().map(|n| n.to_os_string())).collect();
    let leftovers = crate::pw_export::mapping::files(to)
        .into_iter()
        .filter(|p| p.file_name().is_some_and(|n| !moved_names.iter().any(|m| m == n)))
        .collect();
    Ok(MovedRecords { old: records, leftovers, replaced })
}

fn tidy(mut env: Environment) -> Environment {
    env.name = env.name.trim().to_string();
    env.start_url = env.start_url.trim().to_string();
    env.allowed_origins = env.allowed_origins.iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
    env.db_id = env.db_id.trim().to_string();
    env.test_prefix = env.test_prefix.trim().to_string();
    env.password_expiry_query = env.password_expiry_query.trim().to_string();
    env.active_session_query = env.active_session_query.trim().to_string();
    env
}

/// Add (`env.id` empty: a new id is made) or replace the environment with
/// that id. Only the saved environment's database has to be known now:
/// another one whose database has since gone is not this save's business.
pub fn save_env(root: &Path, env: Environment, known_db_ids: &[String]) -> Result<EnvFile, String> {
    save_env_with(root, env, || known_db_ids.to_vec())
}

/// `save_env` with the known database ids asked for UNDER the file's lock -
/// what the commands use. A database removed a moment before (its removal
/// holds this lock too, see `when_db_unused`) is then never written into an
/// environment by a save that looked it up just before.
pub fn save_env_with(
    root: &Path,
    env: Environment,
    known_db_ids: impl FnOnce() -> Vec<String>,
) -> Result<EnvFile, String> {
    let _held = lock();
    let known_db_ids = known_db_ids();
    let mut file = load_or_init_locked(root, None)?;
    let mut env = tidy(env);
    if env.id.is_empty() {
        env.id = new_id(&env.name, &file.environments);
        // A new environment never arrives with writes asked about less.
        env.db_auto_approve = false;
        env.reset_expired_passwords = false;
        file.environments.push(env.clone());
    } else {
        let slot = file
            .environments
            .iter_mut()
            .find(|e| e.id == env.id)
            .ok_or_else(|| "that environment is not there any more".to_string())?;
        // Kept from the file, read under this lock: only
        // `set_db_auto_approve` moves it.
        env.db_auto_approve = slot.db_auto_approve;
        env.reset_expired_passwords = slot.reset_expired_passwords;
        *slot = env.clone();
    }
    let mut known = known_db_ids.to_vec();
    known.extend(file.environments.iter().filter(|e| e.id != env.id).map(|e| e.db_id.clone()));
    validate(&file, &known)?;
    write(root, &file)?;
    Ok(file)
}

/// Runs `f` only while no environment uses the database `db_id`, holding
/// the file's lock throughout so no save can start using it halfway. When
/// one does, answers `refuse` of the names of every environment using it,
/// and `f` never runs. No file yet means no environment uses anything, and
/// none is made.
pub fn when_db_unused<T>(
    root: &Path,
    db_id: &str,
    refuse: impl FnOnce(&[String]) -> String,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _held = lock();
    let users: Vec<String> = read(root)?
        .map(|file| file.environments.into_iter().filter(|e| e.db_id == db_id).map(|e| e.name).collect())
        .unwrap_or_default();
    if users.is_empty() {
        f()
    } else {
        Err(refuse(&users))
    }
}

/// Every listed environment as `(id, name)`, or `None` when the file is not
/// there or cannot be read. Takes the environments lock.
pub fn listed_names(root: &Path) -> Option<Vec<(String, String)>> {
    let _held = lock();
    read(root).ok().flatten().map(|f| f.environments.into_iter().map(|e| (e.id, e.name)).collect())
}

/// The name of the listed environment `id`, or `None`. Takes the
/// environments lock.
pub fn listed_name(root: &Path, id: &str) -> Option<String> {
    let _held = lock();
    read(root).ok().flatten()?.environments.into_iter().find(|e| e.id == id).map(|e| e.name)
}

/// The ids in `environments.json`, or `None` when the file is not there or
/// cannot be read. Takes the environments lock.
pub fn listed_ids(root: &Path) -> Option<Vec<String>> {
    let _held = lock();
    read(root).ok().flatten().map(|f| f.environments.into_iter().map(|e| e.id).collect())
}

/// Remove an environment and its local files: accounts, saved sessions and
/// proposals. Refused for the last one and the active one.
pub fn remove_env(root: &Path, id: &str) -> Result<EnvFile, String> {
    // Busy or the export environment: refused before anything changes. This
    // is before the lock on purpose - the refusal must never wait on it.
    let shared = crate::autorun::roots::shared_of(root);
    // The only environment, and the active one, are the export environment
    // too (it starts as the active one): "last" and "active" are the
    // sentences that help there. Read without the lock, like the refusal.
    if let Some(f) = read(root).ok().flatten() {
        if f.environments.len() == 1 && f.environments[0].id == id {
            return Err(LAST_ENV.to_string());
        }
        if f.active == id {
            return Err(crate::autorun::roots::REMOVE_ACTIVE.to_string());
        }
    }
    if let Some(why) = crate::autorun::roots::removal_refusal(&shared, id) {
        return Err(why.to_string());
    }
    let (file, name) = {
        let _held = lock();
        let mut file = load_or_init_locked(root, None)?;
        let Some(name) = file.environments.iter().find(|e| e.id == id).map(|e| e.name.clone()) else {
            return Err("that environment is not there any more".to_string());
        };
        if file.environments.len() == 1 {
            return Err(LAST_ENV.to_string());
        }
        if file.active == id {
            return Err(crate::autorun::roots::REMOVE_ACTIVE.to_string());
        }
        // Checked again here, from the list already held: naming it the
        // export environment may have landed after the check above.
        if export_id_of(&file) == Some(id) {
            return Err(REMOVE_EXPORT_ENV.to_string());
        }
        file.environments.retain(|e| e.id != id);
        write(root, &file)?;
        // `id` matched an entry that passed `check_structure`, so it is a
        // well-formed id and these paths stay inside the root.
        let gone = |r: std::io::Result<()>| match r {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                crate::applog::warn(format!("Environments: a removed environment's file stayed behind: {e}"))
            }
            _ => {}
        };
        gone(std::fs::remove_file(accounts_path_for(root, id)));
        gone(std::fs::remove_dir_all(sessions_dir_for(root, id)));
        gone(std::fs::remove_file(proposals_path_for(root, id)));
        (file, name)
    };
    // The Auto Run folder waits in the removed area for 30 days. This runs
    // after the environments lock is released (the move takes the switch
    // lock), and a failure is logged, not fatal: the environment is gone
    // from the list already.
    if let Err(e) = crate::autorun::roots::retire(&shared, id, &name, "removed", crate::autorun::sessions::now_ms()) {
        crate::applog::warn(format!("Environments: a removed environment's Auto Run folder stayed in place: {e}"));
    }
    Ok(file)
}

/// An account the assistant proposed for an environment, as the webview
/// sees it: a login it found (in a seed script, a spec, the database). A
/// password the assistant read with it stays in Rust - this says only
/// whether there is one. There is deliberately no password field.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ProposedAccount {
    pub key: String,
    pub label: String,
    pub username: String,
    pub role: Option<String>,
    pub has_password: bool,
    /// Why the assistant gave no password for this login (for example "the
    /// database holds only a hash"), when it said. Not a secret: the person
    /// sees it beside the account.
    pub no_password: Option<String>,
    /// What the environment's expiry and active session queries said of this
    /// login when it was proposed: `expired`, `due soon`, `signed in
    /// elsewhere`. Never a secret.
    pub marks: Vec<String>,
}

/// A proposed account as it is kept in the proposals file. `password` is
/// the one the assistant read from the same database lookup as the login,
/// accepted only for an environment marked as a test environment. Like an
/// account's own password it is kept in plain JSON (the owner's decision
/// for test logins, see `autorun::accounts`), and it leaves the file when
/// its proposal is added, dismissed or replaced. `no_password` is the
/// assistant's reason for giving none; it is kept so the person can see it.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredProposal {
    pub key: String,
    pub label: String,
    pub username: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_password: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<String>,
}

/// Hand-written so a password cannot reach a log line through `{:?}`.
impl std::fmt::Debug for StoredProposal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredProposal")
            .field("key", &self.key)
            .field("label", &self.label)
            .field("username", &self.username)
            .field("role", &self.role)
            .field("password", &self.password.as_ref().map(|_| "(hidden)"))
            .finish()
    }
}

impl StoredProposal {
    /// What the webview may see of it.
    pub fn view(&self) -> ProposedAccount {
        ProposedAccount {
            key: self.key.clone(),
            label: self.label.clone(),
            username: self.username.clone(),
            role: self.role.clone(),
            has_password: self.password.is_some(),
            no_password: self.no_password.clone(),
            marks: self.marks.clone(),
        }
    }
}

/// The most accounts one proposal may carry.
pub const MAX_PROPOSALS: usize = 100;

/// The longest password a proposal may carry, in characters.
pub const MAX_PROPOSED_PASSWORD: usize = 256;

/// The longest reason a proposal may give for having no password, in
/// characters.
pub const MAX_NO_PASSWORD_REASON: usize = 200;

/// Said when a `no_password` reason is a stored value (a password or a hash)
/// and not words.
pub const NO_PASSWORD_IS_A_VALUE: &str = "no_password is a reason in words, not a value.";

/// Whether a `no_password` reason looks like a password or a hash and not
/// words: a single token of 16 or more characters with no spaces in it, or
/// text holding the kind of long opaque token the capture redaction hides.
/// The reason is shown beside the account and kept in plain JSON, so it must
/// not be where a stored value ends up.
pub fn no_password_is_a_value(reason: &str) -> bool {
    let reason = reason.trim();
    (reason.chars().count() >= 16 && !reason.chars().any(char::is_whitespace))
        || crate::autorun::capture_redact::holds_opaque_token(reason)
}

/// Said, with the keys that lack one, when a proposal for a test environment
/// has an account with neither a password nor a reason for having none.
pub fn proposed_password_missing(keys: &[&str]) -> String {
    format!(
        "This is a test environment: give each account its password from the same database lookup that found its login (never a hash), or say why there is none with no_password. Missing for: {}.",
        keys.join(", ")
    )
}

/// Said when a proposal carries a password and the active environment is
/// not marked as a test environment. The whole call is refused.
pub const PROPOSED_PASSWORD_NOT_TEST: &str = "passwords can only be proposed for an environment marked as a test environment - mark it in Edit environments, or propose the logins without passwords";

/// A proposal as it may be kept: at most `MAX_PROPOSALS`, each key a usable
/// account key and used once, each with a username, and a password, when
/// there is one, that is not empty and at most `MAX_PROPOSED_PASSWORD`
/// characters. No message repeats a password.
pub fn validate_proposals(list: &[StoredProposal]) -> Result<(), String> {
    if list.len() > MAX_PROPOSALS {
        return Err(format!(
            "at most {MAX_PROPOSALS} accounts can be proposed at once - this proposal has {}",
            list.len()
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for p in list {
        if !crate::autorun::accounts::valid_key(&p.key) {
            return Err(format!(
                "\"{}\" is not a usable account key - use lowercase letters, digits, dot, underscore or hyphen",
                p.key
            ));
        }
        if !seen.insert(p.key.as_str()) {
            return Err(format!("the account key \"{}\" appears more than once", p.key));
        }
        if p.username.trim().is_empty() {
            return Err(format!("the account \"{}\" has no username", p.key));
        }
        match &p.password {
            Some(pw) if pw.is_empty() => {
                return Err(format!(
                    "the password proposed for \"{}\" is empty - give the password, or no_password with the reason there is none",
                    p.key
                ))
            }
            Some(pw) if pw.chars().count() > MAX_PROPOSED_PASSWORD => {
                return Err(format!(
                    "the password proposed for \"{}\" is longer than {MAX_PROPOSED_PASSWORD} characters",
                    p.key
                ))
            }
            _ => {}
        }
        if let Some(why) = &p.no_password {
            if p.password.is_some() {
                return Err(format!(
                    "the account \"{}\" has both a password and no_password - give one or the other",
                    p.key
                ));
            }
            if why.trim().is_empty() {
                return Err(format!("the no_password reason for \"{}\" is empty - say why there is no password", p.key));
            }
            if why.chars().count() > MAX_NO_PASSWORD_REASON {
                return Err(format!(
                    "the no_password reason for \"{}\" is longer than {MAX_NO_PASSWORD_REASON} characters",
                    p.key
                ));
            }
            if no_password_is_a_value(why) {
                return Err(format!("the no_password reason for \"{}\": {NO_PASSWORD_IS_A_VALUE}", p.key));
            }
        }
    }
    Ok(())
}

/// Said when the proposals file cannot be parsed.
pub const PROPOSALS_UNREADABLE: &str =
    "the proposed accounts are not readable - dismiss them, or ask the assistant to propose them again";

/// One environment's proposed accounts as kept, passwords included - for
/// Rust only. The webview gets `StoredProposal::view` of each.
pub fn load_proposals(root: &Path, env_id: &str) -> Result<Vec<StoredProposal>, String> {
    if !valid_id(env_id) {
        return Err(format!("\"{env_id}\" is not a usable environment id"));
    }
    match std::fs::read_to_string(proposals_path_for(root, env_id)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            // Neither the answer nor the log carries the parser's words,
            // which can quote a value - and a value here can be a password.
            serde_json::from_str(s).map_err(|e| {
                crate::applog::warn(format!(
                    "Environments: the proposed accounts file did not parse ({:?} at line {}, column {})",
                    e.classify(),
                    e.line(),
                    e.column()
                ));
                PROPOSALS_UNREADABLE.to_string()
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(format!("the proposed accounts could not be read: {e}")),
    }
}

/// Drop every password from one environment's proposal, keeping the
/// logins - for when the environment stops being a test environment.
/// Returns how many were dropped.
pub fn strip_proposed_passwords(root: &Path, env_id: &str) -> Result<usize, String> {
    let mut list = load_proposals(root, env_id)?;
    let mut dropped = 0;
    for p in &mut list {
        if p.password.take().is_some() {
            dropped += 1;
        }
    }
    if dropped > 0 {
        save_proposals(root, env_id, &list)?;
    }
    Ok(dropped)
}

/// Replace one environment's proposal with `list`, validated first. An
/// empty list removes the file.
pub fn save_proposals(root: &Path, env_id: &str, list: &[StoredProposal]) -> Result<(), String> {
    if !valid_id(env_id) {
        return Err(format!("\"{env_id}\" is not a usable environment id"));
    }
    validate_proposals(list)?;
    let path = proposals_path_for(root, env_id);
    if list.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("the proposed accounts could not be removed: {e}"))
            }
            Ok(()) => {
                crate::autorun::data_changed::notify(root, crate::events::AutorunDataKind::Proposals);
                Ok(())
            }
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(path.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path, &json)?;
    crate::autorun::data_changed::notify(root, crate::events::AutorunDataKind::Proposals);
    Ok(())
}

/// Make `id` the active environment. Whether a switch is allowed right now
/// (nothing recording or running) is the command's question, not this one.
pub fn set_active(root: &Path, id: &str) -> Result<EnvFile, String> {
    let _held = lock();
    let mut file = load_or_init_locked(root, None)?;
    if !file.environments.iter().any(|e| e.id == id) {
        return Err("that environment is not there any more".to_string());
    }
    file.active = id.to_string();
    write(root, &file)?;
    Ok(file)
}
