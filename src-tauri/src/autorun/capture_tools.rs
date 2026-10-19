//! What the assistant's capture tools answer: the supervised browser's
//! captured requests (`net_capture`), or a set imported from a HAR file,
//! listed, searched and read one at a time - and the capture's filter, its
//! export to TCM's own `captures` folder and an import from there.
//!
//! Every value an answer shows passes the capture's redaction
//! (`capture_redact`) first: the path and query, each header value, the
//! request and response bodies, a failure and a tab name. A search matches
//! the REDACTED text, so asking for a secret's value can never confirm it.
//! The live store is read only through its `list` and `get`, so a request
//! held back while the application's site is not confirmed never shows.
//!
//! The functions here are pure over what they are handed; the bridge
//! (`ai_bridge::capture_tool_in`) finds the store and the imported sets.

use crate::autorun::capture_redact::{redact_body, redact_header, redact_path_query, redact_text};
use crate::browser::net_capture::{Body, CaptureFilter, Captured, NetCapture, MAX_CAPTURED};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The imported sets, by name. Each set is shared, so a copy of the whole
/// (what the Rust cache hands a reader, `commands::autorun::imported_captures`)
/// copies the names and pointers, never the requests.
pub type ImportedSets = BTreeMap<String, Arc<Vec<Captured>>>;

/// A body's page: this many characters at most.
pub const BODY_PAGE_CHARS: usize = 20_000;
/// Requests one list or search answer carries when none is asked.
pub const DEFAULT_LIMIT: usize = 25;
/// Requests one list or search answer carries at most.
pub const MAX_LIMIT: usize = 100;
/// The longest export or import name (a file stem).
pub const MAX_NAME_CHARS: usize = 80;
/// The longest `path_contains` a filter takes.
pub const MAX_FILTER_PATH_CHARS: usize = 200;
/// The largest HAR file an import reads.
pub const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;
/// TCM's own folder for exports and imports, under the Auto Run root.
pub const CAPTURES_DIR: &str = "captures";
/// The resource types the capture keeps, as Chrome names them.
pub const CAPTURE_TYPES: [&str; 3] = ["XHR", "Fetch", "Document"];

/// Said, with nothing listed, while the application's site is not
/// confirmed: nothing is shown until a sign-in arrives on it.
pub const NOTHING_CAPTURED_YET: &str = "Nothing captured yet: the app's site is confirmed once TCM signs in (or reaches the app's own address). Sign in through a replay or a discovery, then try again.";

/// Said with nothing listed once the site is confirmed.
pub const NOTHING_KEPT_YET: &str =
    "Nothing captured yet: the page has made none of the requests the capture keeps (the app's own XHR, fetch and page loads) since the browser opened or the filter was set.";

/// Said by a live read while the browser keeps no capture at all.
pub const NO_CAPTURE: &str = "This browser keeps no capture.";

/// Imported sets kept at once.
pub const MAX_IMPORTED_SETS: usize = 5;

/// Said when a sixth set is imported.
pub const IMPORTS_FULL: &str = "Five captures are loaded; a sign-in clears them, or restart TCM.";

/// Said for a file named like the live capture.
pub const LIVE_IS_RESERVED: &str =
    "\"live\" is the live capture's name: no export or import may take it - choose another name";

/// Said with a set filter: it decides what is kept from now on only.
pub const FILTER_FROM_NOW_ON: &str =
    "The filter decides what is kept from now on: requests already left out cannot be brought back, and what is kept stays.";

/// Which set a read looks at: the live capture, newest first, or an
/// imported one.
pub struct Set<'a> {
    /// What the answer calls it: `live`, or the imported set's name.
    pub name: String,
    /// Newest first.
    pub entries: Vec<&'a Captured>,
    /// Live, and the application's site is not confirmed yet.
    pub unconfirmed: bool,
    /// The whole set holds nothing (`entries` may be only a part of it).
    pub empty: bool,
}

