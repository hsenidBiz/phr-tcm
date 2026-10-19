//! The supervised browser's own requests and responses, kept for the
//! assistant to read: what the page really asked its server and what it
//! was answered, so an API template is built from a real request instead
//! of a guess.
//!
//! Beside `net_record`, not inside it: the record keeps every request a
//! step may check, with nothing a person could not see in the address bar;
//! this keeps much more (allow-listed headers, bodies) and so only for
//! the requests worth it - the page's own XHR, fetch and documents, to the
//! application's own site, polls left out exactly as the record marks them
//! (`observe` asks the tab's record). Only the supervised browser captures
//! (`Cdp::capture_requests`); an unattended run's browser never does.
//!
//! A request is kept only when it goes to the application's site - never
//! the "same site as its document" rule alone: a top-level navigation is
//! always same site as itself, and the identity provider's own pages and
//! XHR carry tokens. The site is known for sure only once a sign-in has
//! arrived: the signed-in marker showed, and the page it showed on is the
//! application's (`confirm_app_site`). Before then the configured address
//! (`set_app_site`) is only provisional - it may be the identity provider's
//! login page - and a request to it, made from a page on it, is held back
//! unseen: shown once the site is confirmed to be that one, forgotten when
//! it is another. Nothing else is kept while the site is unknown or
//! provisional. A request redirected off the application's site never has
//! its response body read (`Body::OffSite`).
//!
//! The page the sign-in typed the account name or password on is the
//! login page - an identity provider's, when it is not on the configured
//! address - and its origin is never taken for the application's
//! (`note_login_site`), even when the signed-in marker shows there too
//! (a provider's own "you are signed in" page): the site stays as it was
//! until the marker shows on another page. The one exception is the
//! configured address itself: an application whose login form is its own
//! has the password typed on its own site, which is the application's.
//! Until a sign-in has typed a login in this browser at all (a saved
//! session restored into a fresh browser types nothing), only the
//! configured address's own origin can be confirmed.
//!
//! Never kept: the `cookie`, `set-cookie`, `authorization` and
//! `proxy-authorization` headers, or any header outside the allow-list
//! (`kept_header`); an `x-` header named like a secret keeps its name only,
//! its value `HIDDEN`. Bodies are kept as the page sent and got them;
//! whoever shows one redacts it first.
//!
//! A response body is read at `loadingFinished` - later the browser may
//! have dropped it. That read is a protocol call and `observe` is not, so
//! the request is queued (`next_read`) and the driver reads it where it
//! already waits on the browser, within a small time budget
//! (`Cdp::read_captured_bodies`). A read the browser does not answer in time
//! goes back to the front (`retry`), at most `MAX_READ_ATTEMPTS` times; a
//! body gone by then is `Body::Dropped`.
//!
//! Bounded: `MAX_CAPTURED` requests, oldest first; a body at most
//! `MAX_BODY_BYTES` (a request's `MAX_REQUEST_BODY_BYTES`); bodies, headers
//! and addresses together at most `MAX_TOTAL_BODY_BYTES`, the oldest bodies
//! dropped first while their requests stay.

use super::cdp::Event;
use super::net_record::{origin, NetRecord};
use base64::Engine;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

/// Requests kept at once; the oldest goes first.
pub const MAX_CAPTURED: usize = 1000;
/// The largest response body kept, decoded.
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// The largest request body kept.
pub const MAX_REQUEST_BODY_BYTES: usize = 256 * 1024;
/// Everything kept, together - bodies, headers and addresses; the oldest
/// bodies are dropped first.
pub const MAX_TOTAL_BODY_BYTES: usize = 32 * 1024 * 1024;
/// Body reads waiting at once. One more drops the oldest waiting (its
/// body `Dropped`): a page finishing requests faster than they are read
/// never grows the queue.
pub const MAX_QUEUED_READS: usize = 64;
/// How many times a read the browser did not answer in time is tried.
pub const MAX_READ_ATTEMPTS: u8 = 3;
/// What a kept header named like a secret shows instead of its value.
pub const HIDDEN: &str = "(hidden)";

