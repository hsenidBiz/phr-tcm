//! The list naming a person's own databases: an id and a label each, and
//! never a login - every login stays in the secret store under
//! `credentials::target(id)`. Kept in `databases.json` under the app's data
//! folder, written atomically.
//!
//! The list starts as `own` alone ("Your own database"), the one own
//! database there was before there could be several. That starting list is
//! what a missing file reads as - nothing is written on a read - so every
//! environment and every saved choice that names `own` keeps meaning what it
//! meant. Every id a removal gives up is kept in `retired`, so it is never
//! handed out again: something that still names it (a backup, a choice
//! saved before the removal) must not quietly come to mean another database.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// The id kept for the one own database there was before there could be
/// several.
pub const OWN_ID: &str = "own";
pub const OWN_LABEL: &str = "Your own database";
/// The longest label, in characters.
pub const MAX_LABEL_CHARS: usize = 80;

pub const NO_LABEL: &str = "Enter a name for the database.";
pub const LONG_LABEL: &str = "Keep the name to 80 characters or fewer.";
/// Said when a store has no file to keep the list in - only the tests' bare
/// `MemoryStore`, never the app's own store.
pub const NO_LIST: &str = "There is nowhere to keep a list of databases.";
/// Said for a list that is there but cannot be read. The reason - a path,
/// an io or JSON error - goes to the log only: it is nothing a person can
/// act on from the card.
pub const READ_FAILED: &str = "The list of your databases could not be read. Settings → Logs has the details.";
/// Said for a list that could not be saved; the reason is logged.
pub const WRITE_FAILED: &str =
    "The list of your databases could not be saved. Try again - Settings → Logs has the details.";

/// Logs the raw reason and answers the plain sentence.
fn failed(sentence: &str, why: impl std::fmt::Display) -> String {
    crate::applog::warn(format!("database list: {why}"));
    sentence.to_string()
}

/// One of a person's own databases.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CustomDb {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Catalog {
    /// In the order the card lists them.
    pub databases: Vec<CustomDb>,
    /// Ids given up by a removal, never handed out again.
    #[serde(default)]
    pub retired: Vec<String>,
}

impl Catalog {
    /// What a missing file reads as.
    pub fn starting() -> Self {
        Catalog { databases: vec![CustomDb { id: OWN_ID.into(), label: OWN_LABEL.into() }], retired: vec![] }
    }

    pub fn find(&self, id: &str) -> Option<&CustomDb> {
        self.databases.iter().find(|d| d.id == id)
    }
}

/// Every read-modify-write of the list holds this, so two commands at once
/// cannot both take the same id or lose each other's change.
static LOCK: Mutex<()> = Mutex::new(());

pub fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// `own`, or `custom-` and eight lowercase hex digits - the only shape
/// `new_id` makes. The id becomes part of a Credential Manager name, so
/// nothing else is let in.
pub fn valid_id(id: &str) -> bool {
    id == OWN_ID
        || id.strip_prefix("custom-").is_some_and(|hex| {
            hex.len() == 8 && hex.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        })
}

/// A label as it is kept: trimmed, not empty, at most `MAX_LABEL_CHARS`,
/// and unlike every name in `taken`, ignoring case.
pub fn check_label<'a>(label: &str, taken: impl IntoIterator<Item = &'a str>) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() {
        return Err(NO_LABEL.into());
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        return Err(LONG_LABEL.into());
    }
    if label.chars().any(char::is_control) {
        return Err("The name can't contain a line break or a tab.".into());
    }
    let folded = label.to_lowercase();
    if taken.into_iter().any(|t| t.trim().to_lowercase() == folded) {
        return Err(format!("A database named \"{label}\" already exists."));
    }
    Ok(label.to_string())
}

/// What every list must hold before anything trusts it: well-formed ids,
/// each once, and labels a save could have written.
fn check(c: &Catalog) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for (i, d) in c.databases.iter().enumerate() {
        if !valid_id(&d.id) {
            return Err(format!("\"{}\" is not a usable database id", d.id));
        }
        if !ids.insert(d.id.as_str()) {
            return Err(format!("the database id \"{}\" appears more than once", d.id));
        }
        check_label(&d.label, c.databases[..i].iter().map(|o| o.label.as_str()))?;
    }
    Ok(())
}

