//! A Chrome DevTools Protocol client for the browser Auto Run drives.
//!
//! One connection to the browser itself (`connect`), with a flattened
//! session per tab: every page-level command carries its tab's session,
//! and every event is routed to the tab whose session it carries
//! (`route_event`). Each tab keeps its own events, dialogs, page log,
//! network record and save guard (`Tab`). A tab the page opens (a popup, a
//! `target=_blank` link) is attached paused and given the same setup as the
//! first before it runs (`on_attached`). Calls go to the current tab:
//! `main`, the tab the run started in, until a script switches
//! (`switch_tab`, `open_tab`). A script names the tabs it uses
//! (`expect_tab`), and every tab but `main` is closed when a case ends
//! (`close_other_tabs`).
//!
//! Three things the first version did not do, each of which showed up as a
//! frozen screen rather than an error:
//!
//! - every call has a deadline, so a browser that stops answering is
//!   reported instead of waited on forever;
//! - events are kept while a call waits for its reply, because the event a
//!   caller wants (a page load) usually arrives before it asks;
//! - a JavaScript dialog is answered the moment it opens - as an armed
//!   `expect_dialog` asks, or else accepted (`dialogs`). Measured on real
//!   Edge: an `alert()` leaves every later call pending until it is handled.
//!
//! A no-save script's connection also intercepts every request
//! (`guard_saves`). A paused request holds the page up, so it is answered
//! the moment it is read, inside whichever call read it; between calls a
//! wait loop's pause keeps reading (`idle`), and the supervised browser
//! has a task that does the same between commands.
//!
//! Downloads are followed the same way (`enable_downloads`): the browser's
//! download events are read inside whichever call is in flight, so one that
//! starts and ends during a click is never missed, and a finished file is
//! renamed from its guid to its own name the moment its end is read.
//!
//! The socket sits behind `Transport` and the client behind `Driver`, so
//! both layers are tested without starting a browser.

use futures::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

use super::downloads::{rename_patiently, sanitise_name, unique_name, DownloadEntry, DownloadState};

/// How long any single protocol call may take.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// The floor under a deadline-shortened call. A browser that accepts
/// frames and stops answering would otherwise turn a 15s click into 30s
/// and a ten-action step into minutes, all while the session is locked,
/// so a wait loop caps its calls at its own remaining budget - but a call
/// made right at the edge of that budget still gets this long to answer
/// rather than being cancelled before it can.
pub const MIN_CALL_TIMEOUT: Duration = Duration::from_millis(250);

/// Events nobody has asked for yet. Bounded: a busy page volunteers
/// thousands, and only the recent ones can still matter.
const MAX_BUFFERED_EVENTS: usize = 256;

/// Dialogs remembered for the caller to read back. Bounded the same way:
/// a page stuck in an alert loop during one long wait must not grow this
/// forever, and only the most recent dialogs are worth reporting.
const MAX_REMEMBERED_DIALOGS: usize = 20;

/// The most stopped saves kept until `take_saves_stopped` is asked: a page
/// that saves in a loop on a browser nobody asks never grows past it.
const MAX_STOPPED_SAVES: usize = 500;

/// One request on the wire.
pub fn frame(id: u64, method: &str, params: serde_json::Value) -> String {
    serde_json::json!({ "id": id, "method": method, "params": params }).to_string()
}

/// Is this frame the answer to `id`? `None` means "not ours" - an event,
/// another request's reply, or something unparseable. `Some(Err(..))` is
/// a real protocol error and must never be flattened into an empty
/// success: in a test runner an empty result reads as "found nothing".
pub fn reply_for(id: u64, raw: &str) -> Option<Result<serde_json::Value, String>> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if v.get("id")?.as_u64()? != id {
        return None;
    }
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown DevTools error");
        return Some(Err(msg.to_string()));
    }
    Some(Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null)))
}

/// Something the browser volunteered: a frame with a method and no id.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub method: String,
    pub params: serde_json::Value,
}

pub fn event_of(raw: &str) -> Option<Event> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if v.get("id").is_some() {
        return None;
    }
    let method = v.get("method")?.as_str()?.to_string();
    Some(Event { method, params: v.get("params").cloned().unwrap_or(serde_json::Value::Null) })
}

#[derive(Debug, Clone, PartialEq)]
pub enum CdpError {
    /// Nothing came back in time. `what` is the method or event waited for.
    Timeout { what: String, ms: u64 },
    /// The socket ended: the browser was closed or crashed.
    Closed,
    /// The browser answered, and the answer was a refusal.
    Protocol { method: String, message: String },
    /// The socket itself failed.
    Transport(String),
    /// A tab rule was not met (`there is no tab report`): the browser is
    /// fine, the script named a tab it cannot use. The sentence is the
    /// whole failure.
    Tab(String),
}

/// The tab a run starts in.
pub const MAIN_TAB: &str = "main";

/// Said by a step that names a tab that is not open.
pub fn no_tab(name: &str) -> String {
    format!("there is no tab {name}")
}

/// Said when a tab is given a name another open tab already has.
pub fn tab_taken(name: &str) -> String {
    format!("there is already a tab {name}")
}

/// Said by `close_tab` or `expect_tab_closed` naming `main`.
pub const MAIN_CANNOT_CLOSE: &str = "main cannot be closed";

/// A wait as whole seconds when it is whole (`10`), else to one place
/// (`1.5`).
pub fn whole_seconds(ms: u64) -> String {
    if ms % 1000 == 0 {
        format!("{}", ms / 1000)
    } else {
        format!("{:.1}", ms as f64 / 1000.0)
    }
}

/// Said by `expect_tab` when no new tab came.
pub fn no_new_tab(within: Duration) -> String {
    format!("no new tab opened within {} seconds", whole_seconds(within.as_millis() as u64))
}

/// Said by `expect_tab` when the new tab is somewhere else.
pub fn tab_address_lacks(text: &str) -> String {
    format!("the new tab's address does not contain \"{text}\"")
}

/// Said by `expect_tab_closed` when the tab is still open.
pub fn tab_did_not_close(name: &str, within: Duration) -> String {
    format!("the \"{name}\" tab did not close within {} seconds", whole_seconds(within.as_millis() as u64))
}

impl CdpError {
    /// Worth another look a moment later. A page between two documents
    /// refuses calls ("Cannot find context with specified id") and then
    /// accepts them; a dead socket or a silent browser does not recover.
    pub fn is_transient(&self) -> bool {
        matches!(self, CdpError::Protocol { .. })
    }
}

/// The wording every wait loop (`wait_ready`, `expect`, `wait_for`) uses
/// when its deadline runs out and not one look ever completed - every
/// protocol call it made either timed out or the transport itself failed.
/// Kept here, alongside `CdpError` rather than inside any one loop, so the
/// wording can never drift between them.
pub(crate) fn browser_silent(waited_ms: u64, target: &str) -> String {
    format!("the browser did not answer for {waited_ms}ms while waiting for {target}")
}

impl std::fmt::Display for CdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CdpError::Timeout { what, ms } => {
                write!(f, "the browser did not answer {what} within {ms}ms")
            }
            CdpError::Closed => write!(f, "the browser closed before answering"),
            CdpError::Protocol { method, message } => write!(f, "{method} was refused: {message}"),
            CdpError::Transport(e) => write!(f, "the browser's DevTools socket failed: {e}"),
            CdpError::Tab(sentence) => write!(f, "{sentence}"),
        }
    }
}

impl std::error::Error for CdpError {}

/// The socket, reduced to the two things the client does with it.
pub trait Transport {
    fn send(&mut self, text: String) -> impl Future<Output = Result<(), String>>;
    /// `None` once the socket has closed.
    fn recv(&mut self) -> impl Future<Output = Option<Result<String, String>>>;
}

type Socket = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

pub struct WsTransport {
    socket: Socket,
}

impl Transport for WsTransport {
    async fn send(&mut self, text: String) -> Result<(), String> {
        self.socket.send(Message::Text(text)).await.map_err(|e| e.to_string())
    }

    async fn recv(&mut self) -> Option<Result<String, String>> {
        loop {
            match self.socket.next().await? {
                Ok(Message::Text(t)) => return Some(Ok(t.to_string())),
                Ok(_) => continue, // pings, binary frames: nothing of ours
                Err(e) => return Some(Err(e.to_string())),
            }
        }
    }
}

/// One tab of the browser. Everything that belongs to "the page" lives
/// here: its events, its dialogs, its page log and network record, and its
/// save guard. Each tab is one flattened session on the browser's one
/// connection (`Cdp::connect`).
pub struct Tab {
    /// The session its commands and events carry. Empty for a client made
    /// with `Cdp::over` around one page's own socket, whose frames carry
    /// none.
    pub session_id: String,
    pub target_id: String,
    /// What a script calls it. The tab a run starts in is `main`; a tab the
    /// page opens has no name until a script gives it one.
    pub name: Option<String>,
    /// Where it is, without its query, fragment, user name or password:
    /// safe to log.
    pub url_without_query: String,
    /// Where it is, whole: matched by `expect_tab`'s `url_contains`, and
    /// never logged or kept anywhere else.
    url: String,
    /// When this connection heard it opened: `expect_tab` claims only a
    /// tab opened since the previous step began.
    opened_at: Instant,
    /// Every frame it has shown, by id: a download names the frame that
    /// started it, and so the tab it belongs to. Bounded.
    frames: HashSet<String>,
    events: VecDeque<Event>,
    dialogs: Vec<String>,
    /// What the page did, for a failure to explain itself (`page_log`).
    page_log: super::page_log::PageLog,
    /// Every request the page made, for a step that checks one
    /// (`net_record`). Fed before the page log, which claims these events.
    net_record: super::net_record::NetRecord,
    /// A no-save script's guard (`guard_saves`): `None` while this tab's
    /// requests are not intercepted.
    guard: Option<SaveGuard>,
    /// Opened while guarded and still paused: it runs only once its
    /// `Fetch.enable` was accepted (`on_setup_reply`).
    held: bool,
    /// Its `Fetch.enable` was answered, either way (`on_setup_reply`):
    /// `open_tab` sends a guarded tab nowhere before then.
    guard_answered: bool,
    /// Its service-worker bypass was refused, so it is never let run.
    unguardable: bool,
    /// Every document this tab has loaded (`Page.frameNavigated`), by
    /// loader id, oldest first; bounded.
    loaders: VecDeque<String>,
    /// The loader of its top-level document now.
    main_loader: Option<String>,
    /// Its top-level frame's id, once a navigation said it.
    main_frame: Option<String>,
    /// While a hold is asked for or on: the documents known when it was
    /// asked for. Only a document NOT among them may count as new.
    known_at_hold: Option<HashSet<String>>,
    /// While a hold is on: the top-level document when it took effect (the
    /// page the sign-in arrived on). Dropped once another top-level
    /// document is announced.
    allowed_main: Option<String>,
    /// While a hold is asked for or on: the documents first announced since
    /// it was asked for. With `allowed_main`, the only documents a save may
    /// come from during the hold (`late_save`).
    allowed_new: HashSet<String>,
    /// Which document sent each request (`Network.requestWillBeSent`), by
    /// the network's request id; bounded, oldest first.
    request_loaders: HashMap<String, String>,
    request_order: VecDeque<String>,
    /// Saves paused during a hold whose document is not known yet: answered
    /// once it is (`resolve_parked`).
    parked: Vec<Parked>,
    /// Why its typed `Fetch.enable` was refused, while the catch-all sent in
    /// its place is not answered yet (`on_setup_reply`).
    typed_refusal: Option<String>,
}

/// A save paused during a hold, waiting to learn which document sent it.
struct Parked {
    request_id: String,
    network_id: String,
    method: String,
    url: String,
    since: Instant,
}

/// How long a parked save waits to learn its document before it is
/// stopped anyway (fail closed).
const PARK_LIMIT: Duration = Duration::from_secs(2);

/// How many documents and requests a tab remembers the loader of.
const MAX_LOADERS: usize = 64;
const MAX_REQUEST_LOADERS: usize = 512;
/// How many frames a tab remembers, to place a download.
const MAX_FRAMES: usize = 256;
/// How many named tabs that closed by themselves are remembered.
const MAX_CLOSED_NAMES: usize = 16;
/// How often a wait for a tab (`expect_tab`, `open_tab`) looks again.
const TAB_LOOK: Duration = Duration::from_millis(100);
/// How long the end of a case waits for the browser to close one tab.
const CLOSE_LIMIT: Duration = Duration::from_secs(2);
/// How long the end of a case waits for the browser to dispose of its
/// context (`dispose_context`).
const DISPOSE_LIMIT: Duration = Duration::from_secs(5);
/// How many network marks are remembered (`net_mark`).
const MAX_NET_MARKS: usize = 16;