/// A body, as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// The whole body.
    Text(String),
    /// Gone: the browser no longer held it when it was read, it was never
    /// read in time, or it was dropped to stay within the budget.
    Dropped,
    /// Not text (a picture, a file, bytes that are not UTF-8): never kept.
    NotText,
    /// Over its cap (`MAX_BODY_BYTES`, `MAX_REQUEST_BODY_BYTES`).
    TooBig,
    /// Answered by another site (sent or redirected off the application's
    /// site): never read.
    OffSite,
}

impl Body {
    fn bytes(&self) -> usize {
        match self {
            Body::Text(t) => t.len(),
            _ => 0,
        }
    }
}

/// A request's first redirect: the 3xx it answered and where it was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub status: u16,
    /// The path it was sent to - never the host, the query or a fragment.
    pub to_path: String,
    /// Sent to another site (scheme, host or port).
    pub other_site: bool,
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
    /// Allow-listed (`kept_header`), lower-case names, sorted, one entry
    /// per name (repeated values joined with ", ").
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
    /// Its first redirect.
    pub redirect: Option<Redirect>,
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
    /// Tries the browser did not answer in time (`retry`).
    pub attempts: u8,
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
    /// Sent or redirected off the application's site: its body is never
    /// read.
    off_site: bool,
    /// Decoded bytes received so far (`Network.dataReceived`).
    decoded: u64,
    /// The response's `Content-Length`, when it said one.
    content_length: Option<u64>,
    /// Bytes its address and headers take (`bytes`).
    meta: usize,
    /// Kept while the site was only provisional: unseen until it is
    /// confirmed (`confirm_app_site`).
    pending: bool,
}

impl Slot {
    fn body_bytes(&self) -> usize {
        self.c.req_body.as_ref().map_or(0, Body::bytes) + self.c.resp_body.as_ref().map_or(0, Body::bytes)
    }

    fn header_bytes(c: &Captured) -> usize {
        c.url_path_query.len() + c.req_headers.iter().chain(&c.resp_headers).map(|(n, v)| n.len() + v.len()).sum::<usize>()
    }
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
    /// Bytes of everything kept now: bodies, headers, addresses.
    bytes: usize,
    reads: VecDeque<BodyRead>,
    /// The application's origin, compared, never shown: provisional
    /// (`set_app_site`) until `confirmed` (`confirm_app_site`).
    app_site: Option<String>,
    confirmed: bool,
    /// The origins the sign-in typed the account name or password on,
    /// compared, never shown: an identity provider's, unless one is the
    /// configured address's own. Never confirmed as the application's
    /// (`confirm_app_site`).
    login_sites: Vec<String>,
}

impl NetCapture {
    /// The application's site, from any address on it (the sign-in's
    /// start page); `None` when it is not known, and then nothing is kept.
    /// Decides what is kept from now on.
    ///
    /// Only provisional (the address may be the identity provider's), and
    /// never over a site a sign-in confirmed. A provisional site that
    /// changes forgets what was held back for the old one.
    pub fn set_app_site(&mut self, url: Option<&str>) {
        if self.confirmed {
            return;
        }
        let site = url.and_then(origin);
        if site != self.app_site {
            self.forget_pending();
        }
        self.app_site = site;
    }

    /// The sign-in typed the account name or password on the page at `url`:
    /// its origin is a login page's, and is never confirmed as the
    /// application's unless it is the configured address's own
    /// (`confirm_app_site`).
    pub fn note_login_site(&mut self, url: &str) {
        let Some(site) = origin(url) else { return };
        if !self.login_sites.contains(&site) {
            self.login_sites.push(site);
        }
    }