impl<'a> Set<'a> {
    /// The live capture, through its `list` only: held-back requests are
    /// never among them.
    pub fn live(cap: &'a NetCapture) -> Self {
        let (entries, _) = cap.list(0, usize::MAX);
        let empty = entries.is_empty();
        Set { name: "live".to_string(), entries, unconfirmed: !cap.knows_app_site(), empty }
    }

    /// An imported set, newest first (by start, then by its order in the file).
    pub fn imported(name: &str, entries: &'a [Captured]) -> Self {
        let mut entries: Vec<&Captured> = entries.iter().collect();
        entries.sort_by(|a, b| b.started_ms.cmp(&a.started_ms).then(b.id.cmp(&a.id)));
        let empty = entries.is_empty();
        Set { name: name.to_string(), entries, unconfirmed: false, empty }
    }

    fn get(&self, id: u64) -> Option<&'a Captured> {
        self.entries.iter().copied().find(|c| c.id == id)
    }

    /// The sentence for an empty set.
    fn empty_note(&self) -> &'static str {
        if self.unconfirmed { NOTHING_CAPTURED_YET } else { NOTHING_KEPT_YET }
    }

    /// Refused for an id the set does not hold.
    fn no_such(&self, id: u64) -> (u16, String) {
        if self.unconfirmed {
            return (404, NOTHING_CAPTURED_YET.to_string());
        }
        let said = if self.name == "live" {
            format!("No request {id} in the capture: it may have been dropped (the capture keeps the newest {MAX_CAPTURED}). list_autorun_requests shows the ids kept.")
        } else {
            format!("No request {id} in the imported set \"{}\". list_autorun_requests with from: \"{}\" shows its ids.", self.name, self.name)
        };
        (404, said)
    }
}

/// The arguments object of a call: `{}` for an empty body.
pub fn args_of(body: &str) -> Result<Value, (u16, String)> {
    if body.trim().is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(body) {
        Ok(v @ Value::Object(_)) => Ok(v),
        Ok(Value::Null) => Ok(json!({})),
        _ => Err((400, "the arguments must be a JSON object".to_string())),
    }
}

/// A whole number argument, given as a number or as its text; `None` when
/// left out.
fn number(args: &Value, key: &str) -> Result<Option<u64>, (u16, String)> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::Number(n) => n.as_u64().map(Some).ok_or_else(|| (400, format!("{key} must be a whole number of 0 or more"))),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        Value::String(s) => s.trim().parse::<u64>().map(Some).map_err(|_| (400, format!("{key} must be a whole number of 0 or more"))),
        _ => Err((400, format!("{key} must be a whole number of 0 or more"))),
    }
}

/// A text argument; `None` when left out or blank.
fn text(args: &Value, key: &str) -> Result<Option<String>, (u16, String)> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        Value::String(s) => Ok(Some(s.trim().to_string())),
        Value::Number(n) => Ok(Some(n.to_string())),
        _ => Err((400, format!("{key} must be text"))),
    }
}

/// The set named by `from`, or `None` for the live capture. A name is
/// checked as a file stem before anything quotes it.
pub fn from_of(args: &Value) -> Result<Option<String>, (u16, String)> {
    match text(args, "from")? {
        None => Ok(None),
        Some(name) if name.eq_ignore_ascii_case("live") => Ok(None),
        Some(name) => file_stem(&name).map(Some).map_err(|why| (400, format!("from: {why}"))),
    }
}

/// The imported set `name`, or why there is none.
pub fn imported_set<'a>(imported: &'a ImportedSets, name: &str) -> Result<Set<'a>, (u16, String)> {
    match imported.get(name) {
        Some(entries) => Ok(Set::imported(name, entries.as_slice())),
        None => {
            let loaded = if imported.is_empty() {
                "none is loaded".to_string()
            } else {
                format!("loaded: {}", imported.keys().cloned().collect::<Vec<_>>().join(", "))
            };
            Err((400, format!("No imported set named \"{name}\" ({loaded}): load it with import_autorun_requests first.")))
        }
    }
}

