//! An explore session's change list: one JSON file per session under
//! `<Auto Run root>/explore/`, written as the assistant does a plain task
//! in the app under test with no test case. It outlives the chat: what the
//! session created, changed or deleted is there for the person to read and
//! for an undo through Clean up to work from.
//!
//! The file holds ids, names, field values and `METHOD /path` request
//! lines - never a password, a cookie, a token, a host or a query string.
//!
//! The session also notes every save the browser sent while it ran
//! (`note_writes`), by method and path alone. When the session ends, a save
//! no change names is handed back (`unaccounted`), so the assistant either
//! names it in its summary or records it.

use super::discovery_map::path_only;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The folder under the Auto Run root that holds the change lists.
pub const EXPLORE_DIR: &str = "explore";

/// An explore session going on a discovery (`DiscoveryState::explore`).
/// `session` is set once its change list file exists.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExploreRun {
    pub session: Option<String>,
}

/// One explore session's change list, as its file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExploreSession {
    pub session: String,
    pub organization: String,
    pub project: String,
    /// The id and the name of the environment the session ran in.
    pub environment: String,
    pub environment_name: String,
    /// The account KEY it signed in as, never a login.
    pub account: String,
    /// RFC 3339, UTC.
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub changes: Vec<ChangeEntry>,
    /// Every save the session's browser sent, in order.
    #[serde(default)]
    pub writes: Vec<SeenWrite>,
}

/// A save the browser sent: method and path alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeenWrite {
    pub method: String,
    pub path: String,
    pub at: String,
}

/// One line of the change list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeEntry {
    pub n: u32,
    pub at: String,
    #[serde(flatten)]
    pub change: Change,
    /// The request that made the change, `POST /path`, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    #[serde(default)]
    pub undone: bool,
}

/// What a change was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "lowercase")]
pub enum Change {
    Created { kind: String, id: String, name: String, verified: bool },
    Changed {
        record: String,
        field: String,
        before: Option<Value>,
        after: Option<Value>,
        #[serde(default)]
        hidden: bool,
    },
    Deleted { kind: String, id: String, name: String, held: Option<Value> },
}

/// How an explore session ended: how many changes it listed and the saves
/// its browser sent that no change names.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExploreEnd {
    pub session: String,
    pub changes: usize,
    pub saves_not_on_the_change_list: Vec<String>,
}

/// Said with the end of a session whose browser sent saves no change names.
pub const UNRECORDED_SAVES: &str =
    "These saves are not on the change list: name each in your summary, or record it with record_explore_change.";

/// Every read-change-write of a session file holds this.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

fn rfc3339(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(i64::try_from(ms).unwrap_or(i64::MAX))
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn now() -> String {
    rfc3339(super::sessions::now_ms())
}

/// The longest account key a session id keeps.
const KEY_MAX: usize = 80;

/// `YYYYMMDD-HHMMSS-<account key>` (UTC) for a session started at
/// `started_at_ms`. A character of the key outside `[A-Za-z0-9._-]` becomes
/// `-`, and a `..` becomes `.`, so the id is a file name and never a path.
pub fn session_id(started_at_ms: u64, account: &str) -> String {
    let stamp = chrono::DateTime::from_timestamp_millis(i64::try_from(started_at_ms).unwrap_or(i64::MAX))
        .unwrap_or_default()
        .format("%Y%m%d-%H%M%S");
    let mut key: String = account
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' })
        .take(KEY_MAX)
        .collect();
    while key.contains("..") {
        key = key.replace("..", ".");
    }
    if key.is_empty() {
        key.push_str("account");
    }
    format!("{stamp}-{key}")
}

/// Whether `id` can be a session's file name: not empty, at most 120
/// characters, only `[A-Za-z0-9._-]` and no `..`.
pub fn safe_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 120
        && !id.contains("..")
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Where a session's change list is.
pub fn session_path(root: &Path, session: &str) -> PathBuf {
    root.join(EXPLORE_DIR).join(format!("{session}.json"))
}

/// Said for an id no session file answers to.
pub fn unknown_session(id: &str) -> String {
    format!("there is no explore session called \"{id}\" - get_explore_changes lists them")
}

fn unreadable(why: &str) -> String {
    format!("the explore session's change list is not readable: {why}")
}

fn load_locked(root: &Path, session: &str) -> Result<ExploreSession, String> {
    if !safe_session_id(session) {
        return Err(unknown_session(session));
    }
    match std::fs::read_to_string(session_path(root, session)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|e| unreadable(&e.to_string()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(unknown_session(session)),
        Err(e) => Err(unreadable(&e.to_string())),
    }
}

fn write_locked(root: &Path, s: &ExploreSession) -> Result<(), String> {
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&session_path(root, &s.session), &json)
}

/// Starts a session's change list for the signed-in account `account` (a
/// KEY) in the active environment: a new file, never an existing one - an
/// id already taken gets `-2`, `-3` and so on. Hands back the session id.
pub fn begin(root: &Path, org: &str, project: &str, account: &str, started_at_ms: u64) -> Result<String, String> {
    let env = crate::environments::active(root)?;
    let _held = lock();
    let dir = root.join(EXPLORE_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("the explore folder could not be made: {e}"))?;
    let base = session_id(started_at_ms, account);
    let mut id = base.clone();
    let mut n = 1u32;
    // The name is taken by creating the file, so two sessions never share one.
    loop {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(session_path(root, &id)) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                n += 1;
                id = format!("{base}-{n}");
            }
            Err(e) => return Err(format!("the explore session's change list could not be made: {e}")),
        }
    }
    let s = ExploreSession {
        session: id.clone(),
        organization: org.to_string(),
        project: project.to_string(),
        environment: env.id,
        environment_name: env.name,
        account: account.to_string(),
        started_at: rfc3339(started_at_ms),
        ended_at: None,
        changes: Vec::new(),
        writes: Vec::new(),
    };
    if let Err(e) = write_locked(root, &s) {
        let _ = std::fs::remove_file(session_path(root, &id));
        return Err(e);
    }
    Ok(id)
}

