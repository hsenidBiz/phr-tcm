//! The page's requests, in the order they started, for a script step that
//! checks what the page asked its server and what it was answered.
//!
//! Beside `page_log`, not inside it: the page log keeps only what explains
//! a failure (failed, refused, unfinished), and its report is unchanged. This
//! keeps every http(s) request the page makes of a server - documents, XHR,
//! fetch and other - finished or not, so a step can look at the ones that
//! started after it began (`mark`, then `since`). Pictures, scripts, styles
//! and fonts are not kept: a heavy page would push a step's own requests out.
//!
//! Kept per request: its id, method, path + query, start order, status,
//! content type, whether it finished or failed, and its first redirect.
//! Never the host, request headers, request bodies or cookies - and the
//! query string kept here is for matching only; nothing that stores an
//! outcome may copy it out. Bounded: the oldest request goes first.

use super::cdp::Event;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Requests remembered at once; the oldest goes first.
pub const MAX_REQUESTS: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetState {
    /// Started, and not yet finished or failed (it may have been answered).
    Pending,
    /// Its whole response arrived.
    Finished,
    /// It failed, with Chrome's reason (`net::ERR_...`).
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetEntry {
    /// Start order: the first request the record sees is 0.
    pub seq: u64,
    /// Chrome's request id.
    pub id: String,
    pub method: String,
    /// The address without its scheme, host or fragment: `/a/b?x=1`.
    pub path_query: String,
    pub status: Option<u16>,
    pub mime: Option<String>,
    pub state: NetState,
    /// What the request itself answered when the server sent it on.
    pub redirect: Option<NetRedirect>,
}

/// A request's first redirect: the 3xx it answered and where it was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetRedirect {
    pub status: u16,
    /// The path it was sent to - never the host, the query or a fragment.
    pub to: String,
    /// Sent to another site (scheme, host or port).
    pub other_site: bool,
}

#[derive(Default)]
pub struct NetRecord {
    /// Oldest first, with consecutive `seq`s - so a request's place is its
    /// `seq` less the first one's.
    entries: VecDeque<NetEntry>,
    /// Request id to `seq`, for the requests still kept.
    by_id: HashMap<String, u64>,
    next_seq: u64,
    /// When each kept request was first seen, by `seq`: how long one has
    /// been in flight (`quiet`).
    seen_at: HashMap<u64, Instant>,
    /// When a kept request last started, finished or failed.
    last_change: Option<Instant>,
}

/// How busy the page's own requests are right now (`NetRecord::quiet`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetQuiet {
    /// Requests still in flight that started within `LONG_LIVED` - a
    /// document, an XHR or a fetch.
    pub in_flight: usize,
    /// How long since a request last started, finished or failed; `None`
    /// when none ever did.
    pub since_change: Option<Duration>,
}

/// A request in flight for longer than this is a long poll or a stream
/// (SignalR, server-sent events) that may never end: it never holds the
/// page unsettled.
pub const LONG_LIVED: Duration = Duration::from_secs(10);

impl NetRecord {
    /// Note what this event says about a request. Never claims the event:
    /// the page log and the event buffer see it as before.
    pub fn observe(&mut self, ev: &Event) {
        let p = &ev.params;
        match ev.method.as_str() {
            "Network.requestWillBeSent" => self.started(p),
            "Network.responseReceived" => {
                if let Some(e) = self.entry(p) {
                    e.status = status_of(&p["response"]["status"]);
                    e.mime = p["response"]["mimeType"].as_str().map(str::to_string);
                }
            }
            "Network.loadingFinished" => {
                if let Some(e) = self.entry(p) {
                    e.state = NetState::Finished;
                    self.last_change = Some(Instant::now());
                }
            }
            "Network.loadingFailed" => {
                let why = p["errorText"].as_str().unwrap_or("failed").to_string();
                if let Some(e) = self.entry(p) {
                    e.state = NetState::Failed(why);
                    self.last_change = Some(Instant::now());
                }
            }
            _ => {}
        }
    }