/// `name` as a file stem in TCM's captures folder: no folder separator, no
/// `..`, nothing Windows refuses in a file name, at most `MAX_NAME_CHARS`.
/// A trailing `.har` is taken off.
pub fn file_stem(name: &str) -> Result<String, String> {
    let name = name.trim();
    let name = match name.len().checked_sub(4) {
        Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".har") => &name[..at],
        _ => name,
    };
    const RULE: &str = "a name is a plain file name without .har - letters, digits, spaces, dots, dashes or underscores, at most 80 characters, no folder (no / or \\) and no ..";
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(RULE.to_string());
    }
    if name.contains("..") || name.ends_with('.') || name.ends_with(' ') || name.starts_with('.') {
        return Err(RULE.to_string());
    }
    if name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return Err(RULE.to_string());
    }
    let base = name.split('.').next().unwrap_or("").trim().to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && base.as_bytes()[3].is_ascii_digit());
    if reserved {
        return Err(RULE.to_string());
    }
    Ok(name.to_string())
}

/// `<root>/captures/<stem>.har`.
pub fn capture_file(root: &Path, stem: &str) -> PathBuf {
    root.join(CAPTURES_DIR).join(format!("{stem}.har"))
}

fn started_iso(ms: u64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms as i64).map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// The request body's size as it shows, redacted: never the raw length,
/// which would give a hidden value's length away.
fn request_size(c: &Captured) -> Option<usize> {
    body_text(c.req_body.as_ref(), request_mime(c)).map(|t| t.len())
}

/// The response's size as it shows: its redacted body's length when the
/// body was kept as text (never the raw length), otherwise what the
/// browser received.
fn response_size(c: &Captured) -> Option<u64> {
    match body_text(c.resp_body.as_ref(), c.mime.as_deref()) {
        Some(t) => Some(t.len() as u64),
        None => c.resp_size,
    }
}

/// One line of a list or a search.
fn summary(c: &Captured) -> Value {
    json!({
        "id": c.id,
        "method": c.method,
        "path": redact_path_query(&c.url_path_query),
        "status": c.status,
        "type": c.kind,
        "request_size": request_size(c),
        "response_size": response_size(c),
        "started": started_iso(c.started_ms),
        "duration_ms": c.duration_ms,
        "failed": c.failed.as_deref().map(redact_text),
    })
}

/// `offset` and `limit` of a list or search.
fn paging(args: &Value) -> Result<(usize, usize), (u16, String)> {
    let offset = number(args, "offset")?.unwrap_or(0) as usize;
    let limit = number(args, "limit")?.map(|n| n as usize).unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    Ok((offset, limit))
}

fn paged(set: &Set, matched: &[&Captured], offset: usize, limit: usize) -> Value {
    let page: Vec<Value> = matched.iter().skip(offset).take(limit).map(|c| summary(c)).collect();
    let returned = page.len();
    let next = (offset + returned < matched.len()).then_some(offset + returned);
    let mut out = json!({
        "set": set.name,
        "total": matched.len(),
        "offset": offset,
        "returned": returned,
        "next_offset": next,
        "requests": page,
    });
    if matched.is_empty() && set.empty {
        out["note"] = json!(set.empty_note());
    }
    out
}

/// `list_autorun_requests`: newest first, paged.
pub fn list(set: &Set, args: &Value) -> (u16, String) {
    let (offset, limit) = match paging(args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    (200, paged(set, &set.entries, offset, limit).to_string())
}

/// What a read takes out of its set, cloned, so the set's lock (the
/// supervised browser's, or the imported sets') is let go before anything
/// is redacted (`answer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    List,
    Search,
    Request,
    Response,
    ResponseBody,
}

/// A read's entries, out of its set: for a list, the page asked for; for a
/// search, the entries the filters that need no redaction let through
/// (method, type, status); for one request, that request.
pub struct Taken {
    name: String,
    unconfirmed: bool,
    empty: bool,
    /// How many the whole set holds.
    total: usize,
    entries: Vec<Captured>,
}