impl Tab {
    /// Note which document is which: a document loaded, or a request sent
    /// by one.
    fn observe_documents(&mut self, ev: &Event) {
        self.note_frame(ev);
        match ev.method.as_str() {
            "Page.frameNavigated" => {
                let frame = &ev.params["frame"];
                let Some(loader) = frame["loaderId"].as_str().filter(|l| !l.is_empty()) else {
                    return;
                };
                let top = frame.get("parentId").is_none();
                if top {
                    self.main_frame = frame["id"].as_str().map(str::to_string);
                }
                self.announce(loader, top);
            }
            // A navigation's own lifecycle carries its document too, and is
            // what a navigation waits for: the page a sign-in arrived on is
            // known even when its `frameNavigated` is read later.
            "Page.lifecycleEvent" => {
                let Some(loader) = ev.params["loaderId"].as_str().filter(|l| !l.is_empty()) else {
                    return;
                };
                let frame = ev.params["frameId"].as_str();
                if frame.is_some() && frame == self.main_frame_id() {
                    self.announce(loader, true);
                }
            }
            "Network.requestWillBeSent" => {
                let (Some(id), Some(loader)) = (ev.params["requestId"].as_str(), ev.params["loaderId"].as_str()) else {
                    return;
                };
                if self.request_loaders.insert(id.to_string(), loader.to_string()).is_none() {
                    self.request_order.push_back(id.to_string());
                    if self.request_order.len() > MAX_REQUEST_LOADERS {
                        if let Some(old) = self.request_order.pop_front() {
                            self.request_loaders.remove(&old);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// A frame this tab shows, and where its top-level frame is now.
    fn note_frame(&mut self, ev: &Event) {
        let frame = match ev.method.as_str() {
            "Page.frameNavigated" => {
                let f = &ev.params["frame"];
                if f.get("parentId").is_none() {
                    if let Some(url) = f["url"].as_str() {
                        self.url_without_query = address_of(url);
                        self.url = url.to_string();
                    }
                }
                f["id"].as_str()
            }
            "Page.frameAttached" => ev.params["frameId"].as_str(),
            _ => None,
        };
        if let Some(id) = frame {
            if self.frames.len() < MAX_FRAMES || self.frames.contains(id) {
                self.frames.insert(id.to_string());
            }
        }
    }

    /// Does this tab show the frame `id`? Its top-level frame's id is its
    /// target's.
    fn shows_frame(&self, id: &str) -> bool {
        !id.is_empty() && (self.target_id == id || self.frames.contains(id))
    }

    /// A document announced: remembered, made the top-level one if it is,
    /// and during a hold counted as new if it was not known when the hold
    /// was asked for. A new top-level document ends the allowance of the
    /// one the hold took effect on.
    fn announce(&mut self, loader: &str, top: bool) {
        if !self.loaders.iter().any(|l| l == loader) {
            if self.loaders.len() >= MAX_LOADERS {
                self.loaders.pop_front();
            }
            self.loaders.push_back(loader.to_string());
        }
        if let Some(known) = &self.known_at_hold {
            if !known.contains(loader) && self.allowed_new.insert(loader.to_string()) && top {
                self.allowed_main = None;
            }
        }
        if top {
            self.main_loader = Some(loader.to_string());
        }
    }

    /// The top-level frame's id: said by a navigation, or the tab's own
    /// target id.
    fn main_frame_id(&self) -> Option<&str> {
        self.main_frame.as_deref().or(Some(self.target_id.as_str()).filter(|t| !t.is_empty()))
    }

    /// A hold was asked for: from now on, a document not known yet counts
    /// as new.
    fn start_hold(&mut self) {
        self.known_at_hold = Some(self.loaders.iter().cloned().collect());
        self.allowed_new.clear();
        self.allowed_main = None;
    }

    /// The hold took effect: the top-level document now is the page the
    /// sign-in arrived on.
    fn hold_took_effect(&mut self) {
        self.allowed_main = self.main_loader.clone();
    }

    fn end_hold(&mut self) {
        self.known_at_hold = None;
        self.allowed_new.clear();
        self.allowed_main = None;
    }

    /// During a hold, may a save from this document go through? Only the
    /// page the sign-in arrived on and documents first announced since the
    /// hold was asked for: an older one, one forgotten (`MAX_LOADERS`) and
    /// one never seen are all stopped.
    fn may_save_from(&self, loader: &str) -> bool {
        self.allowed_main.as_deref() == Some(loader) || self.allowed_new.contains(loader)
    }

    /// During a hold, is this save still stopped? A ping (a beacon) always
    /// is: only a page being left sends one at that moment. A top-level
    /// navigation starts a new document and goes through (a form the
    /// sign-in page posts). Anything else goes through only from a document
    /// the sign-in owns (`may_save_from`). `None`: its document is not
    /// known yet.
    fn late_save(&self, params: &serde_json::Value) -> Option<bool> {
        if params["resourceType"].as_str() == Some("Ping") {
            return Some(true);
        }
        if params["resourceType"].as_str() == Some("Document") {
            let frame = params["frameId"].as_str();
            if frame.is_some() && frame == self.main_frame_id() {
                return Some(false);
            }
        }
        // Without the Network domain there is no telling which document
        // sent it: stopped (fail closed).
        let Some(network_id) = params["networkId"].as_str() else {
            return Some(true);
        };
        self.request_loaders.get(network_id).map(|l| !self.may_save_from(l))
    }

    fn new(session_id: String, target_id: String, name: Option<String>, url: &str) -> Tab {
        Tab {
            session_id,
            target_id,
            name,
            url_without_query: address_of(url),
            url: url.to_string(),
            opened_at: Instant::now(),
            frames: HashSet::new(),
            events: VecDeque::new(),
            dialogs: vec![],
            page_log: Default::default(),
            net_record: Default::default(),
            guard: None,
            held: false,
            guard_answered: false,
            unguardable: false,
            loaders: VecDeque::new(),
            main_loader: None,
            main_frame: None,
            known_at_hold: None,
            allowed_main: None,
            allowed_new: HashSet::new(),
            request_loaders: HashMap::new(),
            request_order: VecDeque::new(),
            parked: Vec::new(),
            typed_refusal: None,
        }
    }
}

/// An address that is safe to log: no query or fragment (they carry
/// tokens), and no user name or password.
fn address_of(url: &str) -> String {
    let cut = super::page_log::without_query(url);
    if let Some(scheme_end) = cut.find("://") {
        let rest = &cut[scheme_end + 3..];
        let authority_end = rest.find('/').unwrap_or(rest.len());
        if let Some(at) = rest[..authority_end].rfind('@') {
            return format!("{}{}", &cut[..scheme_end + 3], &rest[at + 1..]);
        }
    }
    cut
}

/// The sentence a no-save case fails with when a tab the page opened could
/// not be guarded. That tab is kept paused, so its own document never
/// loads (but see `on_attached` for what can still reach it).
pub const TAB_HELD_UNGUARDED: &str =
    "this script must not save, but a tab the page opened could not be guarded, so it was kept paused";

/// The same for a tab that was already running when it was attached (open
/// before the connection was made): it could not be kept paused.
pub const TAB_OPEN_UNGUARDED: &str = "this script must not save, but a tab that was already open could not be guarded";

/// Which of a new tab's setup frames is waited on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SetupStep {
    Bypass,
    /// `typed`: sent with the typed patterns, so a refusal is retried with
    /// the catch-all.
    Fetch { typed: bool },
    /// The page log, dialog handler or lifecycle events: a refusal only
    /// loses that, and is logged.
    Watch(&'static str),
}

/// One frame read off the socket, parsed once.
enum Incoming {
    Reply { id: u64, answer: Result<serde_json::Value, String> },
    Event { session: Option<String>, ev: Event },
    Other,
}

fn classify(raw: &str) -> Incoming {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Incoming::Other;
    };
    if let Some(id) = v.get("id") {
        let Some(id) = id.as_u64() else {
            return Incoming::Other;
        };
        if let Some(err) = v.get("error") {
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown DevTools error");
            return Incoming::Reply { id, answer: Err(msg.to_string()) };
        }
        return Incoming::Reply { id, answer: Ok(v.get_mut("result").map(|r| r.take()).unwrap_or_default()) };
    }
    let Some(method) = v.get("method").and_then(|m| m.as_str()).map(str::to_string) else {
        return Incoming::Other;
    };
    let session = v.get("sessionId").and_then(|s| s.as_str()).map(str::to_string);
    let params = v.get_mut("params").map(|p| p.take()).unwrap_or_default();
    Incoming::Event { session, ev: Event { method, params } }
}

/// One request on the wire, on a session. No session (`""`) is the
/// browser itself, or a page's own socket.
fn frame_in(id: u64, method: &str, params: serde_json::Value, session: &str) -> String {
    if session.is_empty() {
        return frame(id, method, params);
    }
    serde_json::json!({ "id": id, "method": method, "params": params, "sessionId": session }).to_string()
}

pub struct Cdp<T: Transport = WsTransport> {
    transport: T,
    next_id: u64,
    /// Every tab still open, `main` first.
    tabs: Vec<Tab>,
    /// `main`'s session. Once that tab is gone, every call is `Closed`.
    main: String,
    /// The tab `call` goes to (`switch_tab`).
    current: String,
    /// The name of the current tab when it closed by itself (a print
    /// preview): every call then fails with `there is no tab <name>`, never
    /// waits, until the script switches to another tab.
    gone_current: Option<String>,
    /// Named tabs that closed by themselves and no `expect_tab_closed` has
    /// claimed yet, oldest first. Bounded.
    closed_by_page: VecDeque<String>,
    /// When the previous step and this one began (`step_began`): a tab
    /// opened since the previous one began is one `expect_tab` may claim.
    step_marks: (Option<Instant>, Option<Instant>),
    /// When this connection was made: with no step begun, `expect_tab`
    /// claims a tab opened since then.
    created: Instant,
    /// Each network mark handed out on the whole browser (`net_mark`), with
    /// every tab's own mark at that moment, newest last. Bounded.
    net_marks: std::sync::Mutex<(u64, VecDeque<(u64, HashMap<String, u64>)>)>,
    /// Driving the whole browser over its own socket (`connect`), so a tab
    /// the page opens is attached, paused until it is set up.
    whole_browser: bool,
    /// When the wait loop that owns this connection runs out of time. See
    /// `set_deadline`.
    deadline: Option<Instant>,
    /// The run's guard words while it guards saves: a tab the page opens is
    /// guarded with them before it runs.
    armed: Option<Vec<String>>,
    /// The run's `hold_saves`, for a tab opened during a sign-in. True only
    /// once the hold has taken effect (`hold_marker`).
    hold: bool,
    /// A hold asked for (`hold_saves(true)`) that has not taken effect yet.
    hold_pending: bool,
    /// The tabs a hold covers, by session: the tab the sign-in runs in (the
    /// current tab when it was asked for) and any tab opened during it.
    /// Every other tab stays guarded as before: its saves are still stopped.
    hold_tabs: HashSet<String>,
    /// The first command sent since the hold was asked for. The hold takes
    /// effect only once its answer (or a later one) is read: the browser
    /// writes every event it sent before that command arrived ahead of the
    /// answer, so a request it paused before then is still judged by the
    /// guard, however late it is read.
    hold_marker: Option<u64>,
    /// A stopped save from a tab that has since closed, not yet reported.
    blocked_elsewhere: Option<String>,
    /// This browser refused `Fetch.enable` with the typed patterns once: it
    /// is sent the catch-all from then on (`fetch_params`).
    typed_refused: bool,
    /// Every save stopped since `take_saves_stopped` was last asked, as
    /// (method, path), at most `MAX_STOPPED_SAVES`.
    saves_stopped: Vec<(String, String)>,
    /// Frames sent without waiting (answers to paused requests, a new tab's
    /// setup) not yet known to be sent, oldest first. A deadline can cut a
    /// call short while one is being written; whatever is still here is
    /// written again before the next frame goes out, so a request is never
    /// left paused. A request answered twice gets a refusal for the second
    /// answer, which nobody reads.
    unsent_answers: VecDeque<String>,
    /// Where downloads go and what came of each (`enable_downloads`):
    /// `None` while the browser's downloads are not followed.
    downloads: Option<DownloadFolder>,
    /// Downloads were switched on per page (`Page.setDownloadBehavior`),
    /// so a new tab is asked too.
    downloads_per_page: bool,
    /// The scripts every new document is seeded with
    /// (`Page.addScriptToEvaluateOnNewDocument`), by `main`'s identifier,
    /// for a new tab to be seeded the same way.
    seeds: Vec<(String, serde_json::Value)>,
    /// The setup frames of new tabs whose answers matter, by id.
    setup: HashMap<u64, (String, SetupStep)>,
    /// Calls sent with `send_deferred` whose answers are collected later,
    /// by id: the session each went to, and its answer once read. Whoever
    /// reads frames next keeps the answer here, so the socket still has one
    /// reader. Bounded by `MAX_DEFERRED`.
    deferred: HashMap<u64, (String, Option<Result<serde_json::Value, String>>)>,
    /// Who answers each dialog, in any tab, and the dialogs seen
    /// (`dialogs`). One for the whole run, never per tab: an
    /// `expect_dialog` claims the next dialog wherever it opens.
    book: super::dialogs::DialogBook,
    /// The page errors every tab met since the runner last took them
    /// (`page_errors`). One for the whole run, like `book`.
    page_errors: super::page_errors::PageErrorBook,
    /// The browser context this connection made for its case
    /// (`drive_new_context`), until it is disposed (`dispose_context`).
    /// While set, a page in any other context is never one of its tabs.
    context: Option<String>,
}

/// A browser's downloads: the folder they land in, and every download in
/// start order.
struct DownloadFolder {
    dir: PathBuf,
    entries: Vec<DownloadEntry>,
    /// Which tab (by session) each download belongs to, by guid.
    owners: HashMap<String, String>,
    /// Ends (completed or canceled) read before their download's begin,
    /// oldest first, applied once the begin arrives. Bounded like the
    /// event buffer: a begin that never comes must not grow this forever.
    early_ends: VecDeque<EarlyEnd>,
}

/// A download's end, read before its begin.
struct EarlyEnd {
    guid: String,
    state: String,
    received: Option<f64>,
}

/// How many early ends are remembered.
const MAX_EARLY_ENDS: usize = 32;

/// How many deferred calls (`send_deferred`) are kept waiting at once.
const MAX_DEFERRED: usize = 16;

/// What a guarded tab does with each paused request.
struct SaveGuard {
    /// The project's own save words, beside the built-in ones.
    patterns: Vec<String>,
    /// While true (a sign-in), every request goes on, saves included.
    hold: bool,
    /// The first save stopped and not yet reported, as the case's sentence.
    blocked: Option<String>,
}

impl Cdp<WsTransport> {
    /// Open the browser's own socket and drive its first page over it
    /// (`drive_first_page`). The page is the first one `/json/list` names,
    /// as it always was.
    pub async fn connect(port: u16) -> Result<Cdp<WsTransport>, String> {
        let tabs = Self::ask(port, "list").await?;
        let page = tabs
            .as_array()
            .and_then(|a| a.iter().find(|t| t["type"] == "page"))
            .ok_or_else(|| "the browser reported no page to drive".to_string())?;
        let target = page["id"].as_str().unwrap_or("").to_string();
        let url = page["url"].as_str().unwrap_or("").to_string();
        let mut cdp = Self::connect_browser(port).await?;
        cdp.drive_first_page(&target, &url).await.map_err(|e| e.to_string())?;
        Ok(cdp)
    }

    /// Open the browser's own socket and drive nothing yet: the caller
    /// makes the page it drives (`drive_new_context`). A browser that is
    /// running but does not answer is given up on after
    /// `ado::HTTP_CONNECT_TIMEOUT`, so a wedged browser kept for a whole run
    /// cannot stall the next case's start.
    pub async fn connect_browser(port: u16) -> Result<Cdp<WsTransport>, String> {
        Self::connect_browser_within(port, crate::ado::HTTP_CONNECT_TIMEOUT).await
    }

    /// `connect_browser`, giving up after `limit`.
    pub async fn connect_browser_within(port: u16, limit: Duration) -> Result<Cdp<WsTransport>, String> {
        let connecting = async {
            let version = Self::ask(port, "version").await?;
            let ws = version["webSocketDebuggerUrl"]
                .as_str()
                .ok_or_else(|| "the browser did not say where its DevTools socket is".to_string())?
                .to_string();
            let (socket, _) = tokio_tungstenite::connect_async(&ws)
                .await
                .map_err(|e| format!("could not open the DevTools socket: {e}"))?;
            Ok(Cdp::over(WsTransport { socket }))
        };
        tokio::time::timeout(limit, connecting)
            .await
            .unwrap_or_else(|_| Err(format!("the DevTools socket did not open within {}ms", limit.as_millis())))
    }

    /// Does a browser answer on this DevTools port yet?
    pub async fn answers(port: u16) -> Result<(), String> {
        Self::ask(port, "version").await.map(|_| ())
    }

    async fn ask(port: u16, what: &str) -> Result<serde_json::Value, String> {
        let url = format!("http://127.0.0.1:{port}/json/{what}");
        let body = reqwest::get(&url)
            .await
            .map_err(|e| format!("the browser did not answer on port {port}: {e}"))?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&body).map_err(|e| e.to_string())
    }
}

impl<T: Transport> Cdp<T> {
    /// A client for one page's own socket: no sessions, one tab.
    pub fn over(transport: T) -> Self {
        Cdp {
            transport,
            next_id: 1,
            tabs: vec![Tab::new(String::new(), String::new(), Some("main".to_string()), "")],
            main: String::new(),
            current: String::new(),
            gone_current: None,
            closed_by_page: VecDeque::new(),
            step_marks: (None, None),
            created: Instant::now(),
            net_marks: std::sync::Mutex::new((0, VecDeque::new())),
            whole_browser: false,
            deadline: None,
            armed: None,
            hold: false,
            hold_pending: false,
            hold_tabs: HashSet::new(),
            hold_marker: None,
            blocked_elsewhere: None,
            typed_refused: false,
            saves_stopped: Vec::new(),
            unsent_answers: VecDeque::new(),
            downloads: None,
            downloads_per_page: false,
            seeds: vec![],
            setup: HashMap::new(),
            deferred: HashMap::new(),
            book: super::dialogs::DialogBook::default(),
            page_errors: super::page_errors::PageErrorBook::default(),
            context: None,
        }
    }

    /// On the browser's own socket: attach to `target` as `main`, with a
    /// flattened session every page-level command then carries; switch on
    /// page events (dialogs) and lifecycle events there; and have every
    /// tab the page opens attached too, paused until it is set up
    /// (`on_attached`).
    ///
    /// A lifecycle event carries the `frameId` and `loaderId` a plain
    /// `Page.loadEventFired` does not, which is what lets a navigation tell
    /// its OWN load apart from one still in flight from an earlier
    /// navigation or from a sub-frame. Nothing else needs enabling: the
    /// accessibility and DOM calls this app uses work without it.
    ///
    /// Auto-attach is asked of the browser, not of the page: a page's own
    /// auto-attach reaches its frames and workers, never a new window.
    pub async fn drive_first_page(&mut self, target: &str, url: &str) -> Result<(), CdpError> {
        let attached = self
            .call_on(None, "Target.attachToTarget", serde_json::json!({ "targetId": target, "flatten": true }), CALL_TIMEOUT)
            .await?;
        let session = attached["sessionId"].as_str().unwrap_or("").to_string();
        if session.is_empty() {
            return Err(CdpError::Protocol {
                method: "Target.attachToTarget".to_string(),
                message: "no session came back".to_string(),
            });
        }
        self.tabs = vec![Tab::new(session.clone(), target.to_string(), Some("main".to_string()), url)];
        self.main = session.clone();
        self.current = session;
        self.whole_browser = true;
        self.call("Page.enable", serde_json::json!({})).await?;
        self.call("Page.setLifecycleEventsEnabled", serde_json::json!({ "enabled": true })).await?;
        self.call_on(
            None,
            "Target.setAutoAttach",
            serde_json::json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true }),
            CALL_TIMEOUT,
        )
        .await?;
        Ok(())
    }

    /// On the browser's own socket: a browser context of this connection's
    /// own, which starts with no cookies and no storage as a fresh profile
    /// does; a blank page in it; and that page driven as `main`
    /// (`drive_first_page`). From then on a page in any other context is
    /// never one of this connection's tabs (`on_attached`), and downloads
    /// are asked of this context (`enable_downloads`). The context stays
    /// until `dispose_context`: after a failure partway the caller closes
    /// the browser, and the context with it.
    pub async fn drive_new_context(&mut self) -> Result<(), CdpError> {
        let made = self
            .call_on(None, "Target.createBrowserContext", serde_json::json!({ "disposeOnDetach": false }), CALL_TIMEOUT)
            .await?;
        let context = made["browserContextId"].as_str().unwrap_or("").to_string();
        if context.is_empty() {
            return Err(CdpError::Protocol {
                method: "Target.createBrowserContext".to_string(),
                message: "no context came back".to_string(),
            });
        }
        self.context = Some(context.clone());
        let page = self
            .call_on(
                None,
                "Target.createTarget",
                serde_json::json!({ "url": "about:blank", "browserContextId": context }),
                CALL_TIMEOUT,
            )
            .await?;
        let target = page["targetId"].as_str().unwrap_or("").to_string();
        if target.is_empty() {
            return Err(CdpError::Protocol {
                method: "Target.createTarget".to_string(),
                message: "no page came back".to_string(),
            });
        }
        self.drive_first_page(&target, "about:blank").await
    }

    /// On the browser's own socket: the page the browser started with,
    /// driven as `main` (`drive_first_page`), as `connect` drives it; a
    /// blank page made in the default context only when there is none. For
    /// a browser that will not make contexts, where each case has a
    /// browser of its own. Making a page beside the one it started with
    /// would leave that one open, and the auto-attach would take it for a
    /// tab the case opened.
    pub async fn drive_new_page(&mut self) -> Result<(), CdpError> {
        let listed = self.call_on(None, "Target.getTargets", serde_json::json!({}), CALL_TIMEOUT).await?;
        let started_with = listed["targetInfos"].as_array().and_then(|all| {
            all.iter().find(|t| t["type"] == "page" && !t["attached"].as_bool().unwrap_or(false))
        });
        if let Some(page) = started_with {
            let target = page["targetId"].as_str().unwrap_or("").to_string();
            let url = page["url"].as_str().unwrap_or("").to_string();
            if !target.is_empty() {
                return self.drive_first_page(&target, &url).await;
            }
        }
        let page =
            self.call_on(None, "Target.createTarget", serde_json::json!({ "url": "about:blank" }), CALL_TIMEOUT).await?;
        let target = page["targetId"].as_str().unwrap_or("").to_string();
        if target.is_empty() {
            return Err(CdpError::Protocol {
                method: "Target.createTarget".to_string(),
                message: "no page came back".to_string(),
            });
        }
        self.drive_first_page(&target, "about:blank").await
    }

    /// The context `drive_new_context` made, until it is disposed.
    pub fn browser_context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    /// Close this connection's context, and every page in it, with its
    /// cookies and storage. Nothing to do for a connection that made none.
    /// An `Err` means the browser could not be asked or did not answer in
    /// time: it may be gone, and is not to be trusted with another case.
    pub async fn dispose_context(&mut self) -> Result<(), CdpError> {
        let Some(context) = self.context.take() else {
            return Ok(());
        };
        self.call_on(None, "Target.disposeBrowserContext", serde_json::json!({ "browserContextId": context }), DISPOSE_LIMIT)
            .await
            .map(|_| ())
    }

    /// Every tab still open, `main` first.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// The tab `call` goes to. `None` once it has closed.
    pub fn current(&self) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.session_id == self.current)
    }

