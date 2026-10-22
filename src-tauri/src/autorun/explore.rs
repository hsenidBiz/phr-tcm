//! An explore session's change list: one JSON file per session under
//! `<Auto Run root>/explore/`, written as the assistant does a plain task
//! in the app under test with no test case. It outlives the chat: what the
//! session created, changed or deleted is there for the person to read and
//! for an undo through Clean up to work from.
//!
//! The file holds ids, names, field values and `METHOD /path` request
//! lines - never a password, a cookie, a token, a host or a query string.
//!
//! The assistant records each change (`record_change`); a create is only
//! ever "verified" by a save this session's own browser made that answered
//! with the new id, and only a verified create reaches the test-made record
//! an undo works from. The session also notes every save the browser sent while it ran
//! (`note_writes`), by method and path alone. When the session ends, a save
//! no change names is handed back (`unaccounted`), so the assistant either
//! names it in its summary or records it.

use super::discovery_map::path_only;
use super::test_made::{self, TestMade};
use crate::browser::net_capture::{Body, Captured};
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

/// What `unreadable` starts with, so a caller can tell a file that does not
/// read from an id that names no file.
const UNREADABLE: &str = "the explore session's change list is not readable";

fn unreadable(why: &str) -> String {
    format!("{UNREADABLE}: {why}")
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

/// Said to a change recorded with no explore session going.
pub const NO_EXPLORE_SESSION: &str =
    "there is no explore session going - start one with start_autorun_discovery and explore: true";

/// Said when a session's change list file does not read: nothing was
/// changed in it.
pub const CHANGES_UNREADABLE: &str =
    "this session's change list could not be read, so nothing was changed in it (see Settings -> Logs)";

/// Said when a change could not be written to the change list.
pub const CHANGE_NOT_KEPT: &str = "the change could not be kept on the change list (see Settings -> Logs)";

/// Said with a create no save of the session answered for.
pub const NOT_VERIFIED: &str = "Not verified: no save this session made answered with that id, so undo can remove it only in the browser, never through a delete template.";

/// Said with a change recorded without what the field held before.
pub const BEFORE_NOT_KNOWN: &str = "Before not known: undo will say it cannot set this back.";

/// Said with a change to a field that holds a secret.
pub const SECRET_NOT_KEPT: &str = "This field holds a secret, so the change list keeps neither value: undo will say it cannot set it back.";

/// Said with a delete.
pub const DELETED_STAYS: &str = "Kept as a delete, which cannot be undone; what it held is kept for your summary.";

/// Said for an entry number the session's list does not have.
pub fn no_such_entry(n: u32) -> String {
    format!("this session has no entry {n}")
}

/// Said for an entry that is a verified create: only the undo tool marks it.
pub fn undone_by_the_tool(n: u32) -> String {
    format!("entry {n} is a verified create: undo_explore_creates removes it, and marks it undone")
}

/// Whether `why` is `unknown_session`'s sentence.
pub fn is_unknown_session(why: &str) -> bool {
    why.starts_with("there is no explore session called")
}

/// A field's text: a string, or a number as it is written. Never empty.
fn text_field(v: &Value, key: &str) -> Option<String> {
    let text = match v.get(key)? {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// A value, unless it is absent or null.
fn present(v: &Value, key: &str) -> Option<Value> {
    v.get(key).filter(|x| !x.is_null()).cloned()
}

/// A change as `record_explore_change` is given it:
/// `{ change: created | changed | deleted, ... }`. A created change is
/// never `verified` here: `record_change` checks that itself.
pub fn change_from_args(v: &Value) -> Result<Change, String> {
    let which = v.get("change").and_then(Value::as_str).map(|s| s.trim().to_ascii_lowercase());
    match which.as_deref() {
        Some("created") => match (text_field(v, "kind"), text_field(v, "id"), text_field(v, "name")) {
            (Some(kind), Some(id), Some(name)) => Ok(Change::Created { kind, id, name, verified: false }),
            _ => Err("a created change needs its kind, id and name: { \"change\": \"created\", \"kind\": <what it is>, \"id\": <its id>, \"name\": <its name>, \"request\": <POST /path, optional> }".to_string()),
        },
        Some("changed") => match (text_field(v, "record"), text_field(v, "field"), v.get("after")) {
            (Some(record), Some(field), Some(after)) => Ok(Change::Changed {
                record,
                field,
                before: present(v, "before"),
                after: Some(after.clone()),
                hidden: false,
            }),
            _ => Err("a changed change needs its record, field and after (and before, when you know it): { \"change\": \"changed\", \"record\": <which record>, \"field\": <which field>, \"before\": <optional>, \"after\": <the new value>, \"request\": <optional> }".to_string()),
        },
        Some("deleted") => match (text_field(v, "kind"), text_field(v, "id"), text_field(v, "name")) {
            (Some(kind), Some(id), Some(name)) => Ok(Change::Deleted { kind, id, name, held: present(v, "held") }),
            _ => Err("a deleted change needs its kind, id and name: { \"change\": \"deleted\", \"kind\": <what it was>, \"id\": <its id>, \"name\": <its name>, \"held\": <what it held, optional>, \"request\": <optional> }".to_string()),
        },
        _ => Err("\"change\" is created, changed or deleted: { \"change\": \"created\" | \"changed\" | \"deleted\", ... }".to_string()),
    }
}

/// The words of a text body: runs of letters, digits, `_` and `-`, a `.`
/// between two letters or digits staying inside the word.
fn words(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut word = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let inside = c.is_alphanumeric()
            || matches!(c, '_' | '-')
            || (c == '.'
                && i > 0
                && chars[i - 1].is_alphanumeric()
                && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric()));
        if inside {
            word.push(c);
        } else if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// Whether `v` holds `id` as a whole value, a string or a number, anywhere.
fn json_carries(v: &Value, id: &str) -> bool {
    match v {
        Value::String(s) => s.trim() == id,
        Value::Number(n) => n.to_string() == id,
        Value::Array(a) => a.iter().any(|x| json_carries(x, id)),
        Value::Object(o) => o.values().any(|x| json_carries(x, id)),
        _ => false,
    }
}

fn body_carries(text: &str, id: &str) -> bool {
    match serde_json::from_str::<Value>(text.trim()) {
        Ok(v) => json_carries(&v, id),
        Err(_) => words(text).iter().any(|w| w == id),
    }
}

/// The first save of the session's own browser that created `id`: a request
/// that creates or changes (POST, PUT or PATCH - never a GET and never a
/// DELETE), started at or after `since_ms`, answered with a 2xx (or none
/// recorded) and a response that carries `id` as a whole JSON value (a
/// string or a number, anywhere in a JSON body) or as a whole word of a text
/// body - never `1274` for `274`. Hands back its `request_line`.
pub fn verify_create(captured: &[&Captured], since_ms: u64, id: &str) -> Option<String> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    captured
        .iter()
        .filter(|c| ["POST", "PUT", "PATCH"].iter().any(|m| c.method.eq_ignore_ascii_case(m)))
        .filter(|c| c.started_ms >= since_ms)
        .filter(|c| c.status.is_none_or(|s| (200..300).contains(&s)))
        .filter(|c| matches!(&c.resp_body, Some(Body::Text(t)) if body_carries(t, id)))
        .min_by_key(|c| c.started_ms)
        .map(|c| request_line(&c.method, &c.url_path_query))
}

/// What recording a change answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recorded {
    /// The entry's number on the list.
    pub n: u32,
    /// For a create: whether a save this session made answered with its id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    /// What the list lacks for this entry, said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `METHOD /path` from what the assistant sent as a change's request: any
/// host or query is dropped. Anything that is not a method and a path is no
/// request.
fn request_of(request: Option<&str>) -> Option<String> {
    let (method, path) = request?.trim().split_once(char::is_whitespace)?;
    let path = path.trim();
    (!method.is_empty() && method.chars().all(|c| c.is_ascii_alphabetic()) && !path.is_empty())
        .then(|| request_line(method, path))
}

/// `v` with every value under a secret-looking name hidden.
fn scrubbed(v: Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, x)| {
                    if crate::autorun::capture_redact::secret_name(&k) {
                        (k, Value::String(crate::browser::net_capture::HIDDEN.to_string()))
                    } else {
                        (k, scrubbed(x))
                    }
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(scrubbed).collect()),
        other => other,
    }
}

