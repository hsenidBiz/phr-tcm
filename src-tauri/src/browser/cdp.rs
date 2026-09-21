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
//! The socket sits behind `Transport` and the client behind `Driver`, so
//! both layers are tested without starting a browser.

use futures::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::future::Future;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

/// How long any single protocol call may take.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

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
}

impl Cdp<WsTransport> {
    /// Ask the browser which socket its page is on, open it, and switch on
    /// page events (loads and dialogs). Nothing else needs enabling: the
    /// accessibility and DOM calls this app uses work without it.
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
        Ok(cdp)
    }
}

impl<T: Transport> Cdp<T> {
    pub fn over(transport: T) -> Self {
        Cdp { transport, next_id: 1, events: VecDeque::new(), dialogs: vec![] }
    }

    /// For tests that need to see what was sent.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CdpError> {
        self.call_within(method, params, CALL_TIMEOUT).await
    }

    pub async fn call_within(
        &mut self,
        method: &str,
        params: serde_json::Value,
        limit: Duration,
    ) -> Result<serde_json::Value, CdpError> {
        let id = self.next_id;
        self.next_id += 1;
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
        let what = method.to_string();
        match tokio::time::timeout(limit, self.read_event(method)).await {
            Ok(answer) => answer,
            Err(_) => Err(CdpError::Timeout { what, ms: limit.as_millis() as u64 }),
        }
    }

    async fn read_event(&mut self, method: &str) -> Result<Event, CdpError> {
        loop {
            let raw = self.next_frame().await?;
            if let Some(ev) = event_of(&raw) {
                if ev.method == method {
                    return Ok(ev);
                }
                self.on_event(ev).await?;
            }
        }
    }

    /// Drop buffered events. Called before a navigation, so the load event
    /// waited for afterwards is that navigation's and not an older one.
    pub fn forget_events(&mut self) {
        self.events.clear();
    }

    /// Dialogs accepted since the last call, as "alert: the message".
    pub fn take_dialogs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.dialogs)
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

/// What the layers above need from a browser connection. `Cdp` is the real
/// one; tests supply a scripted fake.
pub trait Driver {
    fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> impl Future<Output = Result<serde_json::Value, CdpError>>;
    fn wait_event(
        &mut self,
        method: &str,
        limit: Duration,
    ) -> impl Future<Output = Result<Event, CdpError>>;
    fn forget_events(&mut self);
    fn take_dialogs(&mut self) -> Vec<String>;
}

impl<T: Transport> Driver for Cdp<T> {
    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CdpError> {
        Cdp::call(self, method, params).await
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
}
