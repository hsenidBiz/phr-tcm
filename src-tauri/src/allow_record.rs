//! The record of the "always allow" entries this app wrote into an AI
//! tool's settings files.
//!
//! An entry is the app's only if the app put it there. One that was in the
//! person's file already - hand-written, or chosen by "Always allow" in the
//! tool itself - is theirs, even when it names the same tool, and the app
//! never takes it out. So the app keeps this record, in its own data folder,
//! of exactly the entries it added, per settings file, and removes only
//! entries on it.
//!
//! A file is named in the record by the tool and the file's scope, never by
//! its path: `claude-code:user`, `claude-code:project:<hash of the
//! repository folder>`. (The hash keeps one repository's entries apart from
//! another's; it is not the folder's name, and no path is written to the log.)
//!
//! The database switch's entries were written by versions before this record
//! existed, so they are on no record. `legacy_db` covers them once: it is set
//! when the record is first created if the switch was on then (an older
//! version wrote them), and ends with the first removal after that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The record's file, in the app's data folder.
pub const FILE_NAME: &str = "allow-written.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowRecord {
    /// Per settings file (see `scope_key`), the entries the app wrote.
    #[serde(default)]
    pub files: BTreeMap<String, Vec<String>>,
    /// The database switch's entries may be taken out without being on this
    /// record (see the module docs). Ends with the first removal.
    #[serde(default)]
    pub legacy_db: bool,
    /// The record is not what is on disk (a new record, or a change).
    #[serde(skip)]
    dirty: bool,
}

/// The settings file of `tool` (`claude-code`, `cursor`) that is the
/// machine's (`repo: None`) or the repository `repo`'s.
pub fn scope_key(tool: &str, repo: Option<&str>) -> String {
    match repo {
        None => format!("{tool}:user"),
        Some(r) => {
            let r = r.trim().replace('\\', "/");
            let r = r.trim_end_matches('/').to_lowercase();
            let digest: String = Sha256::digest(r.as_bytes()).iter().take(6).map(|b| format!("{b:02x}")).collect();
            format!("{tool}:project:{digest}")
        }
    }
}

impl AllowRecord {
    pub fn path(root: &Path) -> PathBuf {
        crate::autorun::roots::shared_of(root).join(FILE_NAME)
    }

    /// The record kept in `root`. When there is none, or it cannot be read,
    /// a new one: `legacy_db` is `db_now`, the database switch as it stands
    /// at this first look, and the record is marked to be saved.
    pub fn load(root: &Path, db_now: bool) -> AllowRecord {
        let kept = std::fs::read_to_string(Self::path(root))
            .ok()
            .and_then(|text| serde_json::from_str::<AllowRecord>(&text).ok());
        kept.unwrap_or(AllowRecord { files: BTreeMap::new(), legacy_db: db_now, dirty: true })
    }

    /// Whether the app wrote `entry` into the file `key`.
    pub fn holds(&self, key: &str, entry: &str) -> bool {
        self.files.get(key).is_some_and(|l| l.iter().any(|e| e == entry))
    }

    /// Note that the app wrote `entries` into the file `key`.
    pub fn add(&mut self, key: &str, entries: &[&str]) {
        let list = self.files.entry(key.to_string()).or_default();
        for e in entries {
            if !list.iter().any(|x| x == e) {
                list.push((*e).to_string());
                self.dirty = true;
            }
        }
    }

    /// Note that `entries` are no longer in the file `key` (taken out, or
    /// never written after all).
    pub fn forget(&mut self, key: &str, entries: &[&str]) {
        let Some(list) = self.files.get_mut(key) else { return };
        let before = list.len();
        list.retain(|x| !entries.iter().any(|e| e == x));
        if list.len() != before {
            self.dirty = true;
        }
        if list.is_empty() {
            self.files.remove(key);
        }
    }

    /// The database switch's older entries have had their one removal.
    pub fn end_legacy_db(&mut self) {
        if self.legacy_db {
            self.legacy_db = false;
            self.dirty = true;
        }
    }

    /// Write the record when it is not what is on disk.
    pub fn save(&mut self, root: &Path) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| format!("could not write the record: {e}"))?;
        if let Err(e) = std::fs::create_dir_all(crate::autorun::roots::shared_of(root)) {
            return Err(format!("could not create the record's folder: {e}"));
        }
        // The path is the app's own data folder; the message carries none.
        crate::ai_tools::atomic_write(&Self::path(root), &text).map_err(|_| "could not save the record".to_string())?;
        self.dirty = false;
        Ok(())
    }
}