/// The session's file for a change: a file that does not read is
/// `CHANGES_UNREADABLE` (logged), an id with no file `unknown_session`.
fn read_for_change(root: &Path, session: &str) -> Result<ExploreSession, String> {
    load_locked(root, session).map_err(|e| {
        if e.starts_with(UNREADABLE) {
            crate::applog::warn(format!("Auto Run explore session: {e}"));
            CHANGES_UNREADABLE.to_string()
        } else {
            e
        }
    })
}

fn write_for_change(root: &Path, s: &ExploreSession) -> Result<(), String> {
    write_locked(root, s).map_err(|e| {
        crate::applog::warn(format!("Auto Run explore session: the change list could not be written: {e}"));
        CHANGE_NOT_KEPT.to_string()
    })
}

/// Puts a verified create on the test-made list, unless this session's
/// entry for it is there already.
fn keep_made(root: &Path, s: &ExploreSession, kind: &str, id: &str, name: &str) -> Result<(), String> {
    let there = test_made::list(root).iter().any(|e| {
        e.fixture == super::cleanup::EXPLORE_MADE
            && e.run_id == s.session
            && e.environment == s.environment
            && e.kind == kind
            && e.id == id
    });
    if there {
        return Ok(());
    }
    test_made::record_explore(
        root,
        TestMade {
            environment: s.environment.clone(),
            kind: kind.to_string(),
            id: id.to_string(),
            name: name.to_string(),
            created_at: now(),
            fixture: super::cleanup::EXPLORE_MADE.to_string(),
            run_id: s.session.clone(),
            case_id: None,
            status: test_made::PRESENT.to_string(),
        },
    )
    .map_err(|e| {
        crate::applog::warn(format!("Auto Run explore session: the create could not be put on the test-made list: {e}"));
        CHANGE_NOT_KEPT.to_string()
    })
}