/// The session's change list as its file holds it. An id that is not a
/// file name is refused as an unknown session.
pub fn load(root: &Path, session: &str) -> Result<ExploreSession, String> {
    let _held = lock();
    load_locked(root, session)
}

/// `METHOD /path`: the method upper-cased and the path with no host, query
/// or fragment.
pub fn request_line(method: &str, path: &str) -> String {
    format!("{} {}", method.trim().to_ascii_uppercase(), path_only(path))
}

/// Adds the saves `writes` (method, path) to the session's file. A file
/// that does not read is an error and is left as it is.
pub fn note_writes(root: &Path, session: &str, writes: &[(String, String)]) -> Result<(), String> {
    if writes.is_empty() {
        return Ok(());
    }
    let _held = lock();
    let mut s = load_locked(root, session)?;
    let at = now();
    for (method, path) in writes {
        s.writes.push(SeenWrite {
            method: method.trim().to_ascii_uppercase(),
            path: path_only(path),
            at: at.clone(),
        });
    }
    write_locked(root, &s)
}

/// A request line as it is compared: lower case, no query, no trailing `/`.
fn compared(line: &str) -> String {
    let line = line.trim();
    let (method, path) = line.split_once(' ').unwrap_or(("", line));
    let path = path_only(path);
    let path = if path.len() > 1 { path.trim_end_matches('/') } else { path.as_str() };
    format!("{} {}", method.trim().to_ascii_lowercase(), path.to_ascii_lowercase())
}

/// The saves the session's browser sent that no change's `request` names,
/// each once, in the order they were sent, each as `request_line`. Compared
/// ignoring case and a trailing `/`.
pub fn unaccounted(s: &ExploreSession) -> Vec<String> {
    let named: std::collections::HashSet<String> =
        s.changes.iter().filter_map(|c| c.request.as_deref()).map(compared).collect();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for w in &s.writes {
        let line = request_line(&w.method, &w.path);
        let key = compared(&line);
        if !named.contains(&key) && seen.insert(key) {
            out.push(line);
        }
    }
    out
}

/// Ends the session: its file gets `ended_at`, and the answer counts its
/// changes and names the saves it did not record.
pub fn end(root: &Path, session: &str) -> Result<ExploreEnd, String> {
    let _held = lock();
    let mut s = load_locked(root, session)?;
    if s.ended_at.is_none() {
        s.ended_at = Some(now());
        write_locked(root, &s)?;
    }
    Ok(ExploreEnd {
        session: s.session.clone(),
        changes: s.changes.len(),
        saves_not_on_the_change_list: unaccounted(&s),
    })
}
