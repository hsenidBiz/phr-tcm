//! The tester's own test accounts.
//!
//! Entered in the app, saved in a plain JSON file beside the scripts. The
//! owner decided against an operating-system credential store: the
//! application under test is internal and device-gated, and the point is
//! that every tester can run the same scripts with their own logins. So a
//! SCRIPT names an account by key and never carries a login; this file is
//! per tester and per machine.
//!
//! What this module still guarantees: a password is never printed (the
//! `Debug` below hides it), and a saved session does not outlive the login
//! it was made with.

use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Account {
    /// What a script writes: `"account": "hr.supervisor"`.
    pub key: String,
    /// What a person reads in the app.
    pub label: String,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("key", &self.key)
            .field("label", &self.label)
            .field("username", &self.username)
            .field("password", &"(hidden)")
            .finish()
    }
}

/// 1 to 64 characters, starting with a lowercase letter or digit, then
/// lowercase letters, digits, dot, underscore or hyphen. Narrow on purpose:
/// the key is also a file name (`sessions/<env id>/<key>.json`).
pub fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else { return false };
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) || key.len() > 64 {
        return false;
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')) {
        return false;
    }
    !is_windows_device_name(key)
}

/// Windows reserves these as device names: a "file" by one of them (or by
/// one of them followed by a dot and anything, `con.x`) does not behave as
/// a regular file, so `sessions/con.json` would silently fail to save.
/// Every char a valid key can otherwise contain is already lowercase ASCII,
/// so no case-folding is needed here.
fn is_windows_device_name(key: &str) -> bool {
    let stem = key.split('.').next().unwrap_or(key);
    matches!(
        stem,
        "con" | "prn"
            | "aux"
            | "nul"
            | "com1" | "com2" | "com3" | "com4" | "com5" | "com6" | "com7" | "com8" | "com9"
            | "lpt1" | "lpt2" | "lpt3" | "lpt4" | "lpt5" | "lpt6" | "lpt7" | "lpt8" | "lpt9"
    )
}

pub fn validate_accounts(accounts: &[Account]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for a in accounts {
        if !valid_key(&a.key) {
            return Err(format!(
                "\"{}\" is not a usable account key - use lowercase letters, digits, dot, underscore or hyphen",
                a.key
            ));
        }
        if !seen.insert(a.key.as_str()) {
            return Err(format!("the account key \"{}\" appears more than once", a.key));
        }
        if a.username.trim().is_empty() {
            return Err(format!("the account \"{}\" has no username", a.key));
        }
    }
    Ok(())
}

/// The ACTIVE environment's accounts file. An error means the environments
/// file could not be read, and nothing should guess a path then.
fn accounts_path(root: &Path) -> Result<PathBuf, String> {
    Ok(accounts_path_for(root, &crate::environments::active_id(root)?))
}

/// One saved sign-in session of the ACTIVE environment:
/// `sessions/<env id>/<key>.json`. Keyed by environment id, never by
/// address, so two environments on one address never share a session.
pub fn session_path(root: &Path, key: &str) -> Result<PathBuf, String> {
    Ok(session_path_for(root, &crate::environments::active_id(root)?, key))
}

pub fn session_path_for(root: &Path, env_id: &str, key: &str) -> PathBuf {
    sessions_dir_for(root, env_id).join(format!("{key}.json"))
}

/// One environment's accounts file: `accounts/<env id>.json`. The id is
/// checked by `environments` before it gets here (`env-` and hex only).
pub fn accounts_path_for(root: &Path, env_id: &str) -> PathBuf {
    crate::autorun::roots::shared_of(root).join("accounts").join(format!("{env_id}.json"))
}

/// One environment's saved sign-in sessions: `sessions/<env id>/`.
pub fn sessions_dir_for(root: &Path, env_id: &str) -> PathBuf {
    crate::autorun::roots::shared_of(root).join("sessions").join(env_id)
}

pub fn load_accounts(root: &Path) -> Result<Vec<Account>, String> {
    load_accounts_at(&accounts_path(root)?)
}

/// One environment's accounts, by id - for a caller that has already read
/// the active environment and must not read it a second time.
pub fn load_accounts_for(root: &Path, env_id: &str) -> Result<Vec<Account>, String> {
    load_accounts_at(&accounts_path_for(root, env_id))
}