    /// A sign-in arrived: its signed-in marker showed on the page at `url`,
    /// which is the application's. Its site is the site from now on; what
    /// was held back for that site is shown, and anything else held back is
    /// forgotten.
    ///
    /// Not when `url` is on a login page's site (`note_login_site`) that is
    /// not the provisional address's own: that is the identity provider
    /// showing the marker on its own page, never the application. And
    /// while no login page has been noted in this browser at all - a saved
    /// session restored into a fresh browser types nothing, so the
    /// provider's page cannot be told from the application's - only the
    /// provisional address's own origin is confirmed. Either way nothing
    /// changes - the site stays as it was, what was held back stays held
    /// back - and one line is logged, naming no host.
    pub fn confirm_app_site(&mut self, url: &str) {
        let Some(site) = origin(url) else { return };
        let configured = self.app_site.as_ref() == Some(&site);
        if !configured && self.login_sites.is_empty() {
            crate::applog::info(
                "Auto Run: the signed-in marker showed on a page that is not on the configured address, before any sign-in typed a login in this browser - its site is not taken for the application's, and nothing is captured until the marker shows on the configured address",
            );
            return;
        }
        if !configured && self.login_sites.contains(&site) {
            crate::applog::info(
                "Auto Run: the signed-in marker showed on the page the sign-in was typed on, which is not the application's address - its site is not taken for the application's, and nothing is captured until the marker shows on the application's own page",
            );
            return;
        }
        if self.app_site.as_ref() != Some(&site) {
            self.forget_pending();
        }
        for s in self.entries.iter_mut() {
            s.pending = false;
        }
        self.app_site = Some(site);
        self.confirmed = true;
    }

    /// Whether the application's site is known for sure: a sign-in arrived
    /// on it (`confirm_app_site`).
    pub fn knows_app_site(&self) -> bool {
        self.confirmed && self.app_site.is_some()
    }

    /// The application's origin once a sign-in confirmed it; `None` while
    /// it is only provisional. Only for an export written to TCM's own
    /// folder (`capture_tools::export`): a HAR's `url` is absolute.
    pub fn confirmed_site(&self) -> Option<&str> {
        if self.confirmed { self.app_site.as_deref() } else { None }
    }

    fn forget_pending(&mut self) {
        let (kept, held): (VecDeque<Slot>, VecDeque<Slot>) =
            std::mem::take(&mut self.entries).into_iter().partition(|s| !s.pending);
        self.entries = kept;
        for s in &held {
            self.forget(s);
        }
    }

    /// Is `url` on the application's site? Never while the site is not
    /// known.
    fn on_app_site(&self, url: &str) -> bool {
        self.app_site.as_ref().is_some_and(|site| origin(url).as_ref() == Some(site))
    }

