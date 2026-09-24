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
/// the key is also a file name (`sessions/<key>.json`).
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

fn accounts_path(root: &Path) -> PathBuf {
    root.join("accounts.json")
}

pub fn session_path(root: &Path, key: &str) -> PathBuf {
    root.join("sessions").join(format!("{key}.json"))
}

pub fn load_accounts(root: &Path) -> Result<Vec<Account>, String> {
    match std::fs::read_to_string(accounts_path(root)) {
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
    let before = load_accounts(root).unwrap_or_default();
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(accounts).map_err(|e| e.to_string())?;
    let path = accounts_path(root);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }

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
            let p = session_path(root, &old.key);
            if p.exists() {
                let _ = std::fs::remove_file(&p);
                dropped.push(old.key.clone());
            }
        }
    }
    Ok(dropped)
}