/// Adds `change` to the end of `s` and writes the file. A verified create
/// goes on the test-made list first, so the list never says "verified" for
/// something an undo cannot find.
fn keep(root: &Path, s: &mut ExploreSession, change: Change, request: Option<String>) -> Result<u32, String> {
    if let Change::Created { kind, id, name, verified: true } = &change {
        keep_made(root, s, kind, id, name)?;
    }
    let n = u32::try_from(s.changes.len()).unwrap_or(u32::MAX).saturating_add(1);
    s.changes.push(ChangeEntry { n, at: now(), change, request, undone: false });
    write_for_change(root, s)?;
    Ok(n)
}

/// Adds `change` to `session`'s list. A created change is checked against
/// `captured` here (`verify_create`, saves started at or after `since_ms`),
/// never taken from the caller; a verified one is also put on the test-made
/// list (fixture `cleanup::EXPLORE_MADE`, the session as the run, the
/// session's environment, status present). A changed field whose name looks
/// like a secret keeps neither value and is `hidden`. A file that does not
/// read is `CHANGES_UNREADABLE` and is left as it is; a write that fails is
/// `CHANGE_NOT_KEPT`.
pub fn record_change(
    root: &Path,
    session: &str,
    change: Change,
    request: Option<String>,
    captured: &[&Captured],
    since_ms: u64,
) -> Result<Recorded, String> {
    let _held = lock();
    let mut s = read_for_change(root, session)?;
    let mut request = request_of(request.as_deref());
    let mut change = change;
    let (mut verified, mut note) = (None, None);
    match &mut change {
        Change::Created { id, verified: ok, .. } => match verify_create(captured, since_ms, id) {
            Some(line) => {
                *ok = true;
                request = Some(line);
                verified = Some(true);
            }
            None => {
                *ok = false;
                verified = Some(false);
                note = Some(NOT_VERIFIED);
            }
        },
        Change::Changed { field, before, after, hidden, .. } => {
            if crate::autorun::capture_redact::secret_name(field) {
                *before = None;
                *after = None;
                *hidden = true;
                note = Some(SECRET_NOT_KEPT);
            } else if before.is_none() {
                note = Some(BEFORE_NOT_KNOWN);
            }
        }
        Change::Deleted { held, .. } => {
            *held = held.take().map(scrubbed);
            note = Some(DELETED_STAYS);
        }
    }
    let n = keep(root, &mut s, change, request)?;
    Ok(Recorded { n, verified, note: note.map(str::to_string) })
}

