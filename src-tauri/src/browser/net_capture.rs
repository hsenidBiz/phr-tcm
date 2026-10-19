//! The supervised browser's own requests and responses, kept for the
//! assistant to read: what the page really asked its server and what it
//! was answered, so an API template is built from a real request instead
//! of a guess.
//!
//! Beside `net_record`, not inside it: the record keeps every request a
//! step may check, with nothing a person could not see in the address bar;
//! this keeps much more (allow-listed headers, bodies) and so only for
//! the requests worth it - the page's own XHR, fetch and documents, to its
//! own site, polls left out, exactly as the record marks them (`observe`
//! asks the tab's record). Only the supervised browser captures
//! (`Cdp::capture_requests`); an unattended run's browser never does.
//!
//! Never kept: the `cookie`, `set-cookie`, `authorization` and
//! `proxy-authorization` headers, or any header outside the allow-list
//! (`kept_header`); a header named like a secret, except `x-csrf-token`,
//! whose name alone is kept. Bodies are kept as the page sent and got them;
//! whoever shows one redacts it first.
//!
//! A response body is read at `loadingFinished` - later the browser may
//! have dropped it. That read is a protocol call and `observe` is not, so
//! the request is queued (`next_read`) and the driver reads it where it
//! already waits on the browser (`Cdp::pump`, the start of a call), at most
//! a few at a time. A body gone by then is `Body::Dropped`.
//!
//! Bounded: `MAX_CAPTURED` requests, oldest first; a body at most
//! `MAX_BODY_BYTES` (a request's `MAX_REQUEST_BODY_BYTES`); all bodies
//! together at most `MAX_TOTAL_BODY_BYTES`, the oldest bodies dropped first
//! while their requests stay.

use super::cdp::Event;
use super::net_record::NetRecord;
use base64::Engine;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

/// Requests kept at once; the oldest goes first.
pub const MAX_CAPTURED: usize = 1000;
/// The largest response body kept.
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// The largest request body kept.
pub const MAX_REQUEST_BODY_BYTES: usize = 256 * 1024;
/// Every body kept, together; the oldest bodies are dropped first.
pub const MAX_TOTAL_BODY_BYTES: usize = 32 * 1024 * 1024;
/// Body reads waiting at once. One more drops the oldest waiting (its
/// body `Dropped`): a page finishing requests faster than they are read
/// never grows the queue.
pub const MAX_QUEUED_READS: usize = 64;

/// A body, as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// The whole body.
    Text(String),
    /// Gone: the browser no longer held it when it was read, or it was
    /// dropped to stay within the budget.
    Dropped,
    /// Not text (a picture, a file, bytes that are not UTF-8): never kept.
    NotText,
    /// Over its cap (`MAX_BODY_BYTES`, `MAX_REQUEST_BODY_BYTES`).
    TooBig,
}

impl Body {
    fn bytes(&self) -> usize {
        match self {
            Body::Text(t) => t.len(),
            _ => 0,
        }
    }
}

/// One request the page made, as kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Captured {
    /// This store's own id: 1 for the first kept, never handed out twice.
    pub id: u64,
    /// Chrome's request id.
    pub request_id: String,
    /// The name of the tab it was made in (`main`); empty for a tab the
    /// page opened that a script never named.
    pub tab: String,
    /// What the page asked: a redirect's later hops never change it.
    pub method: String,
    /// The address without its scheme, host or fragment: `/a/b?x=1`.
    pub url_path_query: String,
    /// Chrome's resource type: `XHR`, `Fetch` or `Document`.
    pub kind: Option<String>,
    pub status: Option<u16>,
    pub mime: Option<String>,
    /// Allow-listed (`kept_header`), lower-case names, sorted.
    pub req_headers: Vec<(String, String)>,
    pub resp_headers: Vec<(String, String)>,
    /// `None` when the request sent none, or its read is still waiting.
    pub req_body: Option<Body>,
    /// `None` until it finished and its body was read (or judged).
    pub resp_body: Option<Body>,
    /// What the browser received for it, headers included
    /// (`encodedDataLength`), once it finished.
    pub resp_size: Option<u64>,
    /// When it started, in milliseconds since the Unix epoch.
    pub started_ms: u64,
    /// From its start to its finish or failure.
    pub duration_ms: Option<u64>,
    /// Chrome's reason (`net::ERR_...`) when it failed.
    pub failed: Option<String>,
    /// Its first redirect: `302 /path` (the path it was sent to, never the
    /// host or the query).
    pub redirect: Option<String>,
}