    /// For tests that need to see what was sent.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// For tests that feed frames in after the client was made.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    fn has_tab(&self, session: &str) -> bool {
        self.tabs.iter().any(|t| t.session_id == session)
    }

    /// The tab an event with this session belongs to. No session is the
    /// current tab.
    fn tab_index(&self, session: Option<&str>) -> Option<usize> {
        let s = session.unwrap_or(&self.current);
        self.tabs.iter().position(|t| t.session_id == s)
    }

    fn current_index(&self) -> Option<usize> {
        self.tab_index(None)
    }

    /// Intercept every request the page makes, and fail the saves among
    /// them (`save_guard::is_save`) before they leave the browser. Every
    /// paused request is answered in `route_event`, the moment it is read,
    /// so nothing waits on the caller. `Err` leaves the tabs not yet asked
    /// unguarded and the run's guard state as it was, and the caller must
    /// not run a no-save script on it. Asked again, it takes the new words
    /// and keeps a stopped save not yet reported.
    ///
    /// Service workers are bypassed first: a page's worker could otherwise
    /// send a request the page's own interception never sees. A browser that
    /// refuses the bypass is not guarded at all (fail closed).
    ///
    /// Every tab is guarded: each one open now, and each one the page opens
    /// later, which is attached paused and guarded before its own document
    /// loads (`on_attached`). The run counts as guarded from the start of
    /// this call, so a tab that opens while the others are still being
    /// asked is guarded too, and the tabs are looked over again until none
    /// is left unguarded.
    ///
    /// What is still NOT covered: out-of-process (cross-site) iframes are
    /// separate targets with their own requests, and a save sent from one
    /// would go through; and a page script can drive the `about:blank`
    /// popup it just opened before that popup's guard is on.
    pub async fn guard_saves(&mut self, patterns: &[String]) -> Result<(), CdpError> {
        let before = (self.armed.replace(patterns.to_vec()), self.hold);
        self.hold = false;
        self.hold_pending = false;
        self.hold_tabs.clear();
        self.hold_marker = None;
        for t in &mut self.tabs {
            t.end_hold();
        }
        let mut asked: Vec<String> = Vec::new();
        // First every tab open now, so each takes the new words; then any
        // tab that came without a guard while those were asked.
        let mut next: Vec<String> = self.tabs.iter().map(|t| t.session_id.clone()).collect();
        while !next.is_empty() {
            for s in next {
                asked.push(s.clone());
                match self.guard_tab(&s, patterns).await {
                    Ok(()) => {}
                    // A tab other than `main` that closed meanwhile has
                    // nothing left to guard.
                    Err(CdpError::Closed) if s != self.main && !self.has_tab(&s) => {}
                    Err(e) => {
                        (self.armed, self.hold) = before;
                        return Err(e);
                    }
                }
            }
            next = self
                .tabs
                .iter()
                .filter(|t| t.guard.is_none() && !asked.contains(&t.session_id))
                .map(|t| t.session_id.clone())
                .collect();
        }
        Ok(())
    }

    /// What `Fetch.enable` is sent in this browser: the typed patterns, or
    /// the catch-all once it refused them.
    fn fetch_params(&self) -> serde_json::Value {
        if self.typed_refused {
            super::save_guard::fetch_enable_all_params()
        } else {
            super::save_guard::fetch_enable_params()
        }
    }

    /// The browser refused the typed patterns and then took the catch-all on
    /// the same session: said once, with its reason, and the catch-all is
    /// sent from now on.
    fn typed_were_refused(&mut self, why: &str) {
        if !self.typed_refused {
            self.typed_refused = true;
            crate::applog::warn(format!("{}{why}", super::save_guard::TYPED_REFUSED));
        }
    }

    async fn guard_tab(&mut self, session: &str, patterns: &[String]) -> Result<(), CdpError> {
        let limit = self.limit_now();
        self.call_on(Some(session.to_string()), "Network.setBypassServiceWorker", serde_json::json!({ "bypass": true }), limit)
            .await?;
        let typed = !self.typed_refused;
        let limit = self.limit_now();
        match self.call_on(Some(session.to_string()), "Fetch.enable", self.fetch_params(), limit).await {
            Ok(_) => {}
            // A browser that does not know one of the types: every request
            // is paused instead. Refused too, the guard fails closed.
            // The browser is switched to the catch-all only once it took it
            // on the same session: a closing tab's refusal is no type's.
            Err(CdpError::Protocol { message, .. }) if typed => {
                let limit = self.limit_now();
                let all = super::save_guard::fetch_enable_all_params();
                self.call_on(Some(session.to_string()), "Fetch.enable", all, limit).await?;
                self.typed_were_refused(&message);
            }
            Err(e) => return Err(e),
        }
        if let Some(t) = self.tabs.iter_mut().find(|t| t.session_id == session) {
            let blocked = t.guard.as_mut().and_then(|g| g.blocked.take());
            t.guard = Some(SaveGuard { patterns: patterns.to_vec(), hold: false, blocked });
        }
        Ok(())
    }

    /// Stop intercepting, in every tab. The run stops counting as guarded
    /// first, so a tab that opens meanwhile is not guarded, and a tab still
    /// held for its guard is let run. A tab's guard goes only once the
    /// browser has said it stopped: until then requests may still be
    /// paused, and they are still answered as a guarded tab answers them.
    /// One tab that will not stop does not keep the others guarded: every
    /// tab is asked, and the first refusal is reported after.
    pub async fn stop_guarding_saves(&mut self) -> Result<(), CdpError> {
        self.armed = None;
        let mut held = Vec::new();
        for t in self.tabs.iter_mut().filter(|t| t.held) {
            t.held = false;
            held.push(t.session_id.clone());
        }
        for s in held {
            self.queue(&s, "Runtime.runIfWaitingForDebugger", serde_json::json!({}), None);
        }
        self.send_unsent_answers().await?;
        let sessions: Vec<String> = self.tabs.iter().map(|t| t.session_id.clone()).collect();
        let mut first_refusal = None;
        for s in sessions {
            let limit = self.limit_now();
            match self.call_on(Some(s.clone()), "Fetch.disable", serde_json::json!({}), limit).await {
                Ok(_) => {
                    if let Some(t) = self.tabs.iter_mut().find(|t| t.session_id == s) {
                        t.guard = None;
                    }
                }
                Err(CdpError::Closed) if s != self.main && !self.has_tab(&s) => {}
                Err(e) => {
                    first_refusal.get_or_insert(e);
                }
            }
        }
        match first_refusal {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// The run is guarded, or some tab still intercepts its requests: a
    /// lift that did not finish is asked again.
    pub fn is_guarding_saves(&self) -> bool {
        self.armed.is_some() || self.tabs.iter().any(|t| t.guard.is_some())
    }

    /// Something must keep reading this connection between calls: a
    /// guarded tab's requests wait on an answer, and on the browser's own
    /// socket a tab the page opens waits to be set up.
    pub fn wants_reading(&self) -> bool {
        self.whole_browser || self.tabs.iter().any(|t| t.guard.is_some())
    }

    /// Let every request through for a while, saves included, without
    /// switching interception off: a sign-in is the runner's own, and what
    /// it sends is not the script's draft. Only in the tab the sign-in runs
    /// in (the current tab), and a tab opened meanwhile (a sign-in's own
    /// popup): every other tab is still the script's, and its saves are
    /// still stopped.
    ///
    /// A request is judged as it was when the browser paused it, not when
    /// its pause is read: a save the page sent as it was left (a beacon on
    /// pagehide) can be read after the sign-in has arrived and asked for
    /// the hold, and it must still be stopped. So the hold takes effect
    /// only once the browser has answered a command sent after it was asked
    /// for (`hold_marker`); every request paused before that is read first,
    /// and is refused. Ending the hold takes effect at once.
    ///
    /// A request the browser pauses after the hold took effect can still be
    /// the page being left (its keepalive beacon reaching the network
    /// late): only the page the sign-in arrived on, and documents first
    /// announced after the hold was asked for, may save; a ping never may
    /// (`Tab::late_save`). A save whose document is not known yet waits,
    /// paused, at most `PARK_LIMIT`, even in the middle of a command.
    pub fn hold_saves(&mut self, hold: bool) {
        if hold {
            if !self.hold && !self.hold_pending {
                self.hold_pending = true;
                self.hold_marker = None;
                self.hold_tabs = HashSet::from([self.current.clone()]);
                for t in self.tabs.iter_mut().filter(|t| t.session_id == self.current) {
                    t.start_hold();
                }
            }
            return;
        }
        self.hold_pending = false;
        self.hold_marker = None;
        self.set_hold(false);
        self.hold_tabs.clear();
        for t in &mut self.tabs {
            t.end_hold();
        }
    }

    fn set_hold(&mut self, hold: bool) {
        self.hold = hold;
        for t in &mut self.tabs {
            let covered = self.hold_tabs.contains(&t.session_id);
            if let Some(g) = t.guard.as_mut() {
                g.hold = hold && covered;
            }
        }
    }

    /// A command's answer was read: a hold waiting on it takes effect.
    fn note_reply(&mut self, id: u64) {
        if self.hold_marker.is_some_and(|m| id >= m) {
            self.hold_pending = false;
            self.hold_marker = None;
            self.set_hold(true);
            for t in self.tabs.iter_mut().filter(|t| self.hold_tabs.contains(&t.session_id)) {
                t.hold_took_effect();
            }
        }
    }

    /// The first save stopped since the last time this was asked, in any
    /// tab, as the case's sentence (`save_guard::blocked`). Reported once.
    pub fn take_save_blocked(&mut self) -> Option<String> {
        let mut first = None;
        for t in &mut self.tabs {
            if let Some(b) = t.guard.as_mut().and_then(|g| g.blocked.take()) {
                first.get_or_insert(b);
            }
        }
        let elsewhere = self.blocked_elsewhere.take();
        first.or(elsewhere)
    }

    /// Every save stopped since the last time this was asked, in any tab,
    /// as (method, path): never the host or the query.
    pub fn take_saves_stopped(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.saves_stopped)
    }

    /// Save every download the page starts into `dir` (made if missing),
    /// and follow each one: the browser saves it under its guid, and once it
    /// completes it is renamed to its own name (`downloads::sanitise_name`,
    /// numbered by `downloads::unique_name` when that is taken).
    ///
    /// Asks the browser itself through the Browser domain, which names files
    /// by guid. A browser that refuses that is asked through the Page domain
    /// instead, which saves under the browser's own name: then the file is
    /// found under its name once it completes, and a repeat name is numbered
    /// by the browser. Asked again with another folder, later downloads go
    /// there and the ones so far are kept.
    pub async fn enable_downloads(&mut self, dir: &Path) -> Result<(), CdpError> {
        std::fs::create_dir_all(dir).map_err(|e| CdpError::Protocol {
            method: "Browser.setDownloadBehavior".to_string(),
            message: format!("the download folder could not be made: {e}"),
        })?;
        let path = dir.to_string_lossy().into_owned();
        let limit = self.limit_now();
        let mut params = serde_json::json!({ "behavior": "allowAndName", "downloadPath": path, "eventsEnabled": true });
        // A connection with a context of its own asks for that context:
        // left out, the browser's default context is the one asked.
        if let Some(context) = &self.context {
            params["browserContextId"] = serde_json::json!(context);
        }
        let asked = self.call_on(None, "Browser.setDownloadBehavior", params, limit).await;
        match asked {
            Ok(_) => self.downloads_per_page = false,
            Err(CdpError::Protocol { .. }) => {
                self.call("Page.setDownloadBehavior", serde_json::json!({ "behavior": "allow", "downloadPath": path }))
                    .await?;
                self.downloads_per_page = true;
            }
            Err(e) => return Err(e),
        }
        let (entries, owners, early_ends) = self
            .downloads
            .take()
            .map(|f| (f.entries, f.owners, f.early_ends))
            .unwrap_or_default();
        self.downloads = Some(DownloadFolder { dir: dir.to_path_buf(), entries, owners, early_ends });
        Ok(())
    }

    /// Every download the current tab started so far, in start order: its
    /// own, and those of a tab no script named or one that has closed (a
    /// popup that only downloaded). Another named tab's are that tab's.
    /// Empty unless `enable_downloads` ran.
    pub fn downloads(&self) -> Vec<DownloadEntry> {
        let Some(f) = self.downloads.as_ref() else {
            return Vec::new();
        };
        let elsewhere = |owner: &String| {
            *owner != self.current
                && self.tabs.iter().any(|t| t.session_id == *owner && t.name.is_some())
        };
        f.entries
            .iter()
            .filter(|e| !f.owners.get(&e.guid).is_some_and(elsewhere))
            .cloned()
            .collect()
    }

    /// Note a download event (`Browser.*`, or the Page domain's twin) for
    /// the tab (`owner`) it belongs to. Says whether it was one, so
    /// `route_event` keeps it out of the buffer.
    fn follow_download(&mut self, ev: &Event, owner: &str) -> bool {
        let begins = matches!(ev.method.as_str(), "Browser.downloadWillBegin" | "Page.downloadWillBegin");
        let moves = matches!(ev.method.as_str(), "Browser.downloadProgress" | "Page.downloadProgress");
        let Some(folder) = self.downloads.as_mut().filter(|_| begins || moves) else {
            return false;
        };
        let guid = ev.params["guid"].as_str().unwrap_or("");
        // The guid is the file's name on disk: anything but a plain id is
        // not followed, so it can never name a path.
        if guid.is_empty() || !guid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return true;
        }
        let dir = folder.dir.clone();
        if begins {
            // A browser that sends both domains' events says it twice.
            if !folder.entries.iter().any(|e| e.guid == guid) {
                let mut entry = DownloadEntry {
                    guid: guid.to_string(),
                    name: sanitise_name(ev.params["suggestedFilename"].as_str().unwrap_or("")),
                    path: dir.join(guid),
                    started_at: Instant::now(),
                    state: DownloadState::InProgress,
                    bytes: 0,
                };
                // Its end may have been read first: applied now, rename
                // included.
                if let Some(i) = folder.early_ends.iter().position(|e| e.guid == guid) {
                    let end = folder.early_ends.remove(i).expect("position was just found");
                    progress_download(&dir, &mut entry, &end.state, end.received);
                }
                folder.owners.insert(guid.to_string(), owner.to_string());
                folder.entries.push(entry);
            }
            return true;
        }
        let state = ev.params["state"].as_str().unwrap_or("");
        let received = ev.params["receivedBytes"].as_f64();
        match folder.entries.iter_mut().find(|e| e.guid == guid) {
            Some(entry) => progress_download(&dir, entry, state, received),
            // An end read before its begin is kept for it. A mere progress
            // report is not: the begin starts the download at no bytes, and
            // the next report says how far it got.
            None if matches!(state, "completed" | "canceled") => {
                if !folder.early_ends.iter().any(|e| e.guid == guid) {
                    if folder.early_ends.len() >= MAX_EARLY_ENDS {
                        folder.early_ends.pop_front();
                    }
                    folder.early_ends.push_back(EarlyEnd { guid: guid.to_string(), state: state.to_string(), received });
                }
            }
            None => {}
        }
        true
    }

    /// Wait this long. A connection that is guarded, following downloads,
    /// or driving the whole browser keeps reading while it waits, so a
    /// request the page makes between two calls is answered at once rather
    /// than left paused until the next call, a download's end is seen while
    /// a step waits for it, and a tab the page opens is set up and let run;
    /// any other just sleeps, as every wait loop always did.
    pub async fn idle(&mut self, wait: Duration) {
        // An armed `expect_dialog` is waiting on a dialog: it is read, and
        // answered, the moment it opens.
        if self.wants_reading() || self.downloads.is_some() || self.book.is_armed() {
            self.pump(wait).await;
        } else {
            tokio::time::sleep(wait).await;
        }
    }

    /// Read and handle whatever the browser sends for this long. Only the
    /// wait for a frame is ever cut short - a frame already read is handled
    /// to the end, so an answer to a paused request is never half sent.
    pub async fn pump(&mut self, wait: Duration) {
        let until = Instant::now() + wait;
        if self.send_unsent_answers().await.is_err() {
            tokio::time::sleep(wait).await;
            return;
        }
        loop {
            if self.resolve_parked().await.is_err() {
                tokio::time::sleep(until.saturating_duration_since(Instant::now())).await;
                return;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            // A parked save running out of time cuts the wait for a frame
            // short, so it is answered on time.
            let due = self.park_deadline().map(|d| d.saturating_duration_since(Instant::now()));
            let wait = due.map_or(left, |d| d.min(left));
            match tokio::time::timeout(wait, self.next_frame()).await {
                Err(_) if wait < left => continue,
                Err(_) => return,
                // The socket is gone: nothing more will come, so the rest
                // of the wait is a plain one.
                Ok(Err(_)) => {
                    tokio::time::sleep(left).await;
                    return;
                }
                Ok(Ok(raw)) => {
                    let handled = match classify(&raw) {
                        Incoming::Event { session, ev } => self.route_event(session, ev).await,
                        Incoming::Reply { id, answer } => self.on_setup_reply(id, answer).await,
                        Incoming::Other => Ok(()),
                    };
                    if handled.is_err() {
                        tokio::time::sleep(until.saturating_duration_since(Instant::now())).await;
                        return;
                    }
                }
            }
        }
    }

    /// Continue or fail one paused request, at once, on the tab it paused
    /// in. Sent without waiting, like the dialog answer: its reply carries
    /// an id nobody waits on and falls through `read_reply` harmlessly. A
    /// tab that is not guarded (any more) continues it - a request is never
    /// left paused. One from a session no tab owns any more is judged by
    /// the run's own guard words.
    async fn answer_paused(
        &mut self,
        at: Option<usize>,
        session: Option<String>,
        params: &serde_json::Value,
    ) -> Result<(), CdpError> {
        let request_id = params["requestId"].as_str().unwrap_or("").to_string();
        let method = params["request"]["method"].as_str().unwrap_or("GET");
        let url = params["request"]["url"].as_str().unwrap_or("");
        let stop = match at {
            Some(i) => {
                let tab = &self.tabs[i];
                let verdict = match tab.guard.as_ref() {
                    Some(g) if super::save_guard::is_save(method, url, &g.patterns) => {
                        if g.hold { tab.late_save(params) } else { Some(true) }
                    }
                    _ => Some(false),
                };
                let Some(stop) = verdict else {
                    // Which document sent it is not known yet: it waits,
                    // paused, until it is (`resolve_parked`).
                    self.tabs[i].parked.push(Parked {
                        request_id,
                        network_id: params["networkId"].as_str().unwrap_or("").to_string(),
                        method: method.to_string(),
                        url: url.to_string(),
                        since: Instant::now(),
                    });
                    return Ok(());
                };
                if stop {
                    if let Some(g) = self.tabs[i].guard.as_mut() {
                        g.blocked.get_or_insert_with(|| super::save_guard::blocked(method, url));
                    }
                }
                stop
            }
            None => match &self.armed {
                Some(p) if !self.hold && super::save_guard::is_save(method, url, p) => {
                    if self.blocked_elsewhere.is_none() {
                        self.blocked_elsewhere = Some(super::save_guard::blocked(method, url));
                    }
                    true
                }
                _ => false,
            },
        };
        let on = at.map(|i| self.tabs[i].session_id.clone()).or(session).unwrap_or_default();
        self.queue_answer(&on, &request_id, method, url, stop);
        self.send_unsent_answers().await
    }

    /// Queue the answer to one paused request: stopped or continued.
    fn queue_answer(&mut self, session: &str, request_id: &str, method: &str, url: &str, stop: bool) {
        let (what, params) = if stop {
            let (method, path) = (method.trim().to_ascii_uppercase(), super::save_guard::path_of(url));
            crate::applog::warn(format!("Auto Run stopped a save the page tried to send: {method} {path}"));
            if self.saves_stopped.len() < MAX_STOPPED_SAVES {
                self.saves_stopped.push((method, path));
            }
            ("Fetch.failRequest", serde_json::json!({ "requestId": request_id, "errorReason": "BlockedByClient" }))
        } else {
            ("Fetch.continueRequest", serde_json::json!({ "requestId": request_id }))
        };
        self.queue(session, what, params, None);
    }

    /// When the oldest parked save runs out of time, if any is parked.
    fn park_deadline(&self) -> Option<Instant> {
        self.tabs.iter().flat_map(|t| t.parked.iter()).map(|p| p.since + PARK_LIMIT).min()
    }

    /// The next frame, or `None` when a parked save ran out of time first:
    /// a command waiting on a page that waits on that save is never held
    /// up past `PARK_LIMIT`.
    async fn next_frame_or_park(&mut self) -> Result<Option<String>, CdpError> {
        match self.park_deadline() {
            None => self.next_frame().await.map(Some),
            Some(due) => match tokio::time::timeout(due.saturating_duration_since(Instant::now()), self.next_frame()).await {
                Ok(frame) => frame.map(Some),
                Err(_) => Ok(None),
            },
        }
    }

    /// Answer every parked save whose fate is known now: its document is
    /// known, the hold or the guard ended, or it waited `PARK_LIMIT`
    /// (stopped, fail closed).
    async fn resolve_parked(&mut self) -> Result<(), CdpError> {
        if self.tabs.iter().all(|t| t.parked.is_empty()) {
            return Ok(());
        }
        let mut answers = Vec::new();
        for t in &mut self.tabs {
            let mut waiting = Vec::new();
            for p in std::mem::take(&mut t.parked) {
                let verdict = match t.guard.as_ref() {
                    None => Some(false),
                    Some(g) if !g.hold => Some(true),
                    Some(_) => match t.request_loaders.get(&p.network_id) {
                        Some(loader) => Some(!t.may_save_from(loader)),
                        None if p.since.elapsed() >= PARK_LIMIT => Some(true),
                        None => None,
                    },
                };
                match verdict {
                    None => waiting.push(p),
                    Some(stop) => {
                        if stop {
                            if let Some(g) = t.guard.as_mut() {
                                g.blocked.get_or_insert_with(|| super::save_guard::blocked(&p.method, &p.url));
                            }
                        }
                        answers.push((t.session_id.clone(), p, stop));
                    }
                }
            }
            t.parked = waiting;
        }
        for (session, p, stop) in answers {
            self.queue_answer(&session, &p.request_id, &p.method, &p.url, stop);
        }
        self.send_unsent_answers().await
    }

    /// One frame to send without waiting for its answer, kept until it is
    /// known to be written (see `unsent_answers`). `step` says the answer
    /// matters to a new tab's setup.
    fn queue(&mut self, session: &str, method: &str, params: serde_json::Value, step: Option<SetupStep>) {
        let id = self.next_id;
        self.next_id += 1;
        if let Some(step) = step {
            self.setup.insert(id, (session.to_string(), step));
        }
        self.unsent_answers.push_back(frame_in(id, method, params, session));
    }

    /// Write every frame still waiting, oldest first. Each leaves the list
    /// only once its write finished, so one cut short is written again.
    async fn send_unsent_answers(&mut self) -> Result<(), CdpError> {
        while let Some(next) = self.unsent_answers.front().cloned() {
            self.transport.send(next).await.map_err(CdpError::Transport)?;
            self.unsent_answers.pop_front();
        }
        Ok(())
    }

    /// A tab attached (`Target.setAutoAttach`). A page the page opened gets
    /// exactly what `main` has, on its own session, before it runs: the
    /// save guard while the run is guarded, the page log, the dialog
    /// handler and lifecycle events, the seed scripts, and per-page
    /// downloads. Only then is it let run - and while guarded, only once
    /// the browser has accepted its interception (`on_setup_reply`), so a
    /// tab that cannot be guarded never loads its own document. That hold
    /// is not airtight: the page that opened it can still script an
    /// `about:blank` popup it holds a handle to before the popup's guard is
    /// on, and a request sent that way is not intercepted. Anything else (a
    /// worker, a service worker included) is let run with no setup at all,
    /// and a second session on a tab already driven is let go.
    async fn on_attached(&mut self, p: &serde_json::Value) -> Result<(), CdpError> {
        let session = p["sessionId"].as_str().unwrap_or("").to_string();
        if session.is_empty() {
            return Ok(());
        }
        let info = &p["targetInfo"];
        let target = info["targetId"].as_str().unwrap_or("").to_string();
        let page = info["type"].as_str() == Some("page");
        let waiting = p["waitingForDebugger"].as_bool().unwrap_or(false);
        let known = self.tabs.iter().any(|t| t.target_id == target || t.session_id == session);
        // A page that names another context is another case's, or the
        // browser's own default context's: let run, and let go. One that
        // does not say its context is not known to be anyone else's (cases
        // run one at a time, so no other context is live): it is taken as
        // this case's tab, and so guarded before it runs while the run is
        // guarded. Letting it go would let it run unguarded.
        let foreign = self
            .context
            .as_deref()
            .is_some_and(|c| info["browserContextId"].as_str().is_some_and(|named| named != c));
        if !page || known || foreign {
            if waiting {
                self.queue(&session, "Runtime.runIfWaitingForDebugger", serde_json::json!({}), None);
            }
            if page && !self.has_tab(&session) {
                self.queue("", "Target.detachFromTarget", serde_json::json!({ "sessionId": session }), None);
            }
            return self.send_unsent_answers().await;
        }
        let mut tab = Tab::new(session.clone(), target, None, info["url"].as_str().unwrap_or(""));
        crate::applog::info(format!("a tab opened: {}", super::save_guard::path_of(&tab.url_without_query)));
        // Opened during a sign-in's hold: every document it loads is first
        // seen after the hold, so the sign-in's own.
        if self.hold || self.hold_pending {
            tab.start_hold();
            self.hold_tabs.insert(session.clone());
        }
        let armed = self.armed.clone();
        if let Some(words) = &armed {
            tab.guard = Some(SaveGuard { patterns: words.clone(), hold: self.hold, blocked: None });
            tab.held = waiting;
        }
        self.tabs.push(tab);
        let s = session.as_str();
        if armed.is_some() {
            self.queue(s, "Network.setBypassServiceWorker", serde_json::json!({ "bypass": true }), Some(SetupStep::Bypass));
            let typed = !self.typed_refused;
            self.queue(s, "Fetch.enable", self.fetch_params(), Some(SetupStep::Fetch { typed }));
        }
        for (method, params) in [
            ("Network.enable", serde_json::json!({})),
            ("Runtime.enable", serde_json::json!({})),
            ("Page.enable", serde_json::json!({})),
            ("Page.setLifecycleEventsEnabled", serde_json::json!({ "enabled": true })),
        ] {
            self.queue(s, method, params, Some(SetupStep::Watch(method)));
        }
        for (_, params) in self.seeds.clone() {
            self.queue(s, "Page.addScriptToEvaluateOnNewDocument", params, None);
        }
        if self.downloads_per_page {
            if let Some(dir) = self.downloads.as_ref().map(|f| f.dir.to_string_lossy().into_owned()) {
                self.queue(s, "Page.setDownloadBehavior", serde_json::json!({ "behavior": "allow", "downloadPath": dir }), None);
            }
        }
        if !(armed.is_some() && waiting) {
            self.queue(s, "Runtime.runIfWaitingForDebugger", serde_json::json!({}), None);
        }
        self.send_unsent_answers().await
    }

    /// The answer to one of a new tab's setup frames. A guarded tab runs
    /// once its interception is on; one whose bypass or interception was
    /// refused stays held, and its case fails (`TAB_HELD_UNGUARDED`, or
    /// `TAB_OPEN_UNGUARDED` for a tab that was never paused). Once the run
    /// is no longer guarded, a held tab is let run whatever the answer. A
    /// refused page log, dialog handler or lifecycle events only loses
    /// that, and is logged.
    async fn on_setup_reply(&mut self, id: u64, answer: Result<serde_json::Value, String>) -> Result<(), CdpError> {
        self.note_reply(id);
        // A deferred call's answer, kept for `collect`.
        if let Some((_, slot)) = self.deferred.get_mut(&id) {
            *slot = Some(answer);
            return Ok(());
        }
        let Some((session, step)) = self.setup.remove(&id) else {
            return Ok(());
        };
        let Some(i) = self.tabs.iter().position(|t| t.session_id == session) else {
            return Ok(());
        };
        let refused = answer.is_err();
        let unarmed = self.armed.is_none();
        // The typed patterns refused while the run is still guarded: asked
        // again with the catch-all, the tab still held until that answer.
        // The browser is switched to the catch-all only once that answer is
        // a yes (below).
        if let (SetupStep::Fetch { typed: true }, Err(message), false) = (step, &answer, unarmed) {
            self.tabs[i].typed_refusal = Some(message.clone());
            let all = super::save_guard::fetch_enable_all_params();
            self.queue(&session, "Fetch.enable", all, Some(SetupStep::Fetch { typed: false }));
            return self.send_unsent_answers().await;
        }
        // Taken only by the catch-all's own answer: the tab's other setup
        // answers arrive in between.
        if matches!(step, SetupStep::Fetch { typed: false }) {
            if let Some(why) = self.tabs[i].typed_refusal.take() {
                if answer.is_ok() {
                    self.typed_were_refused(&why);
                }
            }
        }
        let tab = &mut self.tabs[i];
        if matches!(step, SetupStep::Fetch { .. }) {
            tab.guard_answered = true;
        }
        if let Err(message) = &answer {
            let (what, method) = match step {
                SetupStep::Bypass => ("guard", "Network.setBypassServiceWorker"),
                SetupStep::Fetch { .. } => ("guard", "Fetch.enable"),
                SetupStep::Watch(m) => ("watch", m),
            };
            // The page's path only: never its host or query.
            crate::applog::warn(format!(
                "Auto Run could not {what} a tab the page opened, {method} was refused ({message}): {}",
                super::save_guard::path_of(&tab.url_without_query)
            ));
        }
        match step {
            SetupStep::Watch(_) => return Ok(()),
            SetupStep::Bypass => tab.unguardable |= refused,
            SetupStep::Fetch { .. } => {}
        }
        let fetch = matches!(step, SetupStep::Fetch { .. });
        if fetch && !unarmed && (refused || tab.unguardable) {
            let sentence = if tab.held { TAB_HELD_UNGUARDED } else { TAB_OPEN_UNGUARDED };
            if let Some(g) = tab.guard.as_mut() {
                g.blocked.get_or_insert_with(|| sentence.to_string());
            }
            return Ok(());
        }
        if tab.held && (fetch || unarmed) {
            tab.held = false;
            self.queue(&session, "Runtime.runIfWaitingForDebugger", serde_json::json!({}), None);
            return self.send_unsent_answers().await;
        }
        Ok(())
    }

    /// A tab closed by itself (detached or crashed): its state goes with
    /// it, except a stopped save not yet reported. Once `main` is gone every
    /// call is `Closed`, as when the browser itself closes. A named tab is
    /// remembered for `expect_tab_closed`; the current one stays current, so
    /// the next call says `there is no tab <name>` rather than acting in
    /// another tab the script never chose.
    fn drop_tab(&mut self, session: &str) {
        let Some(tab) = self.forget_tab(session) else {
            return;
        };
        let Some(name) = tab.name.filter(|_| session != self.main) else {
            return;
        };
        if self.closed_by_page.len() >= MAX_CLOSED_NAMES {
            self.closed_by_page.pop_front();
        }
        self.closed_by_page.push_back(name.clone());
        if self.current == session {
            self.gone_current = Some(name);
        }
    }

    /// Take a tab out of the list. A stopped save not yet reported is kept.
    fn forget_tab(&mut self, session: &str) -> Option<Tab> {
        let i = self.tabs.iter().position(|t| t.session_id == session)?;
        let mut tab = self.tabs.remove(i);
        if let Some(b) = tab.guard.as_mut().and_then(|g| g.blocked.take()) {
            self.blocked_elsewhere.get_or_insert(b);
        }
        self.setup.retain(|_, (s, _)| s != session);
        Some(tab)
    }

    /// Make the tab with this session current.
    fn make_current(&mut self, session: String) {
        self.current = session;
        self.gone_current = None;
    }

    /// The error a call to a session no tab owns any more gets: the current
    /// tab that closed by itself is `there is no tab <name>`; anything else
    /// is the browser gone.
    fn gone(&self, session: &str) -> CdpError {
        match &self.gone_current {
            Some(name) if session == self.current && session != self.main => CdpError::Tab(no_tab(name)),
            _ => CdpError::Closed,
        }
    }

    fn tab_named(&self, name: &str) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.name.as_deref() == Some(name))
    }

    /// A step began. A tab opened since the step before it began may be
    /// claimed by `expect_tab`.
    pub fn step_began(&mut self) {
        self.step_marks = (self.step_marks.1, Some(Instant::now()));
    }

    /// The current tab's name: `main`, a name a script gave, or the name of
    /// the current tab that closed by itself.
    pub fn tab_name(&self) -> String {
        if let Some(name) = &self.gone_current {
            return name.clone();
        }
        self.current().and_then(|t| t.name.clone()).unwrap_or_else(|| MAIN_TAB.to_string())
    }

    /// The current tab's name, when that tab closed by itself.
    pub fn missing_tab(&self) -> Option<String> {
        self.gone_current.clone()
    }

    /// Make the tab called `name` current and bring it to the front.
    pub async fn switch_tab(&mut self, name: &str) -> Result<(), CdpError> {
        let Some(tab) = self.tab_named(name) else {
            return Err(CdpError::Tab(no_tab(name)));
        };
        let (session, target) = (tab.session_id.clone(), tab.target_id.clone());
        self.make_current(session);
        if self.whole_browser && !target.is_empty() {
            let limit = self.limit_now();
            match self.call_on(None, "Target.activateTarget", serde_json::json!({ "targetId": target }), limit).await {
                Ok(_) => {}
                // Only the front of the screen is lost: the steps still act
                // in this tab.
                Err(CdpError::Protocol { message, .. }) => {
                    crate::applog::warn(format!("Auto Run could not bring the \"{name}\" tab to the front: {message}"));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Close the tab called `name`. `main` is never closed. When it was the
    /// current tab, `main` is current now, and the answer says so (`true`).
    pub async fn close_tab(&mut self, name: &str) -> Result<bool, CdpError> {
        if name == MAIN_TAB {
            return Err(CdpError::Tab(MAIN_CANNOT_CLOSE.to_string()));
        }
        let Some(tab) = self.tab_named(name) else {
            return Err(CdpError::Tab(no_tab(name)));
        };
        let (session, target) = (tab.session_id.clone(), tab.target_id.clone());
        let limit = self.limit_now();
        self.call_on(None, "Target.closeTarget", serde_json::json!({ "targetId": target }), limit).await?;
        self.forget_tab(&session);
        let was_current = self.current == session;
        if was_current {
            let main = self.main.clone();
            self.make_current(main);
        }
        Ok(was_current)
    }

    /// Wait up to `within` for the newest tab nobody has named that opened
    /// since the previous step began (or this one, for a first step), and
    /// call it `name`. With `url_contains`, its address must contain that
    /// text, looked at until the wait ends. Answers its address without the
    /// query. A tab opened earlier is never claimed.
    pub async fn expect_tab(
        &mut self,
        name: &str,
        url_contains: Option<&str>,
        within: Duration,
    ) -> Result<String, CdpError> {
        if self.tab_named(name).is_some() {
            return Err(CdpError::Tab(tab_taken(name)));
        }
        let since = self.step_marks.0.or(self.step_marks.1).unwrap_or(self.created);
        let until = Instant::now() + within;
        loop {
            let newest = self
                .tabs
                .iter_mut()
                .filter(|t| t.name.is_none() && t.opened_at >= since)
                .max_by_key(|t| t.opened_at);
            let mut elsewhere = false;
            if let Some(t) = newest {
                match url_contains {
                    Some(text) if !t.url.contains(text) => elsewhere = true,
                    _ => {
                        t.name = Some(name.to_string());
                        return Ok(t.url_without_query.clone());
                    }
                }
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(CdpError::Tab(match (elsewhere, url_contains) {
                    (true, Some(text)) => tab_address_lacks(text),
                    _ => no_new_tab(within),
                }));
            }
            self.pump(left.min(TAB_LOOK)).await;
        }
    }

    /// Wait up to `within` for the tab called `name` to close by itself. A
    /// tab that already did since it was named passes at once. When it was
    /// the current tab, `main` is current now.
    pub async fn expect_tab_closed(&mut self, name: &str, within: Duration) -> Result<(), CdpError> {
        if name == MAIN_TAB {
            return Err(CdpError::Tab(MAIN_CANNOT_CLOSE.to_string()));
        }
        let until = Instant::now() + within;
        loop {
            if let Some(i) = self.closed_by_page.iter().position(|n| n == name) {
                self.closed_by_page.remove(i);
                if self.gone_current.as_deref() == Some(name) {
                    let main = self.main.clone();
                    self.make_current(main);
                }
                return Ok(());
            }
            if self.tab_named(name).is_none() {
                return Err(CdpError::Tab(no_tab(name)));
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(CdpError::Tab(tab_did_not_close(name, within)));
            }
            self.pump(left.min(TAB_LOOK)).await;
        }
    }

    /// Open a new tab called `name`, blank, in `main`'s browser context, and
    /// make it current. It is attached and set up like any tab the page
    /// opens (`on_attached`) - guarded first while the run is guarded - and
    /// only then is it handed back, so the caller's navigation is its first
    /// real request. While guarded, that is once the browser has ANSWERED
    /// its `Fetch.enable`: a tab whose guard was refused is closed again,
    /// and the step fails with `TAB_HELD_UNGUARDED`.
    pub async fn open_tab(&mut self, name: &str) -> Result<(), CdpError> {
        if name == MAIN_TAB || self.tab_named(name).is_some() {
            return Err(CdpError::Tab(tab_taken(name)));
        }
        let main_target = self.tabs.iter().find(|t| t.session_id == self.main).map(|t| t.target_id.clone());
        let mut params = serde_json::json!({ "url": "about:blank" });
        if let Some(target) = main_target.filter(|t| !t.is_empty()) {
            let limit = self.limit_now();
            match self.call_on(None, "Target.getTargetInfo", serde_json::json!({ "targetId": target }), limit).await {
                Ok(info) => {
                    if let Some(context) = info["targetInfo"]["browserContextId"].as_str() {
                        params["browserContextId"] = serde_json::json!(context);
                    }
                }
                // The default context is main's when nothing says otherwise.
                Err(CdpError::Protocol { .. }) => {}
                Err(e) => return Err(e),
            }
        }
        let limit = self.limit_now();
        let made = self.call_on(None, "Target.createTarget", params, limit).await?;
        let target = made["targetId"].as_str().unwrap_or("").to_string();
        let until = Instant::now() + limit;
        loop {
            if let Some(t) = self.tabs.iter_mut().find(|t| !target.is_empty() && t.target_id == target) {
                let guarded = t.guard.is_some();
                if guarded && t.guard_answered && (t.unguardable || t.guard.as_ref().is_some_and(|g| g.blocked.is_some())) {
                    // Never sent anywhere: closed again, its stop dropped
                    // (the step's own failure says it).
                    let session = t.session_id.clone();
                    if let Some(g) = t.guard.as_mut() {
                        g.blocked = None;
                    }
                    let limit = self.limit_now();
                    let closed =
                        self.call_on(None, "Target.closeTarget", serde_json::json!({ "targetId": target }), limit).await;
                    self.forget_tab(&session);
                    if let Err(e) = closed {
                        crate::applog::warn(format!("Auto Run could not close a tab it could not guard: {e}"));
                    }
                    return Err(CdpError::Tab(TAB_HELD_UNGUARDED.to_string()));
                }
                if (!guarded || t.guard_answered) && !t.held {
                    t.name = Some(name.to_string());
                    let session = t.session_id.clone();
                    self.make_current(session);
                    return Ok(());
                }
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(CdpError::Timeout { what: "Target.createTarget".to_string(), ms: limit.as_millis() as u64 });
            }
            self.pump(left.min(TAB_LOOK)).await;
        }
    }

    /// The end of a case: every tab but `main` is closed, `main` is current
    /// again, and no tab is remembered as named or closed. Best effort, and
    /// quick: a browser that does not answer at once is not waited on (it
    /// is closed anyway, or the next case's first call says so).
    pub async fn close_other_tabs(&mut self) {
        let others: Vec<(String, String)> = self
            .tabs
            .iter()
            .filter(|t| t.session_id != self.main)
            .map(|t| (t.session_id.clone(), t.target_id.clone()))
            .collect();
        for (session, target) in others {
            let answer = if self.whole_browser {
                self.call_on(None, "Target.closeTarget", serde_json::json!({ "targetId": target }), CLOSE_LIMIT).await
            } else {
                Ok(serde_json::Value::Null)
            };
            match answer {
                // Asked: gone, whatever the answer was, or whether it came.
                Ok(_) | Err(CdpError::Protocol { .. }) | Err(CdpError::Timeout { .. }) | Err(CdpError::Tab(_)) => {
                    if let Err(e) = &answer {
                        crate::applog::warn(format!("Auto Run asked to close a tab at the end of the case: {e}"));
                    }
                    self.forget_tab(&session);
                }
                // The browser itself is gone: nothing more can be asked.
                Err(CdpError::Closed) => {
                    self.forget_tab(&session);
                    break;
                }
                // Never sent: kept, and nothing more is asked.
                Err(e @ CdpError::Transport(_)) => {
                    crate::applog::warn(format!("Auto Run could not close a tab at the end of the case: {e}"));
                    break;
                }
            }
        }
        let main = self.main.clone();
        self.make_current(main);
        self.closed_by_page.clear();
        self.step_marks = (None, None);
    }

    /// Every download so far, from every tab, in start order.
    pub fn all_downloads(&self) -> Vec<DownloadEntry> {
        self.downloads.as_ref().map(|f| f.entries.clone()).unwrap_or_default()
    }

    /// The tab (by session) a download event belongs to: the one whose
    /// session it carries; else the one showing the frame that started it;
    /// else the current tab.
    fn download_owner(&self, session: Option<&str>, ev: &Event) -> String {
        if let Some(t) = session.and_then(|s| self.tabs.iter().find(|t| t.session_id == s)) {
            return t.session_id.clone();
        }
        let frame = ev.params["frameId"].as_str().unwrap_or("");
        match self.tabs.iter().find(|t| t.shows_frame(frame)) {
            Some(t) => t.session_id.clone(),
            None => self.current.clone(),
        }
    }

    /// Cap every later `call` at this instant as well as at
    /// `CALL_TIMEOUT`. A wait loop sets it when it starts and clears it
    /// (`None`) before it returns, so one silent call can never outlive
    /// the whole action's budget - and nothing after the loop inherits a
    /// deadline that has already passed.
    pub fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }

    /// `CALL_TIMEOUT`, or what is left of the current deadline if that is
    /// sooner, never less than `MIN_CALL_TIMEOUT`.
    fn limit_now(&self) -> Duration {
        match self.deadline {
            None => CALL_TIMEOUT,
            Some(d) => {
                CALL_TIMEOUT.min(d.saturating_duration_since(Instant::now())).max(MIN_CALL_TIMEOUT)
            }
        }
    }

    pub async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CdpError> {
        self.call_within(method, params, self.limit_now()).await
    }

    /// A call to the current tab. A seed script added there is kept, so a
    /// tab the page opens later is seeded the same way.
    pub async fn call_within(
        &mut self,
        method: &str,
        params: serde_json::Value,
        limit: Duration,
    ) -> Result<serde_json::Value, CdpError> {
        let session = self.current.clone();
        if !self.has_tab(&session) {
            return Err(self.gone(&session));
        }
        let seeding = method == "Page.addScriptToEvaluateOnNewDocument";
        let unseeding = method == "Page.removeScriptToEvaluateOnNewDocument";
        let seed = if seeding || unseeding { Some(params.clone()) } else { None };
        let answer = match self.call_on(Some(session.clone()), method, params, limit).await {
            // The current tab closed while it was asked.
            Err(CdpError::Closed) if !self.has_tab(&session) => Err(self.gone(&session)),
            other => other,
        };
        if let Some(seed) = seed {
            if seeding {
                if let Ok(r) = &answer {
                    self.seeds.push((r["identifier"].as_str().unwrap_or("").to_string(), seed));
                }
            } else if let Some(id) = seed["identifier"].as_str() {
                self.seeds.retain(|(i, _)| i != id);
            }
        }
        answer
    }

    /// One call on a session (`None` is the browser itself), and its
    /// answer. A tab that closes while its call waits is `Closed`.
    async fn call_on(
        &mut self,
        session: Option<String>,
        method: &str,
        params: serde_json::Value,
        limit: Duration,
    ) -> Result<serde_json::Value, CdpError> {
        self.resolve_parked().await?;
        self.send_unsent_answers().await?;
        let id = self.next_id;
        self.next_id += 1;
        if self.hold_pending && self.hold_marker.is_none() {
            self.hold_marker = Some(id);
        }
        self.transport
            .send(frame_in(id, method, params, session.as_deref().unwrap_or("")))
            .await
            .map_err(CdpError::Transport)?;
        match tokio::time::timeout(limit, self.read_reply(id, method, session)).await {
            Ok(answer) => answer,
            Err(_) => Err(CdpError::Timeout { what: method.to_string(), ms: limit.as_millis() as u64 }),
        }
    }

    /// Send a call to the current tab without waiting for its answer, and
    /// return its id for `collect`. Nothing is read here: the answer is
    /// kept by whichever read comes next, a later call's or `collect`'s.
    /// At most `MAX_DEFERRED` are kept at once; the oldest is let go.
    pub async fn send_deferred(&mut self, method: &str, params: serde_json::Value) -> Result<u64, CdpError> {
        let session = self.current.clone();
        if !self.has_tab(&session) {
            return Err(self.gone(&session));
        }
        self.resolve_parked().await?;
        self.send_unsent_answers().await?;
        let id = self.next_id;
        self.next_id += 1;
        if self.hold_pending && self.hold_marker.is_none() {
            self.hold_marker = Some(id);
        }
        if self.deferred.len() >= MAX_DEFERRED {
            if let Some(oldest) = self.deferred.keys().min().copied() {
                self.deferred.remove(&oldest);
            }
        }
        self.deferred.insert(id, (session.clone(), None));
        if let Err(e) = self.transport.send(frame_in(id, method, params, &session)).await {
            self.deferred.remove(&id);
            return Err(CdpError::Transport(e));
        }
        Ok(id)
    }

    /// The answer to a call `send_deferred` sent, read for at most `limit`
    /// from now. A tab that closed first, or a socket that went away, is
    /// `Closed`. Either way the call is forgotten.
    pub async fn collect(&mut self, id: u64, method: &str, limit: Duration) -> Result<serde_json::Value, CdpError> {
        let answer = tokio::time::timeout(limit, self.read_deferred(id)).await;
        self.deferred.remove(&id);
        match answer {
            Err(_) => Err(CdpError::Timeout { what: method.to_string(), ms: limit.as_millis() as u64 }),
            Ok(Err(e)) => Err(e),
            Ok(Ok(Err(message))) => Err(CdpError::Protocol { method: method.to_string(), message }),
            Ok(Ok(Ok(v))) => Ok(v),
        }
    }

    /// Read frames until the deferred call `id` has its answer.
    async fn read_deferred(&mut self, id: u64) -> Result<Result<serde_json::Value, String>, CdpError> {
        loop {
            let session = match self.deferred.get_mut(&id) {
                None => return Err(CdpError::Closed),
                Some((session, slot)) => match slot.take() {
                    Some(answer) => return Ok(answer),
                    None => session.clone(),
                },
            };
            if !self.has_tab(&session) {
                return Err(CdpError::Closed);
            }
            let Some(raw) = self.next_frame_or_park().await? else {
                self.resolve_parked().await?;
                continue;
            };
            match classify(&raw) {
                Incoming::Reply { id: got, answer } => self.on_setup_reply(got, answer).await?,
                Incoming::Event { session, ev } => self.route_event(session, ev).await?,
                Incoming::Other => {}
            }
            self.resolve_parked().await?;
        }
    }

    async fn read_reply(
        &mut self,
        id: u64,
        method: &str,
        session: Option<String>,
    ) -> Result<serde_json::Value, CdpError> {
        loop {
            let Some(raw) = self.next_frame_or_park().await? else {
                self.resolve_parked().await?;
                continue;
            };
            match classify(&raw) {
                Incoming::Reply { id: got, answer } if got == id => {
                    self.note_reply(got);
                    return answer.map_err(|message| CdpError::Protocol { method: method.to_string(), message });
                }
                // A new tab's setup, or a reply to a call that already
                // timed out, or to a fire-and-forget answer.
                Incoming::Reply { id: got, answer } => self.on_setup_reply(got, answer).await?,
                Incoming::Event { session, ev } => self.route_event(session, ev).await?,
                Incoming::Other => {}
            }
            self.resolve_parked().await?;
            if let Some(s) = &session {
                if !self.has_tab(s) {
                    return Err(CdpError::Closed);
                }
            }
        }
    }

    async fn next_frame(&mut self) -> Result<String, CdpError> {
        match self.transport.recv().await {
            None => Err(CdpError::Closed),
            Some(Err(e)) => Err(CdpError::Transport(e)),
            Some(Ok(raw)) => Ok(raw),
        }
    }

    /// Hand an event to the tab whose session it carries (no session: the
    /// current tab), after the browser's own: a tab attached, detached or
    /// crashed. A download event belongs to the tab whose session it
    /// carries, and otherwise to `main`. An event for a session no tab owns
    /// is dropped, except a paused request, which is always answered.
    async fn route_event(&mut self, session: Option<String>, ev: Event) -> Result<(), CdpError> {
        match ev.method.as_str() {
            "Target.attachedToTarget" => return self.on_attached(&ev.params).await,
            "Target.detachedFromTarget" => {
                if let Some(s) = ev.params["sessionId"].as_str() {
                    self.drop_tab(&s.to_string());
                }
                return Ok(());
            }
            "Target.targetCrashed" => {
                let target = ev.params["targetId"].as_str().unwrap_or("");
                if let Some(s) = self.tabs.iter().find(|t| t.target_id == target).map(|t| t.session_id.clone()) {
                    self.drop_tab(&s);
                }
                return Ok(());
            }
            "Inspector.targetCrashed" | "Inspector.detached" if session.is_some() => {
                self.drop_tab(session.as_deref().unwrap_or(""));
                return Ok(());
            }
            _ => {}
        }
        let at = self.tab_index(session.as_deref());
        // A paused request holds the page up until it is answered, so it is
        // answered here, the moment it is read - inside whichever call or
        // idle wait read it.
        if ev.method == "Fetch.requestPaused" {
            return self.answer_paused(at, session, &ev.params).await;
        }
        if ev.method == "Page.javascriptDialogOpening" {
            // A dialog in a tab not registered yet (one the page opened a
            // moment ago) is still answered on its own session: left open,
            // it would hold that page up.
            let on = match (at, &session) {
                (Some(i), _) => self.tabs[i].session_id.clone(),
                (None, Some(s)) => s.clone(),
                (None, None) => return Ok(()),
            };
            let kind = ev.params["type"].as_str().unwrap_or("dialog");
            let message = ev.params["message"].as_str().unwrap_or("");
            // The run's armed `expect_dialog` claims it, from whichever tab;
            // nobody's is accepted, as always, and noted on the tab.
            let answer = self.book.opened(kind, message);
            if let Some(i) = at {
                let tab = &mut self.tabs[i];
                if self.book.seen().last().is_some_and(|s| s.claimed_by.is_none()) {
                    if tab.dialogs.len() >= MAX_REMEMBERED_DIALOGS {
                        tab.dialogs.remove(0);
                    }
                    tab.dialogs.push(format!("{kind}: {message}"));
                }
            }
            // Sent without waiting: its reply carries an id nobody is
            // waiting on and falls through `read_reply` harmlessly.
            let id = self.next_id;
            self.next_id += 1;
            self.transport
                .send(frame_in(id, "Page.handleJavaScriptDialog", answer, &on))
                .await
                .map_err(CdpError::Transport)?;
            return Ok(());
        }
        // Read the moment it arrives, like a paused request: a download
        // that starts and ends during one click is still followed.
        let owner = self.download_owner(session.as_deref(), &ev);
        if self.follow_download(&ev, &owner) {
            return Ok(());
        }
        let Some(i) = at else {
            return Ok(());
        };
        // Any tab's script errors and 5xx answers, for a script that checks
        // page errors - read before the page log takes the event.
        self.page_errors.observe(&ev);
        let tab = &mut self.tabs[i];
        tab.observe_documents(&ev);
        let parked = ev.method == "Network.requestWillBeSent" && !tab.parked.is_empty();
        // The record sees every event first and never claims one, so the
        // page log below is fed exactly as before.
        tab.net_record.observe(&ev);
        // Network and console events go to the page log, never into the
        // buffer: a page volunteers thousands, and they would push out the
        // load event a navigation is about to wait for.
        if tab.page_log.observe(&ev) {
            if parked {
                self.resolve_parked().await?;
            }
            return Ok(());
        }
        if tab.events.len() >= MAX_BUFFERED_EVENTS {
            tab.events.pop_front();
        }
        tab.events.push_back(ev);
        Ok(())
    }

    /// The next event with this method in the current tab: one already
    /// buffered, or the next to arrive within `limit`.
    pub async fn wait_event(&mut self, method: &str, limit: Duration) -> Result<Event, CdpError> {
        let Some(i) = self.current_index() else {
            return Err(self.gone(&self.current));
        };
        if let Some(p) = self.tabs[i].events.iter().position(|e| e.method == method) {
            return Ok(self.tabs[i].events.remove(p).expect("position was just found"));
        }
        self.send_unsent_answers().await?;
        let what = method.to_string();
        match tokio::time::timeout(limit, self.read_event(method)).await {
            Ok(answer) => answer,
            Err(_) => Err(CdpError::Timeout { what, ms: limit.as_millis() as u64 }),
        }
    }

    async fn read_event(&mut self, method: &str) -> Result<Event, CdpError> {
        loop {
            let Some(raw) = self.next_frame_or_park().await? else {
                self.resolve_parked().await?;
                continue;
            };
            match classify(&raw) {
                Incoming::Event { session, ev } => {
                    let at = self.tab_index(session.as_deref());
                    // The browser's own tab events are always handled: a
                    // tab is set up before anyone hears of it.
                    if ev.method == method && at.is_some() && at == self.current_index() && !ev.method.starts_with("Target.") {
                        // Handed straight to the caller, so it skips
                        // `route_event`: the record and the downloads must
                        // still hear of it.
                        let i = at.expect("checked above");
                        self.tabs[i].observe_documents(&ev);
                        self.tabs[i].net_record.observe(&ev);
                        let owner = self.download_owner(session.as_deref(), &ev);
                        self.follow_download(&ev, &owner);
                        return Ok(ev);
                    }
                    self.route_event(session, ev).await?;
                }
                Incoming::Reply { id, answer } => self.on_setup_reply(id, answer).await?,
                Incoming::Other => {}
            }
            self.resolve_parked().await?;
            if self.current_index().is_none() {
                return Err(self.gone(&self.current));
            }
        }
    }

    /// Drop the current tab's buffered events. Called before a navigation,
    /// so the load event waited for afterwards is that navigation's and not
    /// an older one. The network record is left alone: a step's requests
    /// survive a navigation or an upload made in that same step.
    pub fn forget_events(&mut self) {
        if let Some(i) = self.current_index() {
            self.tabs[i].events.clear();
        }
    }

    /// Dialogs the current tab accepted since the last call, as "alert: the
    /// message".
    pub fn take_dialogs(&mut self) -> Vec<String> {
        match self.current_index() {
            Some(i) => std::mem::take(&mut self.tabs[i].dialogs),
            None => Vec::new(),
        }
    }

    /// What the current tab has been doing, as far as the events read so
    /// far say (`page_log::PageLog::report`). Empty unless `page_log::watch`
    /// ran.
    pub fn page_log(&self) -> Vec<String> {
        self.current().map(|t| t.page_log.report()).unwrap_or_default()
    }

    /// Where the current tab's network record stands now
    /// (`NetRecord::mark`).
    ///
    /// On the whole browser the mark covers every tab, so a step that moves
    /// to another tab still sees only what that tab started after the mark.
    pub fn net_mark(&self) -> u64 {
        if !self.whole_browser {
            return self.current().map(|t| t.net_record.mark()).unwrap_or(0);
        }
        let each: HashMap<String, u64> = self.tabs.iter().map(|t| (t.session_id.clone(), t.net_record.mark())).collect();
        let mut marks = self.net_marks.lock().unwrap_or_else(|e| e.into_inner());
        marks.0 += 1;
        let id = marks.0;
        if marks.1.len() >= MAX_NET_MARKS {
            marks.1.pop_front();
        }
        marks.1.push_back((id, each));
        id
    }

    /// The requests the current tab started since `mark`, oldest first
    /// (`NetRecord::since`). Empty unless `page_log::watch` switched the
    /// Network domain on.
    pub fn net_since(&self, mark: u64) -> Vec<super::net_record::NetEntry> {
        let Some(t) = self.current() else {
            return Vec::new();
        };
        if !self.whole_browser {
            return t.net_record.since(mark);
        }
        // A tab opened after the mark: all it did came after it.
        let marks = self.net_marks.lock().unwrap_or_else(|e| e.into_inner());
        let from = marks.1.iter().find(|(id, _)| *id == mark).and_then(|(_, each)| each.get(&t.session_id).copied());
        t.net_record.since(from.unwrap_or(0))
    }

    /// Run an expression in the page and return the raw DevTools result.
    pub async fn eval(&mut self, expression: &str) -> Result<serde_json::Value, CdpError> {
        self.call(
            "Runtime.evaluate",
            serde_json::json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
        )
        .await
    }
}

/// One `downloadProgress` applied to its download: bytes so far, and on
/// `completed` the file named. A download already finished ignores it (a
/// second domain's echo of the same end).
fn progress_download(dir: &Path, entry: &mut DownloadEntry, state: &str, received: Option<f64>) {
    if entry.state != DownloadState::InProgress {
        return;
    }
    if let Some(n) = received {
        entry.bytes = n.max(0.0) as u64;
    }
    match state {
        "completed" => {
            entry.state = DownloadState::Completed;
            name_finished(dir, entry);
        }
        "canceled" => entry.state = DownloadState::Canceled,
        _ => {}
    }
}

/// A finished download, moved from its guid to its own name. A browser
/// that names files itself (the Page domain's fallback) left it under its
/// name already, so it is looked for there. A rename that fails is tried
/// again for a moment (`rename_patiently`); this runs while an event is
/// handled, so that moment holds up the connection, and only when a file
/// is held. A file that still cannot be moved stays under its guid, and
/// the log says why.
fn name_finished(dir: &Path, entry: &mut DownloadEntry) {
    let saved = dir.join(&entry.guid);
    if std::fs::symlink_metadata(&saved).is_ok_and(|m| m.is_file()) {
        // Check-then-act, and safe: the browser writes only guid names here
        // and this driver is the only one writing other names, one at a time.
        let to = dir.join(unique_name(dir, &entry.name));
        match rename_patiently(&saved, &to, |a, b| std::fs::rename(a, b), std::thread::sleep) {
            Ok(()) => entry.path = to,
            Err(e) => crate::applog::warn(format!("Auto Run could not name a finished download: {e}")),
        }
    } else if dir.join(&entry.name).is_file() {
        entry.path = dir.join(&entry.name);
    }
    if let Ok(meta) = std::fs::metadata(&entry.path) {
        entry.bytes = meta.len();
    }
}

/// What the layers above need from a browser connection. `Cdp` is the real
/// one; tests supply a scripted fake.
pub trait Driver {
    fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> impl Future<Output = Result<serde_json::Value, CdpError>>;
    /// `call` with its own limit in place of `CALL_TIMEOUT` and any
    /// deadline - for the one call known to take longer than any other (an
    /// API template step that uploads a file). A driver with no limits of
    /// its own (a test's fake) just makes the call.
    fn call_within(
        &mut self,
        method: &str,
        params: serde_json::Value,
        _limit: Duration,
    ) -> impl Future<Output = Result<serde_json::Value, CdpError>> {
        self.call(method, params)
    }
    fn wait_event(
        &mut self,
        method: &str,
        limit: Duration,
    ) -> impl Future<Output = Result<Event, CdpError>>;
    fn forget_events(&mut self);
    fn take_dialogs(&mut self) -> Vec<String>;
    /// Failed and unfinished requests and console errors, for a failure
    /// to explain itself. A driver that keeps none (a test's fake) has none.
    fn page_log(&self) -> Vec<String> {
        Vec::new()
    }
    /// Where the network record stands (`Cdp::net_mark`). A driver that
    /// keeps no record (a test's fake) is always at the start of an empty
    /// one.
    fn net_mark(&self) -> u64 {
        0
    }
    /// The requests started since `mark` (`Cdp::net_since`). None for a
    /// driver that keeps no record.
    fn net_since(&self, _mark: u64) -> Vec<super::net_record::NetEntry> {
        Vec::new()
    }
    /// See `Cdp::set_deadline`. Every wait loop sets one and clears it on
    /// every path out.
    fn set_deadline(&mut self, deadline: Option<Instant>);
    /// See `Cdp::guard_saves`. A driver with no guard of its own (a test's
    /// fake) is asked for `Fetch.enable` like any other call, so it can
    /// answer or refuse it. A refusal of the typed patterns is asked again
    /// with the catch-all, and logged only once that is a yes; unlike `Cdp`
    /// it remembers nothing, so the next guard asks for the typed patterns
    /// again (and logs again).
    fn guard_saves(&mut self, _patterns: &[String]) -> impl Future<Output = Result<(), CdpError>> {
        async move {
            self.call("Network.setBypassServiceWorker", serde_json::json!({ "bypass": true })).await?;
            // Typed patterns refused: every request is paused instead; that
            // refused too fails closed, and says nothing about types.
            match self.call("Fetch.enable", super::save_guard::fetch_enable_params()).await {
                Ok(_) => Ok(()),
                Err(CdpError::Protocol { message, .. }) => {
                    self.call("Fetch.enable", super::save_guard::fetch_enable_all_params()).await?;
                    crate::applog::warn(format!("{}{message}", super::save_guard::TYPED_REFUSED));
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
    }
    /// See `Cdp::stop_guarding_saves`.
    fn stop_guarding_saves(&mut self) -> impl Future<Output = Result<(), CdpError>> {
        async move { self.call("Fetch.disable", serde_json::json!({})).await.map(|_| ()) }
    }
    /// See `Cdp::is_guarding_saves`. A driver with no guard of its own
    /// never is.
    fn is_guarding_saves(&self) -> bool {
        false
    }
    /// See `Cdp::hold_saves`. Nothing to hold without a guard.
    fn hold_saves(&mut self, _hold: bool) {}
    /// See `Cdp::take_save_blocked`. A driver with no guard stopped nothing.
    fn take_save_blocked(&mut self) -> Option<String> {
        None
    }
    /// See `Cdp::take_saves_stopped`. A driver with no guard stopped nothing.
    fn take_saves_stopped(&mut self) -> Vec<(String, String)> {
        Vec::new()
    }
    /// The run's dialog book (`dialogs`). A driver that answers no dialogs
    /// of its own (a test's bare fake) has none: nothing is armed, and an
    /// `expect_dialog` sees no dialog.
    fn dialog_book(&mut self) -> Option<&mut super::dialogs::DialogBook> {
        None
    }
    /// The run's page errors (`page_errors`). A driver that reads no page
    /// events (a test's bare fake) has none.
    fn page_error_book(&mut self) -> Option<&mut super::page_errors::PageErrorBook> {
        None
    }
    /// See `Cdp::idle`: a wait loop's pause between two looks.
    fn idle(&mut self, wait: Duration) -> impl Future<Output = ()> {
        tokio::time::sleep(wait)
    }
    /// See `Cdp::enable_downloads`. A driver that follows no downloads (a
    /// test's fake) sends nothing and is ready at once.
    fn enable_downloads(&mut self, _dir: &Path) -> impl Future<Output = Result<(), CdpError>> {
        async { Ok(()) }
    }
    /// See `Cdp::downloads`. None for a driver that follows none.
    fn downloads(&self) -> Vec<DownloadEntry> {
        Vec::new()
    }
    /// See `Cdp::all_downloads`. A driver with one tab has only its own.
    fn all_downloads(&self) -> Vec<DownloadEntry> {
        self.downloads()
    }

    // Tabs. A driver with one tab of its own (a test's fake) is always in
    // `main`, and opens, finds and closes no other.

    /// See `Cdp::step_began`.
    fn step_began(&mut self) {}
    /// See `Cdp::tab_name`.
    fn tab_name(&self) -> String {
        MAIN_TAB.to_string()
    }
    /// See `Cdp::missing_tab`.
    fn missing_tab(&self) -> Option<String> {
        None
    }
    /// See `Cdp::expect_tab`.
    fn expect_tab(
        &mut self,
        _name: &str,
        _url_contains: Option<&str>,
        within: Duration,
    ) -> impl Future<Output = Result<String, CdpError>> {
        async move { Err(CdpError::Tab(no_new_tab(within))) }
    }
    /// See `Cdp::open_tab`.
    fn open_tab(&mut self, _name: &str) -> impl Future<Output = Result<(), CdpError>> {
        async { Err(CdpError::Tab("this browser cannot open another tab".to_string())) }
    }
    /// See `Cdp::switch_tab`.
    fn switch_tab(&mut self, name: &str) -> impl Future<Output = Result<(), CdpError>> {
        let answer = if name == MAIN_TAB { Ok(()) } else { Err(CdpError::Tab(no_tab(name))) };
        async move { answer }
    }
    /// See `Cdp::close_tab`.
    fn close_tab(&mut self, name: &str) -> impl Future<Output = Result<bool, CdpError>> {
        let answer = if name == MAIN_TAB { MAIN_CANNOT_CLOSE.to_string() } else { no_tab(name) };
        async move { Err(CdpError::Tab(answer)) }
    }
    /// See `Cdp::expect_tab_closed`.
    fn expect_tab_closed(&mut self, name: &str, _within: Duration) -> impl Future<Output = Result<(), CdpError>> {
        let answer = if name == MAIN_TAB { MAIN_CANNOT_CLOSE.to_string() } else { no_tab(name) };
        async move { Err(CdpError::Tab(answer)) }
    }
    /// See `Cdp::close_other_tabs`.
    fn close_other_tabs(&mut self) -> impl Future<Output = ()> {
        async {}
    }
    /// See `Cdp::send_deferred`. `None`: this driver cannot send a call
    /// and collect its answer later, so the caller makes the call as usual.
    fn send_deferred(
        &mut self,
        _method: &str,
        _params: serde_json::Value,
    ) -> impl Future<Output = Option<Result<u64, CdpError>>> {
        async { None }
    }
    /// See `Cdp::collect`. Only ever asked for an id `send_deferred` gave.
    fn collect(
        &mut self,
        _id: u64,
        _method: &str,
        _limit: Duration,
    ) -> impl Future<Output = Result<serde_json::Value, CdpError>> {
        async { Err(CdpError::Closed) }
    }
}

impl<T: Transport> Driver for Cdp<T> {
    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CdpError> {
        Cdp::call(self, method, params).await
    }
    async fn call_within(
        &mut self,
        method: &str,
        params: serde_json::Value,
        limit: Duration,
    ) -> Result<serde_json::Value, CdpError> {
        Cdp::call_within(self, method, params, limit).await
    }
    async fn wait_event(&mut self, method: &str, limit: Duration) -> Result<Event, CdpError> {
        Cdp::wait_event(self, method, limit).await
    }
    fn forget_events(&mut self) {
        Cdp::forget_events(self)
    }
    fn dialog_book(&mut self) -> Option<&mut super::dialogs::DialogBook> {
        Some(&mut self.book)
    }
    fn page_error_book(&mut self) -> Option<&mut super::page_errors::PageErrorBook> {
        Some(&mut self.page_errors)
    }
    fn take_dialogs(&mut self) -> Vec<String> {
        Cdp::take_dialogs(self)
    }
    fn page_log(&self) -> Vec<String> {
        Cdp::page_log(self)
    }
    fn net_mark(&self) -> u64 {
        Cdp::net_mark(self)
    }
    fn net_since(&self, mark: u64) -> Vec<super::net_record::NetEntry> {
        Cdp::net_since(self, mark)
    }
    fn set_deadline(&mut self, deadline: Option<Instant>) {
        Cdp::set_deadline(self, deadline)
    }
    async fn guard_saves(&mut self, patterns: &[String]) -> Result<(), CdpError> {
        Cdp::guard_saves(self, patterns).await
    }
    async fn stop_guarding_saves(&mut self) -> Result<(), CdpError> {
        Cdp::stop_guarding_saves(self).await
    }
    fn is_guarding_saves(&self) -> bool {
        Cdp::is_guarding_saves(self)
    }
    fn hold_saves(&mut self, hold: bool) {
        Cdp::hold_saves(self, hold)
    }
    fn take_save_blocked(&mut self) -> Option<String> {
        Cdp::take_save_blocked(self)
    }
    fn take_saves_stopped(&mut self) -> Vec<(String, String)> {
        Cdp::take_saves_stopped(self)
    }
    async fn idle(&mut self, wait: Duration) {
        Cdp::idle(self, wait).await
    }
    async fn enable_downloads(&mut self, dir: &Path) -> Result<(), CdpError> {
        Cdp::enable_downloads(self, dir).await
    }
    fn downloads(&self) -> Vec<DownloadEntry> {
        Cdp::downloads(self)
    }
    fn all_downloads(&self) -> Vec<DownloadEntry> {
        Cdp::all_downloads(self)
    }
    fn step_began(&mut self) {
        Cdp::step_began(self)
    }
    fn tab_name(&self) -> String {
        Cdp::tab_name(self)
    }
    fn missing_tab(&self) -> Option<String> {
        Cdp::missing_tab(self)
    }
    async fn expect_tab(&mut self, name: &str, url_contains: Option<&str>, within: Duration) -> Result<String, CdpError> {
        Cdp::expect_tab(self, name, url_contains, within).await
    }
    async fn open_tab(&mut self, name: &str) -> Result<(), CdpError> {
        Cdp::open_tab(self, name).await
    }
    async fn switch_tab(&mut self, name: &str) -> Result<(), CdpError> {
        Cdp::switch_tab(self, name).await
    }
    async fn close_tab(&mut self, name: &str) -> Result<bool, CdpError> {
        Cdp::close_tab(self, name).await
    }
    async fn expect_tab_closed(&mut self, name: &str, within: Duration) -> Result<(), CdpError> {
        Cdp::expect_tab_closed(self, name, within).await
    }
    async fn send_deferred(&mut self, method: &str, params: serde_json::Value) -> Option<Result<u64, CdpError>> {
        Some(Cdp::send_deferred(self, method, params).await)
    }
    async fn collect(&mut self, id: u64, method: &str, limit: Duration) -> Result<serde_json::Value, CdpError> {
        Cdp::collect(self, id, method, limit).await
    }
    async fn close_other_tabs(&mut self) {
        Cdp::close_other_tabs(self).await
    }
}