/// The create a `send_api_request` call made, whose id the runner took from
/// the response itself - so it needs no check against the page's saves.
/// Kept as a verified create. Hands back its entry number.
pub(crate) fn record_send_create(
    root: &Path,
    session: &str,
    kind: &str,
    id: &Value,
    name: &str,
    request: &str,
) -> Result<u32, String> {
    let args = serde_json::json!({ "kind": kind, "id": id, "name": name });
    let (Some(kind), Some(id), Some(name)) = (text_field(&args, "kind"), text_field(&args, "id"), text_field(&args, "name"))
    else {
        return Err(CHANGE_NOT_KEPT.to_string());
    };
    let _held = lock();
    let mut s = read_for_change(root, session)?;
    keep(root, &mut s, Change::Created { kind, id, name, verified: true }, request_of(Some(request)))
}

/// Marks the entries `entries` undone: a change the assistant set back by
/// hand. A verified create is refused (`undone_by_the_tool`) - only the undo
/// tool removes one, and marks it - and an entry number the list does not
/// have is `no_such_entry`. Either refuses all of them, and nothing is
/// written.
pub fn mark_undone(root: &Path, session: &str, entries: &[u32]) -> Result<(), String> {
    let _held = lock();
    let mut s = read_for_change(root, session)?;
    for &n in entries {
        match s.changes.iter().find(|c| c.n == n) {
            None => return Err(no_such_entry(n)),
            Some(ChangeEntry { change: Change::Created { verified: true, .. }, .. }) => {
                return Err(undone_by_the_tool(n))
            }
            Some(_) => {}
        }
    }
    if entries.is_empty() {
        return Ok(());
    }
    for c in s.changes.iter_mut().filter(|c| entries.contains(&c.n)) {
        c.undone = true;
    }
    write_for_change(root, &s)
}

/// Marks the create of `kind` `id` undone, once the undo tool removed it.
/// `Ok(false)` when the list has no such create.
pub fn mark_create_undone(root: &Path, session: &str, kind: &str, id: &str) -> Result<bool, String> {
    let _held = lock();
    let mut s = read_for_change(root, session)?;
    let mut found = false;
    for c in s.changes.iter_mut() {
        if matches!(&c.change, Change::Created { kind: k, id: i, .. } if k == kind && i == id) {
            c.undone = true;
            found = true;
        }
    }
    if found {
        write_for_change(root, &s)?;
    }
    Ok(found)
}

/// This project's explore sessions, newest first. A file that does not read
/// is logged and left out.
pub fn list(root: &Path, org: &str, project: &str) -> Vec<ExploreSession> {
    let _held = lock();
    let Ok(dir) = std::fs::read_dir(root.join(EXPLORE_DIR)) else {
        return Vec::new();
    };
    let mut out: Vec<ExploreSession> = Vec::new();
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if !safe_session_id(stem) {
            continue;
        }
        match load_locked(root, stem) {
            Ok(s) if s.organization.eq_ignore_ascii_case(org) && s.project.eq_ignore_ascii_case(project) => out.push(s),
            Ok(_) => {}
            Err(e) => crate::applog::warn(format!("Auto Run explore session: a change list was skipped: {e}")),
        }
    }
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at).then_with(|| b.session.cmp(&a.session)));
    out
}