    /// Note what this event, in the tab with this session and name, showing
    /// the page at `page_url`, says about a request. `record` is that tab's
    /// network record, which has seen the event already: whether a starting
    /// request is kept is what it says of the request's type and polling.
    pub fn observe(&mut self, session: &str, tab: &str, page_url: &str, ev: &Event, record: &NetRecord) {
        let p = &ev.params;
        let Some(request_id) = p["requestId"].as_str() else { return };
        if ev.method == "Network.requestWillBeSent" {
            return self.started(session, tab, page_url, request_id, p, record);
        }
        let Some(at) = self.index_of(session, request_id) else { return };
        match ev.method.as_str() {
            "Network.responseReceived" => {
                let r = &p["response"];
                let headers = kept_headers(&r["headers"]);
                let slot = &mut self.entries[at];
                slot.content_length = header_value(&r["headers"], "content-length").and_then(|v| v.trim().parse().ok());
                slot.c.status = status_of(&r["status"]);
                slot.c.mime = r["mimeType"].as_str().map(str::to_string);
                slot.c.resp_headers = headers;
                let meta = Slot::header_bytes(&slot.c);
                self.bytes = self.bytes + meta - slot.meta;
                slot.meta = meta;
                self.keep_within_budget();
            }
            "Network.dataReceived" => {
                let n = p["dataLength"].as_f64().filter(|n| *n >= 0.0).unwrap_or(0.0) as u64;
                let slot = &mut self.entries[at];
                slot.decoded = slot.decoded.saturating_add(n);
            }
            "Network.loadingFinished" => {
                let slot = &mut self.entries[at];
                slot.c.duration_ms = elapsed_ms(slot.started_at, p);
                slot.c.resp_size = p["encodedDataLength"].as_f64().filter(|n| *n >= 0.0).map(|n| n as u64);
                let cap = MAX_BODY_BYTES as u64;
                let id = slot.c.id;
                if slot.off_site {
                    slot.c.resp_body = Some(Body::OffSite);
                } else if !slot.c.mime.as_deref().is_some_and(text_like) {
                    slot.c.resp_body = Some(Body::NotText);
                } else if slot.decoded > cap || slot.content_length.is_some_and(|n| n > cap) {
                    slot.c.resp_body = Some(Body::TooBig);
                } else {
                    self.queue(BodyRead {
                        id,
                        session: session.to_string(),
                        request_id: request_id.to_string(),
                        method: RESPONSE_BODY,
                        attempts: 0,
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

    fn started(&mut self, session: &str, tab: &str, page_url: &str, request_id: &str, p: &Value, record: &NetRecord) {
        let url = p["request"]["url"].as_str().unwrap_or("");
        // A redirect arrives under the same id: the request goes on, keeping
        // what the page asked; the first hop is kept, the answer is the
        // last hop's - and never read once a hop left the application.
        if let Some(at) = self.index_of(session, request_id) {
            let entry = record.get(request_id);
            let hop_off = !self.on_app_site(url);
            let slot = &mut self.entries[at];
            if slot.c.redirect.is_none() {
                if let Some(r) = entry.and_then(|e| e.redirect.as_ref()) {
                    slot.c.redirect = Some(Redirect { status: r.status, to_path: r.to.clone(), other_site: r.other_site });
                }
            }
            slot.off_site |= hop_off;
            slot.c.status = None;
            slot.c.mime = None;
            slot.c.resp_headers.clear();
            slot.content_length = None;
            slot.decoded = 0;
            let meta = Slot::header_bytes(&slot.c);
            self.bytes = self.bytes + meta - slot.meta;
            slot.meta = meta;
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
        if !matches!(kind, "XHR" | "Fetch" | "Document") || !self.on_app_site(url) || !self.filter.keeps(kind, &e.path_query) {
            return;
        }
        // A provisional site may be the identity provider's: held back, and
        // only from a page on it.
        let pending = !self.confirmed;
        if pending && !self.on_app_site(page_url) {
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
                attempts: 0,
            });
        }
        let started_ms = p["wallTime"]
            .as_f64()
            .map(|s| (s * 1000.0).round() as u64)
            .unwrap_or_else(|| {
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
            });
        let c = Captured {
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
        };
        let meta = Slot::header_bytes(&c);
        let slot = Slot {
            c,
            session: session.to_string(),
            started_at: p["timestamp"].as_f64(),
            off_site: false,
            decoded: 0,
            content_length: None,
            meta,
            pending,
        };
        self.bytes += meta + slot.body_bytes();
        self.entries.push_back(slot);
        self.by_request.insert((session.to_string(), request_id.to_string()), id);
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

    /// A request leaves: its bytes, its id and its waiting reads go with it.
    fn forget(&mut self, s: &Slot) {
        self.bytes -= s.meta + s.body_bytes();
        let key = (s.session.clone(), s.c.request_id.clone());
        if self.by_request.get(&key) == Some(&s.c.id) {
            self.by_request.remove(&key);
        }
        self.reads.retain(|r| r.id != s.c.id);
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

    /// The browser did not answer `read` in time: it goes back to the front
    /// of the queue. A `counted` try (one given its whole time) counts
    /// towards `MAX_READ_ATTEMPTS`, after which the body is `Dropped`; a try
    /// cut short by its caller's own wait does not.
    pub fn retry(&mut self, mut read: BodyRead, counted: bool) {
        if self.index_by_id(read.id).is_none() {
            return;
        }
        if counted {
            read.attempts += 1;
        }
        if read.attempts >= MAX_READ_ATTEMPTS {
            self.settle(&read, Body::Dropped);
        } else {
            self.reads.push_front(read);
        }
    }

    /// The browser's answer to `read` (`Network.getResponseBody`'s `body`
    /// or `getRequestPostData`'s `postData`, base64 or not); `None` when it
    /// refused - the body is gone.
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

    /// Drop the oldest bodies until everything fits `MAX_TOTAL_BODY_BYTES`,
    /// their requests staying; when addresses and headers alone are still
    /// over it, the oldest requests go too.
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
        while self.bytes > MAX_TOTAL_BODY_BYTES {
            let Some(old) = self.entries.pop_front() else { return };
            self.forget(&old);
        }
    }

    /// Kept requests, newest first, from `offset`, at most `limit`, and how
    /// many are kept in all.
    /// Held-back requests (`pending`) are not among them.
    pub fn list(&self, offset: usize, limit: usize) -> (Vec<&Captured>, usize) {
        let shown = || self.entries.iter().rev().filter(|s| !s.pending);
        (shown().skip(offset).take(limit).map(|s| &s.c).collect(), shown().count())
    }

    /// One kept request by the capture's id.
    pub fn get(&self, id: u64) -> Option<&Captured> {
        self.index_by_id(id).map(|at| &self.entries[at]).filter(|s| !s.pending).map(|s| &s.c)
    }

    /// What is kept from now on. What was kept stays; what was left out
    /// is not recoverable.
    pub fn set_filter(&mut self, filter: CaptureFilter) {
        self.filter = filter;
    }

    pub fn filter(&self) -> &CaptureFilter {
        &self.filter
    }

    /// Forget every request and every waiting read; the filter and the
    /// application's site stay, and an id is never handed out again.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.by_request.clear();
        self.reads.clear();
        self.bytes = 0;
    }

    /// Bytes of everything kept now - bodies, headers, addresses
    /// (`MAX_TOTAL_BODY_BYTES` at most).
    pub fn kept_bytes(&self) -> usize {
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
pub(crate) fn text_like(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m.starts_with("text/")
        || ["json", "javascript", "ecmascript", "xml", "x-www-form-urlencoded", "graphql", "html"].iter().any(|w| m.contains(w))
}

/// The value of the header `name` (lower case), in any case, as sent.
fn header_value<'a>(headers: &'a Value, name: &str) -> Option<&'a str> {
    headers.as_object()?.iter().find(|(n, _)| n.trim().eq_ignore_ascii_case(name)).and_then(|(_, v)| v.as_str())
}

/// The headers kept, lower-case names, sorted, one entry per name: a name
/// sent twice (in two cases, or as Chrome joins repeats, by a line break)
/// has its values joined with ", ". See `kept_header`.
pub(crate) fn kept_headers(headers: &Value) -> Vec<(String, String)> {
    let Some(map) = headers.as_object() else { return Vec::new() };
    let mut joined: Vec<(String, Vec<String>)> = Vec::new();
    for (name, v) in map {
        let name = name.trim().to_ascii_lowercase();
        let raw = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
        let values = raw.split('\n').map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        match joined.iter_mut().find(|(n, _)| *n == name) {
            Some((_, all)) => all.extend(values),
            None => joined.push((name, values.collect())),
        }
    }
    let mut kept: Vec<(String, String)> =
        joined.into_iter().filter_map(|(name, all)| kept_header(&name, all.join(", ")).map(|v| (name, v))).collect();
    kept.sort();
    kept
}

/// `value` as kept for the header `name` (lower case), or `None`: the
/// plain ones as sent; any other `x-` header too, its value `HIDDEN` when
/// its name sounds like a secret (`secret_header`); nothing else - never
/// `cookie`, `set-cookie`, `authorization` or `proxy-authorization`.
fn kept_header(name: &str, value: String) -> Option<String> {
    const NEVER: [&str; 4] = ["cookie", "set-cookie", "authorization", "proxy-authorization"];
    const PLAIN: [&str; 4] = ["content-type", "accept", "content-length", "x-requested-with"];
    if NEVER.contains(&name) {
        return None;
    }
    if PLAIN.contains(&name) {
        return Some(value);
    }
    if !name.starts_with("x-") {
        return None;
    }
    Some(if secret_header(name) { HIDDEN.to_string() } else { value })
}

/// A header name that sounds like it carries a secret: the one list of
/// secret words every capture redaction uses (`capture_redact::secret_name`).
fn secret_header(name: &str) -> bool {
    crate::autorun::capture_redact::secret_name(name)
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