/// What is kept from now on. Empty lists and no path keep every request
/// the capture takes at all. Types are Chrome's (`XHR`, `Fetch`,
/// `Document`), compared in any case; the path is a substring of the path
/// and query, in any case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureFilter {
    pub include_types: Vec<String>,
    pub exclude_types: Vec<String>,
    pub path_contains: Option<String>,
}

impl CaptureFilter {
    fn keeps(&self, kind: &str, path_query: &str) -> bool {
        let named = |list: &[String]| list.iter().any(|t| t.eq_ignore_ascii_case(kind));
        (self.include_types.is_empty() || named(&self.include_types))
            && !named(&self.exclude_types)
            && self
                .path_contains
                .as_deref()
                .is_none_or(|p| path_query.to_ascii_lowercase().contains(&p.to_ascii_lowercase()))
    }
}

/// A body waiting to be read: which request, on which tab's session, and
/// the protocol call that reads it (`method`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRead {
    /// The capture's id for the request.
    pub id: u64,
    pub session: String,
    pub request_id: String,
    /// `Network.getResponseBody` or `Network.getRequestPostData`.
    pub method: &'static str,
}

impl BodyRead {
    fn of_response(&self) -> bool {
        self.method == RESPONSE_BODY
    }
}

const RESPONSE_BODY: &str = "Network.getResponseBody";
const REQUEST_BODY: &str = "Network.getRequestPostData";

/// One kept request, with what only the store needs.
struct Slot {
    c: Captured,
    session: String,
    /// Its start by the browser's own clock, in seconds.
    started_at: Option<f64>,
}

#[derive(Default)]
pub struct NetCapture {
    /// Oldest first, ids rising.
    entries: VecDeque<Slot>,
    /// (session, Chrome's request id) to the capture's id, for the kept.
    by_request: HashMap<(String, String), u64>,
    /// The last id handed out.
    last_id: u64,
    filter: CaptureFilter,
    /// Bytes of every body kept now.
    bytes: usize,
    reads: VecDeque<BodyRead>,
}

impl NetCapture {
    /// Note what this event, in the tab with this session and name, says
    /// about a request. `record` is that tab's network record, which has
    /// seen the event already: whether a starting request is kept is what
    /// it says of the request's type, site and polling.
    pub fn observe(&mut self, session: &str, tab: &str, ev: &Event, record: &NetRecord) {
        let p = &ev.params;
        let Some(request_id) = p["requestId"].as_str() else { return };
        if ev.method == "Network.requestWillBeSent" {
            return self.started(session, tab, request_id, p, record);
        }
        let Some(at) = self.index_of(session, request_id) else { return };
        match ev.method.as_str() {
            "Network.responseReceived" => {
                let r = &p["response"];
                let c = &mut self.entries[at].c;
                c.status = status_of(&r["status"]);
                c.mime = r["mimeType"].as_str().map(str::to_string);
                c.resp_headers = kept_headers(&r["headers"]);
            }
            "Network.loadingFinished" => {
                let slot = &mut self.entries[at];
                slot.c.duration_ms = elapsed_ms(slot.started_at, p);
                let size = p["encodedDataLength"].as_f64().filter(|n| *n >= 0.0).map(|n| n as u64);
                slot.c.resp_size = size;
                let id = slot.c.id;
                if !slot.c.mime.as_deref().is_some_and(text_like) {
                    slot.c.resp_body = Some(Body::NotText);
                } else if size.is_some_and(|n| n > MAX_BODY_BYTES as u64) {
                    slot.c.resp_body = Some(Body::TooBig);
                } else {
                    self.queue(BodyRead {
                        id,
                        session: session.to_string(),
                        request_id: request_id.to_string(),
                        method: RESPONSE_BODY,
                    });
                }
            }
            "Network.loadingFailed" => {
                let slot = &mut self.entries[at];
                slot.c.duration_ms = elapsed_ms(slot.started_at, p);
                slot.c.failed = Some(p["errorText"].as_str().unwrap_or("failed").to_string());
            }
            _ => {}
        }
    }