impl Taken {
    fn set(&self) -> Set<'_> {
        Set { name: self.name.clone(), entries: self.entries.iter().collect(), unconfirmed: self.unconfirmed, empty: self.empty }
    }
}

/// Take what `read` needs out of `set`, cheaply: nothing is redacted here.
/// A malformed argument is refused at once.
pub fn take(set: &Set, read: Read, args: &Value) -> Result<Taken, (u16, String)> {
    let entries: Vec<Captured> = match read {
        Read::List => {
            let (offset, limit) = paging(args)?;
            set.entries.iter().skip(offset).take(limit).map(|c| (*c).clone()).collect()
        }
        Read::Search => {
            paging(args)?;
            let want = search_filters(args)?;
            set.entries.iter().filter(|c| cheap_match(&want, c)).map(|c| (*c).clone()).collect()
        }
        Read::Request | Read::Response | Read::ResponseBody => {
            let id = id_of(args)?;
            set.get(id).cloned().into_iter().collect()
        }
    };
    Ok(Taken { name: set.name.clone(), unconfirmed: set.unconfirmed, empty: set.empty, total: set.entries.len(), entries })
}

/// `read`'s answer from what `take` took: the redaction and the body
/// matching happen here, with no lock held.
pub fn answer(taken: &Taken, read: Read, args: &Value) -> (u16, String) {
    let set = taken.set();
    match read {
        Read::List => {
            let (offset, _) = match paging(args) {
                Ok(p) => p,
                Err(e) => return e,
            };
            let page: Vec<Value> = set.entries.iter().map(|c| summary(c)).collect();
            let returned = page.len();
            let next = (offset + returned < taken.total).then_some(offset + returned);
            let mut out = json!({
                "set": set.name,
                "total": taken.total,
                "offset": offset,
                "returned": returned,
                "next_offset": next,
                "requests": page,
            });
            if taken.empty {
                out["note"] = json!(set.empty_note());
            }
            (200, out.to_string())
        }
        Read::Search => search(&set, args),
        Read::Request => request(&set, args),
        Read::Response => response(&set, args),
        Read::ResponseBody => response_body(&set, args),
    }
}

/// A status filter: `404`, or a class such as `4xx`.
fn status_matches(want: &str, status: Option<u16>) -> bool {
    let Some(s) = status else { return false };
    let w = want.trim().to_ascii_lowercase();
    match w.strip_suffix("xx") {
        Some(class) if class.len() == 1 => class.parse::<u16>().is_ok_and(|c| s / 100 == c),
        _ => w.parse::<u16>().is_ok_and(|n| n == s),
    }
}

/// A body's redacted text, when it was kept as text.
fn body_text(body: Option<&Body>, mime: Option<&str>) -> Option<String> {
    match body {
        Some(Body::Text(t)) => Some(redact_body(mime, t)),
        _ => None,
    }
}

/// The request's own content type, the mime its body is redacted as.
fn request_mime(c: &Captured) -> Option<&str> {
    c.req_headers.iter().find(|(n, _)| n == "content-type").map(|(_, v)| v.as_str())
}

fn contains_ci(hay: &str, needle: &str) -> bool {
    hay.to_lowercase().contains(&needle.to_lowercase())
}

/// A search's filters, by name; a status that is not a code or a class is
/// refused.
fn search_filters(args: &Value) -> Result<BTreeMap<&'static str, String>, (u16, String)> {
    let mut want = BTreeMap::new();
    for key in ["path", "method", "status", "type", "request_contains", "response_contains"] {
        if let Some(v) = text(args, key)? {
            want.insert(key, v);
        }
    }
    if let Some(t) = want.get_mut("type") {
        let Some(canon) = CAPTURE_TYPES.iter().find(|k| k.eq_ignore_ascii_case(t)) else {
            return Err((400, format!("type: the capture keeps only these types: {}", CAPTURE_TYPES.join(", "))));
        };
        *t = canon.to_string();
    }
    if let Some(s) = want.get("status") {
        let ok = s.parse::<u16>().is_ok() || s.to_ascii_lowercase().strip_suffix("xx").is_some_and(|c| c.len() == 1 && c.parse::<u16>().is_ok());
        if !ok {
            return Err((400, "status must be a status code such as 404, or a class such as 4xx".to_string()));
        }
    }
    Ok(want)
}

