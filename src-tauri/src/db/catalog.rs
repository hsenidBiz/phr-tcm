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
        Err(e) => return Err(format!("The list of databases could not be read: {e}")),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let c: Catalog =
        serde_json::from_str(text).map_err(|e| format!("The list of databases is not readable: {e}"))?;
    check(&c).map_err(|e| format!("The list of databases is not readable: {e}"))?;
    Ok(c)
}

pub fn write(file: Option<&Path>, c: &Catalog) -> Result<(), String> {
    let file = file.ok_or_else(|| NO_LIST.to_string())?;
    check(c)?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("The list of databases could not be saved: {e}"))?;
    }
    let json = serde_json::to_string_pretty(c).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(file, &json)
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