    fn started(&mut self, session: &str, tab: &str, request_id: &str, p: &Value, record: &NetRecord) {
        // A redirect arrives under the same id: the request goes on, keeping
        // what the page asked; the first hop is kept, the answer is the
        // last hop's.
        if let Some(at) = self.index_of(session, request_id) {
            let c = &mut self.entries[at].c;
            if c.redirect.is_none() {
                if let Some(r) = record.get(request_id).and_then(|e| e.redirect.as_ref()) {
                    c.redirect = Some(format!("{} {}", r.status, r.to));
                }
            }
            c.status = None;
            c.mime = None;
            c.resp_headers.clear();
            return;
        }
        let Some(e) = record.get(request_id) else { return };
        if e.polling {
            // A second start of the same method and path: this one and the
            // earlier the record has just marked are both polls.
            self.forget_polls(session, record);
            return;
        }
        let kind = e.kind.as_deref().unwrap_or("");
        if !matches!(kind, "XHR" | "Fetch" | "Document") || !e.same_site || !self.filter.keeps(kind, &e.path_query) {
            return;
        }
        self.last_id += 1;
        let id = self.last_id;
        let r = &p["request"];
        let mut req_body = None;
        if let Some(post) = r["postData"].as_str() {
            req_body = Some(capped(post.to_string(), MAX_REQUEST_BODY_BYTES));
        } else if r["hasPostData"].as_bool() == Some(true) {
            self.queue(BodyRead {
                id,
                session: session.to_string(),
                request_id: request_id.to_string(),
                method: REQUEST_BODY,
            });
        }
        let started_ms = p["wallTime"]
            .as_f64()
            .map(|s| (s * 1000.0).round() as u64)
            .unwrap_or_else(|| {
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
            });
        let added = req_body.as_ref().map_or(0, Body::bytes);
        self.entries.push_back(Slot {
            c: Captured {
                id,
                request_id: request_id.to_string(),
                tab: tab.to_string(),
                method: e.method.clone(),
                url_path_query: e.path_query.clone(),
                kind: e.kind.clone(),
                req_headers: kept_headers(&r["headers"]),
                req_body,
                started_ms,
                ..Default::default()
            },
            session: session.to_string(),
            started_at: p["timestamp"].as_f64(),
        });
        self.by_request.insert((session.to_string(), request_id.to_string()), id);
        self.bytes += added;
        while self.entries.len() > MAX_CAPTURED {
            if let Some(old) = self.entries.pop_front() {
                self.forget(&old);
            }
        }
        self.keep_within_budget();
    }

    /// Take out every kept request of this tab the record now marks a poll.
    fn forget_polls(&mut self, session: &str, record: &NetRecord) {
        let (kept, polls): (VecDeque<Slot>, VecDeque<Slot>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|s| s.session != session || !record.get(&s.c.request_id).is_some_and(|e| e.polling));
        self.entries = kept;
        for s in &polls {
            self.forget(s);
        }
    }

    /// A request leaves: its bodies' bytes and its id go with it.
    fn forget(&mut self, s: &Slot) {
        self.bytes -= s.c.req_body.as_ref().map_or(0, Body::bytes) + s.c.resp_body.as_ref().map_or(0, Body::bytes);
        let key = (s.session.clone(), s.c.request_id.clone());
        if self.by_request.get(&key) == Some(&s.c.id) {
            self.by_request.remove(&key);
        }
    }

    fn queue(&mut self, read: BodyRead) {
        if self.reads.len() >= MAX_QUEUED_READS {
            if let Some(old) = self.reads.pop_front() {
                self.settle(&old, Body::Dropped);
            }
        }
        self.reads.push_back(read);
    }

    /// The next body to read, oldest first.
    pub fn next_read(&mut self) -> Option<BodyRead> {
        self.reads.pop_front()
    }

    /// Bodies waiting to be read.
    pub fn queued_reads(&self) -> usize {
        self.reads.len()
    }

    /// The browser's answer to `read` (`Network.getResponseBody`'s `body`
    /// or `getRequestPostData`'s `postData`, base64 or not); `None` when it
    /// refused or never answered - the body is gone.
    pub fn answered(&mut self, read: &BodyRead, answer: Option<&Value>) {
        let field = if read.of_response() { "body" } else { "postData" };
        let body = match answer.and_then(|a| a[field].as_str().map(|raw| (raw, a["base64Encoded"].as_bool() == Some(true)))) {
            None => Body::Dropped,
            Some((raw, false)) => Body::Text(raw.to_string()),
            Some((raw, true)) => match base64::engine::general_purpose::STANDARD.decode(raw).map(String::from_utf8) {
                Ok(Ok(text)) => Body::Text(text),
                _ => Body::NotText,
            },
        };
        let cap = if read.of_response() { MAX_BODY_BYTES } else { MAX_REQUEST_BODY_BYTES };
        let body = match body {
            Body::Text(t) => capped(t, cap),
            other => other,
        };
        self.settle(read, body);
        self.keep_within_budget();
    }

    /// Put a read's outcome on its request, if it is still kept.
    fn settle(&mut self, read: &BodyRead, body: Body) {
        let Some(at) = self.index_by_id(read.id) else { return };
        let added = body.bytes();
        let c = &mut self.entries[at].c;
        let slot = if read.of_response() { &mut c.resp_body } else { &mut c.req_body };
        let before = slot.replace(body);
        self.bytes = self.bytes + added - before.as_ref().map_or(0, Body::bytes);
    }