    fn started(&mut self, p: &Value) {
        let url = p["request"]["url"].as_str().unwrap_or("");
        // A redirect arrives under the same id: the same request goes on,
        // to the new address, keeping the place it started in - and the
        // method and address the page asked for, which are what a check
        // matches. The 3xx it answered is kept, with the path (only) it was
        // sent to; the later hops' answer and end are followed as before.
        if let Some(e) = self.entry(p) {
            if e.redirect.is_none() {
                if let Some(status) = status_of(&p["redirectResponse"]["status"]) {
                    let from = p["redirectResponse"]["url"].as_str().unwrap_or("");
                    let to = path_query(url).map(|pq| pq.split('?').next().unwrap_or("").to_string());
                    e.redirect = Some(NetRedirect {
                        status,
                        to: to.unwrap_or_default(),
                        other_site: origin(from).is_some() && origin(from) != origin(url),
                    });
                }
            }
            e.status = None;
            e.mime = None;
            e.state = NetState::Pending;
            return;
        }
        // What the page asks a server, not what it loads to show itself.
        if !matches!(p["type"].as_str(), None | Some("Document" | "XHR" | "Fetch" | "Other")) {
            return;
        }
        let Some(path_query) = path_query(url) else { return };
        let Some(id) = p["requestId"].as_str() else { return };
        let method = p["request"]["method"].as_str().unwrap_or("GET").to_string();
        let seq = self.next_seq;
        self.next_seq += 1;
        self.by_id.insert(id.to_string(), seq);
        let now = Instant::now();
        self.seen_at.insert(seq, now);
        self.last_change = Some(now);
        self.entries.push_back(NetEntry {
            seq,
            id: id.to_string(),
            method,
            path_query,
            status: None,
            mime: None,
            state: NetState::Pending,
            redirect: None,
        });
        while self.entries.len() > MAX_REQUESTS {
            if let Some(old) = self.entries.pop_front() {
                // Only if the id still points at this entry: a later entry
                // never reuses an id, but a stale mapping must not survive.
                if self.by_id.get(&old.id) == Some(&old.seq) {
                    self.by_id.remove(&old.id);
                }
                self.seen_at.remove(&old.seq);
            }
        }
    }

    fn entry(&mut self, p: &Value) -> Option<&mut NetEntry> {
        let seq = *self.by_id.get(p["requestId"].as_str()?)?;
        let first = self.entries.front()?.seq;
        let at = usize::try_from(seq.checked_sub(first)?).ok()?;
        self.entries.get_mut(at)
    }

    /// The `seq` the next request to start will get. A step takes this when
    /// it begins and later asks for what came `since`.
    pub fn mark(&self) -> u64 {
        self.next_seq
    }

    /// How busy the page's requests are at `now`: those still in flight
    /// that are not long-lived (`LONG_LIVED`), and how long since any
    /// started or ended. What a click or a fill waits on (`actions::settle`).
    pub fn quiet(&self, now: Instant) -> NetQuiet {
        let in_flight = self
            .entries
            .iter()
            .filter(|e| e.state == NetState::Pending)
            .filter(|e| self.seen_at.get(&e.seq).is_some_and(|at| now.saturating_duration_since(*at) < LONG_LIVED))
            .count();
        NetQuiet { in_flight, since_change: self.last_change.map(|at| now.saturating_duration_since(at)) }
    }

    /// The requests that started at or after `mark`, oldest first, as they
    /// stand now.
    pub fn since(&self, mark: u64) -> Vec<NetEntry> {
        self.entries.iter().filter(|e| e.seq >= mark).cloned().collect()
    }
}

/// A status as Chrome sends it - an integer, sometimes written as a float.
fn status_of(v: &Value) -> Option<u16> {
    let n = v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))?;
    u16::try_from(n).ok()
}

/// `scheme://host:port` of an http(s) address, in lower case, to tell one
/// site from another - compared, never kept.
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // Credentials are not the site.
    let host = host.rsplit('@').next().unwrap_or(host).to_ascii_lowercase();
    let default = if scheme == "https" { ":443" } else { ":80" };
    let host = host.strip_suffix(default).unwrap_or(&host).to_string();
    Some(format!("{scheme}://{host}"))
}

/// `/path?query` of an http(s) address; `None` for any other scheme
/// (`data:`, `blob:`, `ws:`, extensions), which is not a request to check.
fn path_query(url: &str) -> Option<String> {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &url[8..]
    } else if lower.starts_with("http://") {
        &url[7..]
    } else {
        return None;
    };
    let rest = rest.split('#').next().unwrap_or("");
    let tail = match rest.find(['/', '?']) {
        Some(i) => &rest[i..],
        None => "",
    };
    Some(if tail.starts_with('/') { tail.to_string() } else { format!("/{tail}") })
}