/// The search filters that need no redaction: method, type and status.
fn cheap_match(want: &BTreeMap<&'static str, String>, c: &Captured) -> bool {
    want.get("method").is_none_or(|m| c.method.eq_ignore_ascii_case(m))
        && want.get("type").is_none_or(|t| c.kind.as_deref().is_some_and(|k| k.eq_ignore_ascii_case(t)))
        && want.get("status").is_none_or(|s| status_matches(s, c.status))
}

/// `search_autorun_requests`: every filter given must match, each a
/// substring in any case of the REDACTED text (method and type compared
/// whole); newest first, paged, with the number that matched.
pub fn search(set: &Set, args: &Value) -> (u16, String) {
    let (offset, limit) = match paging(args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let want = match search_filters(args) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let matched: Vec<&Captured> = set
        .entries
        .iter()
        .copied()
        .filter(|c| {
            cheap_match(&want, c)
                && want.get("path").is_none_or(|p| contains_ci(&redact_path_query(&c.url_path_query), p))
                && want.get("request_contains").is_none_or(|w| {
                    body_text(c.req_body.as_ref(), request_mime(c)).is_some_and(|t| contains_ci(&t, w))
                })
                && want.get("response_contains").is_none_or(|w| {
                    body_text(c.resp_body.as_ref(), c.mime.as_deref()).is_some_and(|t| contains_ci(&t, w))
                })
        })
        .collect();
    (200, paged(set, &matched, offset, limit).to_string())
}

fn headers(list: &[(String, String)]) -> Value {
    Value::Array(list.iter().map(|(n, v)| json!({ "name": n, "value": redact_header(n, v) })).collect())
}

/// Why a body is not there to read.
fn missing_body(body: Option<&Body>, response: bool, c: &Captured) -> Option<String> {
    let said = match body {
        Some(Body::Text(_)) => return None,
        Some(Body::Dropped) => "The body is gone: the browser no longer held it when it was read, or it was dropped to keep the capture within its budget.",
        Some(Body::NotText) => "Not text (a picture, a file, or bytes that are not UTF-8): never kept.",
        Some(Body::TooBig) => "Over the capture's size limit (2 MB for a response, 256 KB for a request): not kept.",
        Some(Body::OffSite) => "Answered by another site: never read.",
        None if !response => "None sent (or it is still being read).",
        None if c.failed.is_some() => "The request failed: there is no body.",
        None if c.status.is_none() => "Not answered yet.",
        None => "Not read yet: ask again in a moment.",
    };
    Some(said.to_string())
}

/// One page of a body's redacted text, from character `offset`.
fn body_page(redacted: &str, offset: usize) -> Result<Value, (u16, String)> {
    let chars = redacted.chars().count();
    if offset > chars {
        return Err((400, format!("offset {offset} is past the body's end ({chars} characters)")));
    }
    let page: String = redacted.chars().skip(offset).take(BODY_PAGE_CHARS).collect();
    let returned = page.chars().count();
    let next = (offset + returned < chars).then_some(offset + returned);
    Ok(json!({ "chars": chars, "offset": offset, "next_offset": next, "text": page }))
}

fn id_of(args: &Value) -> Result<u64, (u16, String)> {
    number(args, "id")?.ok_or_else(|| (400, "id is needed: the request's id, as list_autorun_requests shows it".to_string()))
}

/// `get_autorun_request`: one request - its allow-listed headers and its
/// body, redacted; a long body in pages from `offset`.
pub fn request(set: &Set, args: &Value) -> (u16, String) {
    let id = match id_of(args) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let offset = match number(args, "offset") {
        Ok(o) => o.unwrap_or(0) as usize,
        Err(e) => return e,
    };
    let Some(c) = set.get(id) else { return set.no_such(id) };
    let body = match body_text(c.req_body.as_ref(), request_mime(c)) {
        Some(t) => match body_page(&t, offset) {
            Ok(p) => p,
            Err(e) => return e,
        },
        None => match missing_body(c.req_body.as_ref(), false, c) {
            Some(why) => json!({ "kept": false, "why": why }),
            None => Value::Null,
        },
    };
    let out = json!({
        "set": set.name,
        "id": c.id,
        "method": c.method,
        "path": redact_path_query(&c.url_path_query),
        "type": c.kind,
        "tab": redact_text(&c.tab),
        "started": started_iso(c.started_ms),
        "headers": headers(&c.req_headers),
        "body": body,
    });
    (200, out.to_string())
}

/// `get_autorun_response`: one response - status, allow-listed headers,
/// mime, size, timing, its first redirect and whether its body is there
/// (`get_autorun_response_body` reads it).
pub fn response(set: &Set, args: &Value) -> (u16, String) {
    let id = match id_of(args) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let Some(c) = set.get(id) else { return set.no_such(id) };
    let body = match body_text(c.resp_body.as_ref(), c.mime.as_deref()) {
        Some(t) => json!({ "kept": true, "chars": t.chars().count() }),
        None => json!({ "kept": false, "why": missing_body(c.resp_body.as_ref(), true, c) }),
    };
    let redirect = c.redirect.as_ref().map(|r| {
        json!({ "status": r.status, "to_path": redact_path_query(&r.to_path), "other_site": r.other_site })
    });
    let out = json!({
        "set": set.name,
        "id": c.id,
        "method": c.method,
        "path": redact_path_query(&c.url_path_query),
        "status": c.status,
        "mime": c.mime,
        "headers": headers(&c.resp_headers),
        "size": response_size(c),
        "started": started_iso(c.started_ms),
        "duration_ms": c.duration_ms,
        "failed": c.failed.as_deref().map(redact_text),
        "redirect": redirect,
        "body": body,
    });
    (200, out.to_string())
}

/// `get_autorun_response_body`: the response body's redacted text, a page
/// of `BODY_PAGE_CHARS` characters from `offset`.
pub fn response_body(set: &Set, args: &Value) -> (u16, String) {
    let id = match id_of(args) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let offset = match number(args, "offset") {
        Ok(o) => o.unwrap_or(0) as usize,
        Err(e) => return e,
    };
    let Some(c) = set.get(id) else { return set.no_such(id) };
    let Some(t) = body_text(c.resp_body.as_ref(), c.mime.as_deref()) else {
        return (404, missing_body(c.resp_body.as_ref(), true, c).unwrap_or_default());
    };
    match body_page(&t, offset) {
        Ok(mut page) => {
            page["set"] = json!(set.name);
            page["id"] = json!(c.id);
            page["mime"] = json!(c.mime);
            (200, page.to_string())
        }
        Err(e) => e,
    }
}

fn types_of(args: &Value, key: &str) -> Result<Vec<String>, (u16, String)> {
    let raw: Vec<String> = match &args[key] {
        Value::Null => vec![],
        Value::String(s) => s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
        Value::Array(items) => {
            let mut out = vec![];
            for i in items {
                match i.as_str() {
                    Some(s) if !s.trim().is_empty() => out.push(s.trim().to_string()),
                    Some(_) => {}
                    None => return Err((400, format!("{key} must be a list of resource types"))),
                }
            }
            out
        }
        _ => return Err((400, format!("{key} must be a list of resource types"))),
    };
    let mut out: Vec<String> = vec![];
    for t in raw {
        let Some(canon) = CAPTURE_TYPES.iter().find(|k| k.eq_ignore_ascii_case(&t)) else {
            return Err((400, format!("{key}: the capture keeps only these types: {}", CAPTURE_TYPES.join(", "))));
        };
        if !out.iter().any(|o| o == canon) {
            out.push(canon.to_string());
        }
    }
    Ok(out)
}

/// What the filter is and what is kept, for `get_autorun_capture` and the
/// answer of a set.
pub fn filter_answer(cap: &NetCapture, imported: &ImportedSets) -> Value {
    let f = cap.filter();
    let (_, kept) = cap.list(0, 0);
    let mut out = json!({
        "include_types": f.include_types,
        "exclude_types": f.exclude_types,
        "path_contains": f.path_contains,
        "kept": kept,
        "max_kept": MAX_CAPTURED,
        "site_confirmed": cap.knows_app_site(),
        "imported_sets": imported.keys().collect::<Vec<_>>(),
    });
    if !cap.knows_app_site() {
        out["note"] = json!(NOTHING_CAPTURED_YET);
    }
    out
}

/// `set_autorun_capture`: the filter from now on, whole - what is left out
/// is left as no filter.
pub fn set_filter(cap: &mut NetCapture, imported: &ImportedSets, args: &Value) -> (u16, String) {
    let include_types = match types_of(args, "include_types") {
        Ok(t) => t,
        Err(e) => return e,
    };
    let exclude_types = match types_of(args, "exclude_types") {
        Ok(t) => t,
        Err(e) => return e,
    };
    let path_contains = match text(args, "path_contains") {
        Ok(p) => p,
        Err(e) => return e,
    };
    if path_contains.as_ref().is_some_and(|p| p.chars().count() > MAX_FILTER_PATH_CHARS) {
        return (400, format!("path_contains is at most {MAX_FILTER_PATH_CHARS} characters"));
    }
    cap.set_filter(CaptureFilter { include_types, exclude_types, path_contains });
    let mut out = filter_answer(cap, imported);
    out["said"] = json!(FILTER_FROM_NOW_ON);
    (200, out.to_string())
}

/// `export_autorun_requests`: the live capture, oldest first, as HAR 1.2
/// redacted exactly as the reads are (`har::to_har`), written atomically to
/// `<root>/captures/<name>.har`. Answers the file's name and how many
/// requests it holds.
pub fn export(cap: &NetCapture, root: &Path, args: &Value) -> (u16, String) {
    match take_export(cap, args) {
        Ok(taken) => write_export(root, taken),
        Err(e) => e,
    }
}

/// What an export takes out of the live capture, cloned, so the browser's
/// lock is let go before anything is redacted or written (`write_export`).
pub struct TakenExport {
    stem: String,
    /// Oldest first.
    entries: Vec<Captured>,
    site: Option<String>,
}

/// The export's name checked, and the live capture's requests cloned,
/// oldest first, with the confirmed site. Nothing is redacted here.
pub fn take_export(cap: &NetCapture, args: &Value) -> Result<TakenExport, (u16, String)> {
    let stem = match text(args, "name")? {
        Some(n) => file_stem(&n).map_err(|why| (400, why))?,
        None => return Err((400, "name is needed: the file's name, without .har".to_string())),
    };
    if stem.eq_ignore_ascii_case("live") {
        return Err((400, LIVE_IS_RESERVED.to_string()));
    }
    let set = Set::live(cap);
    if set.entries.is_empty() {
        return Err((404, set.empty_note().to_string()));
    }
    let entries = set.entries.iter().rev().map(|c| (*c).clone()).collect();
    Ok(TakenExport { stem, entries, site: cap.confirmed_site().map(str::to_string) })
}

/// The taken requests as HAR 1.2, redacted (`har::to_har`), written
/// atomically to `<root>/captures/<stem>.har`.
pub fn write_export(root: &Path, taken: TakenExport) -> (u16, String) {
    let TakenExport { stem, entries, site } = taken;
    let oldest_first: Vec<&Captured> = entries.iter().collect();
    let har = crate::autorun::har::to_har(&oldest_first, site.as_deref());
    let text = match serde_json::to_string_pretty(&har) {
        Ok(t) => t,
        Err(e) => {
            crate::applog::warn(format!("Auto Run: a capture export could not be written as JSON: {e}"));
            return (500, "the capture could not be written as HAR".to_string());
        }
    };
    let path = capture_file(root, &stem);
    let replaced = path.exists();
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            crate::applog::warn(format!("Auto Run: the captures folder could not be made: {e}"));
            return (500, "the captures folder could not be made - see Settings, Logs".to_string());
        }
    }
    if let Err(e) = crate::ai_tools::atomic_write(&path, &text) {
        crate::applog::warn(format!("Auto Run: a capture export could not be saved: {e}"));
        return (500, "the export could not be saved - see Settings, Logs".to_string());
    }
    crate::applog::info(format!("Auto Run: the assistant exported {} captured requests", oldest_first.len()));
    let out = json!({
        "file": format!("{stem}.har"),
        "folder": CAPTURES_DIR,
        "requests": oldest_first.len(),
        "replaced": replaced,
        "said": format!("Saved {} requests, redacted, to {stem}.har in TCM's captures folder. import_autorun_requests with name \"{stem}\" reads it back as its own set.", oldest_first.len()),
    });
    (200, out.to_string())
}