/// The list in `file`; the starting list when there is no file, or no
/// file yet. A file that exists but cannot be read is an error and is
/// left exactly as it is - never replaced with the starting list.
pub fn read(file: Option<&Path>) -> Result<Catalog, String> {
    let Some(file) = file else { return Ok(Catalog::starting()) };
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Catalog::starting()),
        Err(e) => return Err(failed(READ_FAILED, format!("could not be read: {e}"))),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let c: Catalog =
        serde_json::from_str(text).map_err(|e| failed(READ_FAILED, format!("is not readable: {e}")))?;
    check(&c).map_err(|e| failed(READ_FAILED, format!("is not readable: {e}")))?;
    Ok(c)
}

pub fn write(file: Option<&Path>, c: &Catalog) -> Result<(), String> {
    let file = file.ok_or_else(|| NO_LIST.to_string())?;
    check(c).map_err(|e| failed(WRITE_FAILED, format!("refused a list that would not read back: {e}")))?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| failed(WRITE_FAILED, format!("could not make its folder: {e}")))?;
    }
    let json = serde_json::to_string_pretty(c).map_err(|e| failed(WRITE_FAILED, e))?;
    crate::ai_tools::atomic_write(file, &json).map_err(|e| failed(WRITE_FAILED, e))
}

/// A fresh id: a hash of the label and the time, re-drawn until it is
/// neither in the list nor retired.
pub fn new_id(label: &str, c: &Catalog) -> String {
    use sha2::{Digest, Sha256};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let mut salt = 0u32;
    loop {
        let digest = Sha256::digest(format!("{label}\u{0}{nanos}\u{0}{salt}").as_bytes());
        let id = format!("custom-{:02x}{:02x}{:02x}{:02x}", digest[0], digest[1], digest[2], digest[3]);
        if c.find(&id).is_none() && !c.retired.contains(&id) {
            return id;
        }
        salt += 1;
    }
}

/// `base`, or `base 2`, `base 3`... - the first that `check_label` takes
/// against `taken`. The base is cut short first if a suffix would push it
/// past `MAX_LABEL_CHARS`.
pub fn free_label(base: &str, taken: &[String]) -> String {
    let base = base.trim();
    let mut n = 1u32;
    loop {
        let suffix = if n == 1 { String::new() } else { format!(" {n}") };
        let room = MAX_LABEL_CHARS - suffix.chars().count();
        let cut: String = base.chars().take(room).collect();
        let label = format!("{}{suffix}", cut.trim_end());
        if check_label(&label, taken.iter().map(String::as_str)).is_ok() {
            return label;
        }
        n += 1;
    }
}

/// Fold the list a backup carried into the list in `file` - a merge, never
/// an overwrite, because every entry here may have a login in the vault
/// that an overwrite would leave with no list naming it.
///
/// - Every entry here stays as it is.
/// - Each of the backup's entries whose id is not here is added after them,
///   with no login (the vault never travels): it reads "Not set up yet" and
///   is filled in with Edit, and an environment restored beside it that
///   names its id means the same database again.
/// - The two `retired` lists are combined, and an id retired on either side
///   is never added: a retired id is never on the list.
/// - A name already taken (here, or by a shipped database) gets " 2", then
///   " 3" and so on.
///
/// A backup list that cannot be read is skipped (logged) - the rest of the
/// backup still restores. A list here that cannot be read is an error and
/// is left exactly as it is.
pub fn merge_backup(file: &Path, backup: &[u8]) -> Result<u32, String> {
    let _held = lock();
    let text = String::from_utf8_lossy(backup);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let theirs: Catalog = match serde_json::from_str(text) {
        Ok(c) => c,
        Err(e) => {
            crate::applog::warn(format!("database list: the backup's list is not readable, skipped: {e}"));
            return Ok(0);
        }
    };
    let mut ours = read(Some(file))?;
    for id in theirs.retired.iter().filter(|id| valid_id(id)) {
        if !ours.retired.contains(id) {
            ours.retired.push(id.clone());
        }
    }
    let mut added = 0;
    for d in theirs.databases {
        if !valid_id(&d.id) || ours.find(&d.id).is_some() || ours.retired.contains(&d.id) {
            continue;
        }
        let taken: Vec<String> = crate::db_defaults::DB_PRESETS
            .iter()
            .map(|p| p.label.to_string())
            .chain(ours.databases.iter().map(|o| o.label.clone()))
            .collect();
        let base = if check_label(&d.label, std::iter::empty()).is_ok() { d.label.trim() } else { "Restored database" };
        ours.databases.push(CustomDb { id: d.id, label: free_label(base, &taken) });
        added += 1;
    }
    write(Some(file), &ours)?;
    Ok(added)
}