    /// Drop the oldest bodies until all of them fit `MAX_TOTAL_BODY_BYTES`;
    /// their requests stay.
    fn keep_within_budget(&mut self) {
        for slot in self.entries.iter_mut() {
            if self.bytes <= MAX_TOTAL_BODY_BYTES {
                return;
            }
            for body in [&mut slot.c.req_body, &mut slot.c.resp_body] {
                if let Some(b @ Body::Text(_)) = body {
                    self.bytes -= b.bytes();
                    *b = Body::Dropped;
                }
            }
        }
    }

    /// Kept requests, newest first, from `offset`, at most `limit`, and how
    /// many are kept in all.
    pub fn list(&self, offset: usize, limit: usize) -> (Vec<&Captured>, usize) {
        (self.entries.iter().rev().skip(offset).take(limit).map(|s| &s.c).collect(), self.entries.len())
    }

    /// One kept request by the capture's id.
    pub fn get(&self, id: u64) -> Option<&Captured> {
        self.index_by_id(id).map(|at| &self.entries[at].c)
    }

    /// What is kept from now on. What was kept stays; what was left out
    /// is not recoverable.
    pub fn set_filter(&mut self, filter: CaptureFilter) {
        self.filter = filter;
    }

    pub fn filter(&self) -> &CaptureFilter {
        &self.filter
    }

    /// Forget every request and every waiting read; the filter stays, and
    /// an id is never handed out again.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.by_request.clear();
        self.reads.clear();
        self.bytes = 0;
    }

    /// Bytes of every body kept now (`MAX_TOTAL_BODY_BYTES` at most).
    pub fn body_bytes(&self) -> usize {
        self.bytes
    }

    fn index_of(&self, session: &str, request_id: &str) -> Option<usize> {
        let id = *self.by_request.get(&(session.to_string(), request_id.to_string()))?;
        self.index_by_id(id)
    }

    fn index_by_id(&self, id: u64) -> Option<usize> {
        self.entries.binary_search_by_key(&id, |s| s.c.id).ok()
    }
}

/// `text` as a body, `TooBig` when it is over `cap` bytes.
fn capped(text: String, cap: usize) -> Body {
    if text.len() > cap {
        Body::TooBig
    } else {
        Body::Text(text)
    }
}

/// A body worth keeping: JSON, text, script, XML, a form, GraphQL, HTML.
fn text_like(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m.starts_with("text/")
        || ["json", "javascript", "ecmascript", "xml", "x-www-form-urlencoded", "graphql", "html"].iter().any(|w| m.contains(w))
}

/// The headers kept, lower-case names, sorted: `content-type`, `accept`,
/// `content-length`, `x-requested-with`, `x-csrf-token` (its name only) and
/// any other `x-` header not named like a secret. Never `cookie`,
/// `set-cookie`, `authorization` or `proxy-authorization` - none is on the
/// list, and a secret's name is checked again besides.
fn kept_headers(headers: &Value) -> Vec<(String, String)> {
    let Some(map) = headers.as_object() else { return Vec::new() };
    let mut kept: Vec<(String, String)> = map
        .iter()
        .filter_map(|(name, v)| {
            let name = name.trim().to_ascii_lowercase();
            let value = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
            kept_header(&name, value).map(|value| (name, value))
        })
        .collect();
    kept.sort();
    kept.dedup_by(|a, b| a.0 == b.0);
    kept
}

/// `value` as kept for the header `name` (lower case), or `None`.
fn kept_header(name: &str, value: String) -> Option<String> {
    const NEVER: [&str; 4] = ["cookie", "set-cookie", "authorization", "proxy-authorization"];
    const PLAIN: [&str; 4] = ["content-type", "accept", "content-length", "x-requested-with"];
    if NEVER.contains(&name) {
        return None;
    }
    if PLAIN.contains(&name) {
        return Some(value);
    }
    if matches!(name, "x-csrf-token" | "x-xsrf-token") {
        return Some(crate::autorun::api_checks::REDACTED.to_string());
    }
    let secret = crate::autorun::api_checks::secret_key(name) || name.contains("auth");
    (name.starts_with("x-") && !secret).then_some(value)
}

/// A status as Chrome sends it - an integer, sometimes written as a float.
fn status_of(v: &Value) -> Option<u16> {
    let n = v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))?;
    u16::try_from(n).ok()
}

/// Milliseconds from `started` (seconds, the browser's clock) to the
/// event `p`'s `timestamp`.
fn elapsed_ms(started: Option<f64>, p: &Value) -> Option<u64> {
    Some(((p["timestamp"].as_f64()? - started?).max(0.0) * 1000.0).round() as u64)
}