/// `import_autorun_requests`: `read_import` then `store_import`.
pub fn import(root: &Path, imported: &mut ImportedSets, args: &Value) -> (u16, String) {
    match read_import(root, args) {
        Ok((stem, entries)) => store_import(imported, stem, entries),
        Err(e) => e,
    }
}

/// `<root>/captures/<name>.har` read and parsed (`har::from_har`, which
/// redacts it again on the way in): no lock is needed for this, and none
/// is taken. The name `live` is the live capture's and is refused.
pub fn read_import(root: &Path, args: &Value) -> Result<(String, Vec<Captured>), (u16, String)> {
    let stem = match text(args, "name")? {
        Some(n) => file_stem(&n).map_err(|why| (400, why))?,
        None => return Err((400, "name is needed: the file's name in TCM's captures folder, without .har".to_string())),
    };
    if stem.eq_ignore_ascii_case("live") {
        return Err((400, LIVE_IS_RESERVED.to_string()));
    }
    let path = capture_file(root, &stem);
    let meta = match std::fs::metadata(&path) {
        Ok(m) if m.is_file() => m,
        _ => {
            return Err((
                404,
                format!("No {stem}.har in TCM's captures folder: export one with export_autorun_requests, or have the person put the HAR file there."),
            ))
        }
    };
    if meta.len() > MAX_IMPORT_BYTES {
        return Err((413, format!("{stem}.har is over {} MB: export a smaller capture", MAX_IMPORT_BYTES / (1024 * 1024))));
    }
    let raw = std::fs::read(&path).map_err(|e| {
        crate::applog::warn(format!("Auto Run: a capture import could not be read: {e}"));
        (500, "the file could not be read - see Settings, Logs".to_string())
    })?;
    let text = String::from_utf8_lossy(&raw);
    let doc = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .map_err(|_| (400, "not a HAR file: not JSON".to_string()))?;
    let entries = crate::autorun::har::from_har(&doc).map_err(|why| (400, why))?;
    Ok((stem, entries))
}

/// Keep a parsed import as the set `stem`, replacing one of that name; at
/// most `MAX_IMPORTED_SETS` sets at once. Never replayed into the browser.
pub fn store_import(imported: &mut ImportedSets, stem: String, entries: Vec<Captured>) -> (u16, String) {
    if !imported.contains_key(&stem) && imported.len() >= MAX_IMPORTED_SETS {
        return (409, IMPORTS_FULL.to_string());
    }
    let n = entries.len();
    let replaced = imported.insert(stem.clone(), Arc::new(entries)).is_some();
    crate::applog::info(format!("Auto Run: the assistant imported {n} requests from a HAR file"));
    let out = json!({
        "set": stem,
        "requests": n,
        "replaced": replaced,
        "said": format!("Loaded {n} requests as the set \"{stem}\": read it with from: \"{stem}\" on list_autorun_requests, search_autorun_requests and the get tools. It is never replayed into the browser."),
    });
    (200, out.to_string())
}