fn load_accounts_at(path: &Path) -> Result<Vec<Account>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|e| format!("the accounts file is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

pub fn find_account(root: &Path, key: &str) -> Result<Option<Account>, String> {
    Ok(load_accounts(root)?.into_iter().find(|a| a.key == key))
}

/// The account picked for a whole unattended run, checked before anything
/// starts. Blank means none: every script runs as its own account, as
/// before. A key that is not usable, or not on this machine, refuses the
/// run rather than signing nobody in halfway through it.
pub fn account_for_run(root: &Path, key: Option<&str>) -> Result<Option<String>, String> {
    let Some(key) = key.map(str::trim).filter(|k| !k.is_empty()) else {
        return Ok(None);
    };
    if !valid_key(key) {
        return Err(format!("\"{key}\" is not a usable account key"));
    }
    match find_account(root, key)? {
        Some(_) => Ok(Some(key.to_string())),
        None => Err(format!("there is no account \"{key}\" on this machine - add it in Auto Run, Accounts")),
    }
}

/// Replace the whole list. Validated first; written to a temporary file and
/// renamed, so a reader never sees half a list. Returns the keys whose
/// saved session was dropped because the login behind it changed or went.
pub fn save_accounts(root: &Path, accounts: &[Account]) -> Result<Vec<String>, String> {
    validate_accounts(accounts)?;
    let env_id = crate::environments::active_id(root)?;
    save_accounts_for(root, &env_id, accounts)
}

/// One process-wide lock around every read-modify-write of an environment's
/// accounts file and its pending-passwords file. Several assistants (and the
/// dialog) can reset and save at the same moment, and each writes a whole
/// list: without this the last writer wins and a new password is lost.
static FILES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn files() -> std::sync::MutexGuard<'static, ()> {
    FILES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Said when the dialog saves a list another writer changed since it loaded
/// the list.
pub const ACCOUNTS_CHANGED: &str = "an account changed while this was open - reopen Accounts";

/// What the accounts file looks like now (its size and time), for a caller
/// that wants to know later whether anything else wrote it. It carries no
/// password.
pub fn accounts_version(root: &Path, env_id: &str) -> String {
    match std::fs::metadata(accounts_path_for(root, env_id)) {
        Ok(m) => {
            let when = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            format!("{}-{when}", m.len())
        }
        Err(_) => "none".to_string(),
    }
}

/// `save_accounts` for one environment by id - for a caller that read the
/// list by that id and must write it back to the same environment, even
/// if a switch lands in between.
pub fn save_accounts_for(root: &Path, env_id: &str, accounts: &[Account]) -> Result<Vec<String>, String> {
    save_accounts_checked(root, env_id, accounts, None)
}

/// `save_accounts_for`, refusing (`ACCOUNTS_CHANGED`) when `loaded_version`
/// (from `accounts_version`, taken before the list was read) is not what the
/// file looks like now.
pub fn save_accounts_checked(
    root: &Path,
    env_id: &str,
    accounts: &[Account],
    loaded_version: Option<&str>,
) -> Result<Vec<String>, String> {
    let _held = files();
    if let Some(v) = loaded_version {
        if v != accounts_version(root, env_id) {
            return Err(ACCOUNTS_CHANGED.to_string());
        }
    }
    save_unlocked(root, env_id, accounts, None, true)
}

/// The write itself, under `files()`. A login that changed loses its saved
/// session, except `keep_session_of`'s; a person's change of a password (so
/// `person`) also lets go of any password held pending for it, which was set
/// against the old one.
fn save_unlocked(
    root: &Path,
    env_id: &str,
    accounts: &[Account],
    keep_session_of: Option<&str>,
    person: bool,
) -> Result<Vec<String>, String> {
    if !crate::environments::valid_id(env_id) {
        return Err(format!("\"{env_id}\" is not a usable environment id"));
    }
    validate_accounts(accounts)?;
    let env_id = env_id.to_string();
    let before = load_accounts_for(root, &env_id).unwrap_or_default();
    let path = accounts_path_for(root, &env_id);
    std::fs::create_dir_all(path.parent().expect("accounts folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(accounts).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path, &json)?;

    let mut dropped = vec![];
    for old in &before {
        // `before` came straight off disk, unvalidated - a hand-edited
        // accounts.json could carry a key like `../../x` that `session_path`
        // would turn into a delete path outside `sessions/` entirely.
        if !valid_key(&old.key) {
            continue;
        }
        let same_login = accounts
            .iter()
            .any(|a| a.key == old.key && a.username == old.username && a.password == old.password);
        if !same_login {
            if person && accounts.iter().any(|a| a.key == old.key && a.password != old.password) {
                let _ = clear_pending_unlocked(root, &env_id, &old.key);
            }
            if keep_session_of == Some(old.key.as_str()) {
                continue;
            }
            let p = session_path_for(root, &env_id, &old.key);
            if p.exists() {
                let _ = std::fs::remove_file(&p);
                dropped.push(old.key.clone());
            }
        }
    }
    Ok(dropped)
}

/// Give one account of one environment a new password. Everything else in
/// the list stays as it is, and the account's saved session goes, as it does
/// whenever a login changes. Never says either password.
pub fn set_password_for(root: &Path, env_id: &str, key: &str, new: &str) -> Result<(), String> {
    let _held = files();
    change_password_unlocked(root, env_id, key, new, false)
}

/// A pending password that a sign-in has just proved becomes the account's,
/// and is let go from the pending file, in one step under the lock. The
/// session that sign-in saved is kept: it was made with this password.
pub fn commit_pending_password(root: &Path, env_id: &str, key: &str, new: &str) -> Result<(), String> {
    let _held = files();
    change_password_unlocked(root, env_id, key, new, true)?;
    clear_pending_unlocked(root, env_id, key)
}

fn change_password_unlocked(root: &Path, env_id: &str, key: &str, new: &str, keep_session: bool) -> Result<(), String> {
    if new.is_empty() {
        return Err("a new password is needed".to_string());
    }
    let mut all = load_accounts_for(root, env_id)?;
    let Some(account) = all.iter_mut().find(|a| a.key == key) else {
        return Err(format!("there is no account \"{key}\" on this machine"));
    };
    account.password = new.to_string();
    save_unlocked(root, env_id, &all, keep_session.then_some(key), false).map(|_| ())
}

/// Where an environment holds the passwords the app set on a site but has
/// not yet seen work: `accounts/<env id>.pending.json`, key to password,
/// beside the accounts file and kept like it (a plain file on this machine,
/// never sent to the webview, never logged).
pub fn pending_path_for(root: &Path, env_id: &str) -> PathBuf {
    crate::autorun::roots::shared_of(root).join("accounts").join(format!("{env_id}.pending.json"))
}

fn read_pending(root: &Path, env_id: &str) -> std::collections::BTreeMap<String, String> {
    std::fs::read_to_string(pending_path_for(root, env_id))
        .ok()
        .and_then(|s| serde_json::from_str(s.strip_prefix('\u{feff}').unwrap_or(&s)).ok())
        .unwrap_or_default()
}

/// The password set on the site for `key` and not yet confirmed, if any.
pub fn pending_password_for(root: &Path, env_id: &str, key: &str) -> Option<String> {
    let _held = files();
    read_pending(root, env_id).remove(key).filter(|p| !p.is_empty())
}

/// Hold `password` for `key` until a sign-in with it works. Written
/// atomically, before anything is sent to the site.
pub fn set_pending_password(root: &Path, env_id: &str, key: &str, password: &str) -> Result<(), String> {
    if !crate::environments::valid_id(env_id) || !valid_key(key) {
        return Err("that account cannot hold a pending password".to_string());
    }
    let _held = files();
    let mut all = read_pending(root, env_id);
    all.insert(key.to_string(), password.to_string());
    write_pending(root, env_id, &all)
}

/// Let go of `key`'s pending password (confirmed, or found not to work).
pub fn clear_pending_password(root: &Path, env_id: &str, key: &str) -> Result<(), String> {
    let _held = files();
    clear_pending_unlocked(root, env_id, key)
}

fn clear_pending_unlocked(root: &Path, env_id: &str, key: &str) -> Result<(), String> {
    let mut all = read_pending(root, env_id);
    if all.remove(key).is_none() {
        return Ok(());
    }
    write_pending(root, env_id, &all)
}

/// Remove an environment's pending passwords. Called when the environment is
/// removed: a held password is a secret, and an environment created later
/// under the same name must not inherit it.
pub fn forget_pending_for(root: &Path, env_id: &str) {
    let _held = files();
    let _ = std::fs::remove_file(pending_path_for(root, env_id));
}

fn write_pending(root: &Path, env_id: &str, all: &std::collections::BTreeMap<String, String>) -> Result<(), String> {
    let path = pending_path_for(root, env_id);
    if all.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(path.parent().expect("accounts folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(all).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path, &json)
}
