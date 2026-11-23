//! Whether a database login may create, update and delete - decided by the
//! server, not by what the login is called.
//!
//! The app asks once, as the database's own login, through the same sqlcmd
//! runner every other statement uses (so it passes the same guard: it is a
//! plain SELECT). The answer is a stored verdict, kept in the Rust cache
//! under a tag of the user, server and database it was asked for. The
//! guard reads the verdict; it never asks the server on a query.
//!
//! Fail closed: a NULL, a half answer, a refusal or no answer at all is
//! read only. A login that was edited has a different tag, so it starts
//! with no verdict. The password is never part of the tag, the verdict or
//! a log line.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

use super::schema::{is_footer, is_rule};
use super::sqlcmd::{run_sql, Connection, Runner};
use crate::cache::{self, keys};

/// Said when the login may only read.
pub const READ_ONLY_REASON: &str = "This login can only read this database.";
/// Said when the login may write.
pub const CAN_WRITE_REASON: &str = "This login can create, update and delete.";
/// Said when the check itself failed. Names no server and no transport text.
pub const CHECK_FAILED_REASON: &str =
    "Could not check this login's permissions. Try Recheck, or see Settings → Logs.";

/// The one read-only question put to the server: the login's permissions on
/// the database it signed in to, and its two write roles.
pub const CHECK_SQL: &str = "SELECT HAS_PERMS_BY_NAME(DB_NAME(), 'DATABASE', 'INSERT') AS i, HAS_PERMS_BY_NAME(DB_NAME(), 'DATABASE', 'UPDATE') AS u, HAS_PERMS_BY_NAME(DB_NAME(), 'DATABASE', 'DELETE') AS d, IS_ROLEMEMBER('db_datawriter') AS w, IS_ROLEMEMBER('db_owner') AS o";

/// What is stored for one login.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    pub can_write: bool,
    /// Unix epoch milliseconds.
    pub checked_at: u64,
}

/// What the webview is told: the verdict, when it was reached, and a short
/// sentence. Nothing else - never a password, never the server's own words.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct WriteCheck {
    pub can_write: bool,
    /// Unix epoch milliseconds.
    pub checked_at: f64,
    pub reason: String,
}

impl WriteCheck {
    pub fn from_stored(s: &Stored) -> WriteCheck {
        WriteCheck {
            can_write: s.can_write,
            checked_at: s.checked_at as f64,
            reason: if s.can_write { CAN_WRITE_REASON } else { READ_ONLY_REASON }.to_string(),
        }
    }
}

/// A non-reversible tag of the login a verdict belongs to: user, server and
/// database, folded to lower case. The password is not in it.
pub fn tag(c: &Connection) -> String {
    let text = format!("{}|{}|{}", c.user.trim(), c.server.trim(), c.database.trim()).to_lowercase();
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// The verdict stored for `c`'s login, if one was ever reached.
pub fn stored(c: &Connection) -> Option<Stored> {
    cache::get::<Stored>(&keys::db_write_verdict(&tag(c)))
}

/// Whether `c`'s login has a stored can-write verdict. The one question the
/// guard asks.
pub fn can_write(c: &Connection) -> bool {
    stored(c).is_some_and(|s| s.can_write)
}

/// Keeps `can_write` as `c`'s verdict, stamped now.
pub fn store(c: &Connection, can_write: bool) -> Stored {
    store_at(c, can_write, cache::now_ms())
}

/// `store` with the time given, for tests.
pub fn store_at(c: &Connection, can_write: bool, checked_at: u64) -> Stored {
    let s = Stored { can_write, checked_at };
    cache::put(&keys::db_write_verdict(&tag(c)), &s);
    s
}

/// Drops `c`'s verdict.
pub fn forget(c: &Connection) {
    cache::forget(&keys::db_write_verdict(&tag(c)));
}

/// Reads the answer to `CHECK_SQL`. `Some(true)` only for all three
/// permissions, or `db_datawriter`, or `db_owner`; anything else that parses
/// is `Some(false)`; text with no answer row in it is `None`.
pub fn parse_verdict(tsv: &str) -> Option<bool> {
    let cell = |s: &str| -> Option<bool> {
        match s.trim() {
            "1" => Some(true),
            "0" => Some(false),
            c if c.eq_ignore_ascii_case("null") => Some(false),
            _ => None,
        }
    };
    for line in tsv.lines().map(|l| l.trim_end_matches('\r')) {
        if line.trim().is_empty() || is_rule(line) || is_footer(line) {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').collect();
        if cells.len() != 5 {
            continue;
        }
        let parsed: Option<Vec<bool>> = cells.iter().map(|c| cell(c)).collect();
        if let Some(v) = parsed {
            return Some((v[0] && v[1] && v[2]) || v[3] || v[4]);
        }
    }
    None
}

/// Asks the server and stores the answer. A failure of any kind removes any
/// verdict (no verdict is read only - fail closed), logs the raw cause with `applog`, and
/// answers `Err` with the sentence for the person.
pub async fn check<R: Runner>(r: &R, exe: &Path, c: &Connection) -> Result<WriteCheck, String> {
    match run_sql(r, exe, c, CHECK_SQL).await {
        Ok((text, _)) => match parse_verdict(&text) {
            Some(can) => Ok(WriteCheck::from_stored(&store(c, can))),
            None => {
                crate::applog::warn(format!("db: permission check on {}/{} gave no answer row", c.server, c.database));
                forget(c);
                Err(CHECK_FAILED_REASON.to_string())
            }
        },
        Err(said) => {
            crate::applog::warn(format!("db: permission check on {}/{} failed: {said}", c.server, c.database));
            forget(c);
            Err(CHECK_FAILED_REASON.to_string())
        }
    }
}
