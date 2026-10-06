//! A Chrome DevTools Protocol client for the one page Auto Run drives.
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

pub struct Cdp<T: Transport = WsTransport> {
    transport: T,
    next_id: u64,
    events: VecDeque<Event>,
    dialogs: Vec<String>,
    /// What the page did, for a failure to explain itself (`page_log`).
    page_log: super::page_log::PageLog,
    /// Every request the page made, for a step that checks one
    /// (`net_record`). Fed before the page log, which claims these events.
    net_record: super::net_record::NetRecord,
    /// When the wait loop that owns this connection runs out of time. See
    /// `set_deadline`.
    deadline: Option<Instant>,
    /// A no-save script's guard (`guard_saves`): `None` while requests are
    /// not intercepted.
    guard: Option<SaveGuard>,
    /// Answers to paused requests not yet known to be sent, oldest first.
    /// A deadline can cut a call short while one is being written; whatever
    /// is still here is written again before the next frame goes out, so a
    /// request is never left paused. A request answered twice gets a refusal
    /// for the second answer, which nobody reads.
    unsent_answers: VecDeque<String>,
    /// Where downloads go and what came of each (`enable_downloads`):
    /// `None` while the browser's downloads are not followed.
    downloads: Option<DownloadFolder>,
    /// A hold asked for (`hold_saves(true)`) that has not taken effect yet.
    hold_pending: bool,
    /// The first command sent since the hold was asked for. The hold takes
    /// effect only once its answer (or a later one) is read: the browser
    /// writes every event it sent before that command arrived ahead of the
    /// answer, so a request it paused before then is still judged by the
    /// guard, however late it is read.
    hold_marker: Option<u64>,
    /// What the page has loaded and which document sent which request, for
    /// a hold to tell the page being left from the sign-in's own page
    /// (`late_save`).
    documents: Documents,
    /// Saves paused during a hold whose document is not known yet: answered
    /// once it is (`resolve_parked`).
    parked: Vec<Parked>,
}

/// Which documents the page has loaded, and which document sent each
/// request.
#[derive(Default)]
struct Documents {
    /// Every document loaded (`Page.frameNavigated`), by loader id, oldest
    /// first; bounded.
    loaders: VecDeque<String>,
    /// The loader of the top-level document now.
    main_loader: Option<String>,
    /// While a hold is on: the documents that were left before it was
    /// asked for. A save one of them sends is still stopped.
    held_out: HashSet<String>,
    /// Which document sent each request (`Network.requestWillBeSent`), by
    /// the network's request id; bounded, oldest first.
    request_loaders: HashMap<String, String>,
    request_order: VecDeque<String>,
}

