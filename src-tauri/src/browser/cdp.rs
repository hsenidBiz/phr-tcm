//! A Chrome DevTools Protocol client for the browser Auto Run drives.
//!
//! One connection to the browser itself (`connect`), with a flattened
//! session per tab: every page-level command carries its tab's session,
//! and every event is routed to the tab whose session it carries
//! (`route_event`). Each tab keeps its own events, dialogs, page log,
//! network record and save guard (`Tab`). A tab the page opens (a popup, a
//! `target=_blank` link) is attached paused and given the same setup as the
//! first before it runs (`on_attached`). Calls go to the current tab, which
//! is always `main`, the tab the run started in.
//!
//! Three things the first version did not do, each of which showed up as a
//! frozen screen rather than an error:
//!
//! - every call has a deadline, so a browser that stops answering is
//!   reported instead of waited on forever;
//! - events are kept while a call waits for its reply, because the event a
//!   caller wants (a page load) usually arrives before it asks;
//! - a JavaScript dialog is accepted the moment it opens. Measured on real
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
use std::collections::{HashMap, VecDeque};
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
    /// Its service-worker bypass was refused, so it is never let run.
    unguardable: bool,
}

impl Tab {
    fn new(session_id: String, target_id: String, name: Option<String>, url: &str) -> Tab {
        Tab {
            session_id,
            target_id,
            name,
            url_without_query: address_of(url),
            events: VecDeque::new(),
            dialogs: vec![],
            page_log: Default::default(),
            net_record: Default::default(),
            guard: None,
            held: false,
            unguardable: false,
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
/// not be guarded. That tab is held where it is, before it can send
/// anything.
pub const TAB_UNGUARDED: &str =
    "this script must not save, but a tab the page opened could not be guarded - it was held before it could send anything";

/// Which of a new tab's setup frames is waited on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SetupStep {
    Bypass,
    Fetch,
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
    /// The tab `call` goes to. Always `main` for now.
    current: String,
    /// Driving the whole browser over its own socket (`connect`), so a tab
    /// the page opens is attached, paused until it is set up.
    whole_browser: bool,
    /// When the wait loop that owns this connection runs out of time. See
    /// `set_deadline`.
    deadline: Option<Instant>,
    /// The run's guard words while it guards saves: a tab the page opens is
    /// guarded with them before it runs.
    armed: Option<Vec<String>>,
    /// The run's `hold_saves`, for a tab opened during a sign-in.
    hold: bool,
    /// A stopped save from a tab that has since closed, not yet reported.
    blocked_elsewhere: Option<String>,
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
        let version = Self::ask(port, "version").await?;
        let ws = version["webSocketDebuggerUrl"]
            .as_str()
            .ok_or_else(|| "the browser did not say where its DevTools socket is".to_string())?
            .to_string();
        let tabs = Self::ask(port, "list").await?;
        let page = tabs
            .as_array()
            .and_then(|a| a.iter().find(|t| t["type"] == "page"))
            .ok_or_else(|| "the browser reported no page to drive".to_string())?;
        let target = page["id"].as_str().unwrap_or("").to_string();
        let url = page["url"].as_str().unwrap_or("").to_string();
        let (socket, _) = tokio_tungstenite::connect_async(&ws)
            .await
            .map_err(|e| format!("could not open the DevTools socket: {e}"))?;
        let mut cdp = Cdp::over(WsTransport { socket });
        cdp.drive_first_page(&target, &url).await.map_err(|e| e.to_string())?;
        Ok(cdp)
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
            whole_browser: false,
            deadline: None,
            armed: None,
            hold: false,
            blocked_elsewhere: None,
            unsent_answers: VecDeque::new(),
            downloads: None,
            downloads_per_page: false,
            seeds: vec![],
            setup: HashMap::new(),
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
    /// so nothing waits on the caller. `Err` leaves the connection
    /// unguarded for the tabs not yet asked, and the caller must not run a
    /// no-save script on it. Asked again, it takes the new words and keeps a
    /// stopped save not yet reported.
    ///
    /// Service workers are bypassed first: a page's worker could otherwise
    /// send a request the page's own interception never sees. A browser that
    /// refuses the bypass is not guarded at all (fail closed).
    ///
    /// Every tab is guarded: each one open now, and each one the page opens
    /// later, which is attached paused and guarded before its first request
    /// (`on_attached`). What is still NOT covered: out-of-process
    /// (cross-site) iframes are separate targets with their own requests,
    /// and a save sent from one would go through.
    pub async fn guard_saves(&mut self, patterns: &[String]) -> Result<(), CdpError> {
        let sessions: Vec<String> = self.tabs.iter().map(|t| t.session_id.clone()).collect();
        for s in sessions {
            match self.guard_tab(&s, patterns).await {
                Ok(()) => {}
                // A tab other than `main` that closed meanwhile has nothing
                // left to guard.
                Err(CdpError::Closed) if s != self.main && !self.has_tab(&s) => {}
                Err(e) => return Err(e),
            }
        }
        self.armed = Some(patterns.to_vec());
        self.hold = false;
        Ok(())
    }

    async fn guard_tab(&mut self, session: &str, patterns: &[String]) -> Result<(), CdpError> {
        let limit = self.limit_now();
        self.call_on(Some(session.to_string()), "Network.setBypassServiceWorker", serde_json::json!({ "bypass": true }), limit)
            .await?;
        let limit = self.limit_now();
        self.call_on(Some(session.to_string()), "Fetch.enable", super::save_guard::fetch_enable_params(), limit).await?;
        if let Some(t) = self.tabs.iter_mut().find(|t| t.session_id == session) {
            let blocked = t.guard.as_mut().and_then(|g| g.blocked.take());
            t.guard = Some(SaveGuard { patterns: patterns.to_vec(), hold: false, blocked });
        }
        Ok(())
    }

    /// Stop intercepting, in every tab. A tab's guard goes only once the
    /// browser has said it stopped: until then requests may still be
    /// paused, and they are still answered as a guarded tab answers them.
    pub async fn stop_guarding_saves(&mut self) -> Result<(), CdpError> {
        let sessions: Vec<String> = self.tabs.iter().map(|t| t.session_id.clone()).collect();
        for s in sessions {
            let limit = self.limit_now();
            match self.call_on(Some(s.clone()), "Fetch.disable", serde_json::json!({}), limit).await {
                Ok(_) => {
                    if let Some(t) = self.tabs.iter_mut().find(|t| t.session_id == s) {
                        t.guard = None;
                    }
                }
                Err(CdpError::Closed) if s != self.main && !self.has_tab(&s) => {}
                Err(e) => return Err(e),
            }
        }
        self.armed = None;
        Ok(())
    }

    pub fn is_guarding_saves(&self) -> bool {
        self.current().is_some_and(|t| t.guard.is_some())
    }

    /// Something must keep reading this connection between calls: a
    /// guarded tab's requests wait on an answer, and on the browser's own
    /// socket a tab the page opens waits to be set up.
    pub fn wants_reading(&self) -> bool {
        self.whole_browser || self.tabs.iter().any(|t| t.guard.is_some())
    }

    /// Let every request through for a while, saves included, without
    /// switching interception off: a sign-in is the runner's own, and what
    /// it sends is not the script's draft. Every tab, and a tab opened
    /// meanwhile.
    pub fn hold_saves(&mut self, hold: bool) {
        self.hold = hold;
        for t in &mut self.tabs {
            if let Some(g) = t.guard.as_mut() {
                g.hold = hold;
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
        let asked = self
            .call_on(
                None,
                "Browser.setDownloadBehavior",
                serde_json::json!({ "behavior": "allowAndName", "downloadPath": path, "eventsEnabled": true }),
                limit,
            )
            .await;
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

    /// Every download the current tab started so far, in start order.
    /// Empty unless `enable_downloads` ran.
    pub fn downloads(&self) -> Vec<DownloadEntry> {
        let Some(f) = self.downloads.as_ref() else {
            return Vec::new();
        };
        f.entries
            .iter()
            .filter(|e| f.owners.get(&e.guid).map_or(true, |o| *o == self.current))
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
        if self.wants_reading() || self.downloads.is_some() {
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
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            match tokio::time::timeout(left, self.next_frame()).await {
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
            Some(i) => match self.tabs[i].guard.as_mut() {
                Some(g) if !g.hold && super::save_guard::is_save(method, url, &g.patterns) => {
                    if g.blocked.is_none() {
                        g.blocked = Some(super::save_guard::blocked(method, url));
                    }
                    true
                }
                _ => false,
            },
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
        let (what, params) = if stop {
            crate::applog::warn(format!(
                "Auto Run stopped a save the page tried to send: {} {}",
                method.to_ascii_uppercase(),
                super::save_guard::path_of(url)
            ));
            ("Fetch.failRequest", serde_json::json!({ "requestId": request_id, "errorReason": "BlockedByClient" }))
        } else {
            ("Fetch.continueRequest", serde_json::json!({ "requestId": request_id }))
        };
        let on = at.map(|i| self.tabs[i].session_id.clone()).or(session).unwrap_or_default();
        self.queue(&on, what, params, None);
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
    /// tab that cannot be guarded never sends anything. Anything else (a
    /// worker) and a second session on a tab already driven are simply let
    /// run.
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
        if !page || known {
            if waiting {
                self.queue(&session, "Runtime.runIfWaitingForDebugger", serde_json::json!({}), None);
            }
            if page && !self.has_tab(&session) {
                self.queue("", "Target.detachFromTarget", serde_json::json!({ "sessionId": session }), None);
            }
            return self.send_unsent_answers().await;
        }
        let mut tab = Tab::new(session.clone(), target, None, info["url"].as_str().unwrap_or(""));
        crate::applog::info(format!("a tab opened: {}", tab.url_without_query));
        let armed = self.armed.clone();
        if let Some(words) = &armed {
            tab.guard = Some(SaveGuard { patterns: words.clone(), hold: self.hold, blocked: None });
            tab.held = waiting;
        }
        self.tabs.push(tab);
        let s = session.as_str();
        if armed.is_some() {
            self.queue(s, "Network.setBypassServiceWorker", serde_json::json!({ "bypass": true }), Some(SetupStep::Bypass));
            self.queue(s, "Fetch.enable", super::save_guard::fetch_enable_params(), Some(SetupStep::Fetch));
        }
        self.queue(s, "Network.enable", serde_json::json!({}), None);
        self.queue(s, "Runtime.enable", serde_json::json!({}), None);
        self.queue(s, "Page.enable", serde_json::json!({}), None);
        self.queue(s, "Page.setLifecycleEventsEnabled", serde_json::json!({ "enabled": true }), None);
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
    /// refused stays held, and its case fails (`TAB_UNGUARDED`).
    async fn on_setup_reply(&mut self, id: u64, answer: Result<serde_json::Value, String>) -> Result<(), CdpError> {
        let Some((session, step)) = self.setup.remove(&id) else {
            return Ok(());
        };
        let Some(i) = self.tabs.iter().position(|t| t.session_id == session) else {
            return Ok(());
        };
        let refused = answer.is_err();
        if let Err(message) = &answer {
            crate::applog::warn(format!("Auto Run could not guard a tab the page opened: {message}"));
        }
        let tab = &mut self.tabs[i];
        if step == SetupStep::Bypass {
            tab.unguardable |= refused;
            return Ok(());
        }
        if refused || tab.unguardable {
            if let Some(g) = tab.guard.as_mut() {
                g.blocked.get_or_insert_with(|| TAB_UNGUARDED.to_string());
            }
            return Ok(());
        }
        if tab.held {
            tab.held = false;
            self.queue(&session, "Runtime.runIfWaitingForDebugger", serde_json::json!({}), None);
            return self.send_unsent_answers().await;
        }
        Ok(())
    }

    /// A tab is gone: its state goes with it, except a stopped save not yet
    /// reported. Once `main` is gone every call is `Closed`, as when the
    /// browser itself closes.
    fn drop_tab(&mut self, session: &str) {
        let Some(i) = self.tabs.iter().position(|t| t.session_id == session) else {
            return;
        };
        let tab = self.tabs.remove(i);
        if let Some(b) = tab.guard.and_then(|g| g.blocked) {
            self.blocked_elsewhere.get_or_insert(b);
        }
        self.setup.retain(|_, (s, _)| s != session);
        if self.current == session && session != self.main {
            self.current = self.main.clone();
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
            return Err(CdpError::Closed);
        }
        let seeding = method == "Page.addScriptToEvaluateOnNewDocument";
        let unseeding = method == "Page.removeScriptToEvaluateOnNewDocument";
        let seed = if seeding || unseeding { Some(params.clone()) } else { None };
        let answer = self.call_on(Some(session), method, params, limit).await;
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
        self.send_unsent_answers().await?;
        let id = self.next_id;
        self.next_id += 1;
        self.transport
            .send(frame_in(id, method, params, session.as_deref().unwrap_or("")))
            .await
            .map_err(CdpError::Transport)?;
        match tokio::time::timeout(limit, self.read_reply(id, method, session)).await {
            Ok(answer) => answer,
            Err(_) => Err(CdpError::Timeout { what: method.to_string(), ms: limit.as_millis() as u64 }),
        }
    }

    async fn read_reply(
        &mut self,
        id: u64,
        method: &str,
        session: Option<String>,
    ) -> Result<serde_json::Value, CdpError> {
        loop {
            let raw = self.next_frame().await?;
            match classify(&raw) {
                Incoming::Reply { id: got, answer } if got == id => {
                    return answer.map_err(|message| CdpError::Protocol { method: method.to_string(), message });
                }
                // A new tab's setup, or a reply to a call that already
                // timed out, or to a fire-and-forget answer.
                Incoming::Reply { id: got, answer } => self.on_setup_reply(got, answer).await?,
                Incoming::Event { session, ev } => self.route_event(session, ev).await?,
                Incoming::Other => {}
            }
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
            let Some(i) = at else {
                return Ok(());
            };
            let kind = ev.params["type"].as_str().unwrap_or("dialog");
            let message = ev.params["message"].as_str().unwrap_or("");
            let tab = &mut self.tabs[i];
            if tab.dialogs.len() >= MAX_REMEMBERED_DIALOGS {
                tab.dialogs.remove(0);
            }
            tab.dialogs.push(format!("{kind}: {message}"));
            let on = tab.session_id.clone();
            // Sent without waiting: its reply carries an id nobody is
            // waiting on and falls through `read_reply` harmlessly.
            let id = self.next_id;
            self.next_id += 1;
            self.transport
                .send(frame_in(id, "Page.handleJavaScriptDialog", serde_json::json!({ "accept": true }), &on))
                .await
                .map_err(CdpError::Transport)?;
            return Ok(());
        }
        // Read the moment it arrives, like a paused request: a download
        // that starts and ends during one click is still followed.
        let owner = match (session.is_some(), at) {
            (true, Some(i)) => self.tabs[i].session_id.clone(),
            _ => self.main.clone(),
        };
        if self.follow_download(&ev, &owner) {
            return Ok(());
        }
        let Some(i) = at else {
            return Ok(());
        };
        let tab = &mut self.tabs[i];
        if ev.method == "Page.frameNavigated" && ev.params["frame"].get("parentId").is_none() {
            if let Some(url) = ev.params["frame"]["url"].as_str() {
                tab.url_without_query = address_of(url);
            }
        }
        // The record sees every event first and never claims one, so the
        // page log below is fed exactly as before.
        tab.net_record.observe(&ev);
        // Network and console events go to the page log, never into the
        // buffer: a page volunteers thousands, and they would push out the
        // load event a navigation is about to wait for.
        if tab.page_log.observe(&ev) {
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
            return Err(CdpError::Closed);
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
            let raw = self.next_frame().await?;
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
                        self.tabs[i].net_record.observe(&ev);
                        let owner = if session.is_some() { self.tabs[i].session_id.clone() } else { self.main.clone() };
                        self.follow_download(&ev, &owner);
                        return Ok(ev);
                    }
                    self.route_event(session, ev).await?;
                }
                Incoming::Reply { id, answer } => self.on_setup_reply(id, answer).await?,
                Incoming::Other => {}
            }
            if self.current_index().is_none() {
                return Err(CdpError::Closed);
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
    pub fn net_mark(&self) -> u64 {
        self.current().map(|t| t.net_record.mark()).unwrap_or(0)
    }

    /// The requests the current tab started since `mark`, oldest first
    /// (`NetRecord::since`). Empty unless `page_log::watch` switched the
    /// Network domain on.
    pub fn net_since(&self, mark: u64) -> Vec<super::net_record::NetEntry> {
        self.current().map(|t| t.net_record.since(mark)).unwrap_or_default()
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
    /// answer or refuse it.
    fn guard_saves(&mut self, _patterns: &[String]) -> impl Future<Output = Result<(), CdpError>> {
        async move {
            self.call("Network.setBypassServiceWorker", serde_json::json!({ "bypass": true })).await?;
            self.call("Fetch.enable", super::save_guard::fetch_enable_params()).await.map(|_| ())
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
    async fn idle(&mut self, wait: Duration) {
        Cdp::idle(self, wait).await
    }
    async fn enable_downloads(&mut self, dir: &Path) -> Result<(), CdpError> {
        Cdp::enable_downloads(self, dir).await
    }
    fn downloads(&self) -> Vec<DownloadEntry> {
        Cdp::downloads(self)
    }
}