impl Documents {
    /// Note a document loaded, or a request sent by one.
    fn observe(&mut self, ev: &Event) {
        match ev.method.as_str() {
            "Page.frameNavigated" => {
                let frame = &ev.params["frame"];
                let Some(loader) = frame["loaderId"].as_str().filter(|l| !l.is_empty()) else {
                    return;
                };
                if !self.loaders.iter().any(|l| l == loader) {
                    if self.loaders.len() >= MAX_LOADERS {
                        self.loaders.pop_front();
                    }
                    self.loaders.push_back(loader.to_string());
                }
                if frame.get("parentId").is_none() {
                    self.main_loader = Some(loader.to_string());
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

    /// The documents a hold asked for now must still not save from: every
    /// one seen except the top-level document on screen, which is the page
    /// the sign-in arrived on.
    fn hold_out_left_documents(&mut self) {
        self.held_out = self.loaders.iter().filter(|l| Some(*l) != self.main_loader.as_ref()).cloned().collect();
    }

    /// During a hold, is this save still stopped? A ping (a beacon) always
    /// is: only a page being left sends one at that moment. Otherwise it is
    /// stopped when it came from a document left before the hold. `None`:
    /// its document is not known yet.
    fn late_save(&self, params: &serde_json::Value) -> Option<bool> {
        if params["resourceType"].as_str() == Some("Ping") {
            return Some(true);
        }
        // A navigation starts a new document: never the page being left,
        // which is gone (a form the sign-in page posts is one).
        if params["resourceType"].as_str() == Some("Document") {
            return Some(false);
        }
        // Without the Network domain there is no telling which document
        // sent it: stopped (fail closed).
        let Some(network_id) = params["networkId"].as_str() else {
            return Some(true);
        };
        self.request_loaders.get(network_id).map(|l| self.held_out.contains(l))
    }
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

/// How many documents and requests are remembered by loader.
const MAX_LOADERS: usize = 64;
const MAX_REQUEST_LOADERS: usize = 512;

/// A browser's downloads: the folder they land in, and every download in
/// start order.
struct DownloadFolder {
    dir: PathBuf,
    entries: Vec<DownloadEntry>,
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

/// What a guarded connection does with each paused request.
struct SaveGuard {
    /// The project's own save words, beside the built-in ones.
    patterns: Vec<String>,
    /// While true (a sign-in), every request goes on, saves included.
    hold: bool,
    /// The first save stopped and not yet reported, as the case's sentence.
    blocked: Option<String>,
}

impl Cdp<WsTransport> {
    /// Ask the browser which socket its page is on, open it, and switch on
    /// page events (dialogs) and lifecycle events. A lifecycle event
    /// carries the `frameId` and `loaderId` a plain `Page.loadEventFired`
    /// does not, which is what lets a navigation tell its OWN load apart
    /// from one still in flight from an earlier navigation or from a
    /// sub-frame. Nothing else needs enabling: the accessibility and DOM
    /// calls this app uses work without it.
    pub async fn connect(port: u16) -> Result<Cdp<WsTransport>, String> {
        let url = format!("http://127.0.0.1:{port}/json/list");
        let body = reqwest::get(&url)
            .await
            .map_err(|e| format!("the browser did not answer on port {port}: {e}"))?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let tabs: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let ws = tabs
            .as_array()
            .and_then(|a| a.iter().find(|t| t["type"] == "page"))
            .and_then(|t| t["webSocketDebuggerUrl"].as_str())
            .ok_or_else(|| "the browser reported no page to drive".to_string())?
            .to_string();
        let (socket, _) = tokio_tungstenite::connect_async(&ws)
            .await
            .map_err(|e| format!("could not open the DevTools socket: {e}"))?;
        let mut cdp = Cdp::over(WsTransport { socket });
        cdp.call("Page.enable", serde_json::json!({})).await.map_err(|e| e.to_string())?;
        cdp.call("Page.setLifecycleEventsEnabled", serde_json::json!({ "enabled": true }))
            .await
            .map_err(|e| e.to_string())?;
        Ok(cdp)
    }
}

impl<T: Transport> Cdp<T> {
    pub fn over(transport: T) -> Self {
        Cdp {
            transport,
            next_id: 1,
            events: VecDeque::new(),
            dialogs: vec![],
            page_log: Default::default(),
            net_record: Default::default(),
            deadline: None,
            guard: None,
            unsent_answers: VecDeque::new(),
            downloads: None,
            hold_pending: false,
            hold_marker: None,
            documents: Documents::default(),
            parked: Vec::new(),
        }
    }

    /// For tests that need to see what was sent.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// For tests that feed frames in after the client was made.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Intercept every request the page makes, and fail the saves among
    /// them (`save_guard::is_save`) before they leave the browser. Every
    /// paused request is answered in `on_event`, the moment it is read, so
    /// nothing waits on the caller. `Err` leaves the connection as it was,
    /// and the caller must not run a no-save script on it. Asked again, it
    /// takes the new words and keeps a stopped save not yet reported.
    ///
    /// Service workers are bypassed first: a page's worker could otherwise
    /// send a request the page's own interception never sees. A browser that
    /// refuses the bypass is not guarded at all (fail closed).
    ///
    /// What is still NOT covered: out-of-process (cross-site) iframes and
    /// popups are separate targets with their own requests, and this
    /// connection intercepts the page's own target only. A save sent from a
    /// cross-site frame or a new window would go through.
    pub async fn guard_saves(&mut self, patterns: &[String]) -> Result<(), CdpError> {
        self.call("Network.setBypassServiceWorker", serde_json::json!({ "bypass": true })).await?;
        self.call("Fetch.enable", super::save_guard::fetch_enable_params()).await?;
        let blocked = self.guard.as_mut().and_then(|g| g.blocked.take());
        self.guard = Some(SaveGuard { patterns: patterns.to_vec(), hold: false, blocked });
        self.hold_pending = false;
        self.hold_marker = None;
        Ok(())
    }

    /// Stop intercepting. The guard goes only once the browser has said it
    /// stopped: until then requests may still be paused, and they are still
    /// answered as a guarded connection answers them.
    pub async fn stop_guarding_saves(&mut self) -> Result<(), CdpError> {
        self.call("Fetch.disable", serde_json::json!({})).await?;
        self.guard = None;
        Ok(())
    }

    pub fn is_guarding_saves(&self) -> bool {
        self.guard.is_some()
    }

    /// Let every request through for a while, saves included, without
    /// switching interception off: a sign-in is the runner's own, and what
    /// it sends is not the script's draft.
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
    /// late): a ping, or a save from a document left before the hold was
    /// asked for, is still stopped (`Documents::late_save`). Only the page
    /// the sign-in arrived on, and what it loads after, may save.
    pub fn hold_saves(&mut self, hold: bool) {
        if hold {
            let held = self.guard.as_ref().is_some_and(|g| g.hold);
            if self.guard.is_some() && !held && !self.hold_pending {
                self.hold_pending = true;
                self.hold_marker = None;
                self.documents.hold_out_left_documents();
            }
            return;
        }
        self.hold_pending = false;
        self.hold_marker = None;
        self.documents.held_out.clear();
        if let Some(g) = self.guard.as_mut() {
            g.hold = false;
        }
    }

    /// A command's answer was read: a hold waiting on it takes effect.
    fn note_reply(&mut self, id: u64) {
        if self.hold_marker.is_some_and(|m| id >= m) {
            self.hold_pending = false;
            self.hold_marker = None;
            if let Some(g) = self.guard.as_mut() {
                g.hold = true;
            }
        }
    }

    /// A frame that answers a command: a hold waiting on it may take effect.
    fn note_any_reply(&mut self, raw: &str) {
        if self.hold_marker.is_none() || !raw.contains("\"id\"") {
            return;
        }
        if let Some(id) = serde_json::from_str::<serde_json::Value>(raw).ok().and_then(|v| v.get("id")?.as_u64()) {
            self.note_reply(id);
        }
    }

    /// The first save stopped since the last time this was asked, as the
    /// case's sentence (`save_guard::blocked`). Reported once.
    pub fn take_save_blocked(&mut self) -> Option<String> {
        self.guard.as_mut().and_then(|g| g.blocked.take())
    }

    /// Save every download the page starts into `dir` (made if missing),
    /// and follow each one: the browser saves it under its guid, and once it
    /// completes it is renamed to its own name (`downloads::sanitise_name`,
    /// numbered by `downloads::unique_name` when that is taken).
    ///
    /// Asks through the Browser domain, which names files by guid. A target
    /// that refuses that is asked through the Page domain instead, which
    /// saves under the browser's own name: then the file is found under its
    /// name once it completes, and a repeat name is numbered by the browser.
    /// Asked again with another folder, later downloads go there and the
    /// ones so far are kept.
    pub async fn enable_downloads(&mut self, dir: &Path) -> Result<(), CdpError> {
        std::fs::create_dir_all(dir).map_err(|e| CdpError::Protocol {
            method: "Browser.setDownloadBehavior".to_string(),
            message: format!("the download folder could not be made: {e}"),
        })?;
        let path = dir.to_string_lossy().into_owned();
        let asked = self
            .call(
                "Browser.setDownloadBehavior",
                serde_json::json!({ "behavior": "allowAndName", "downloadPath": path, "eventsEnabled": true }),
            )
            .await;
        match asked {
            Ok(_) => {}
            Err(CdpError::Protocol { .. }) => {
                self.call("Page.setDownloadBehavior", serde_json::json!({ "behavior": "allow", "downloadPath": path }))
                    .await?;
            }
            Err(e) => return Err(e),
        }
        let (entries, early_ends) =
            self.downloads.take().map(|f| (f.entries, f.early_ends)).unwrap_or_default();
        self.downloads = Some(DownloadFolder { dir: dir.to_path_buf(), entries, early_ends });
        Ok(())
    }

    /// Every download followed so far, in start order. Empty unless
    /// `enable_downloads` ran.
    pub fn downloads(&self) -> Vec<DownloadEntry> {
        self.downloads.as_ref().map(|f| f.entries.clone()).unwrap_or_default()
    }

    /// Note a download event (`Browser.*`, or the Page domain's twin). Says
    /// whether it was one, so `on_event` keeps it out of the buffer.
    fn follow_download(&mut self, ev: &Event) -> bool {
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

    /// Wait this long. A connection that is guarded or following downloads
    /// keeps reading while it waits, so a request the page makes between
    /// two calls is answered at once rather than left paused until the next
    /// call, and a download's end is seen while a step waits for it; any
    /// other just sleeps, as every wait loop always did.
    pub async fn idle(&mut self, wait: Duration) {
        if self.guard.is_some() || self.downloads.is_some() {
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
            match tokio::time::timeout(left, self.next_frame()).await {
                Err(_) => return,
                // The socket is gone: nothing more will come, so the rest
                // of the wait is a plain one.
                Ok(Err(_)) => {
                    tokio::time::sleep(left).await;
                    return;
                }
                Ok(Ok(raw)) => {
                    self.note_any_reply(&raw);
                    if let Some(ev) = event_of(&raw) {
                        if self.on_event(ev).await.is_err() {
                            tokio::time::sleep(until.saturating_duration_since(Instant::now())).await;
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Continue or fail one paused request, at once. Sent without waiting,
    /// like the dialog answer: its reply carries an id nobody waits on and
    /// falls through `read_reply` harmlessly. A connection that is not
    /// guarded (any more) continues it - a request is never left paused.
    async fn answer_paused(&mut self, params: &serde_json::Value) -> Result<(), CdpError> {
        let request_id = params["requestId"].as_str().unwrap_or("").to_string();
        let method = params["request"]["method"].as_str().unwrap_or("GET");
        let url = params["request"]["url"].as_str().unwrap_or("");
        let verdict = match self.guard.as_ref() {
            Some(g) if super::save_guard::is_save(method, url, &g.patterns) => {
                if g.hold { self.documents.late_save(params) } else { Some(true) }
            }
            _ => Some(false),
        };
        let Some(stop) = verdict else {
            // Which document sent it is not known yet: it waits, paused,
            // until it is (`resolve_parked`).
            self.parked.push(Parked {
                request_id,
                network_id: params["networkId"].as_str().unwrap_or("").to_string(),
                method: method.to_string(),
                url: url.to_string(),
                since: Instant::now(),
            });
            return Ok(());
        };
        if stop {
            if let Some(g) = self.guard.as_mut() {
                g.blocked.get_or_insert_with(|| super::save_guard::blocked(method, url));
            }
        }
        self.queue_answer(&request_id, method, url, stop);
        self.send_unsent_answers().await
    }

    /// Queue the answer to one paused request: stopped or continued.
    fn queue_answer(&mut self, request_id: &str, method: &str, url: &str, stop: bool) {
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
        let id = self.next_id;
        self.next_id += 1;
        // Kept until it is known to be written: see `unsent_answers`.
        self.unsent_answers.push_back(frame(id, what, params));
    }

    /// Answer every parked save whose fate is known now: its document is
    /// known, the hold or the guard ended, or it waited `PARK_LIMIT`
    /// (stopped, fail closed).
    async fn resolve_parked(&mut self) -> Result<(), CdpError> {
        if self.parked.is_empty() {
            return Ok(());
        }
        let mut answers = Vec::new();
        for p in std::mem::take(&mut self.parked) {
            let verdict = match self.guard.as_ref() {
                None => Some(false),
                Some(g) if !g.hold => Some(true),
                Some(_) => match self.documents.request_loaders.get(&p.network_id) {
                    Some(loader) => Some(self.documents.held_out.contains(loader)),
                    None if p.since.elapsed() >= PARK_LIMIT => Some(true),
                    None => None,
                },
            };
            match verdict {
                None => self.parked.push(p),
                Some(stop) => {
                    if stop {
                        if let Some(g) = self.guard.as_mut() {
                            g.blocked.get_or_insert_with(|| super::save_guard::blocked(&p.method, &p.url));
                        }
                    }
                    answers.push((p, stop));
                }
            }
        }
        for (p, stop) in answers {
            self.queue_answer(&p.request_id, &p.method, &p.url, stop);
        }
        self.send_unsent_answers().await
    }

    /// Write every answer still waiting, oldest first. Each leaves the list
    /// only once its write finished, so one cut short is written again.
    async fn send_unsent_answers(&mut self) -> Result<(), CdpError> {
        while let Some(next) = self.unsent_answers.front().cloned() {
            self.transport.send(next).await.map_err(CdpError::Transport)?;
            self.unsent_answers.pop_front();
        }
        Ok(())
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

    pub async fn call_within(
        &mut self,
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
            .send(frame(id, method, params))
            .await
            .map_err(CdpError::Transport)?;
        match tokio::time::timeout(limit, self.read_reply(id, method)).await {
            Ok(answer) => answer,
            Err(_) => Err(CdpError::Timeout { what: method.to_string(), ms: limit.as_millis() as u64 }),
        }
    }

    async fn read_reply(&mut self, id: u64, method: &str) -> Result<serde_json::Value, CdpError> {
        loop {
            let raw = self.next_frame().await?;
            self.note_any_reply(&raw);
            if let Some(answer) = reply_for(id, &raw) {
                return answer.map_err(|message| CdpError::Protocol {
                    method: method.to_string(),
                    message,
                });
            }
            if let Some(ev) = event_of(&raw) {
                self.on_event(ev).await?;
            }
            // Anything else is a reply to a call that already timed out,
            // or to the fire-and-forget dialog handling below.
        }
    }

    async fn next_frame(&mut self) -> Result<String, CdpError> {
        match self.transport.recv().await {
            None => Err(CdpError::Closed),
            Some(Err(e)) => Err(CdpError::Transport(e)),
            Some(Ok(raw)) => Ok(raw),
        }
    }

    async fn on_event(&mut self, ev: Event) -> Result<(), CdpError> {
        // A paused request holds the page up until it is answered, so it is
        // answered here, the moment it is read - inside whichever call or
        // idle wait read it.
        if ev.method == "Fetch.requestPaused" {
            return self.answer_paused(&ev.params).await;
        }
        if ev.method == "Page.javascriptDialogOpening" {
            let kind = ev.params["type"].as_str().unwrap_or("dialog");
            let message = ev.params["message"].as_str().unwrap_or("");
            if self.dialogs.len() >= MAX_REMEMBERED_DIALOGS {
                self.dialogs.remove(0);
            }
            self.dialogs.push(format!("{kind}: {message}"));
            // Sent without waiting: its reply carries an id nobody is
            // waiting on and falls through `read_reply` harmlessly.
            let id = self.next_id;
            self.next_id += 1;
            self.transport
                .send(frame(id, "Page.handleJavaScriptDialog", serde_json::json!({ "accept": true })))
                .await
                .map_err(CdpError::Transport)?;
            return Ok(());
        }
        // Read the moment it arrives, like a paused request: a download
        // that starts and ends during one click is still followed.
        if self.follow_download(&ev) {
            return Ok(());
        }
        self.documents.observe(&ev);
        let parked = ev.method == "Network.requestWillBeSent" && !self.parked.is_empty();
        // The record sees every event first and never claims one, so the
        // page log below is fed exactly as before.
        self.net_record.observe(&ev);
        // Network and console events go to the page log, never into the
        // buffer: a page volunteers thousands, and they would push out the
        // load event a navigation is about to wait for.
        if self.page_log.observe(&ev) {
            if parked {
                self.resolve_parked().await?;
            }
            return Ok(());
        }
        if self.events.len() >= MAX_BUFFERED_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(ev);
        Ok(())
    }

    /// The next event with this method: one already buffered, or the next
    /// to arrive within `limit`.
    pub async fn wait_event(&mut self, method: &str, limit: Duration) -> Result<Event, CdpError> {
        if let Some(i) = self.events.iter().position(|e| e.method == method) {
            return Ok(self.events.remove(i).expect("position was just found"));
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
            self.note_any_reply(&raw);
            if let Some(ev) = event_of(&raw) {
                if ev.method == method {
                    // Handed straight to the caller, so it skips `on_event`:
                    // the record, the documents and the downloads must
                    // still hear of it.
                    self.documents.observe(&ev);
                    self.net_record.observe(&ev);
                    self.follow_download(&ev);
                    return Ok(ev);
                }
                self.on_event(ev).await?;
            }
        }
    }

    /// Drop buffered events. Called before a navigation, so the load event
    /// waited for afterwards is that navigation's and not an older one.
    /// The network record is left alone: a step's requests survive a
    /// navigation or an upload made in that same step.
    pub fn forget_events(&mut self) {
        self.events.clear();
    }

    /// Dialogs accepted since the last call, as "alert: the message".
    pub fn take_dialogs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.dialogs)
    }

    /// What the page has been doing, as far as the events read so far say
    /// (`page_log::PageLog::report`). Empty unless `page_log::watch` ran.
    pub fn page_log(&self) -> Vec<String> {
        self.page_log.report()
    }

    /// Where the network record stands now (`NetRecord::mark`).
    pub fn net_mark(&self) -> u64 {
        self.net_record.mark()
    }

    /// The requests started since `mark`, oldest first (`NetRecord::since`).
    /// Empty unless `page_log::watch` switched the Network domain on.
    pub fn net_since(&self, mark: u64) -> Vec<super::net_record::NetEntry> {
        self.net_record.since(mark)
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
