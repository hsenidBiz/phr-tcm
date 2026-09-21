# Auto Run: Browser Driver Foundations, Locators and Assertions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Auto Run able to execute a test case's steps reliably: a DevTools client that cannot hang, real mouse and keyboard input gated on "can a person actually click this now", locators that address controls by role and name, text or CSS (scoped, visible-only, indexed), assertions that retry until a timeout, and a screenshot whenever an action fails.

**Architecture:** `browser/cdp.rs` becomes a small client over a `Transport` trait with typed errors, a per-call timeout, an event buffer and automatic handling of JavaScript dialogs; everything above it talks to a `Driver` trait so it can be tested with a scripted fake. `browser/page.rs` wraps the handful of protocol calls the rest needs (object handles, functions called on an element with arguments, never string-built JavaScript). `browser/locator.rs` resolves a `Target` to element handles, using Chrome's own accessibility tree for role and name. `browser/input.rs` waits until an element is actionable and then sends real input. `browser/expect.rs` is one retry loop for six checks. `browser/actions.rs` keeps the `Action` enum and dispatches. Saved scripts stay valid: the `selector` field still accepts a plain string.

**Tech Stack:** Rust (tokio, tokio-tungstenite, serde_json, base64, specta), Chrome DevTools Protocol over the page's WebSocket, React 19 + TypeScript for one small screenshot viewer, vitest, Rust integration tests.

**Background:** `docs/research/2026-09-21-phr-playwright-automation-analysis.md` sections 9 to 11. This plan is phases 1 and 2 of section 11. It borrows ideas from that repo (role and name locators, visible-only matching, retrying assertions) and is not a port of it.

**Verified on this machine before planning (headless Edge, real protocol):**
- `Accessibility.queryAXTree { objectId, role }` returns each node's computed `name.value` and `backendDOMNodeId`, and does not return `display:none` elements. Its `accessibleName` filter is exact-match only, and names can carry stray whitespace (`"Method Name * "`), so name matching is done in Rust on a role-only query.
- The same call scoped with another element's id returns only that subtree.
- `DOM.resolveNode { backendNodeId, objectGroup }` gives an object handle that `Runtime.callFunctionOn` accepts.
- `Input.dispatchMouseEvent` (moved, pressed, released) clicks; `Input.insertText` types and fires one genuine `input` event; select-all plus a Backspace key event clears a field.
- A page `alert()` leaves every later call pending until `Page.handleJavaScriptDialog` is sent; `Page.javascriptDialogOpening` arrives as an event.
- Only `Page.enable` is required. The accessibility and DOM calls above work without enabling their domains.
- `Page.navigate` reports a load failure in `errorText`; `Page.loadEventFired` follows a successful one.

## Global Constraints

- Every Rust test is an integration test under `src-tauri/tests/`; never a `#[cfg(test)]` module inside `src/`. Shared test helpers live in `src-tauri/tests/common/mod.rs` and are pulled in with `mod common;`.
- Run one build or test command at a time on this shared machine and wait for it. Rust commands run from `src-tauri/` with `$env:CARGO_TARGET_DIR="target/gate"`.
- `src/bindings.ts` is generated: after any change to `Action`, `ActionOutcome`, `Target`, `LocatorStep` or a command signature, run `cargo test --test bindings` and commit the result. Never hand-edit it. A whitespace-only diff on it after a build is reverted with `git checkout -- src/bindings.ts`.
- No new crates. `tokio`, `tokio-tungstenite`, `futures`, `serde_json`, `base64` and `specta` are already dependencies.
- Auto Run never calls Azure DevOps. Nothing in this plan changes that.
- The app never launches the browser headless: `tests/browser_launch.rs` pins `launch_args`, and it must keep passing unchanged. Only the ignored live tests in Task 8 pass `--headless=new`, through a separate function.
- Auto Run stays a development-build tab (`AUTO_RUN_ENABLED`). The assistant-facing tools stay hidden (`HIDDEN_TOOLS`). Neither is touched here.
- Values from a script (selectors, text, fill values) reach the page only as `Runtime.callFunctionOn` `arguments` or as protocol parameters. They are never concatenated into JavaScript source.
- Every saved script that parses today must still parse and run: `selector` keeps accepting a plain string with today's meaning (CSS, or `text=` with last match winning).
- Timeouts, in one place (`browser/timing.rs`): action 15000 ms, expectation 10000 ms, navigation 30000 ms, protocol call 30000 ms, poll 100 ms, highlight 350 ms.
- A failure that is the browser's fault says so in words containing "browser"; a failure that is the page's says what was seen. The human still sets every verdict.
- No em dashes in text a user or an assistant reads (outcome details, the guide, UI strings).
- Colours via tokens; `src/ui-consistency.test.ts` must not be weakened.
- Commits use a Bash heredoc `git commit -q -F - <<'EOF' ... EOF` ending with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

**Out of scope, on purpose:** several browser contexts or accounts at once (needs flattened protocol sessions), key presses other than the Backspace used to clear a field, hover, file chooser, downloads, iframes, an origin allowlist for `navigate`, moving passwords out of script JSON, unattended whole-script replay, writing results to Azure DevOps, reopening the assistant tools. These are phases 3 to 6 of the research document.

**One behaviour change to expect:** a click or fill on an element that exists but is hidden, disabled, moving or covered used to be forced through with JavaScript. It now waits up to 15 s and then fails with the reason. That is the point of the work, and it can turn a script that "passed" by clicking something no person could click into an honest failure.

---

## File map

| File | Responsibility |
| --- | --- |
| `src-tauri/src/browser/cdp.rs` (rewrite) | `Transport`, `WsTransport`, `Cdp<T>`, `CdpError`, `Event`, `Driver`; timeouts, event buffer, dialog auto-accept. |
| `src-tauri/src/browser/timing.rs` (create) | `Timing` and its defaults. |
| `src-tauri/src/browser/page.rs` (create) | Handles, `call_value`, `call_elements`, `eval_value`, `resolve_backend`, `release`, `screenshot`. |
| `src-tauri/src/browser/locator.rs` (create) | `Target`, `LocatorStep`, validation, description, resolution to handles. |
| `src-tauri/src/browser/input.rs` (create) | Actionability wait, real click, real fill. |
| `src-tauri/src/browser/expect.rs` (create) | The retrying assertion loop and its six checks. |
| `src-tauri/src/browser/actions.rs` (rewrite) | `Action`, `ActionOutcome`, validation, dispatch. |
| `src-tauri/src/browser/launch.rs` (modify) | `launch_with(which, extra_args)`; `launch_in` delegates. |
| `src-tauri/src/browser/mod.rs` (modify) | Module list and header comment. |
| `src-tauri/src/autorun/store.rs` (modify) | Validate actions on save; `save_shot`, `load_shot`, `safe_shot_name`. |
| `src-tauri/src/autorun/guide.rs` (modify) | `ACTION_KINDS` and the guide text for locators and expectations. |
| `src-tauri/src/commands/autorun.rs` (modify) | Drop `Evaluator`; screenshot on failure; `auto_run_shot`. |
| `src-tauri/src/lib.rs` (modify) | Register `auto_run_shot`. |
| `src-tauri/tests/common/mod.rs` (create) | `ScriptedDriver`, `FakePage`. |
| `src-tauri/tests/browser_cdp.rs`, `browser_actions.rs`, `autorun_guide.rs`, `autorun_store.rs` (modify); `browser_page.rs`, `browser_locator.rs`, `browser_input.rs`, `browser_expect.rs`, `browser_live.rs` (create) | Tests. |
| `src-tauri/tests/fixtures/autorun-live.html` (create) | The page the live tests drive. |
| `src/screens/AutoRun/RunPane.tsx`, `RunPane.test.tsx` (modify) | "View screenshot" under a failed action. |
| `src/bindings.ts` (generated) | Regenerated in Tasks 5, 6 and 7. |

---

### Task 1: A DevTools client that cannot hang

**Files:**
- Rewrite: `src-tauri/src/browser/cdp.rs`
- Modify: `src-tauri/src/commands/autorun.rs:24-28` (one line, so the crate keeps compiling until Task 5)
- Test: `src-tauri/tests/browser_cdp.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces (all in `v2_lib::browser::cdp`):
  - `pub const CALL_TIMEOUT: Duration` (30 s)
  - `pub fn frame(id: u64, method: &str, params: Value) -> String` and `pub fn reply_for(id: u64, raw: &str) -> Option<Result<Value, String>>` (unchanged)
  - `pub struct Event { pub method: String, pub params: Value }`, `pub fn event_of(raw: &str) -> Option<Event>`
  - `pub enum CdpError { Timeout { what: String, ms: u64 }, Closed, Protocol { method: String, message: String }, Transport(String) }` with `Display` and `pub fn is_transient(&self) -> bool` (true only for `Protocol`)
  - `pub trait Transport { fn send(&mut self, text: String) -> impl Future<Output = Result<(), String>>; fn recv(&mut self) -> impl Future<Output = Option<Result<String, String>>>; }`
  - `pub struct WsTransport`, `pub struct Cdp<T: Transport = WsTransport>`
  - `impl<T: Transport> Cdp<T>`: `over(transport: T) -> Self`, `transport(&self) -> &T`, `async call(&mut self, method: &str, params: Value) -> Result<Value, CdpError>`, `async call_within(&mut self, method, params, limit: Duration)`, `async wait_event(&mut self, method: &str, limit: Duration) -> Result<Event, CdpError>`, `forget_events(&mut self)`, `take_dialogs(&mut self) -> Vec<String>`, `async eval(&mut self, expression: &str) -> Result<Value, CdpError>`
  - `impl Cdp<WsTransport>`: `async connect(port: u16) -> Result<Cdp<WsTransport>, String>` (now also sends `Page.enable`)
  - `pub trait Driver { fn call(&mut self, method: &str, params: Value) -> impl Future<Output = Result<Value, CdpError>>; fn wait_event(&mut self, method: &str, limit: Duration) -> impl Future<Output = Result<Event, CdpError>>; fn forget_events(&mut self); fn take_dialogs(&mut self) -> Vec<String>; }`, implemented for every `Cdp<T>`

- [ ] **Step 1: Write the failing tests**

In `src-tauri/tests/browser_cdp.rs`, replace the first `use` line with:

```rust
use std::collections::VecDeque;
use std::time::Duration;
use v2_lib::browser::cdp::{event_of, frame, reply_for, Cdp, CdpError, Transport};
```

and append, above the ignored real-browser test:

```rust
/// Stands in for the socket: hands back canned frames in order and keeps
/// what was sent. When it runs dry it either reports the socket closed or,
/// for the timeout test, never answers at all.
struct FakeTransport {
    incoming: VecDeque<String>,
    sent: Vec<String>,
    hang_when_empty: bool,
}

impl FakeTransport {
    fn new(frames: &[&str]) -> Self {
        FakeTransport {
            incoming: frames.iter().map(|f| f.to_string()).collect(),
            sent: vec![],
            hang_when_empty: false,
        }
    }
    fn hanging(frames: &[&str]) -> Self {
        FakeTransport { hang_when_empty: true, ..FakeTransport::new(frames) }
    }
}

impl Transport for FakeTransport {
    async fn send(&mut self, text: String) -> Result<(), String> {
        self.sent.push(text);
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, String>> {
        match self.incoming.pop_front() {
            Some(f) => Some(Ok(f)),
            None if self.hang_when_empty => {
                std::future::pending::<()>().await;
                None
            }
            None => None,
        }
    }
}

#[test]
fn an_event_is_a_frame_with_a_method_and_no_id() {
    let ev = event_of(r#"{"method":"Page.loadEventFired","params":{"timestamp":1}}"#).unwrap();
    assert_eq!(ev.method, "Page.loadEventFired");
    assert_eq!(ev.params["timestamp"], 1);
    assert!(event_of(r#"{"id":3,"result":{}}"#).is_none());
    assert!(event_of("not json").is_none());
}

/// Events that arrive while a call is waiting are kept, not thrown away:
/// the load event for a navigation usually lands before anyone asks for it.
#[tokio::test]
async fn a_call_skips_events_and_keeps_them_for_later() {
    let t = FakeTransport::new(&[
        r#"{"method":"Page.loadEventFired","params":{"timestamp":1}}"#,
        r#"{"id":1,"result":{"ok":true}}"#,
    ]);
    let mut cdp = Cdp::over(t);
    let got = cdp.call("Page.enable", serde_json::json!({})).await.unwrap();
    assert_eq!(got["ok"], true);
    let ev = cdp.wait_event("Page.loadEventFired", Duration::from_millis(50)).await.unwrap();
    assert_eq!(ev.params["timestamp"], 1);
}

#[tokio::test]
async fn wait_event_reads_the_socket_when_nothing_is_buffered() {
    let t = FakeTransport::new(&[
        r#"{"method":"Network.dataReceived","params":{}}"#,
        r#"{"method":"Page.loadEventFired","params":{"timestamp":2}}"#,
    ]);
    let mut cdp = Cdp::over(t);
    let ev = cdp.wait_event("Page.loadEventFired", Duration::from_millis(200)).await.unwrap();
    assert_eq!(ev.params["timestamp"], 2);
}

/// The old client waited forever. A browser that has stopped answering is
/// a harness failure with a name and a number, not a frozen screen.
#[tokio::test]
async fn a_call_that_is_never_answered_times_out_and_says_what_it_was() {
    let mut cdp = Cdp::over(FakeTransport::hanging(&[]));
    let err = cdp
        .call_within("Runtime.evaluate", serde_json::json!({}), Duration::from_millis(50))
        .await
        .unwrap_err();
    assert_eq!(err, CdpError::Timeout { what: "Runtime.evaluate".into(), ms: 50 });
    assert!(err.to_string().contains("Runtime.evaluate"), "{err}");
    assert!(!err.is_transient());
}

#[tokio::test]
async fn a_closed_socket_is_its_own_error() {
    let mut cdp = Cdp::over(FakeTransport::new(&[]));
    let err = cdp.call("Page.enable", serde_json::json!({})).await.unwrap_err();
    assert_eq!(err, CdpError::Closed);
    assert!(err.to_string().contains("browser"), "{err}");
}

/// A refusal names the method. It is also the one kind worth retrying: a
/// page between two documents refuses calls for a moment.
#[tokio::test]
async fn a_protocol_error_names_the_method_and_is_transient() {
    let t = FakeTransport::new(&[r#"{"id":1,"error":{"code":-32000,"message":"No node found"}}"#]);
    let mut cdp = Cdp::over(t);
    let err = cdp.call("DOM.resolveNode", serde_json::json!({})).await.unwrap_err();
    assert_eq!(
        err,
        CdpError::Protocol { method: "DOM.resolveNode".into(), message: "No node found".into() }
    );
    assert!(err.is_transient());
}

/// Measured on real Edge: an alert() leaves every later call pending until
/// the dialog is handled. The client accepts it at once and remembers what
/// it said, so the run carries on and the person is told.
#[tokio::test]
async fn a_javascript_dialog_is_accepted_and_remembered() {
    let t = FakeTransport::new(&[
        r#"{"method":"Page.javascriptDialogOpening","params":{"type":"alert","message":"Saved!"}}"#,
        r#"{"id":1,"result":{"done":true}}"#,
    ]);
    let mut cdp = Cdp::over(t);
    let got = cdp.call("Runtime.evaluate", serde_json::json!({})).await.unwrap();
    assert_eq!(got["done"], true);

    let sent: Vec<serde_json::Value> = cdp
        .transport()
        .sent
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let handled = sent
        .iter()
        .find(|f| f["method"] == "Page.handleJavaScriptDialog")
        .expect("the dialog was never handled");
    assert_eq!(handled["params"]["accept"], true);

    assert_eq!(cdp.take_dialogs(), vec!["alert: Saved!".to_string()]);
    assert!(cdp.take_dialogs().is_empty(), "taking the dialogs must empty the list");
}

#[tokio::test]
async fn forgetting_events_drops_what_was_buffered() {
    let t = FakeTransport::new(&[
        r#"{"method":"Page.loadEventFired","params":{}}"#,
        r#"{"id":1,"result":{}}"#,
    ]);
    let mut cdp = Cdp::over(t);
    cdp.call("Page.enable", serde_json::json!({})).await.unwrap();
    cdp.forget_events();
    let err = cdp.wait_event("Page.loadEventFired", Duration::from_millis(20)).await.unwrap_err();
    assert_eq!(err, CdpError::Closed, "the fake socket is empty, so this reads as closed");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

From `src-tauri/`: `$env:CARGO_TARGET_DIR="target/gate"; cargo test --test browser_cdp`
Expected: compile errors, `event_of`, `CdpError`, `Transport` and `Cdp::over` not found.

- [ ] **Step 3: Rewrite `src-tauri/src/browser/cdp.rs`**

```rust
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
```

- [ ] **Step 4: Keep the command module compiling**

`Cdp::eval` now returns `CdpError`. In `src-tauri/src/commands/autorun.rs`, the `impl Evaluator for Cdp` body (line 26) becomes:

```rust
        Cdp::eval(self, expression).await.map_err(|e| e.to_string())
```

(The whole `impl` goes away in Task 5.)

- [ ] **Step 5: Run the tests**

From `src-tauri/`: `$env:CARGO_TARGET_DIR="target/gate"; cargo test --test browser_cdp`
Expected: 12 passed, 1 ignored.
Then `cargo test --test browser_actions --test autorun_commands` to confirm nothing else broke. Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/browser/cdp.rs src-tauri/src/commands/autorun.rs src-tauri/tests/browser_cdp.rs
git commit -q -F - <<'EOF'
feat(v2): Auto Run DevTools client gets timeouts, typed errors, kept events and dialog handling

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 2: Page helpers and the scripted test driver

**Files:**
- Create: `src-tauri/src/browser/page.rs`
- Modify: `src-tauri/src/browser/mod.rs`
- Create: `src-tauri/tests/common/mod.rs`
- Test: `src-tauri/tests/browser_page.rs`

**Interfaces:**
- Consumes: `Driver`, `CdpError` (Task 1).
- Produces (in `v2_lib::browser::page`):
  - `pub const GROUP: &str = "tcm-autorun"`, `pub type Handle = String` (a DevTools `objectId`)
  - `pub async fn eval_value<D: Driver>(d: &mut D, expression: &str) -> Result<Value, CdpError>`
  - `pub async fn document<D: Driver>(d: &mut D) -> Result<Handle, CdpError>`
  - `pub async fn call_value<D: Driver>(d: &mut D, on: &Handle, function: &str, values: &[Value]) -> Result<Value, CdpError>`
  - `pub async fn call_elements<D: Driver>(d: &mut D, on: &Handle, function: &str, values: &[Value]) -> Result<Vec<Handle>, CdpError>`
  - `pub async fn resolve_backend<D: Driver>(d: &mut D, backend_node_id: i64) -> Result<Handle, CdpError>`
  - `pub async fn release<D: Driver>(d: &mut D)`
- Produces (in `tests/common/mod.rs`): `ScriptedDriver::new(handler)`, fields `calls: Vec<(String, Value)>`, `events: VecDeque<Event>`, `on_call_events: Vec<(String, Event)>`, `dialogs: Vec<String>`; methods `methods() -> Vec<String>`, `calls_to(method) -> Vec<Value>`.

- [ ] **Step 1: Write the scripted driver**

Create `src-tauri/tests/common/mod.rs`:

```rust
//! Test doubles shared by the browser tests. Each integration test file is
//! its own crate, so not every file uses every helper.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::time::Duration;
use v2_lib::browser::cdp::{CdpError, Driver, Event};

type Handler =
    Box<dyn FnMut(&str, &serde_json::Value) -> Result<serde_json::Value, CdpError> + Send>;

/// A browser that answers from a closure and remembers every call.
pub struct ScriptedDriver {
    pub calls: Vec<(String, serde_json::Value)>,
    handler: Handler,
    /// Events already waiting.
    pub events: VecDeque<Event>,
    /// Events that appear once a call to the named method has been made -
    /// how a test says "the load event follows Page.navigate".
    pub on_call_events: Vec<(String, Event)>,
    pub dialogs: Vec<String>,
}

impl ScriptedDriver {
    pub fn new(
        handler: impl FnMut(&str, &serde_json::Value) -> Result<serde_json::Value, CdpError>
            + Send
            + 'static,
    ) -> Self {
        ScriptedDriver {
            calls: vec![],
            handler: Box::new(handler),
            events: VecDeque::new(),
            on_call_events: vec![],
            dialogs: vec![],
        }
    }

    pub fn methods(&self) -> Vec<String> {
        self.calls.iter().map(|(m, _)| m.clone()).collect()
    }

    pub fn calls_to(&self, method: &str) -> Vec<serde_json::Value> {
        self.calls.iter().filter(|(m, _)| m == method).map(|(_, p)| p.clone()).collect()
    }
}

impl Driver for ScriptedDriver {
    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CdpError> {
        self.calls.push((method.to_string(), params.clone()));
        let mut fired = vec![];
        self.on_call_events.retain(|(m, ev)| {
            if m == method {
                fired.push(ev.clone());
                false
            } else {
                true
            }
        });
        self.events.extend(fired);
        (self.handler)(method, &params)
    }

    async fn wait_event(&mut self, method: &str, _limit: Duration) -> Result<Event, CdpError> {
        match self.events.iter().position(|e| e.method == method) {
            Some(i) => Ok(self.events.remove(i).expect("position was just found")),
            None => Err(CdpError::Timeout { what: method.to_string(), ms: 0 }),
        }
    }

    fn forget_events(&mut self) {
        self.events.clear();
    }

    fn take_dialogs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.dialogs)
    }
}
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/tests/browser_page.rs`:

```rust
//! The few protocol calls everything else is built from.

mod common;

use common::ScriptedDriver;
use serde_json::json;
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::page::{self, GROUP};

#[tokio::test]
async fn eval_value_unwraps_the_value() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "result": { "type": "string", "value": "https://x/" } })));
    assert_eq!(page::eval_value(&mut d, "location.href").await.unwrap(), json!("https://x/"));
}

/// A thrown exception comes back as a SUCCESSFUL reply carrying
/// exceptionDetails. Reading it as a value would turn "the page threw"
/// into "the check found nothing".
#[tokio::test]
async fn a_thrown_exception_is_an_error_not_an_empty_value() {
    let mut d = ScriptedDriver::new(|_, _| {
        Ok(json!({
            "result": { "type": "object", "subtype": "error" },
            "exceptionDetails": { "text": "Uncaught", "exception": { "description": "SyntaxError: bad selector" } }
        }))
    });
    let err = page::eval_value(&mut d, "x").await.unwrap_err();
    match err {
        CdpError::Protocol { message, .. } => assert!(message.contains("bad selector"), "{message}"),
        other => panic!("expected a protocol error, got {other:?}"),
    }
}

#[tokio::test]
async fn the_document_handle_lives_in_our_object_group() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "result": { "objectId": "doc-1" } })));
    assert_eq!(page::document(&mut d).await.unwrap(), "doc-1");
    let p = &d.calls_to("Runtime.evaluate")[0];
    assert_eq!(p["expression"], "document");
    assert_eq!(p["objectGroup"], GROUP);
}

/// Script values travel as arguments. They are never pasted into source,
/// so no value can close a string and run its own code.
#[tokio::test]
async fn call_value_passes_values_as_arguments_not_source() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "result": { "value": true } })));
    let got = page::call_value(
        &mut d,
        &"el-1".to_string(),
        "function(a) { return a.length > 0; }",
        &[json!("he said \"hi\"</script>")],
    )
    .await
    .unwrap();
    assert_eq!(got, json!(true));
    let p = &d.calls_to("Runtime.callFunctionOn")[0];
    assert_eq!(p["objectId"], "el-1");
    assert_eq!(p["arguments"][0]["value"], "he said \"hi\"</script>");
    assert_eq!(p["returnByValue"], true);
    assert_eq!(p["awaitPromise"], true);
    assert!(!p["functionDeclaration"].as_str().unwrap().contains("he said"));
}

#[tokio::test]
async fn call_elements_turns_an_array_into_handles_in_index_order() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
        "Runtime.getProperties" => Ok(json!({ "result": [
            { "name": "length", "value": { "type": "number", "value": 2 } },
            { "name": "1", "value": { "objectId": "el-b" } },
            { "name": "0", "value": { "objectId": "el-a" } }
        ] })),
        other => panic!("unexpected {other}"),
    });
    let got = page::call_elements(&mut d, &"doc".to_string(), "function() { return []; }", &[])
        .await
        .unwrap();
    assert_eq!(got, vec!["el-a".to_string(), "el-b".to_string()]);
    assert_eq!(d.calls_to("Runtime.callFunctionOn")[0]["objectGroup"], GROUP);
    assert_eq!(d.calls_to("Runtime.getProperties")[0]["objectId"], "arr");
}

#[tokio::test]
async fn resolve_backend_asks_for_a_handle_in_our_group() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "object": { "objectId": "el-7" } })));
    assert_eq!(page::resolve_backend(&mut d, 7).await.unwrap(), "el-7");
    let p = &d.calls_to("DOM.resolveNode")[0];
    assert_eq!(p["backendNodeId"], 7);
    assert_eq!(p["objectGroup"], GROUP);
}

/// Releasing is housekeeping. A failure there must never fail the action.
#[tokio::test]
async fn release_swallows_its_own_failure() {
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    page::release(&mut d).await;
    assert_eq!(d.methods(), vec!["Runtime.releaseObjectGroup".to_string()]);
}
```

- [ ] **Step 3: Run to verify failure**

From `src-tauri/`: `$env:CARGO_TARGET_DIR="target/gate"; cargo test --test browser_page`
Expected: compile error, `v2_lib::browser::page` not found.

- [ ] **Step 4: Write the module**

Create `src-tauri/src/browser/page.rs`:

```rust
//! The handful of DevTools calls everything above is built from.
//!
//! An element is held as a `Handle` (a DevTools objectId) in one object
//! group, released before each new attempt so handles never pile up in the
//! page. Functions run ON a handle with their inputs passed as arguments:
//! nothing from a script is ever concatenated into JavaScript source.

use super::cdp::{CdpError, Driver};
use serde_json::{json, Value};

/// Every handle this app creates belongs to this group.
pub const GROUP: &str = "tcm-autorun";

/// A DevTools objectId.
pub type Handle = String;

/// A reply that carries `exceptionDetails` is the page throwing, delivered
/// as a success. It has to be an error here, or a bad selector reads as
/// "nothing matched".
fn thrown(method: &str, reply: &Value) -> Option<CdpError> {
    let ex = reply.get("exceptionDetails")?;
    let message = ex["exception"]["description"]
        .as_str()
        .or_else(|| ex["text"].as_str())
        .unwrap_or("the page threw an error")
        .to_string();
    Some(CdpError::Protocol { method: method.to_string(), message })
}

fn handle_of(method: &str, remote: &Value) -> Result<Handle, CdpError> {
    remote["objectId"].as_str().map(str::to_string).ok_or_else(|| CdpError::Protocol {
        method: method.to_string(),
        message: "the page returned no object".to_string(),
    })
}

fn arguments(values: &[Value]) -> Vec<Value> {
    values.iter().map(|v| json!({ "value": v })).collect()
}

/// Evaluate an expression that this app wrote (never script input) and
/// return its value.
pub async fn eval_value<D: Driver>(d: &mut D, expression: &str) -> Result<Value, CdpError> {
    let r = d
        .call(
            "Runtime.evaluate",
            json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
        )
        .await?;
    if let Some(e) = thrown("Runtime.evaluate", &r) {
        return Err(e);
    }
    Ok(r["result"]["value"].clone())
}

/// A handle on the current document: the root every locator starts from.
pub async fn document<D: Driver>(d: &mut D) -> Result<Handle, CdpError> {
    let r = d
        .call("Runtime.evaluate", json!({ "expression": "document", "objectGroup": GROUP }))
        .await?;
    if let Some(e) = thrown("Runtime.evaluate", &r) {
        return Err(e);
    }
    handle_of("Runtime.evaluate", &r["result"])
}

/// Call `function` with `this` bound to `on`, and return its value.
pub async fn call_value<D: Driver>(
    d: &mut D,
    on: &Handle,
    function: &str,
    values: &[Value],
) -> Result<Value, CdpError> {
    let r = d
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": on,
                "functionDeclaration": function,
                "arguments": arguments(values),
                "returnByValue": true,
                "awaitPromise": true,
            }),
        )
        .await?;
    if let Some(e) = thrown("Runtime.callFunctionOn", &r) {
        return Err(e);
    }
    Ok(r["result"]["value"].clone())
}

/// Call `function` (which must return an array of elements) and return a
/// handle for each, in array order.
pub async fn call_elements<D: Driver>(
    d: &mut D,
    on: &Handle,
    function: &str,
    values: &[Value],
) -> Result<Vec<Handle>, CdpError> {
    let r = d
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": on,
                "functionDeclaration": function,
                "arguments": arguments(values),
                "objectGroup": GROUP,
            }),
        )
        .await?;
    if let Some(e) = thrown("Runtime.callFunctionOn", &r) {
        return Err(e);
    }
    let array = handle_of("Runtime.callFunctionOn", &r["result"])?;
    let props = d
        .call("Runtime.getProperties", json!({ "objectId": array, "ownProperties": true }))
        .await?;
    let mut indexed: Vec<(usize, Handle)> = props["result"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let i = p["name"].as_str()?.parse::<usize>().ok()?;
            let id = p["value"]["objectId"].as_str()?;
            Some((i, id.to_string()))
        })
        .collect();
    indexed.sort_by_key(|(i, _)| *i);
    Ok(indexed.into_iter().map(|(_, h)| h).collect())
}

/// An accessibility node names its element by backend node id; this turns
/// that into a handle functions can be called on.
pub async fn resolve_backend<D: Driver>(d: &mut D, backend_node_id: i64) -> Result<Handle, CdpError> {
    let r = d
        .call("DOM.resolveNode", json!({ "backendNodeId": backend_node_id, "objectGroup": GROUP }))
        .await?;
    handle_of("DOM.resolveNode", &r["object"])
}

/// Let go of every handle. Housekeeping: a failure here is ignored, never
/// reported as the action's failure.
pub async fn release<D: Driver>(d: &mut D) {
    let _ = d.call("Runtime.releaseObjectGroup", json!({ "objectGroup": GROUP })).await;
}
```

In `src-tauri/src/browser/mod.rs` the module list becomes:

```rust
pub mod launch;
pub mod cdp;
pub mod page;
pub mod actions;
```

- [ ] **Step 5: Run the tests**

`cargo test --test browser_page` - expected: 7 passed.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/browser/page.rs src-tauri/src/browser/mod.rs src-tauri/tests/common/mod.rs src-tauri/tests/browser_page.rs
git commit -q -F - <<'EOF'
feat(v2): Auto Run page helpers: element handles and functions called with arguments

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 3: Locators

**Files:**
- Create: `src-tauri/src/browser/locator.rs`
- Modify: `src-tauri/src/browser/mod.rs`
- Test: `src-tauri/tests/browser_locator.rs`

**Interfaces:**
- Consumes: `page::{document, call_value, call_elements, resolve_backend, Handle}`, `Driver`, `CdpError`.
- Produces (in `v2_lib::browser::locator`):
  - `pub struct LocatorStep { pub role: Option<String>, pub name: Option<String>, pub text: Option<String>, pub css: Option<String>, pub exact: bool, pub visible: Option<bool>, pub nth: Option<i32> }` (`Default`, serde `deny_unknown_fields`)
  - `pub enum Target { Legacy(String), One(LocatorStep), Chain(Vec<LocatorStep>) }` (serde `untagged`), `impl From<&str> for Target`, `impl From<String> for Target`
  - `Target::validate(&self) -> Result<(), String>`, `Target::describe(&self) -> String`, `Target::is_legacy(&self) -> bool`
  - `pub fn name_matches(got: &str, want: &str, exact: bool) -> bool`
  - `pub async fn resolve<D: Driver>(d: &mut D, target: &Target) -> Result<Vec<Handle>, CdpError>`
  - `pub const CSS_JS`, `TEXT_JS`, `LEGACY_JS`, `VISIBLE_JS: &str`

JSON forms a script may use in a `selector` field:

```json
"#save"
"text=Sign in"
{ "role": "button", "name": "Add Method" }
{ "text": "Step 1 of 6", "exact": true }
{ "css": "table.grid tbody tr", "nth": 2 }
[ { "role": "dialog", "name": "Add Rating Method" }, { "role": "button", "name": "Add Method" } ]
```

Rules: a step names exactly one of `role`, `text`, `css`. `name` only goes with `role`. Matching is case-insensitive "contains" on whitespace-collapsed text unless `exact` is true (then equal, case-sensitive). Every step keeps only visible elements unless it says `"visible": false`. `nth` is zero-based and applies to that step's matches. A list narrows left to right: each step searches inside the previous step's matches.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/tests/browser_locator.rs`:

```rust
//! How a script says WHICH element, and how that becomes handles.

mod common;

use common::ScriptedDriver;
use serde_json::json;
use v2_lib::browser::locator::{name_matches, resolve, LocatorStep, Target, CSS_JS, LEGACY_JS, VISIBLE_JS};

fn step(json: serde_json::Value) -> LocatorStep {
    serde_json::from_value(json).unwrap()
}

#[test]
fn a_plain_string_is_still_a_selector() {
    let t: Target = serde_json::from_value(json!("text=Sign in")).unwrap();
    assert_eq!(t, Target::Legacy("text=Sign in".into()));
    assert!(t.is_legacy());
    // And it goes back out as the same plain string, so a saved script
    // does not change shape by being loaded and saved.
    assert_eq!(serde_json::to_value(&t).unwrap(), json!("text=Sign in"));
}

#[test]
fn an_object_is_one_step_and_a_list_is_a_chain() {
    let one: Target = serde_json::from_value(json!({ "role": "button", "name": "Save" })).unwrap();
    assert_eq!(one, Target::One(step(json!({ "role": "button", "name": "Save" }))));
    let chain: Target = serde_json::from_value(json!([
        { "role": "dialog", "name": "Add Rating Method" },
        { "role": "button", "name": "Add Method" }
    ]))
    .unwrap();
    assert!(matches!(chain, Target::Chain(ref v) if v.len() == 2));
    // Defaults are left out when written back.
    assert_eq!(
        serde_json::to_value(&one).unwrap(),
        json!({ "role": "button", "name": "Save" })
    );
}

/// A typo must not silently become "match anything".
#[test]
fn an_unknown_field_is_refused() {
    assert!(serde_json::from_value::<Target>(json!({ "role": "button", "nme": "Save" })).is_err());
}

#[test]
fn validation_names_the_problem() {
    let ok = |v: serde_json::Value| serde_json::from_value::<Target>(v).unwrap().validate();
    assert!(ok(json!("#go")).is_ok());
    assert!(ok(json!({ "css": "#go", "nth": 0 })).is_ok());
    assert!(ok(json!("  ")).unwrap_err().contains("empty"));
    assert!(ok(json!({})).unwrap_err().contains("one of role, text or css"));
    assert!(ok(json!({ "role": "button", "css": "#go" })).unwrap_err().contains("only one"));
    assert!(ok(json!({ "text": "Save", "name": "Save" })).unwrap_err().contains("name only goes with role"));
    assert!(ok(json!({ "css": "#go", "nth": -1 })).unwrap_err().contains("nth"));
    assert!(ok(json!([])).unwrap_err().contains("empty"));
    assert!(ok(json!({ "role": " " })).unwrap_err().contains("empty"));
}

#[test]
fn a_target_describes_itself_the_way_a_person_would() {
    let t: Target = serde_json::from_value(json!([
        { "role": "dialog", "name": "Add Rating Method" },
        { "role": "button", "name": "Add Method", "nth": 1 }
    ]))
    .unwrap();
    assert_eq!(t.describe(), r#"button "Add Method" #2 in dialog "Add Rating Method""#);
    assert_eq!(Target::from("#go").describe(), "#go");
    let text: Target = serde_json::from_value(json!({ "text": "Step 1 of 6" })).unwrap();
    assert_eq!(text.describe(), r#"text "Step 1 of 6""#);
}

/// Measured on real Edge: a label's name arrives as "Method Name * " with
/// the trailing space, and the protocol's own name filter is exact-only.
#[test]
fn names_match_loosely_unless_exact_is_asked_for() {
    assert!(name_matches("Method Name * ", "method name", false));
    assert!(name_matches("  Save   changes ", "Save changes", true));
    assert!(!name_matches("Save changes", "save changes", true));
    assert!(!name_matches("Save", "Save changes", false));
}

/// The role path: Chrome computes role and name; this app matches the
/// name, turns each node into a handle, and drops the ones nobody can see.
#[tokio::test]
async fn a_role_locator_uses_the_accessibility_tree() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Accessibility.queryAXTree" => {
            assert_eq!(params["objectId"], "doc");
            assert_eq!(params["role"], "button");
            assert!(params.get("accessibleName").is_none(), "names are matched here, not by the browser");
            Ok(json!({ "nodes": [
                { "ignored": false, "name": { "value": "Save changes " }, "backendDOMNodeId": 7 },
                { "ignored": true,  "name": { "value": "Save changes" },  "backendDOMNodeId": 8 },
                { "ignored": false, "name": { "value": "Cancel" },        "backendDOMNodeId": 9 },
                { "ignored": false, "name": { "value": "Save changes" },  "backendDOMNodeId": 10 }
            ] }))
        }
        "DOM.resolveNode" => Ok(json!({ "object": { "objectId": format!("el-{}", params["backendNodeId"]) } })),
        "Runtime.callFunctionOn" => {
            assert_eq!(params["functionDeclaration"], VISIBLE_JS);
            // Node 10 exists but cannot be seen.
            Ok(json!({ "result": { "value": params["objectId"] == "el-7" } }))
        }
        other => panic!("unexpected {other}"),
    });
    let t: Target = serde_json::from_value(json!({ "role": "button", "name": "save changes" })).unwrap();
    assert_eq!(resolve(&mut d, &t).await.unwrap(), vec!["el-7".to_string()]);
}

#[tokio::test]
async fn visible_false_keeps_hidden_matches() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Accessibility.queryAXTree" => Ok(json!({ "nodes": [
            { "ignored": false, "name": { "value": "Save" }, "backendDOMNodeId": 7 }
        ] })),
        "DOM.resolveNode" => Ok(json!({ "object": { "objectId": format!("el-{}", params["backendNodeId"]) } })),
        other => panic!("visibility must not be checked, got {other}"),
    });
    let t: Target = serde_json::from_value(json!({ "role": "button", "visible": false })).unwrap();
    assert_eq!(resolve(&mut d, &t).await.unwrap(), vec!["el-7".to_string()]);
}

/// A chain searches INSIDE the previous step's matches, and nth picks
/// from that step's matches.
#[tokio::test]
async fn a_chain_narrows_inside_the_previous_match() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Accessibility.queryAXTree" => {
            assert_eq!(params["objectId"], "doc");
            Ok(json!({ "nodes": [
                { "ignored": false, "name": { "value": "Add Rating Method" }, "backendDOMNodeId": 3 }
            ] }))
        }
        "DOM.resolveNode" => Ok(json!({ "object": { "objectId": "dialog" } })),
        "Runtime.callFunctionOn" if params["functionDeclaration"] == VISIBLE_JS => {
            Ok(json!({ "result": { "value": true } }))
        }
        "Runtime.callFunctionOn" => {
            assert_eq!(params["functionDeclaration"], CSS_JS);
            assert_eq!(params["objectId"], "dialog", "the css step must search inside the dialog");
            assert_eq!(params["arguments"][0]["value"], "tr");
            assert_eq!(params["arguments"][1]["value"], true, "visible-only by default");
            Ok(json!({ "result": { "objectId": "arr" } }))
        }
        "Runtime.getProperties" => Ok(json!({ "result": [
            { "name": "0", "value": { "objectId": "row-0" } },
            { "name": "1", "value": { "objectId": "row-1" } },
            { "name": "2", "value": { "objectId": "row-2" } }
        ] })),
        other => panic!("unexpected {other}"),
    });
    let t: Target = serde_json::from_value(json!([
        { "role": "dialog", "name": "Add Rating Method" },
        { "css": "tr", "nth": 1 }
    ]))
    .unwrap();
    assert_eq!(resolve(&mut d, &t).await.unwrap(), vec!["row-1".to_string()]);
}

#[tokio::test]
async fn a_legacy_string_keeps_its_old_meaning() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" => {
            assert_eq!(params["functionDeclaration"], LEGACY_JS);
            assert_eq!(params["arguments"][0]["value"], "text=Sign in");
            Ok(json!({ "result": { "objectId": "arr" } }))
        }
        "Runtime.getProperties" => Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el" } } ] })),
        other => panic!("unexpected {other}"),
    });
    assert_eq!(resolve(&mut d, &Target::from("text=Sign in")).await.unwrap(), vec!["el".to_string()]);
}

#[tokio::test]
async fn nothing_matching_is_an_empty_list_not_an_error() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Accessibility.queryAXTree" => Ok(json!({ "nodes": [] })),
        other => panic!("unexpected {other}"),
    });
    let t: Target = serde_json::from_value(json!([{ "role": "dialog" }, { "css": "button" }])).unwrap();
    assert!(resolve(&mut d, &t).await.unwrap().is_empty());
}
```

- [ ] **Step 2: Run to verify failure**

`cargo test --test browser_locator` - expected: compile error, `v2_lib::browser::locator` not found.

- [ ] **Step 3: Write the module**

Create `src-tauri/src/browser/locator.rs`:

```rust
//! Saying WHICH element, in the words a test case uses.
//!
//! A test case says "click Add Method in the Add Rating Method dialog". A
//! locator says the same thing: a role and a name, inside another role and
//! name. Chrome computes role and name itself (the accessibility tree), so
//! this app does not re-implement the accessible-name rules - it asks.
//!
//! Every step keeps only what a person could see unless it says otherwise.
//! Real applications keep hidden copies of their dialogs and menus in the
//! page; a locator that matched those would click nothing, or the wrong
//! thing, and report success.

use super::cdp::{CdpError, Driver};
use super::page::{self, Handle};
use serde_json::json;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct LocatorStep {
    /// An ARIA role as Chrome reports it: button, link, textbox, dialog,
    /// heading, checkbox, combobox, searchbox, row, cell...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The accessible name. Only with `role`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Visible text; the deepest element carrying it wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<String>,
    /// Equal (case-sensitive) instead of contains (case-insensitive).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exact: bool,
    /// `Some(false)` also matches what cannot be seen. Absent means
    /// visible only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Zero-based pick from this step's matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nth: Option<i32>,
}

/// What an action points at. A plain string keeps the meaning it has
/// always had, so every script saved before locators existed still runs.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(untagged)]
pub enum Target {
    Legacy(String),
    One(LocatorStep),
    Chain(Vec<LocatorStep>),
}

impl From<&str> for Target {
    fn from(s: &str) -> Self {
        Target::Legacy(s.to_string())
    }
}

impl From<String> for Target {
    fn from(s: String) -> Self {
        Target::Legacy(s)
    }
}

fn blank(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|v| v.trim().is_empty())
}

impl LocatorStep {
    fn validate(&self) -> Result<(), String> {
        if blank(&self.role) || blank(&self.name) || blank(&self.text) || blank(&self.css) {
            return Err("a locator has an empty role, name, text or css".to_string());
        }
        let kinds = [self.role.is_some(), self.text.is_some(), self.css.is_some()]
            .iter()
            .filter(|b| **b)
            .count();
        if kinds == 0 {
            return Err("a locator needs one of role, text or css".to_string());
        }
        if kinds > 1 {
            return Err("a locator takes only one of role, text or css".to_string());
        }
        if self.name.is_some() && self.role.is_none() {
            return Err("name only goes with role".to_string());
        }
        if self.nth.is_some_and(|n| n < 0) {
            return Err("nth counts from 0 and cannot be negative".to_string());
        }
        Ok(())
    }

    fn describe(&self) -> String {
        let mut s = if let Some(role) = &self.role {
            match &self.name {
                Some(name) => format!("{role} \"{name}\""),
                None => role.clone(),
            }
        } else if let Some(text) = &self.text {
            format!("text \"{text}\"")
        } else {
            self.css.clone().unwrap_or_default()
        };
        if let Some(n) = self.nth {
            s.push_str(&format!(" #{}", n + 1));
        }
        s
    }
}

impl Target {
    pub fn is_legacy(&self) -> bool {
        matches!(self, Target::Legacy(_))
    }

    fn steps(&self) -> &[LocatorStep] {
        match self {
            Target::Legacy(_) => &[],
            Target::One(s) => std::slice::from_ref(s),
            Target::Chain(v) => v,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Target::Legacy(s) if s.trim().is_empty() => Err("a selector is empty".to_string()),
            Target::Legacy(_) => Ok(()),
            Target::Chain(v) if v.is_empty() => Err("a locator list is empty".to_string()),
            _ => self.steps().iter().try_for_each(LocatorStep::validate),
        }
    }

    /// Innermost first, the way a person says it: `button "Add Method" in
    /// dialog "Add Rating Method"`.
    pub fn describe(&self) -> String {
        match self {
            Target::Legacy(s) => s.clone(),
            _ => self
                .steps()
                .iter()
                .rev()
                .map(LocatorStep::describe)
                .collect::<Vec<_>>()
                .join(" in "),
        }
    }
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whitespace is collapsed on both sides first: accessible names arrive
/// with stray spaces. Then contains (case-insensitive), or equal when
/// `exact`.
pub fn name_matches(got: &str, want: &str, exact: bool) -> bool {
    let (got, want) = (collapse(got), collapse(want));
    if exact {
        got == want
    } else {
        got.to_lowercase().contains(&want.to_lowercase())
    }
}

/// `this` is the root. Arguments: selector, visibleOnly.
pub const CSS_JS: &str = r#"function(sel, visibleOnly) {
  const seen = (e) => {
    if (!visibleOnly) return true;
    const r = e.getBoundingClientRect();
    return e.checkVisibility({ visibilityProperty: true }) && r.width > 0 && r.height > 0;
  };
  return Array.from(this.querySelectorAll(sel)).filter(seen);
}"#;

/// `this` is the root. Arguments: text, exact, visibleOnly. The DEEPEST
/// element carrying the text wins: the control itself, not the panel it
/// sits in. textContent is a cheap first pass before the costly innerText.
pub const TEXT_JS: &str = r#"function(want, exact, visibleOnly) {
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  const needle = norm(want);
  const lower = needle.toLowerCase();
  const isButtonInput = (e) => e instanceof HTMLInputElement && /^(button|submit|reset)$/.test(e.type);
  const textOf = (e) => norm(isButtonInput(e) ? e.value : e.innerText);
  const hit = (e) => {
    if (!isButtonInput(e) && !norm(e.textContent).toLowerCase().includes(lower)) return false;
    const t = textOf(e);
    return exact ? t === needle : t.toLowerCase().includes(lower);
  };
  const seen = (e) => {
    if (!visibleOnly) return true;
    const r = e.getBoundingClientRect();
    return e.checkVisibility({ visibilityProperty: true }) && r.width > 0 && r.height > 0;
  };
  const skip = /^(SCRIPT|STYLE|HEAD|HTML|BODY|NOSCRIPT|TEMPLATE)$/;
  const all = Array.from(this.querySelectorAll('*'))
    .filter((e) => e instanceof HTMLElement && !skip.test(e.tagName) && hit(e));
  const matched = new Set(all);
  const parents = new Set();
  for (const e of all) {
    for (let p = e.parentElement; p; p = p.parentElement) if (matched.has(p)) parents.add(p);
  }
  return all.filter((e) => !parents.has(e) && seen(e));
}"#;

/// The selector strings scripts have always used: CSS (first match), or
/// `text=words` (last match). No visibility filter, exactly as before.
pub const LEGACY_JS: &str = r#"function(sel) {
  if (sel.startsWith('text=')) {
    const want = sel.slice(5).trim().toLowerCase();
    const all = Array.from(this.querySelectorAll(
      'button,a,[role=button],label,input,textarea,select,td,th,li,summary,h1,h2,h3,span,div'));
    const hits = all.filter((e) => ((e.innerText || e.value || '') + '').trim().toLowerCase().includes(want));
    return hits.length ? [hits[hits.length - 1]] : [];
  }
  const el = this.querySelector(sel);
  return el ? [el] : [];
}"#;

/// `this` is the element.
pub const VISIBLE_JS: &str = r#"function() {
  const r = this.getBoundingClientRect();
  return this.checkVisibility({ visibilityProperty: true }) && r.width > 0 && r.height > 0;
}"#;

async fn by_role<D: Driver>(
    d: &mut D,
    root: &Handle,
    step: &LocatorStep,
    role: &str,
    visible_only: bool,
) -> Result<Vec<Handle>, CdpError> {
    // Role only. The protocol's own name filter is exact-match, and names
    // carry stray whitespace, so the name is matched here.
    let r = d
        .call("Accessibility.queryAXTree", json!({ "objectId": root, "role": role }))
        .await?;
    let mut out = vec![];
    for node in r["nodes"].as_array().into_iter().flatten() {
        if node["ignored"].as_bool().unwrap_or(false) {
            continue;
        }
        if let Some(want) = &step.name {
            let got = node["name"]["value"].as_str().unwrap_or("");
            if !name_matches(got, want, step.exact) {
                continue;
            }
        }
        let Some(backend) = node["backendDOMNodeId"].as_i64() else { continue };
        let handle = page::resolve_backend(d, backend).await?;
        if visible_only
            && !page::call_value(d, &handle, VISIBLE_JS, &[]).await?.as_bool().unwrap_or(false)
        {
            continue;
        }
        out.push(handle);
    }
    Ok(out)
}

async fn find_in<D: Driver>(
    d: &mut D,
    root: &Handle,
    step: &LocatorStep,
) -> Result<Vec<Handle>, CdpError> {
    let visible_only = step.visible.unwrap_or(true);
    if let Some(role) = &step.role {
        return by_role(d, root, step, role, visible_only).await;
    }
    if let Some(text) = &step.text {
        return page::call_elements(d, root, TEXT_JS, &[json!(text), json!(step.exact), json!(visible_only)])
            .await;
    }
    let css = step.css.as_deref().unwrap_or("");
    page::call_elements(d, root, CSS_JS, &[json!(css), json!(visible_only)]).await
}

/// Every element the target matches right now. Empty is an answer, not an
/// error: callers decide whether "nothing yet" means wait or fail.
pub async fn resolve<D: Driver>(d: &mut D, target: &Target) -> Result<Vec<Handle>, CdpError> {
    let doc = page::document(d).await?;
    if let Target::Legacy(sel) = target {
        return page::call_elements(d, &doc, LEGACY_JS, &[json!(sel)]).await;
    }
    let mut roots = vec![doc];
    for step in target.steps() {
        let mut next = vec![];
        for root in &roots {
            next.extend(find_in(d, root, step).await?);
        }
        if let Some(n) = step.nth {
            next = next.into_iter().nth(n as usize).into_iter().collect();
        }
        roots = next;
        if roots.is_empty() {
            break;
        }
    }
    Ok(roots)
}
```

Add `pub mod locator;` to `src-tauri/src/browser/mod.rs` after `pub mod page;`.

- [ ] **Step 4: Run the tests**

`cargo test --test browser_locator` - expected: 11 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/browser/locator.rs src-tauri/src/browser/mod.rs src-tauri/tests/browser_locator.rs
git commit -q -F - <<'EOF'
feat(v2): Auto Run locators: role and name, text, css, scoped, visible-only, nth

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 4: Waiting until an element can be used, then using it for real

**Files:**
- Create: `src-tauri/src/browser/timing.rs`, `src-tauri/src/browser/input.rs`
- Modify: `src-tauri/src/browser/mod.rs`
- Test: `src-tauri/tests/browser_input.rs`

**Interfaces:**
- Consumes: `locator::{resolve, Target}`, `page::{call_value, release, Handle}`, `Driver`, `CdpError`.
- Produces:
  - `v2_lib::browser::timing::Timing { pub action_ms: u64, pub expect_ms: u64, pub nav_ms: u64, pub poll_ms: u64, pub highlight_ms: u64 }` with `Default` = 15000, 10000, 30000, 100, 350
  - In `v2_lib::browser::input`: `pub enum Blocked { Page(String), Harness(String) }`, `pub struct Ready { pub handle: Handle, pub x: f64, pub y: f64 }`
  - `pub async fn wait_ready<D: Driver>(d: &mut D, target: &Target, need_editable: bool, timing: &Timing) -> Result<Ready, Blocked>`
  - `pub async fn click<D: Driver>(d: &mut D, ready: &Ready) -> Result<(), CdpError>`
  - `pub async fn fill<D: Driver>(d: &mut D, ready: &Ready, value: &str) -> Result<(), Blocked>`
  - `pub const PROBE_JS`, `FOCUS_JS: &str`

An element is ready when it is the only match (structured locators), visible, not moving, enabled, editable if it is to be typed into, and the point about to be clicked actually lands on it. Anything else is a reason, and the last reason is what the person reads if the wait runs out.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/tests/browser_input.rs`:

```rust
//! "Can a person actually click this now?" - and real input once they can.

mod common;

use common::ScriptedDriver;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::input::{click, fill, wait_ready, Blocked, Ready, FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::Target;
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0 }
}

fn probe(over: Value) -> Value {
    let mut base = json!({
        "visible": true, "enabled": true, "editable": true, "stable": true,
        "hit": true, "x": 40.5, "y": 12.0, "covered_by": ""
    });
    for (k, v) in over.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

/// A page where a css locator finds `found` elements and the probe gives
/// `probes` in turn, repeating the last.
fn page(found: usize, probes: Vec<Value>) -> (ScriptedDriver, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let d = ScriptedDriver::new(move |method, params| match method {
        "Runtime.releaseObjectGroup" => Ok(json!({})),
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" if params["functionDeclaration"] == PROBE_JS => {
            let i = counter.fetch_add(1, Ordering::SeqCst).min(probes.len() - 1);
            Ok(json!({ "result": { "value": probes[i] } }))
        }
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
        "Runtime.getProperties" => Ok(json!({ "result": (0..found)
            .map(|i| json!({ "name": i.to_string(), "value": { "objectId": format!("el-{i}") } }))
            .collect::<Vec<_>>() })),
        other => panic!("unexpected {other}"),
    });
    (d, asked)
}

fn css(sel: &str) -> Target {
    serde_json::from_value(json!({ "css": sel })).unwrap()
}

#[tokio::test]
async fn a_ready_element_comes_back_with_where_to_click() {
    let (mut d, _) = page(1, vec![probe(json!({}))]);
    let ready = wait_ready(&mut d, &css("#go"), false, &quick()).await.ok().unwrap();
    assert_eq!(ready.handle, "el-0");
    assert_eq!((ready.x, ready.y), (40.5, 12.0));
}

/// The whole point: a button that is disabled while the page loads is
/// waited for, not clicked blind and not failed at once.
#[tokio::test]
async fn it_waits_for_a_disabled_element_to_become_enabled() {
    let (mut d, asked) = page(
        1,
        vec![probe(json!({ "enabled": false })), probe(json!({ "enabled": false })), probe(json!({}))],
    );
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_ok());
    assert_eq!(asked.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn each_reason_is_said_in_words() {
    let cases = [
        (json!({ "visible": false }), "is not visible"),
        (json!({ "stable": false }), "is still moving"),
        (json!({ "enabled": false }), "is disabled"),
        (json!({ "hit": false, "covered_by": "div.overlay" }), "is covered by div.overlay"),
    ];
    for (over, words) in cases {
        let (mut d, _) = page(1, vec![probe(over)]);
        match wait_ready(&mut d, &css("#go"), false, &quick()).await {
            Err(Blocked::Page(msg)) => {
                assert!(msg.contains(words), "{msg}");
                assert!(msg.contains("#go"), "the target is missing from: {msg}");
                assert!(msg.contains("300ms"), "the wait is missing from: {msg}");
            }
            _ => panic!("expected a page reason containing {words:?}"),
        }
    }
}

#[tokio::test]
async fn typing_needs_something_that_takes_text() {
    let (mut d, _) = page(1, vec![probe(json!({ "editable": false }))]);
    match wait_ready(&mut d, &css("#go"), true, &quick()).await {
        Err(Blocked::Page(msg)) => assert!(msg.contains("cannot be typed into"), "{msg}"),
        _ => panic!("expected a page reason"),
    }
    // The same element is fine to CLICK.
    let (mut d, _) = page(1, vec![probe(json!({ "editable": false }))]);
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_ok());
}

#[tokio::test]
async fn nothing_found_and_too_many_found_are_both_reasons() {
    let (mut d, _) = page(0, vec![probe(json!({}))]);
    match wait_ready(&mut d, &css("#nope"), false, &quick()).await {
        Err(Blocked::Page(msg)) => assert!(msg.contains("not found"), "{msg}"),
        _ => panic!("expected a page reason"),
    }
    // A structured locator has to mean ONE element, or the click is a guess.
    let (mut d, _) = page(3, vec![probe(json!({}))]);
    match wait_ready(&mut d, &css("button"), false, &quick()).await {
        Err(Blocked::Page(msg)) => assert!(msg.contains("matched 3 elements"), "{msg}"),
        _ => panic!("expected a page reason"),
    }
}

/// A page between two documents refuses calls for a moment. That is a
/// reason to look again, not a harness failure.
#[tokio::test]
async fn a_refused_call_is_retried() {
    let n = Arc::new(AtomicUsize::new(0));
    let c = n.clone();
    let mut d = ScriptedDriver::new(move |method, params| match method {
        "Runtime.releaseObjectGroup" => Ok(json!({})),
        "Runtime.evaluate" => {
            if c.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(CdpError::Protocol {
                    method: "Runtime.evaluate".into(),
                    message: "Cannot find context with specified id".into(),
                })
            } else {
                Ok(json!({ "result": { "objectId": "doc" } }))
            }
        }
        "Runtime.callFunctionOn" if params["functionDeclaration"] == PROBE_JS => {
            Ok(json!({ "result": { "value": probe(json!({})) } }))
        }
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
        "Runtime.getProperties" => Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el-0" } } ] })),
        other => panic!("unexpected {other}"),
    });
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_ok());
}

#[tokio::test]
async fn a_dead_browser_is_a_harness_failure_at_once() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.releaseObjectGroup" => Ok(json!({})),
        _ => Err(CdpError::Closed),
    });
    match wait_ready(&mut d, &css("#go"), false, &quick()).await {
        Err(Blocked::Harness(msg)) => assert!(msg.contains("browser"), "{msg}"),
        _ => panic!("expected a harness failure"),
    }
    assert!(d.calls.len() <= 3, "it must not keep polling a dead browser: {:?}", d.methods());
}

#[tokio::test]
async fn a_click_is_three_real_mouse_events_at_the_probed_point() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    click(&mut d, &Ready { handle: "el".into(), x: 40.5, y: 12.0 }).await.unwrap();
    let sent = d.calls_to("Input.dispatchMouseEvent");
    let kinds: Vec<&str> = sent.iter().map(|p| p["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["mouseMoved", "mousePressed", "mouseReleased"]);
    assert!(sent.iter().all(|p| p["x"] == 40.5 && p["y"] == 12.0));
    assert_eq!(sent[1]["button"], "left");
    assert_eq!(sent[1]["clickCount"], 1);
}

/// Measured on real Edge: Input.insertText fires a genuine input event,
/// which is what a framework-controlled field listens for.
#[tokio::test]
async fn a_fill_selects_what_is_there_and_types_over_it() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "text" } })),
        _ => Ok(json!({})),
    });
    fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "Custom 4-Point").await.ok().unwrap();
    let focus = &d.calls_to("Runtime.callFunctionOn")[0];
    assert_eq!(focus["functionDeclaration"], FOCUS_JS);
    assert_eq!(focus["objectId"], "el");
    assert_eq!(d.calls_to("Input.insertText")[0]["text"], "Custom 4-Point");
}

#[tokio::test]
async fn filling_with_nothing_clears_the_field() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "text" } })),
        _ => Ok(json!({})),
    });
    fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "").await.ok().unwrap();
    assert!(d.calls_to("Input.insertText").is_empty());
    let keys = d.calls_to("Input.dispatchKeyEvent");
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["type"], "keyDown");
    assert_eq!(keys[0]["key"], "Backspace");
    assert_eq!(keys[1]["type"], "keyUp");
}

/// A native <select> is set in the page (typing into one does nothing),
/// and an option that is not there is said plainly.
#[tokio::test]
async fn a_native_select_is_chosen_not_typed() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "select-ok" } })),
        _ => Ok(json!({})),
    });
    fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "Numeric").await.ok().unwrap();
    assert!(d.calls_to("Input.insertText").is_empty());

    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "select-missing" } })),
        _ => Ok(json!({})),
    });
    match fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "Nope").await {
        Err(Blocked::Page(msg)) => assert!(msg.contains("no option") && msg.contains("Nope"), "{msg}"),
        _ => panic!("expected a page reason"),
    }
}
```

- [ ] **Step 2: Run to verify failure**

`cargo test --test browser_input` - expected: compile error, `v2_lib::browser::input` not found.

- [ ] **Step 3: Write the two modules**

Create `src-tauri/src/browser/timing.rs`:

```rust
//! How long the runner waits, in one place.

#[derive(Debug, Clone, PartialEq)]
pub struct Timing {
    /// A click or fill waiting for its element to be usable.
    pub action_ms: u64,
    /// An expectation waiting to come true.
    pub expect_ms: u64,
    /// A navigation waiting for the page to load.
    pub nav_ms: u64,
    /// Between looks.
    pub poll_ms: u64,
    /// The outline stays up this long before the action, so a watcher can
    /// see WHERE it is about to land.
    pub highlight_ms: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { action_ms: 15_000, expect_ms: 10_000, nav_ms: 30_000, poll_ms: 100, highlight_ms: 350 }
    }
}
```

Create `src-tauri/src/browser/input.rs`:

```rust
//! Real input, and the wait that comes before it.
//!
//! The first runner called `el.click()` on whatever `querySelector` found.
//! That clicks things no person could: hidden, disabled, mid-animation, or
//! sitting under a modal. A pass produced that way proves nothing. Here an
//! action waits until the element could genuinely be used, then sends the
//! same mouse and keyboard events a person's hardware would.

use super::cdp::{CdpError, Driver};
use super::locator::{resolve, Target};
use super::page::{self, Handle};
use super::timing::Timing;
use serde_json::json;
use std::time::{Duration, Instant};

/// Why an action did not happen.
#[derive(Debug, Clone, PartialEq)]
pub enum Blocked {
    /// The page's doing, in words for the person watching.
    Page(String),
    /// The browser connection's doing. The app under test did nothing wrong.
    Harness(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ready {
    pub handle: Handle,
    /// Viewport coordinates of the element's centre, in CSS pixels.
    pub x: f64,
    pub y: f64,
}

/// `this` is the element. Scrolls it into view, measures it twice 50 ms
/// apart (a timer, not requestAnimationFrame: a minimised window stops
/// painting and the frame callback would never fire), and hit-tests its
/// centre.
pub const PROBE_JS: &str = r#"async function() {
  this.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
  const a = this.getBoundingClientRect();
  await new Promise((r) => setTimeout(r, 50));
  const b = this.getBoundingClientRect();
  const x = b.left + b.width / 2, y = b.top + b.height / 2;
  const top = document.elementFromPoint(x, y);
  const label = top && top.closest ? top.closest('label') : null;
  const hit = !!top && (top === this || this.contains(top) || (label && label.control === this));
  const say = (e) => !e ? 'nothing' : e.tagName.toLowerCase() + (e.id ? '#' + e.id : '') +
    (typeof e.className === 'string' && e.className.trim() ? '.' + e.className.trim().split(/\s+/).join('.') : '');
  const editable =
    (this instanceof HTMLInputElement && !this.readOnly &&
      !/^(checkbox|radio|file|button|submit|reset|image|range|color|hidden)$/.test(this.type)) ||
    (this instanceof HTMLTextAreaElement && !this.readOnly) ||
    this instanceof HTMLSelectElement || this.isContentEditable;
  return {
    visible: this.checkVisibility({ visibilityProperty: true }) && b.width > 0 && b.height > 0,
    enabled: !this.disabled && this.getAttribute('aria-disabled') !== 'true' && !this.closest('fieldset[disabled]'),
    editable: !!editable,
    stable: a.left === b.left && a.top === b.top && a.width === b.width && a.height === b.height,
    hit, x, y, covered_by: hit ? '' : say(top),
  };
}"#;

/// `this` is the element. Argument: the value. A native select is set
/// here; anything else is focused with its contents selected, so what is
/// typed next replaces them.
pub const FOCUS_JS: &str = r#"function(value) {
  if (this instanceof HTMLSelectElement) {
    const opt = Array.from(this.options).find((o) => o.label.trim() === value.trim() || o.value === value);
    if (!opt) return 'select-missing';
    this.value = opt.value;
    this.dispatchEvent(new Event('input', { bubbles: true }));
    this.dispatchEvent(new Event('change', { bubbles: true }));
    return 'select-ok';
  }
  this.focus();
  if (this.isContentEditable) {
    const r = document.createRange();
    r.selectNodeContents(this);
    const s = getSelection();
    s.removeAllRanges();
    s.addRange(r);
  } else if (typeof this.select === 'function') {
    this.select();
  }
  return 'text';
}"#;

enum Look {
    Ready(Ready),
    NotYet(String),
}

async fn look<D: Driver>(d: &mut D, target: &Target, need_editable: bool) -> Result<Look, CdpError> {
    let handles = resolve(d, target).await?;
    if handles.is_empty() {
        return Ok(Look::NotYet("not found".to_string()));
    }
    if handles.len() > 1 && !target.is_legacy() {
        return Ok(Look::NotYet(format!(
            "matched {} elements - narrow it, or add nth",
            handles.len()
        )));
    }
    let handle = handles.into_iter().next().expect("checked non-empty");
    let p = page::call_value(d, &handle, PROBE_JS, &[]).await?;
    let flag = |k: &str| p[k].as_bool().unwrap_or(false);
    let why = if !flag("visible") {
        "is not visible".to_string()
    } else if !flag("stable") {
        "is still moving".to_string()
    } else if !flag("enabled") {
        "is disabled".to_string()
    } else if need_editable && !flag("editable") {
        "cannot be typed into".to_string()
    } else if !flag("hit") {
        format!("is covered by {}", p["covered_by"].as_str().unwrap_or("another element"))
    } else {
        return Ok(Look::Ready(Ready {
            handle,
            x: p["x"].as_f64().unwrap_or(0.0),
            y: p["y"].as_f64().unwrap_or(0.0),
        }));
    };
    Ok(Look::NotYet(why))
}

/// Look, and keep looking, until the element can be used or the action
/// timeout runs out. The last reason seen is the one reported.
pub async fn wait_ready<D: Driver>(
    d: &mut D,
    target: &Target,
    need_editable: bool,
    timing: &Timing,
) -> Result<Ready, Blocked> {
    let deadline = Instant::now() + Duration::from_millis(timing.action_ms);
    loop {
        page::release(d).await;
        let why = match look(d, target, need_editable).await {
            Ok(Look::Ready(r)) => return Ok(r),
            Ok(Look::NotYet(why)) => why,
            Err(e) if e.is_transient() => e.to_string(),
            Err(e) => return Err(Blocked::Harness(e.to_string())),
        };
        if Instant::now() >= deadline {
            return Err(Blocked::Page(format!(
                "waited {}ms: {} {}",
                timing.action_ms,
                target.describe(),
                why
            )));
        }
        tokio::time::sleep(Duration::from_millis(timing.poll_ms)).await;
    }
}

pub async fn click<D: Driver>(d: &mut D, ready: &Ready) -> Result<(), CdpError> {
    for (kind, button, count) in
        [("mouseMoved", "none", 0), ("mousePressed", "left", 1), ("mouseReleased", "left", 1)]
    {
        d.call(
            "Input.dispatchMouseEvent",
            json!({ "type": kind, "x": ready.x, "y": ready.y, "button": button, "clickCount": count }),
        )
        .await?;
    }
    Ok(())
}

fn harness(e: CdpError) -> Blocked {
    Blocked::Harness(e.to_string())
}

pub async fn fill<D: Driver>(d: &mut D, ready: &Ready, value: &str) -> Result<(), Blocked> {
    let kind = page::call_value(d, &ready.handle, FOCUS_JS, &[json!(value)]).await.map_err(harness)?;
    match kind.as_str().unwrap_or("text") {
        "select-ok" => return Ok(()),
        "select-missing" => {
            return Err(Blocked::Page(format!("the list has no option \"{value}\"")));
        }
        _ => {}
    }
    if value.is_empty() {
        for kind in ["keyDown", "keyUp"] {
            d.call(
                "Input.dispatchKeyEvent",
                json!({
                    "type": kind, "key": "Backspace", "code": "Backspace",
                    "windowsVirtualKeyCode": 8, "nativeVirtualKeyCode": 8
                }),
            )
            .await
            .map_err(harness)?;
        }
        return Ok(());
    }
    d.call("Input.insertText", json!({ "text": value })).await.map_err(harness)?;
    Ok(())
}
```

In `src-tauri/src/browser/mod.rs` the module list becomes:

```rust
pub mod launch;
pub mod cdp;
pub mod timing;
pub mod page;
pub mod locator;
pub mod input;
pub mod actions;
```

- [ ] **Step 4: Run the tests**

`cargo test --test browser_input` - expected: 11 passed. The whole file should finish in a few seconds (the waits are 300 ms).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/browser/timing.rs src-tauri/src/browser/input.rs src-tauri/src/browser/mod.rs src-tauri/tests/browser_input.rs
git commit -q -F - <<'EOF'
feat(v2): Auto Run waits until an element can really be used, then sends real input

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 5: The actions run on the new driver

**Files:**
- Rewrite: `src-tauri/src/browser/actions.rs`
- Modify: `src-tauri/src/commands/autorun.rs:9-10,24-28` (drop `Evaluator`)
- Modify: `src-tauri/src/autorun/store.rs:103-124` (validate actions on save)
- Modify: `src-tauri/tests/common/mod.rs` (add `FakePage`)
- Rewrite: `src-tauri/tests/browser_actions.rs`
- Modify: `src-tauri/tests/autorun_store.rs` (one test)
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: `Driver`, `CdpError`, `page::*`, `locator::{resolve, Target}`, `input::{wait_ready, click, fill, Blocked}`, `timing::Timing`.
- Produces (in `v2_lib::browser::actions`):
  - `pub enum Action { Navigate { url: String }, Click { selector: Target }, Fill { selector: Target, value: String }, WaitFor { selector: Target, timeout_ms: u32 }, CheckText { value: String }, CheckUrl { contains: String } }` (serde `tag = "kind"`, `snake_case`, as today; only the type of `selector` changes)
  - `Action::validate(&self) -> Result<(), String>`
  - `pub struct ActionOutcome { pub ok: bool, pub detail: String }` with `ActionOutcome::passed(detail)`, `ActionOutcome::failed(detail)`
  - `pub async fn execute<D: Driver>(d: &mut D, action: &Action) -> ActionOutcome` and `pub async fn execute_with<D: Driver>(d: &mut D, action: &Action, timing: &Timing) -> ActionOutcome`
  - `pub(crate) fn blocked(b: Blocked) -> ActionOutcome`, `pub(crate) fn harness(e: CdpError) -> ActionOutcome` (Task 6 uses them)
  - `pub const HIGHLIGHT_JS`, `CHECK_TEXT_JS: &str`
  - Removed: `Evaluator`, `js_for`, `highlight_js`
- Produces (in `tests/common/mod.rs`): `FakePage` with `driver(self) -> ScriptedDriver`, and `ready_probe() -> Value`

- [ ] **Step 1: Add `FakePage` to the shared helpers**

Append to `src-tauri/tests/common/mod.rs`:

```rust
use serde_json::{json, Value};
use v2_lib::browser::actions::{CHECK_TEXT_JS, HIGHLIGHT_JS};
use v2_lib::browser::input::{FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::VISIBLE_JS;

pub fn ready_probe() -> Value {
    json!({
        "visible": true, "enabled": true, "editable": true, "stable": true,
        "hit": true, "x": 10.0, "y": 20.0, "covered_by": ""
    })
}

/// A page described by what it would answer. Every locator finds `found`
/// elements (none until the `appears_on_look`-th look), and each function
/// the runner calls gets the matching field back.
pub struct FakePage {
    pub found: usize,
    /// 1 = there from the first look.
    pub appears_on_look: usize,
    /// Answers to the actionability probe, in turn; the last repeats.
    pub probes: Vec<Value>,
    pub visible: bool,
    pub fill_kind: &'static str,
    pub body_has_text: bool,
    pub href: &'static str,
    pub navigate_reply: Value,
}

impl Default for FakePage {
    fn default() -> Self {
        FakePage {
            found: 1,
            appears_on_look: 1,
            probes: vec![ready_probe()],
            visible: true,
            fill_kind: "text",
            body_has_text: true,
            href: "https://app.example/home",
            navigate_reply: json!({ "frameId": "F", "loaderId": "L" }),
        }
    }
}

impl FakePage {
    pub fn driver(self) -> ScriptedDriver {
        let page = self;
        let mut looks = 0usize;
        let mut probed = 0usize;
        ScriptedDriver::new(move |method, params| {
            let f = params["functionDeclaration"].as_str().unwrap_or("");
            Ok(match method {
                "Runtime.evaluate" if params["expression"] == "document" => {
                    json!({ "result": { "objectId": "doc" } })
                }
                "Runtime.evaluate" => json!({ "result": { "value": page.href } }),
                "Runtime.callFunctionOn" if f == PROBE_JS => {
                    let i = probed.min(page.probes.len() - 1);
                    probed += 1;
                    json!({ "result": { "value": page.probes[i] } })
                }
                "Runtime.callFunctionOn" if f == VISIBLE_JS => json!({ "result": { "value": page.visible } }),
                "Runtime.callFunctionOn" if f == HIGHLIGHT_JS => json!({ "result": { "value": true } }),
                "Runtime.callFunctionOn" if f == FOCUS_JS => json!({ "result": { "value": page.fill_kind } }),
                "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => {
                    json!({ "result": { "value": page.body_has_text } })
                }
                // Any locator function: an array of elements.
                "Runtime.callFunctionOn" => json!({ "result": { "objectId": "arr" } }),
                "Runtime.getProperties" => {
                    looks += 1;
                    let n = if looks >= page.appears_on_look { page.found } else { 0 };
                    json!({ "result": (0..n)
                        .map(|i| json!({ "name": i.to_string(), "value": { "objectId": format!("el-{i}") } }))
                        .collect::<Vec<_>>() })
                }
                "Page.navigate" => page.navigate_reply.clone(),
                _ => json!({}),
            })
        })
    }
}
```

- [ ] **Step 2: Write the failing tests**

Replace the whole of `src-tauri/tests/browser_actions.rs` with:

```rust
//! What each action does to the browser, and how the executor reports it.
//! A described fake page stands in for Edge, so every rule here is pinned
//! without opening a window. `tests/browser_live.rs` checks the same things
//! against the real one.

mod common;

use common::{ready_probe, FakePage, ScriptedDriver};
use serde_json::json;
use v2_lib::browser::actions::{execute_with, Action};
use v2_lib::browser::cdp::{CdpError, Event};
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0 }
}

/// Every script saved before locators existed has string selectors. They
/// must keep parsing, and keep meaning what they meant.
#[test]
fn a_script_written_before_locators_still_parses() {
    let old = json!([
        { "kind": "navigate", "url": "https://app.example/login" },
        { "kind": "wait_for", "selector": "#user", "timeout_ms": 5000 },
        { "kind": "fill", "selector": "#user", "value": "tester" },
        { "kind": "click", "selector": "text=Sign in" },
        { "kind": "check_text", "value": "Dashboard" },
        { "kind": "check_url", "contains": "/home" }
    ]);
    let actions: Vec<Action> = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(actions.len(), 6);
    assert_eq!(serde_json::to_value(&actions).unwrap(), old, "and they go back out unchanged");
}

#[test]
fn an_action_can_point_with_a_locator() {
    let a: Action = serde_json::from_value(json!({
        "kind": "click",
        "selector": [{ "role": "dialog", "name": "Add Rating Method" }, { "role": "button", "name": "Add Method" }]
    }))
    .unwrap();
    assert!(a.validate().is_ok());
}

#[test]
fn validation_catches_what_would_only_fail_at_run_time() {
    let bad = |v: serde_json::Value| serde_json::from_value::<Action>(v).unwrap().validate().unwrap_err();
    assert!(bad(json!({ "kind": "click", "selector": {} })).contains("role, text or css"));
    assert!(bad(json!({ "kind": "navigate", "url": "javascript:alert(1)" })).contains("http"));
    assert!(bad(json!({ "kind": "navigate", "url": "" })).contains("http"));
    assert!(bad(json!({ "kind": "check_text", "value": " " })).contains("empty"));
    assert!(bad(json!({ "kind": "check_url", "contains": "" })).contains("empty"));
}

/// Highlight first so the watcher sees WHERE, then a real click at the
/// point the probe measured.
#[tokio::test]
async fn a_click_highlights_then_sends_real_mouse_events() {
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &Action::Click { selector: "#go".into() }, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "clicked #go");
    let methods = d.methods();
    let first_mouse = methods.iter().position(|m| m == "Input.dispatchMouseEvent").unwrap();
    let highlight = d
        .calls
        .iter()
        .position(|(_, p)| p["functionDeclaration"] == v2_lib::browser::actions::HIGHLIGHT_JS)
        .expect("never highlighted");
    assert!(highlight < first_mouse, "the highlight must come before the click");
    let mouse = d.calls_to("Input.dispatchMouseEvent");
    assert_eq!(mouse.len(), 3);
    assert_eq!((mouse[1]["x"].as_f64(), mouse[1]["y"].as_f64()), (Some(10.0), Some(20.0)));
}

/// The human is the oracle, so the executor's job is to report faithfully:
/// a missing element is a plain false with a reason, and nothing is clicked.
#[tokio::test]
async fn a_missing_element_fails_with_the_reason_and_clicks_nothing() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &Action::Click { selector: "#nope".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("not found") && out.detail.contains("#nope"), "{}", out.detail);
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty());
}

#[tokio::test]
async fn a_covered_element_is_not_clicked_through() {
    let mut covered = ready_probe();
    covered["hit"] = json!(false);
    covered["covered_by"] = json!("div.modal-backdrop");
    let mut d = FakePage { probes: vec![covered], ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &Action::Click { selector: "#go".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("covered by div.modal-backdrop"), "{}", out.detail);
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty());
}

/// A value with quotes and a closing script tag travels as data. It never
/// appears inside any JavaScript source this app sends.
#[tokio::test]
async fn a_fill_types_the_value_and_never_puts_it_in_source() {
    let nasty = "he said \"hi\"\n</script>";
    let mut d = FakePage::default().driver();
    let out = execute_with(
        &mut d,
        &Action::Fill { selector: "#user".into(), value: nasty.into() },
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "filled #user");
    assert_eq!(d.calls_to("Input.insertText")[0]["text"], nasty);
    for (method, params) in &d.calls {
        let source = params["functionDeclaration"].as_str().or(params["expression"].as_str()).unwrap_or("");
        assert!(!source.contains("he said"), "{method} carried the value in its source");
    }
}

#[tokio::test]
async fn a_fill_into_something_that_takes_no_text_says_so() {
    let mut not_editable = ready_probe();
    not_editable["editable"] = json!(false);
    let mut d = FakePage { probes: vec![not_editable], ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &Action::Fill { selector: "#logo".into(), value: "x".into() },
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.contains("cannot be typed into"), "{}", out.detail);
}

/// A dropped socket must not read as a failed assertion about the app
/// under test - it is a failure of the harness, and it says so.
#[tokio::test]
async fn a_transport_error_is_reported_as_a_harness_problem() {
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    let out = execute_with(&mut d, &Action::CheckText { value: "Dashboard".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("browser"), "{}", out.detail);
}

/// Waiting polls rather than sleeping a fixed guess, and gives up with a
/// verdict instead of hanging the run.
#[tokio::test]
async fn wait_for_polls_until_it_appears() {
    let mut d = FakePage { appears_on_look: 3, ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &Action::WaitFor { selector: "#late".into(), timeout_ms: 2000 },
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(d.calls_to("Runtime.getProperties").len(), 3);
}

#[tokio::test]
async fn wait_for_gives_up_after_its_own_timeout() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &Action::WaitFor { selector: "#never".into(), timeout_ms: 120 },
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.contains("120ms") && out.detail.contains("#never"), "{}", out.detail);
}

#[tokio::test]
async fn navigate_waits_for_the_page_to_load() {
    let mut d = FakePage::default().driver();
    d.on_call_events.push((
        "Page.navigate".into(),
        Event { method: "Page.loadEventFired".into(), params: json!({}) },
    ));
    // A stale load event from earlier must not satisfy this navigation.
    d.events.push_back(Event { method: "Page.loadEventFired".into(), params: json!({ "stale": true }) });
    let out = execute_with(&mut d, &Action::Navigate { url: "https://app.example/login".into() }, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(d.calls_to("Page.navigate")[0]["url"], "https://app.example/login");
    assert!(d.events.is_empty(), "the navigation's own load event should have been consumed");
}

#[tokio::test]
async fn navigate_reports_a_page_that_would_not_load() {
    let mut d = FakePage {
        navigate_reply: json!({ "frameId": "F", "errorText": "net::ERR_NAME_NOT_RESOLVED" }),
        ..FakePage::default()
    }
    .driver();
    let out = execute_with(&mut d, &Action::Navigate { url: "https://nope.invalid/".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("ERR_NAME_NOT_RESOLVED"), "{}", out.detail);
}

#[tokio::test]
async fn navigate_that_never_finishes_loading_says_so() {
    let mut d = FakePage::default().driver(); // no load event will come
    let out = execute_with(&mut d, &Action::Navigate { url: "https://app.example/slow".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("did not finish loading"), "{}", out.detail);
}

/// A scheme that is not a page is refused before the browser is asked.
#[tokio::test]
async fn navigate_refuses_a_javascript_url_without_touching_the_browser() {
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &Action::Navigate { url: "javascript:alert(1)".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(d.calls.is_empty(), "{:?}", d.methods());
}

#[tokio::test]
async fn check_text_and_check_url_answer_from_the_page() {
    let mut d = FakePage::default().driver();
    assert!(execute_with(&mut d, &Action::CheckText { value: "Dashboard".into() }, &quick()).await.ok);
    let out = execute_with(&mut d, &Action::CheckUrl { contains: "/home".into() }, &quick()).await;
    assert!(out.ok && out.detail.contains("https://app.example/home"), "{}", out.detail);

    let mut d = FakePage { body_has_text: false, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &Action::CheckText { value: "Dashboard".into() }, &quick()).await;
    assert!(!out.ok && out.detail.contains("does NOT contain"), "{}", out.detail);
    assert!(!execute_with(&mut d, &Action::CheckUrl { contains: "/login".into() }, &quick()).await.ok);
}

/// A page that pops an alert no longer freezes the run; the person is told
/// it happened.
#[tokio::test]
async fn a_dialog_the_page_showed_is_mentioned() {
    let mut d = FakePage::default().driver();
    d.dialogs.push("alert: Saved!".into());
    let out = execute_with(&mut d, &Action::Click { selector: "#go".into() }, &quick()).await;
    assert!(out.ok);
    assert!(out.detail.contains("alert: Saved!") && out.detail.contains("accepted"), "{}", out.detail);
}

#[tokio::test]
async fn an_invalid_action_fails_without_touching_the_browser() {
    let a: Action = serde_json::from_value(json!({ "kind": "click", "selector": {} })).unwrap();
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &a, &quick()).await;
    assert!(!out.ok && out.detail.contains("role, text or css"), "{}", out.detail);
    assert!(d.calls.is_empty());
}
```

- [ ] **Step 3: Run to verify failure**

`cargo test --test browser_actions` - expected: compile errors (`execute_with`, `HIGHLIGHT_JS`, `Action::validate` missing; `selector` is a `String`).

- [ ] **Step 4: Rewrite `src-tauri/src/browser/actions.rs`**

```rust
//! The typed steps a script is made of, and what each does to the browser.
//!
//! Every action answers `{ ok, detail }`. `detail` is written for the
//! human watching, because in this runner the person - not the machine -
//! decides the verdict. An action that cannot tell what happened says so
//! rather than guessing.

use super::cdp::{CdpError, Driver};
use super::input::{self, Blocked};
use super::locator::{resolve, Target};
use super::page;
use super::timing::Timing;
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Navigate { url: String },
    Click { selector: Target },
    Fill { selector: Target, value: String },
    WaitFor { selector: Target, timeout_ms: u32 },
    CheckText { value: String },
    CheckUrl { contains: String },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ActionOutcome {
    pub ok: bool,
    pub detail: String,
}

impl ActionOutcome {
    pub fn passed(detail: impl Into<String>) -> Self {
        ActionOutcome { ok: true, detail: detail.into() }
    }
    pub fn failed(detail: impl Into<String>) -> Self {
        ActionOutcome { ok: false, detail: detail.into() }
    }
}

/// A harness failure, said plainly: the app under test did nothing wrong,
/// the browser connection did.
pub(crate) fn harness(e: CdpError) -> ActionOutcome {
    ActionOutcome::failed(format!("the browser did not answer: {e}"))
}

pub(crate) fn blocked(b: Blocked) -> ActionOutcome {
    match b {
        Blocked::Page(why) => ActionOutcome::failed(why),
        Blocked::Harness(why) => ActionOutcome::failed(format!("the browser did not answer: {why}")),
    }
}

fn is_page_url(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("file://")
}

impl Action {
    /// What can be known to be wrong before a browser is involved. Run on
    /// save, so a bad script is refused where it is written, and again on
    /// execute, so nothing invalid reaches the page.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Action::Navigate { url } if !is_page_url(url) => {
                Err(format!("navigate needs an http, https or file address, not {url:?}"))
            }
            Action::Navigate { .. } => Ok(()),
            Action::Click { selector }
            | Action::Fill { selector, .. }
            | Action::WaitFor { selector, .. } => selector.validate(),
            Action::CheckText { value } if value.trim().is_empty() => {
                Err("check_text has an empty value".to_string())
            }
            Action::CheckUrl { contains } if contains.trim().is_empty() => {
                Err("check_url has an empty value".to_string())
            }
            Action::CheckText { .. } | Action::CheckUrl { .. } => Ok(()),
        }
    }
}

/// `this` is the element about to be touched.
pub const HIGHLIGHT_JS: &str = r#"function() {
  const prev = this.style.outline;
  this.style.outline = '3px solid #7c5cff';
  setTimeout(() => { this.style.outline = prev; }, 1200);
  return true;
}"#;

/// `this` is the document. Argument: the words to look for.
pub const CHECK_TEXT_JS: &str = r#"function(want) {
  const hay = (this.body ? this.body.innerText : '') || '';
  return hay.toLowerCase().includes(String(want).toLowerCase());
}"#;

async fn point_and_pause<D: Driver>(
    d: &mut D,
    ready: &input::Ready,
    timing: &Timing,
) -> Result<(), CdpError> {
    page::call_value(d, &ready.handle, HIGHLIGHT_JS, &[]).await?;
    if timing.highlight_ms > 0 {
        tokio::time::sleep(Duration::from_millis(timing.highlight_ms)).await;
    }
    Ok(())
}

async fn navigate<D: Driver>(d: &mut D, url: &str, timing: &Timing) -> ActionOutcome {
    // Older load events would satisfy the wait below before this page has
    // even started.
    d.forget_events();
    let reply = match d.call("Page.navigate", json!({ "url": url })).await {
        Ok(r) => r,
        Err(e) => return harness(e),
    };
    if let Some(err) = reply["errorText"].as_str() {
        return ActionOutcome::failed(format!("{url} would not load: {err}"));
    }
    // No loaderId means the same document (a #fragment): nothing loads.
    if reply.get("loaderId").is_none() {
        return ActionOutcome::passed(format!("moved to {url}"));
    }
    match d.wait_event("Page.loadEventFired", Duration::from_millis(timing.nav_ms)).await {
        Ok(_) => ActionOutcome::passed(format!("loaded {url}")),
        Err(CdpError::Timeout { .. }) => ActionOutcome::failed(format!(
            "{url} did not finish loading within {}ms",
            timing.nav_ms
        )),
        Err(e) => harness(e),
    }
}

async fn wait_for<D: Driver>(d: &mut D, target: &Target, timeout_ms: u32, timing: &Timing) -> ActionOutcome {
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    loop {
        page::release(d).await;
        match resolve(d, target).await {
            Ok(found) if !found.is_empty() => {
                return ActionOutcome::passed(format!("found {}", target.describe()));
            }
            Ok(_) => {}
            Err(e) if e.is_transient() => {}
            Err(e) => return harness(e),
        }
        if Instant::now() >= deadline {
            return ActionOutcome::failed(format!(
                "waited {timeout_ms}ms and never saw {}",
                target.describe()
            ));
        }
        tokio::time::sleep(Duration::from_millis(timing.poll_ms)).await;
    }
}

async fn run<D: Driver>(d: &mut D, action: &Action, timing: &Timing) -> ActionOutcome {
    match action {
        Action::Navigate { url } => navigate(d, url.trim(), timing).await,
        Action::Click { selector } => {
            let ready = match input::wait_ready(d, selector, false, timing).await {
                Ok(r) => r,
                Err(b) => return blocked(b),
            };
            if let Err(e) = point_and_pause(d, &ready, timing).await {
                return harness(e);
            }
            match input::click(d, &ready).await {
                Ok(()) => ActionOutcome::passed(format!("clicked {}", selector.describe())),
                Err(e) => harness(e),
            }
        }
        Action::Fill { selector, value } => {
            let ready = match input::wait_ready(d, selector, true, timing).await {
                Ok(r) => r,
                Err(b) => return blocked(b),
            };
            if let Err(e) = point_and_pause(d, &ready, timing).await {
                return harness(e);
            }
            match input::fill(d, &ready, value).await {
                Ok(()) => ActionOutcome::passed(format!("filled {}", selector.describe())),
                Err(b) => blocked(b),
            }
        }
        Action::WaitFor { selector, timeout_ms } => wait_for(d, selector, *timeout_ms, timing).await,
        Action::CheckText { value } => {
            let doc = match page::document(d).await {
                Ok(h) => h,
                Err(e) => return harness(e),
            };
            match page::call_value(d, &doc, CHECK_TEXT_JS, &[json!(value)]).await {
                Ok(v) if v.as_bool().unwrap_or(false) => {
                    ActionOutcome::passed(format!("page contains {value}"))
                }
                Ok(_) => ActionOutcome::failed(format!("page does NOT contain {value}")),
                Err(e) => harness(e),
            }
        }
        Action::CheckUrl { contains } => match page::eval_value(d, "location.href").await {
            Ok(v) => {
                let href = v.as_str().unwrap_or("");
                ActionOutcome { ok: href.contains(contains.as_str()), detail: format!("url is {href}") }
            }
            Err(e) => harness(e),
        },
    }
}

/// Run one action with the standard waits.
pub async fn execute<D: Driver>(d: &mut D, action: &Action) -> ActionOutcome {
    execute_with(d, action, &Timing::default()).await
}

pub async fn execute_with<D: Driver>(d: &mut D, action: &Action, timing: &Timing) -> ActionOutcome {
    if let Err(why) = action.validate() {
        return ActionOutcome::failed(format!("this action cannot run: {why}"));
    }
    let mut out = run(d, action, timing).await;
    let dialogs = d.take_dialogs();
    if !dialogs.is_empty() {
        out.detail.push_str(&format!(
            " (the page showed {} and it was accepted)",
            dialogs.join("; ")
        ));
    }
    out
}
```

- [ ] **Step 5: Drop `Evaluator` from the command module**

In `src-tauri/src/commands/autorun.rs`, line 9 becomes:

```rust
use crate::browser::actions::{execute, ActionOutcome};
```

and delete the `impl Evaluator for Cdp { ... }` block (lines 24-28). `execute(&mut session.cdp, action)` compiles unchanged, because `Cdp` is a `Driver`.

- [ ] **Step 6: Refuse an invalid action when a script is saved**

In `src-tauri/src/autorun/store.rs`, in pass 1 of `save_scripts_atomically`, after the `sc.steps.is_empty()` check and before `let json = ...`, add:

```rust
        for step in &sc.steps {
            for (i, action) in step.actions.iter().enumerate() {
                if let Err(why) = action.validate() {
                    return Err(SaveScriptsError::Invalid(format!(
                        "case {} step {} action {}: {why}",
                        sc.case_id,
                        step.step_number,
                        i + 1
                    )));
                }
            }
        }
```

Add to `src-tauri/tests/autorun_store.rs` (match the file's existing imports and temp-dir helper; if it has none, use `tempfile::tempdir()` as below):

```rust
/// A locator that names nothing, or a javascript: address, is refused
/// where the script is saved - not discovered halfway through a run.
#[test]
fn a_script_with_an_invalid_action_is_refused_whole() {
    let dir = tempfile::tempdir().unwrap();
    let script: v2_lib::autorun::CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": 501,
        "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] },
            { "step_number": 2, "actions": [
                { "kind": "check_text", "value": "ok" },
                { "kind": "click", "selector": { "name": "Save" } }
            ] }
        ]
    }))
    .unwrap();
    let err = v2_lib::autorun::store::save_scripts_atomically(dir.path(), &[script]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("case 501 step 2 action 2"), "{msg}");
    assert!(v2_lib::autorun::store::load_script(dir.path(), 501).unwrap().is_none(), "nothing may be written");
}
```

- [ ] **Step 7: Run the tests**

One at a time: `cargo test --test browser_actions` (expected: 18 passed), `cargo test --test autorun_store`, `cargo test --test autorun_guide`, `cargo test --test autorun_commands`, `cargo test --test autorun_bridge`. All pass. `autorun_guide` compiles unchanged because `"s".into()` now makes a `Target`.

- [ ] **Step 8: Regenerate the bindings and typecheck**

From `src-tauri/`: `cargo test --test bindings`. Expected: passes, and `src/bindings.ts` now has `Target` and `LocatorStep`, with `selector: Target` on the click, fill and wait_for variants. If specta refuses `Target` (an untagged enum), STOP and report BLOCKED with the exact error; do not hand-edit the bindings.
From the repo root: `npx tsc --noEmit`. Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/browser/actions.rs src-tauri/src/commands/autorun.rs src-tauri/src/autorun/store.rs src-tauri/tests/common/mod.rs src-tauri/tests/browser_actions.rs src-tauri/tests/autorun_store.rs src/bindings.ts
git commit -q -F - <<'EOF'
feat(v2): Auto Run actions use locators, real input and a load-aware navigate

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 6: Expectations that wait, and the guide that teaches them

**Files:**
- Create: `src-tauri/src/browser/expect.rs`
- Modify: `src-tauri/src/browser/mod.rs`, `src-tauri/src/browser/actions.rs` (six variants, their validation and dispatch)
- Modify: `src-tauri/src/autorun/guide.rs` (`ACTION_KINDS`, the actions list, the selectors section, both examples)
- Modify: `src-tauri/tests/common/mod.rs` (two `FakePage` fields)
- Create: `src-tauri/tests/browser_expect.rs`
- Modify: `src-tauri/tests/autorun_guide.rs` (the drift test's samples, one new test)
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: `locator::{resolve, Target, VISIBLE_JS}`, `page::*`, `actions::{ActionOutcome, harness}`, `Driver`.
- Produces (in `v2_lib::browser::expect`):
  - `pub enum Check<'a> { Visible, Hidden, Text(&'a str), ContainsText(&'a str), Count(u32), Attribute { name: &'a str, equals: &'a str } }`
  - `pub async fn expect<D: Driver>(d: &mut D, target: &Target, check: Check<'_>, timeout_ms: u64, poll_ms: u64) -> ActionOutcome`
  - `pub const READ_TEXT_JS`, `READ_ATTR_JS: &str`
- Produces (new `Action` variants, all with `selector: Target` and an optional `timeout_ms: Option<u32>` that defaults to the 10 s expectation timeout):
  - `ExpectVisible`, `ExpectHidden`, `ExpectText { equals: String }`, `ExpectContainsText { value: String }`, `ExpectCount { equals: u32 }`, `ExpectAttribute { name: String, equals: String }`
  - JSON kinds: `expect_visible`, `expect_hidden`, `expect_text`, `expect_contains_text`, `expect_count`, `expect_attribute`

Rules. Each expectation looks again every poll until it holds or the timeout ends, and a failure says what was last seen. Text is compared with whitespace collapsed; `expect_text` is equal and case-sensitive, `expect_contains_text` is contains and case-sensitive. A text or attribute expectation needs the target to be exactly one element. `expect_hidden` holds when nothing the target matches can be seen (which includes "is not there at all"). `expect_count` counts what the target matches, which is visible elements unless the locator says `"visible": false`. `check_text` and `check_url` stay as they are: one look, no waiting.

- [ ] **Step 1: Extend `FakePage`**

In `src-tauri/tests/common/mod.rs` add `READ_ATTR_JS, READ_TEXT_JS` imports:

```rust
use v2_lib::browser::expect::{READ_ATTR_JS, READ_TEXT_JS};
```

add two fields to `FakePage` (and to its `Default`: `texts: vec!["Saved"]`, `attribute: None`):

```rust
    /// Answers to "what does it say", in turn; the last repeats.
    pub texts: Vec<&'static str>,
    pub attribute: Option<&'static str>,
```

and in `driver()`, declare `let mut read = 0usize;` beside `probed`, then add these arms above the catch-all `"Runtime.callFunctionOn"` arm:

```rust
                "Runtime.callFunctionOn" if f == READ_TEXT_JS => {
                    let i = read.min(page.texts.len() - 1);
                    read += 1;
                    json!({ "result": { "value": page.texts[i] } })
                }
                "Runtime.callFunctionOn" if f == READ_ATTR_JS => json!({ "result": { "value": page.attribute } }),
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/tests/browser_expect.rs`:

```rust
//! Expectations look again until they hold or time runs out, and a failure
//! says what was actually seen.

mod common;

use common::FakePage;
use serde_json::json;
use v2_lib::browser::actions::{execute_with, Action};
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 250, nav_ms: 300, poll_ms: 10, highlight_ms: 0 }
}

fn action(v: serde_json::Value) -> Action {
    serde_json::from_value(v).unwrap()
}

#[tokio::test]
async fn visible_holds_once_the_element_shows_up() {
    let mut d = FakePage { appears_on_look: 3, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_visible", "selector": { "css": "#toast" } })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "#toast is visible");
}

#[tokio::test]
async fn visible_fails_with_what_it_saw() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_visible", "selector": { "css": "#toast" } })), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("250ms") && out.detail.contains("#toast") && out.detail.contains("is not on the page"), "{}", out.detail);

    // A legacy selector has no visibility filter of its own, so the
    // expectation checks: there, but not showing, is not visible.
    let mut d = FakePage { visible: false, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_visible", "selector": "#toast" })), &quick()).await;
    assert!(!out.ok && out.detail.contains("is there but cannot be seen"), "{}", out.detail);
}

#[tokio::test]
async fn hidden_holds_when_nothing_can_be_seen() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_hidden", "selector": { "role": "dialog" } })), &quick()).await;
    assert!(out.ok, "{}", out.detail);

    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_hidden", "selector": { "css": "#spinner" } })), &quick()).await;
    assert!(!out.ok && out.detail.contains("is still visible"), "{}", out.detail);
}

/// The text settles after a moment, the way a saved record's name does.
#[tokio::test]
async fn text_is_compared_after_collapsing_whitespace_and_retried() {
    let mut d = FakePage { texts: vec!["Saving...", "  Custom   4-Point\n"], ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_text", "selector": { "css": "h2" }, "equals": "Custom 4-Point" })),
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
}

#[tokio::test]
async fn a_text_mismatch_shows_both_sides() {
    let mut d = FakePage { texts: vec!["Custom 5-Point"], ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_text", "selector": { "css": "h2" }, "equals": "Custom 4-Point" })),
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.contains("\"Custom 4-Point\"") && out.detail.contains("\"Custom 5-Point\""), "{}", out.detail);
}

#[tokio::test]
async fn contains_text_is_a_substring_and_case_matters() {
    let mut d = FakePage { texts: vec!["Rating method saved successfully"], ..FakePage::default() }.driver();
    let yes = action(json!({ "kind": "expect_contains_text", "selector": { "css": ".toast" }, "value": "saved successfully" }));
    assert!(execute_with(&mut d, &yes, &quick()).await.ok);
    let no = action(json!({ "kind": "expect_contains_text", "selector": { "css": ".toast" }, "value": "Saved Successfully" }));
    assert!(!execute_with(&mut d, &no, &quick()).await.ok);
}

/// Reading "the" text of three elements would be a guess.
#[tokio::test]
async fn a_text_expectation_needs_exactly_one_element() {
    let mut d = FakePage { found: 3, ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_text", "selector": { "css": "li" }, "equals": "one" })),
        &quick(),
    )
    .await;
    assert!(!out.ok && out.detail.contains("matched 3 elements"), "{}", out.detail);
}

#[tokio::test]
async fn count_says_how_many_it_found() {
    let mut d = FakePage { found: 3, ..FakePage::default() }.driver();
    let three = action(json!({ "kind": "expect_count", "selector": { "css": "tbody tr" }, "equals": 3 }));
    assert!(execute_with(&mut d, &three, &quick()).await.ok);
    let two = action(json!({ "kind": "expect_count", "selector": { "css": "tbody tr" }, "equals": 2 }));
    let out = execute_with(&mut d, &two, &quick()).await;
    assert!(!out.ok && out.detail.contains("expected 2") && out.detail.contains("counted 3"), "{}", out.detail);
    // Zero is a real expectation: "the row is gone".
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let none = action(json!({ "kind": "expect_count", "selector": { "css": "tbody tr" }, "equals": 0 }));
    assert!(execute_with(&mut d, &none, &quick()).await.ok);
}

#[tokio::test]
async fn attribute_compares_and_reports_a_missing_one() {
    let check = action(json!({ "kind": "expect_attribute", "selector": { "css": "#agree" }, "name": "aria-checked", "equals": "true" }));
    let mut d = FakePage { attribute: Some("true"), ..FakePage::default() }.driver();
    assert!(execute_with(&mut d, &check, &quick()).await.ok);
    let mut d = FakePage { attribute: None, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(!out.ok && out.detail.contains("has no aria-checked"), "{}", out.detail);
}

#[tokio::test]
async fn its_own_timeout_wins_over_the_default() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_visible", "selector": { "css": "#x" }, "timeout_ms": 40 })),
        &quick(),
    )
    .await;
    assert!(out.detail.contains("40ms"), "{}", out.detail);
}

#[test]
fn an_expectation_is_validated_like_any_other_action() {
    assert!(action(json!({ "kind": "expect_visible", "selector": {} })).validate().is_err());
    assert!(action(json!({ "kind": "expect_attribute", "selector": "#a", "name": " ", "equals": "x" })).validate().is_err());
    // An empty expected text is legitimate: "the field is now empty".
    assert!(action(json!({ "kind": "expect_text", "selector": "#a", "equals": "" })).validate().is_ok());
    assert!(action(json!({ "kind": "expect_contains_text", "selector": "#a", "value": "" })).validate().is_err());
}
```

- [ ] **Step 3: Run to verify failure**

`cargo test --test browser_expect` - expected: compile error, `v2_lib::browser::expect` not found.

- [ ] **Step 4: Write the module**

Create `src-tauri/src/browser/expect.rs`:

```rust
//! One loop for every expectation: look, and if it does not hold yet, look
//! again until the timeout. Pages settle; a record's name appears a moment
//! after Save. A single look turns that moment into a false failure, and a
//! fixed sleep turns it into a slow test that still fails on a bad day.
//!
//! When time runs out, the detail says what was last seen. "Expected X" is
//! not evidence; "expected X but saw Y" is.

use super::actions::{harness, ActionOutcome};
use super::cdp::{CdpError, Driver};
use super::locator::{resolve, Target, VISIBLE_JS};
use super::page::{self, Handle};
use serde_json::json;
use std::time::{Duration, Instant};

pub enum Check<'a> {
    Visible,
    Hidden,
    Text(&'a str),
    ContainsText(&'a str),
    Count(u32),
    Attribute { name: &'a str, equals: &'a str },
}

/// `this` is the element. What a person would read: a field's value, a
/// list's chosen option, otherwise the rendered text.
pub const READ_TEXT_JS: &str = r#"function() {
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  if (this instanceof HTMLSelectElement) return norm(this.selectedOptions[0] ? this.selectedOptions[0].label : '');
  if (this instanceof HTMLInputElement || this instanceof HTMLTextAreaElement) return norm(this.value);
  return norm(this.innerText);
}"#;

/// `this` is the element. Argument: the attribute name. null when absent.
pub const READ_ATTR_JS: &str = r#"function(name) { return this.getAttribute(name); }"#;

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn visible<D: Driver>(d: &mut D, h: &Handle) -> Result<bool, CdpError> {
    Ok(page::call_value(d, h, VISIBLE_JS, &[]).await?.as_bool().unwrap_or(false))
}

fn only(handles: &[Handle]) -> Result<&Handle, String> {
    match handles {
        [] => Err("is not on the page".to_string()),
        [one] => Ok(one),
        many => Err(format!("matched {} elements - narrow it, or add nth", many.len())),
    }
}

/// One look. `Ok(Ok(detail))` holds; `Ok(Err(why))` does not hold yet.
async fn look<D: Driver>(
    d: &mut D,
    target: &Target,
    check: &Check<'_>,
) -> Result<Result<String, String>, CdpError> {
    let what = target.describe();
    let handles = resolve(d, target).await?;
    Ok(match check {
        Check::Visible => match handles.first() {
            None => Err("is not on the page".to_string()),
            Some(h) => {
                if visible(d, h).await? {
                    Ok(format!("{what} is visible"))
                } else {
                    Err("is there but cannot be seen".to_string())
                }
            }
        },
        Check::Hidden => {
            let mut seen = false;
            for h in &handles {
                if visible(d, h).await? {
                    seen = true;
                    break;
                }
            }
            if seen {
                Err("is still visible".to_string())
            } else {
                Ok(format!("{what} is hidden"))
            }
        }
        Check::Count(n) => {
            if handles.len() as u32 == *n {
                Ok(format!("counted {n}: {what}"))
            } else {
                Err(format!("expected {n}, counted {}", handles.len()))
            }
        }
        Check::Text(want) => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let got = page::call_value(d, h, READ_TEXT_JS, &[]).await?;
                let (got, want) = (collapse(got.as_str().unwrap_or("")), collapse(want));
                if got == want {
                    Ok(format!("{what} says {want:?}"))
                } else {
                    Err(format!("expected text {want:?} but saw {got:?}"))
                }
            }
        },
        Check::ContainsText(want) => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let got = page::call_value(d, h, READ_TEXT_JS, &[]).await?;
                let (got, want) = (collapse(got.as_str().unwrap_or("")), collapse(want));
                if got.contains(&want) {
                    Ok(format!("{what} contains {want:?}"))
                } else {
                    Err(format!("expected it to contain {want:?} but saw {got:?}"))
                }
            }
        },
        Check::Attribute { name, equals } => match only(&handles) {
            Err(why) => Err(why),
            Ok(h) => {
                let got = page::call_value(d, h, READ_ATTR_JS, &[json!(name)]).await?;
                match got.as_str() {
                    None => Err(format!("has no {name} attribute")),
                    Some(v) if v == *equals => Ok(format!("{what} has {name}={equals:?}")),
                    Some(v) => Err(format!("expected {name}={equals:?} but saw {v:?}")),
                }
            }
        },
    })
}

pub async fn expect<D: Driver>(
    d: &mut D,
    target: &Target,
    check: Check<'_>,
    timeout_ms: u64,
    poll_ms: u64,
) -> ActionOutcome {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        page::release(d).await;
        let why = match look(d, target, &check).await {
            Ok(Ok(detail)) => return ActionOutcome::passed(detail),
            Ok(Err(why)) => why,
            Err(e) if e.is_transient() => e.to_string(),
            Err(e) => return harness(e),
        };
        if Instant::now() >= deadline {
            return ActionOutcome::failed(format!(
                "waited {timeout_ms}ms: {} {why}",
                target.describe()
            ));
        }
        tokio::time::sleep(Duration::from_millis(poll_ms)).await;
    }
}
```

Add `pub mod expect;` to `src-tauri/src/browser/mod.rs` after `pub mod input;`.

- [ ] **Step 5: Add the six actions**

In `src-tauri/src/browser/actions.rs`:

Add `use super::expect::{self, Check};` to the imports.

Append to `enum Action`, after `CheckUrl`:

```rust
    ExpectVisible {
        selector: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectHidden {
        selector: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectText {
        selector: Target,
        equals: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectContainsText {
        selector: Target,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectCount {
        selector: Target,
        equals: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectAttribute {
        selector: Target,
        name: String,
        equals: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
```

In `Action::validate`, replace the arm that starts `Action::Click { selector }` and the final `Action::CheckText { .. } | Action::CheckUrl { .. } => Ok(()),` arm with:

```rust
            Action::Click { selector }
            | Action::Fill { selector, .. }
            | Action::WaitFor { selector, .. }
            | Action::ExpectVisible { selector, .. }
            | Action::ExpectHidden { selector, .. }
            | Action::ExpectText { selector, .. }
            | Action::ExpectCount { selector, .. } => selector.validate(),
            Action::ExpectContainsText { selector, value, .. } => {
                if value.trim().is_empty() {
                    return Err("expect_contains_text has an empty value - everything contains nothing".to_string());
                }
                selector.validate()
            }
            Action::ExpectAttribute { selector, name, .. } => {
                if name.trim().is_empty() {
                    return Err("expect_attribute has an empty name".to_string());
                }
                selector.validate()
            }
            Action::CheckText { value } if value.trim().is_empty() => {
                Err("check_text has an empty value".to_string())
            }
            Action::CheckUrl { contains } if contains.trim().is_empty() => {
                Err("check_url has an empty value".to_string())
            }
            Action::CheckText { .. } | Action::CheckUrl { .. } => Ok(()),
```

(That replaces the existing `CheckText`/`CheckUrl` arms too; the `Navigate` arms above them stay.)

In `run`, add after the `Action::CheckUrl` arm:

```rust
        Action::ExpectVisible { selector, timeout_ms } => {
            expect::expect(d, selector, Check::Visible, wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectHidden { selector, timeout_ms } => {
            expect::expect(d, selector, Check::Hidden, wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectText { selector, equals, timeout_ms } => {
            expect::expect(d, selector, Check::Text(equals), wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectContainsText { selector, value, timeout_ms } => {
            expect::expect(d, selector, Check::ContainsText(value), wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectCount { selector, equals, timeout_ms } => {
            expect::expect(d, selector, Check::Count(*equals), wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectAttribute { selector, name, equals, timeout_ms } => {
            expect::expect(d, selector, Check::Attribute { name, equals }, wait(timeout_ms, timing), timing.poll_ms).await
        }
```

and above `run`:

```rust
fn wait(own: &Option<u32>, timing: &Timing) -> u64 {
    own.map(u64::from).unwrap_or(timing.expect_ms)
}
```

`expect.rs` imports from `actions.rs` and `actions.rs` from `expect.rs`. Rust allows that between modules of one crate.

- [ ] **Step 6: Run the expectation tests**

`cargo test --test browser_expect` - expected: 11 passed. Then `cargo test --test browser_actions` - still 18 passed.

- [ ] **Step 7: Update the guide and its drift test**

In `src-tauri/src/autorun/guide.rs`, `ACTION_KINDS` becomes:

```rust
pub const ACTION_KINDS: &[&str] = &[
    "navigate",
    "click",
    "fill",
    "wait_for",
    "check_text",
    "check_url",
    "expect_visible",
    "expect_hidden",
    "expect_text",
    "expect_contains_text",
    "expect_count",
    "expect_attribute",
];
```

In the guide text, replace everything from the line `## The actions` down to (not including) `## Three things that make a source-derived selector wrong` with:

```text
## The actions

Each step of the test case becomes one entry with a `step_number` and a
list of `actions`, run in order:

- `{ "kind": "navigate", "url": "https://..." }` - waits for the page to load
- `{ "kind": "click", "selector": ... }`
- `{ "kind": "fill", "selector": ..., "value": "..." }` - also picks an
  option in a native list, by its label
- `{ "kind": "wait_for", "selector": ..., "timeout_ms": 5000 }`
- `{ "kind": "expect_visible", "selector": ... }`
- `{ "kind": "expect_hidden", "selector": ... }` - gone counts as hidden
- `{ "kind": "expect_text", "selector": ..., "equals": "..." }`
- `{ "kind": "expect_contains_text", "selector": ..., "value": "..." }`
- `{ "kind": "expect_count", "selector": ..., "equals": 3 }`
- `{ "kind": "expect_attribute", "selector": ..., "name": "aria-checked", "equals": "true" }`
- `{ "kind": "check_text", "value": "..." }`  - is this text anywhere on the page, right now?
- `{ "kind": "check_url", "contains": "..." }` - is this in the address, right now?

There is nothing else. An action of any other kind is rejected.

`click` and `fill` wait up to 15 seconds for their element to be usable:
the only match, visible, not moving, enabled, and not covered by
something else. Then they use the real mouse and keyboard. If the wait
runs out, the outcome says which of those was the problem. You do not
need a `wait_for` in front of them.

Every `expect_` action looks again until it holds, for up to 10 seconds
(add `"timeout_ms"` to change that), and a failure says what it actually
saw. Prefer them to `check_text`, which looks once and cannot tell you
where on the page the words were. Text is compared with runs of
whitespace collapsed, and case matters.

Never add a fixed pause. There is no action for one, on purpose.

## Selectors

A `selector` says which element. The best form is a locator, because it
says it the way the test case does - by what the control IS and what it
is CALLED:

- `{ "role": "button", "name": "Add Method" }`
- `{ "role": "textbox", "name": "Method Name" }`
- `{ "role": "dialog", "name": "Add Rating Method" }`
- `{ "text": "Step 1 of 6" }` - the deepest element showing those words
- `{ "css": "#save" }` - for a real id or data-testid

`role` is the ARIA role (button, link, textbox, checkbox, combobox,
searchbox, dialog, heading, row, cell...) and `name` is the
accessible name: a button's words, a field's label. Both come from the browser's own
accessibility tree, so they match what a screen reader would announce.

`name` and `text` match loosely - contains, ignoring case and extra
spaces. Add `"exact": true` when a near miss exists ("Save" and "Save as
new").

A list narrows from left to right, each entry searching inside the one
before it:

    [ { "role": "dialog", "name": "Add Rating Method" },
      { "role": "button", "name": "Add Method" } ]

Only what a person could SEE is matched. Applications keep hidden copies
of dialogs and menus in the page; they are ignored unless an entry says
`"visible": false`.

A locator must end up meaning exactly one element. If it matches several,
the action fails and says how many: narrow it with a list, `"exact"`, or
`"nth"` (zero-based: `{ "css": "tbody tr", "nth": 0 }` is the first row).
Only `expect_count` and `expect_hidden` are happy with many.

A misspelt field is rejected, not ignored. Each entry takes exactly one
of `role`, `text` or `css`.

A plain string still works and means what it always has: a CSS selector
(first match), or `text=Some Words` (last match). It has no visibility
filter, so prefer a locator in anything new.

Prefer, in this order: `role` with `name`, then a `data-testid` or `id`
through `css`, then `text`. Words are the most readable and the most
fragile - they break when the wording changes, which is exactly when a
human would notice anyway.

If you can read the application's source, USE IT to find selectors. That
is what source access is for: the real label of a field or id of a button
beats a guess every time. Read the component, take it, move on.

```

In the `## Three things...` section, item 3's last sentence ("Put a `wait_for` on something inside the rendered result before acting on it - never a bare `navigate` followed by a `click`.") becomes:

```text
   absent at page load. `click`, `fill` and every `expect_` action wait
   for them; `check_text` does not, so follow a navigation with an
   `expect_visible` on something inside the rendered result.
```

In `## Where each fact is allowed to come from`, the sentence "`check_text` and `check_url` come from there and nowhere else." becomes "Every `expect_` and `check_` action comes from there and nowhere else."

In `## Steps you cannot automate`, "Do not invent a `check_text`" becomes "Do not invent an expectation".

In BOTH JSON examples (the worked example and the save payload), replace each `{ "kind": "check_text", "value": "Dashboard" }` with:

```json
{ "kind": "expect_visible", "selector": { "role": "heading", "name": "Dashboard" } }
```

and each `{ "kind": "click", "selector": "text=Sign in" }` with:

```json
{ "kind": "click", "selector": { "role": "button", "name": "Sign in" } }
```

and each `{ "kind": "check_text", "value": "Objectives" }` with:

```json
{ "kind": "expect_visible", "selector": { "role": "heading", "name": "Objectives" } }
```

In `## When a script is already failing`, replace the first paragraph with:

```text
The runner reports each action's outcome in plain words, e.g.
"waited 15000ms: button "Save" is covered by div.modal-backdrop" or
"expected text "Saved" but saw "Saving..."". That names the problem
directly - read the source again for the right locator and save a
corrected script. A failed action also keeps a screenshot the person can
open. Do not weaken a check to make a run go green.
```

In `src-tauri/tests/autorun_guide.rs`, extend `samples` in `the_guide_names_every_action_the_executor_can_run` with (after the `CheckUrl` sample):

```rust
        Action::ExpectVisible { selector: "s".into(), timeout_ms: None },
        Action::ExpectHidden { selector: "s".into(), timeout_ms: None },
        Action::ExpectText { selector: "s".into(), equals: "v".into(), timeout_ms: None },
        Action::ExpectContainsText { selector: "s".into(), value: "v".into(), timeout_ms: None },
        Action::ExpectCount { selector: "s".into(), equals: 1, timeout_ms: None },
        Action::ExpectAttribute { selector: "s".into(), name: "n".into(), equals: "v".into(), timeout_ms: None },
```

and add:

```rust
/// The guide has to teach locators, or an assistant keeps writing the
/// fragile string form. And it must never suggest a fixed pause.
#[test]
fn the_guide_teaches_locators_and_forbids_pauses() {
    let g = autorun_guide();
    for term in ["\"role\"", "\"name\"", "\"exact\"", "\"nth\"", "\"visible\": false", "accessible name"] {
        assert!(g.contains(term), "the guide never mentions {term}");
    }
    assert!(g.contains("Never add a fixed pause"), "the guide must rule out sleeps");
    assert!(!g.contains('\u{2014}'), "no em dashes in text an assistant reads");
    // Every example that uses a locator must validate, not just parse.
    let example = first_balanced(&g, g.find("## A worked example").unwrap(), '[', ']');
    let steps: Vec<v2_lib::autorun::StepScript> = serde_json::from_str(example).unwrap();
    for s in &steps {
        for a in &s.actions {
            a.validate().unwrap_or_else(|e| panic!("the worked example has an invalid action: {e}"));
        }
    }
}
```

- [ ] **Step 8: Run the guide tests, regenerate bindings, typecheck**

`cargo test --test autorun_guide` - expected: all pass (the two example-parsing tests prove the edited JSON is still valid).
`cargo test --test bindings`, then from the repo root `npx tsc --noEmit`. Expected: pass and clean.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/browser/expect.rs src-tauri/src/browser/mod.rs src-tauri/src/browser/actions.rs src-tauri/src/autorun/guide.rs src-tauri/tests/common/mod.rs src-tauri/tests/browser_expect.rs src-tauri/tests/autorun_guide.rs src/bindings.ts
git commit -q -F - <<'EOF'
feat(v2): Auto Run expectations wait until they hold and say what they saw

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 7: A screenshot whenever an action fails

**Files:**
- Modify: `src-tauri/src/browser/page.rs` (`screenshot`)
- Modify: `src-tauri/src/browser/actions.rs` (`ActionOutcome.screenshot`)
- Modify: `src-tauri/src/autorun/store.rs` (`save_shot`, `save_shot_keeping`, `load_shot`, `safe_shot_name`)
- Modify: `src-tauri/src/commands/autorun.rs` (`auto_run_step` captures; new `auto_run_shot`)
- Modify: `src-tauri/src/lib.rs:149` (register the command)
- Modify: `src/screens/AutoRun/RunPane.tsx`, `src/screens/AutoRun/RunPane.test.tsx`
- Test: `src-tauri/tests/browser_page.rs`, `src-tauri/tests/autorun_store.rs`
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: `Driver`, `CdpError`, `store::configured_root`, `commands::autorun::root`.
- Produces:
  - `page::screenshot<D: Driver>(d: &mut D) -> Result<Vec<u8>, CdpError>` (JPEG bytes)
  - `ActionOutcome.screenshot: Option<String>` - a file name inside `<autorun root>/shots/`, never a path and never the image itself. Absent from JSON when `None`, so every run saved before this still loads.
  - `store::save_shot(root: &Path, bytes: &[u8]) -> Result<String, String>` (keeps the newest 200), `store::save_shot_keeping(root, bytes, keep: usize)`, `store::load_shot(root: &Path, name: &str) -> Result<Vec<u8>, String>`, `store::safe_shot_name(name: &str) -> bool`
  - Command `auto_run_shot(name: String) -> Result<String, String>` returning a `data:image/jpeg;base64,...` URL; TypeScript `commands.autoRunShot(name)`

- [ ] **Step 1: Write the failing Rust tests**

Append to `src-tauri/tests/browser_page.rs`:

```rust
#[tokio::test]
async fn a_screenshot_comes_back_as_jpeg_bytes() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "data": "/9j/4AAQ" })));
    let bytes = page::screenshot(&mut d).await.unwrap();
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "JPEG files start FF D8 FF");
    let p = &d.calls_to("Page.captureScreenshot")[0];
    assert_eq!(p["format"], "jpeg");
    assert_eq!(p["quality"], 60);
}

#[tokio::test]
async fn a_screenshot_that_is_not_base64_is_an_error() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "data": "not base64 !!" })));
    assert!(page::screenshot(&mut d).await.is_err());
}
```

Append to `src-tauri/tests/autorun_store.rs`:

```rust
use v2_lib::autorun::store::{load_shot, safe_shot_name, save_shot, save_shot_keeping};

#[test]
fn a_shot_is_saved_under_a_safe_name_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let name = save_shot(dir.path(), &[0xFF, 0xD8, 0xFF, 0x00]).unwrap();
    assert!(safe_shot_name(&name), "{name}");
    assert!(dir.path().join("shots").join(&name).is_file());
    assert_eq!(load_shot(dir.path(), &name).unwrap(), vec![0xFF, 0xD8, 0xFF, 0x00]);
}

/// The name arrives from the webview. It must never be able to read a file
/// outside the shots folder.
#[test]
fn a_shot_name_cannot_leave_the_shots_folder() {
    for bad in ["../runs/run-1.json", "shot-1.jpg/../../x", "C:\\x.jpg", "shot-1.png", "", "shot-..jpg", "x.jpg"] {
        assert!(!safe_shot_name(bad), "{bad:?} was accepted");
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(load_shot(dir.path(), "../runs/run-1.json").is_err());
}

/// Screenshots are evidence for the run in front of the person, not an
/// archive. Only the newest are kept, so the folder (and the app's backup
/// of it) cannot grow without limit.
#[test]
fn only_the_newest_shots_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut names = vec![];
    for i in 0..5u8 {
        names.push(save_shot_keeping(dir.path(), &[i], 3).unwrap());
    }
    let mut left: Vec<String> = std::fs::read_dir(dir.path().join("shots"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    left.sort();
    assert_eq!(left, names[2..].to_vec(), "the three newest stay");
}
```

- [ ] **Step 2: Run to verify failure**

`cargo test --test browser_page` and then `cargo test --test autorun_store` - expected: compile errors (`page::screenshot`, `save_shot` not found).

- [ ] **Step 3: Implement the Rust side**

In `src-tauri/src/browser/page.rs`, add `use base64::Engine;` to the imports and append:

```rust
/// What the person would see right now, as JPEG bytes.
pub async fn screenshot<D: Driver>(d: &mut D) -> Result<Vec<u8>, CdpError> {
    let method = "Page.captureScreenshot";
    let r = d.call(method, json!({ "format": "jpeg", "quality": 60 })).await?;
    let data = r["data"].as_str().ok_or_else(|| CdpError::Protocol {
        method: method.to_string(),
        message: "the browser returned no image".to_string(),
    })?;
    base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| CdpError::Protocol {
        method: method.to_string(),
        message: format!("the image could not be decoded: {e}"),
    })
}
```

In `src-tauri/src/browser/actions.rs`, `ActionOutcome` becomes:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ActionOutcome {
    pub ok: bool,
    pub detail: String,
    /// A file in the autorun `shots` folder, taken when the action failed.
    /// A name, never a path and never the image: run files stay small, and
    /// the webview cannot ask for anything outside that folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
}

impl ActionOutcome {
    pub fn passed(detail: impl Into<String>) -> Self {
        ActionOutcome { ok: true, detail: detail.into(), screenshot: None }
    }
    pub fn failed(detail: impl Into<String>) -> Self {
        ActionOutcome { ok: false, detail: detail.into(), screenshot: None }
    }
}
```

and the `CheckUrl` arm's struct literal becomes:

```rust
            Ok(v) => {
                let href = v.as_str().unwrap_or("");
                let detail = format!("url is {href}");
                if href.contains(contains.as_str()) {
                    ActionOutcome::passed(detail)
                } else {
                    ActionOutcome::failed(detail)
                }
            }
```

Find every other literal: `grep -rn "ActionOutcome {" src-tauri/src src-tauri/tests`. Each one outside `actions.rs` gains `screenshot: None`.

In `src-tauri/src/autorun/store.rs`, append:

```rust
/// How many failure screenshots are kept. They are evidence for the run in
/// front of the person, not an archive.
const MAX_SHOTS: usize = 200;

static SHOT_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn shots_dir(root: &Path) -> PathBuf {
    root.join("shots")
}

/// `shot-<digits>-<digits>.jpg` and nothing else. The name comes back from
/// the webview, so it is checked, not trusted.
pub fn safe_shot_name(name: &str) -> bool {
    let Some(middle) = name.strip_prefix("shot-").and_then(|n| n.strip_suffix(".jpg")) else {
        return false;
    };
    let mut parts = middle.split('-');
    let ok = |p: Option<&str>| p.is_some_and(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()));
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

pub fn save_shot(root: &Path, bytes: &[u8]) -> Result<String, String> {
    save_shot_keeping(root, bytes, MAX_SHOTS)
}

/// Save, then drop the oldest beyond `keep`. Names sort by time: epoch
/// milliseconds, then a zero-padded counter for shots in the same
/// millisecond.
pub fn save_shot_keeping(root: &Path, bytes: &[u8], keep: usize) -> Result<String, String> {
    let dir = shots_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let seq = SHOT_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let name = format!("shot-{ms}-{seq:06}.jpg");
    std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;

    let mut all: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| safe_shot_name(n))
        .collect();
    all.sort();
    if all.len() > keep {
        for old in &all[..all.len() - keep] {
            let _ = std::fs::remove_file(dir.join(old));
        }
    }
    Ok(name)
}

pub fn load_shot(root: &Path, name: &str) -> Result<Vec<u8>, String> {
    if !safe_shot_name(name) {
        return Err(format!("{name:?} is not a screenshot name"));
    }
    std::fs::read(shots_dir(root).join(name)).map_err(|e| format!("that screenshot is gone: {e}"))
}
```

In `src-tauri/src/commands/autorun.rs`, add `use crate::browser::page;` and `use base64::Engine;` to the imports, and replace `auto_run_step` with:

```rust
/// A picture of the page at the moment an action failed. Best effort: a
/// browser that cannot take one (it has gone away) just means no picture -
/// the failure is already reported in words.
async fn shot_of_failure(cdp: &mut Cdp, app: &tauri::AppHandle) -> Option<String> {
    let bytes = page::screenshot(cdp).await.ok()?;
    let root = root(app).ok()?;
    store::save_shot(&root, &bytes).ok()
}

/// Run one step's actions in order and report every outcome. Actions
/// after a failure still run: the watcher learns more from "the click
/// worked, the check did not" than from a run that stops at the first
/// red.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_step(
    app: tauri::AppHandle,
    step: StepScript,
) -> Result<Vec<ActionOutcome>, String> {
    let mut slot = SESSION.lock().await;
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    let mut out = Vec::new();
    for action in &step.actions {
        let mut outcome = execute(&mut session.cdp, action).await;
        if !outcome.ok {
            outcome.screenshot = shot_of_failure(&mut session.cdp, &app).await;
        }
        out.push(outcome);
    }
    Ok(out)
}

/// One failure screenshot as a data URL the webview can show. The name is
/// checked in `store::load_shot`; nothing outside the shots folder can be
/// read through here.
#[tauri::command]
#[specta::specta]
pub fn auto_run_shot(app: tauri::AppHandle, name: String) -> Result<String, String> {
    let bytes = store::load_shot(&root(&app)?, &name)?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}
```

In `src-tauri/src/lib.rs`, after `autorun::auto_run_new_id,` add:

```rust
            autorun::auto_run_shot,
```

- [ ] **Step 4: Run the Rust tests and regenerate bindings**

One at a time: `cargo test --test browser_page` (9 passed), `cargo test --test autorun_store`, `cargo test --test browser_actions`, `cargo test --test browser_expect`, then `cargo test --test bindings`. Confirm with `grep -n "autoRunShot\|screenshot" src/bindings.ts`: the command exists and `ActionOutcome` has `screenshot?: string | null`. The frontend's `autoRunStep(step)` signature is unchanged, because an `AppHandle` parameter is supplied by Tauri and never appears in the bindings.

- [ ] **Step 5: Write the failing frontend test**

Append to `src/screens/AutoRun/RunPane.test.tsx`:

```tsx
test("a failed action offers the screenshot taken when it failed", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS };
    if (cmd === "auto_run_new_id") return "run-1";
    if (cmd === "auto_run_step")
      return [
        { ok: true, detail: "loaded https://app.example/" },
        { ok: false, detail: 'waited 15000ms: button "Save" not found', screenshot: "shot-1-000001.jpg" },
      ];
    if (cmd === "auto_run_shot") {
      expect((args as { name: string }).name).toBe("shot-1-000001.jpg");
      return "data:image/jpeg;base64,AAAA";
    }
    return null;
  });
  renderPane([{ id: 1, title: "Valid login" }]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  const run = await screen.findByRole("button", { name: "Run step 1" });
  await waitFor(() => expect(run).toBeEnabled());
  fireEvent.click(run);

  expect(await screen.findByText(/button "Save" not found/)).toBeInTheDocument();
  // Only the failed action has one.
  expect(screen.getAllByRole("button", { name: "View screenshot" })).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "View screenshot" }));
  const img = await screen.findByRole("img", { name: "Screenshot of the failed action" });
  expect(img).toHaveAttribute("src", "data:image/jpeg;base64,AAAA");
});
```

Run `npx vitest run src/screens/AutoRun/RunPane.test.tsx`. Expected: this test fails (no "View screenshot" button). If it fails earlier than that, read `runStep` in `RunPane.tsx` and match the test's setup to how a step is actually enabled; do not change production behaviour to suit the test.

- [ ] **Step 6: Show the screenshot**

In `src/screens/AutoRun/RunPane.tsx` (`Modal`, `toast` and `unwrapStr` are already imported):

Beside the component's other `useState` calls add:

```tsx
  // The failure screenshot on show, as a data URL, or null.
  const [shot, setShot] = useState<string | null>(null);
  const openShot = (name: string) =>
    unwrapStr(commands.autoRunShot(name))
      .then(setShot)
      .catch((e) => toast.error(`Could not open the screenshot: ${e.message ?? e}`));
```

Replace the outcome paragraph (currently lines 342-349) with:

```tsx
              {(results[s.step_number] ?? []).map((o, i) => (
                <p
                  key={i}
                  className={cn("mt-1 text-xs", o.ok ? "text-muted" : "text-danger")}
                >
                  {o.detail}
                  {o.screenshot && (
                    <button
                      type="button"
                      className="ml-2 text-muted underline hover:text-accent"
                      onClick={() => openShot(o.screenshot!)}
                    >
                      View screenshot
                    </button>
                  )}
                </p>
              ))}
```

and, as the last child of the component's root element, add:

```tsx
      {shot && (
        <Modal onClose={() => setShot(null)} className="max-h-[90vh] max-w-5xl overflow-auto p-3">
          <img src={shot} alt="Screenshot of the failed action" className="max-w-full" />
        </Modal>
      )}
```

- [ ] **Step 7: Run the frontend tests**

`npx vitest run src/screens/AutoRun src/ui-consistency.test.ts`, then `npx tsc --noEmit`. Expected: all pass, clean.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/browser/page.rs src-tauri/src/browser/actions.rs src-tauri/src/autorun/store.rs src-tauri/src/commands/autorun.rs src-tauri/src/lib.rs src-tauri/tests/browser_page.rs src-tauri/tests/autorun_store.rs src/bindings.ts src/screens/AutoRun/RunPane.tsx src/screens/AutoRun/RunPane.test.tsx
git commit -q -F - <<'EOF'
feat(v2): Auto Run keeps a screenshot of the page whenever an action fails

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 8: Prove it against a real browser, then the full gates

Everything so far is tested against fakes that answer the way this plan believes Chrome answers. This task checks those beliefs. The live tests are `#[ignore]`d so the normal suite never opens a browser; they are run once here, deliberately.

**Files:**
- Modify: `src-tauri/src/browser/launch.rs` (`launch_with`)
- Modify: `src-tauri/src/browser/mod.rs` (header comment)
- Create: `src-tauri/tests/fixtures/autorun-live.html`
- Create: `src-tauri/tests/browser_live.rs`
- Modify: `src-tauri/tests/browser_launch.rs` (one test)

**Interfaces:**
- Produces: `launch::args_with(port: u16, profile_dir: &Path, extra: &[&str]) -> Vec<String>`; `launch::launch_with(which: Browser, extra_args: &[&str]) -> Result<LaunchedBrowser, String>`; `launch_in(which)` is `launch_with(which, &[])`. `launch_args` is unchanged and still never hides the window.

- [ ] **Step 1: `launch_with`**

Add to `src-tauri/tests/browser_launch.rs`:

```rust
/// Extra switches are for the live tests only (they run headless so they
/// do not take over the desktop). The app's own arguments must stay
/// exactly as they were, with the start page last.
#[test]
fn extra_switches_go_before_the_start_page() {
    let dir = std::path::PathBuf::from("C:/tmp/p");
    let args = v2_lib::browser::launch::args_with(9222, &dir, &["--headless=new"]);
    assert_eq!(args.last().map(String::as_str), Some("about:blank"));
    assert!(args.contains(&"--headless=new".to_string()));
    assert_eq!(
        v2_lib::browser::launch::args_with(9222, &dir, &[]),
        v2_lib::browser::launch::launch_args(9222, &dir)
    );
}
```

In `src-tauri/src/browser/launch.rs`, add below `launch_args`:

```rust
/// `launch_args` plus extra switches, kept in front of the start page
/// (Chromium treats everything after the first non-switch as a URL).
pub fn args_with(port: u16, profile_dir: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = launch_args(port, profile_dir);
    let start_page = args.pop();
    args.extend(extra.iter().map(|s| s.to_string()));
    args.extend(start_page);
    args
}
```

rename the body of `launch_in` into `launch_with` and leave `launch_in` as a one-liner:

```rust
/// Start the chosen browser. The error names the browser the person
/// asked for, so "not found" is actionable rather than a mystery.
pub fn launch_in(which: Browser) -> Result<LaunchedBrowser, String> {
    launch_with(which, &[])
}

/// The same, with extra switches. The app never passes any: the window is
/// always visible. The live tests pass `--headless=new`.
pub fn launch_with(which: Browser, extra_args: &[&str]) -> Result<LaunchedBrowser, String> {
```

and inside it change `.args(launch_args(port, &profile_dir))` to `.args(args_with(port, &profile_dir, extra_args))`.

Run `cargo test --test browser_launch`. Expected: all pass, including the existing never-headless test.

- [ ] **Step 2: The fixture page**

Create `src-tauri/tests/fixtures/autorun-live.html`. It reproduces, in one page, the situations the research found in a real application: a hidden twin of a button, a hidden copy of a dialog, a control that is disabled and covered for the first 700 ms, a field that reacts to input events, a native list, an alert.

```html
<!doctype html>
<meta charset="utf-8">
<title>Auto Run live fixture</title>
<style>
  #overlay { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.4); }
</style>
<main>
  <h1>Fixture</h1>
  <p id="count">clicked 0</p>
  <button id="save" onclick="bump()">Save changes</button>
  <button style="display:none" onclick="bump()">Save changes</button>
  <button id="late" disabled onclick="bump()">Late button</button>

  <label>Method Name * <input id="name" oninput="document.getElementById('echo').textContent = this.value"></label>
  <p id="echo"></p>

  <label>Type
    <select id="type" onchange="document.getElementById('picked').textContent = this.value">
      <option value="">Select type</option>
      <option value="n">Numeric</option>
      <option value="t">Text</option>
    </select>
  </label>
  <p id="picked"></p>

  <div role="dialog" aria-label="Add Rating Method">
    <button onclick="document.getElementById('which').textContent = 'dialog button'">Add Method</button>
  </div>
  <div role="dialog" aria-label="Add Rating Method" style="display:none">
    <button>Add Method</button>
  </div>
  <button onclick="document.getElementById('which').textContent = 'page button'">Add Method</button>
  <p id="which"></p>

  <button id="alerter" onclick="alert('Saved!'); bump()">Alert me</button>

  <ul id="rows"><li>one</li><li>two</li><li>three</li></ul>
  <div id="spinner">Loading</div>
</main>
<div id="overlay"></div>
<script>
  let n = 0;
  function bump() { n++; document.getElementById('count').textContent = 'clicked ' + n; }
  setTimeout(() => {
    document.getElementById('late').disabled = false;
    document.getElementById('overlay').remove();
    document.getElementById('spinner').remove();
  }, 700);
</script>
```

- [ ] **Step 3: The live tests**

Create `src-tauri/tests/browser_live.rs`:

```rust
//! The same rules as the unit tests, against a REAL browser. Ignored by
//! default: they start (headless) Edge. Run them on purpose:
//!
//!   cargo test --test browser_live -- --ignored --test-threads=1
//!
//! One at a time, because each starts its own browser.

use serde_json::json;
use v2_lib::browser::actions::{execute_with, Action, ActionOutcome};
use v2_lib::browser::cdp::Cdp;
use v2_lib::browser::launch::{launch_with, Browser, LaunchedBrowser};
use v2_lib::browser::page;
use v2_lib::browser::timing::Timing;

struct Live {
    browser: LaunchedBrowser,
    cdp: Cdp,
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.browser.child.kill();
        let _ = std::fs::remove_dir_all(&self.browser.profile_dir);
    }
}

fn fixture_url() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/autorun-live.html").replace('\\', "/");
    // The repo path has spaces in it on this machine.
    format!("file:///{}", path.trim_start_matches('/').replace(' ', "%20"))
}

/// Short enough that a failing test fails fast, long enough for the
/// fixture's 700 ms of being disabled and covered.
fn timing() -> Timing {
    Timing { action_ms: 4000, expect_ms: 3000, nav_ms: 15000, poll_ms: 100, highlight_ms: 0 }
}

async fn run(live: &mut Live, action: serde_json::Value) -> ActionOutcome {
    let action: Action = serde_json::from_value(action).expect("the test wrote an invalid action");
    execute_with(&mut live.cdp, &action, &timing()).await
}

async fn open() -> Live {
    let browser = launch_with(Browser::Edge, &["--headless=new"]).expect("Edge did not start");
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let cdp = Cdp::connect(browser.port).await.expect("could not connect to Edge");
    let mut live = Live { browser, cdp };
    let out = run(&mut live, json!({ "kind": "navigate", "url": fixture_url() })).await;
    assert!(out.ok, "the fixture did not load: {}", out.detail);
    live
}

fn must(out: ActionOutcome) {
    assert!(out.ok, "{}", out.detail);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_role_locator_ignores_the_hidden_twin_and_really_clicks() {
    let mut live = open().await;
    // Two buttons are called "Save changes"; one is display:none. If the
    // hidden one counted, this would fail with "matched 2 elements".
    must(run(&mut live, json!({ "kind": "click", "selector": { "role": "button", "name": "save changes" } })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#count", "equals": "clicked 1" })).await);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_click_waits_out_disabled_and_covered() {
    let mut live = open().await;
    // #late is disabled and under a full-page overlay for 700 ms.
    must(run(&mut live, json!({ "kind": "click", "selector": { "css": "#late" } })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#count", "equals": "clicked 1" })).await);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn typing_reaches_the_page_as_real_input_and_can_be_cleared() {
    let mut live = open().await;
    let field = json!({ "role": "textbox", "name": "Method Name" });
    must(run(&mut live, json!({ "kind": "fill", "selector": field, "value": "Custom 4-Point" })).await);
    // #echo is written by the field's own oninput handler.
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#echo", "equals": "Custom 4-Point" })).await);
    must(run(&mut live, json!({ "kind": "fill", "selector": field, "value": "Second" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#echo", "equals": "Second" })).await);
    must(run(&mut live, json!({ "kind": "fill", "selector": field, "value": "" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#echo", "equals": "" })).await);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_native_list_is_chosen_by_its_label() {
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "fill", "selector": { "css": "#type" }, "value": "Numeric" })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#picked", "equals": "n" })).await);
    let out = run(&mut live, json!({ "kind": "fill", "selector": { "css": "#type" }, "value": "Nope" })).await;
    assert!(!out.ok && out.detail.contains("no option"), "{}", out.detail);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_chain_reaches_the_button_inside_the_dialog() {
    let mut live = open().await;
    // On its own the name is ambiguous: a dialog button and a page button.
    let bare = run(&mut live, json!({ "kind": "click", "selector": { "role": "button", "name": "Add Method", "exact": true } })).await;
    assert!(!bare.ok && bare.detail.contains("matched 2 elements"), "{}", bare.detail);
    must(run(&mut live, json!({ "kind": "click", "selector": [
        { "role": "dialog", "name": "Add Rating Method" },
        { "role": "button", "name": "Add Method" }
    ] })).await);
    must(run(&mut live, json!({ "kind": "expect_text", "selector": "#which", "equals": "dialog button" })).await);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn an_alert_does_not_freeze_the_run() {
    let mut live = open().await;
    let out = run(&mut live, json!({ "kind": "click", "selector": { "css": "#alerter" } })).await;
    assert!(out.ok, "{}", out.detail);
    // The very next call would hang forever on the old client.
    let next = run(&mut live, json!({ "kind": "expect_text", "selector": "#count", "equals": "clicked 1" })).await;
    assert!(next.ok, "{}", next.detail);
    assert!(
        out.detail.contains("alert: Saved!") || next.detail.contains("alert: Saved!"),
        "nobody was told about the alert: {:?} / {:?}",
        out.detail,
        next.detail
    );
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn expectations_wait_and_say_what_they_saw() {
    let mut live = open().await;
    must(run(&mut live, json!({ "kind": "expect_visible", "selector": { "role": "heading", "name": "Fixture" } })).await);
    must(run(&mut live, json!({ "kind": "expect_count", "selector": { "css": "#rows li" }, "equals": 3 })).await);
    // The spinner is there at first and removed after 700 ms.
    must(run(&mut live, json!({ "kind": "expect_hidden", "selector": { "css": "#spinner" } })).await);
    must(run(&mut live, json!({ "kind": "expect_contains_text", "selector": { "css": "#rows" }, "value": "two" })).await);

    let started = std::time::Instant::now();
    let out = run(&mut live, json!({ "kind": "expect_text", "selector": "#count", "equals": "clicked 9", "timeout_ms": 600 })).await;
    assert!(!out.ok);
    assert!(out.detail.contains("\"clicked 9\"") && out.detail.contains("\"clicked 0\""), "{}", out.detail);
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "it must give up at its timeout");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_page_that_will_not_load_is_reported_and_a_screenshot_is_a_jpeg() {
    let mut live = open().await;
    let shot = page::screenshot(&mut live.cdp).await.expect("no screenshot");
    assert_eq!(&shot[..3], &[0xFF, 0xD8, 0xFF]);
    // Port 1 is refused by the browser itself, so this needs no network.
    let out = run(&mut live, json!({ "kind": "navigate", "url": "http://127.0.0.1:1/" })).await;
    assert!(!out.ok && out.detail.contains("would not load"), "{}", out.detail);
}
```

- [ ] **Step 4: Run the live tests**

From `src-tauri/`: `$env:CARGO_TARGET_DIR="target/gate"; cargo test --test browser_live -- --ignored --test-threads=1`
Expected: 8 passed. This starts headless Edge eight times; no window should appear.

If a test fails, the fake in `tests/common/mod.rs` is wrong about Chrome, not the other way round. Fix the production code to match what the browser really does, then correct the fake and the unit test that encoded the wrong belief, and say so in the task report. If Edge cannot be started or reached from a test binary on this machine (the antivirus here has blocked unsigned binaries before, error 10013), do not work around it: report BLOCKED with the exact error so the user can run the command themselves.

- [ ] **Step 5: Tidy and check for leftovers**

Replace the header comment of `src-tauri/src/browser/mod.rs` with:

```rust
//! Driving a real, visible browser for the test runner.
//!
//! Deliberately NOT a bundled automation framework: the app talks the
//! Chrome DevTools Protocol to the Edge or Chrome already on the machine.
//! That keeps the installer lean (Velopack ships deltas; a bundled browser
//! would wreck them) and means tests run in the browser people actually
//! use, not a webview stand-in.
//!
//! Bottom to top: `cdp` is the protocol client (deadlines, kept events,
//! dialogs); `page` holds element handles and calls functions on them;
//! `locator` says which element; `input` waits until it can be used and
//! uses it for real; `expect` looks until something holds; `actions` is
//! what a script is written in.
```

Then, from the repo root:

```bash
grep -rn "Evaluator\|js_for\|highlight_js\|find_helper" src-tauri/src src-tauri/tests
```

Expected: no matches.

- [ ] **Step 6: Full gates, one at a time**

1. From `src-tauri/`: `$env:CARGO_TARGET_DIR="target/gate"; cargo test --tests` - all pass, no warnings from the files this plan touched. If `git status` then shows `src/bindings.ts` modified with a whitespace-only diff, `git checkout -- src/bindings.ts`.
2. From the repo root: `npx tsc --noEmit` - clean.
3. `npx vitest run --exclude "**/.claude/**"` - all pass (`src/App.test.tsx` has a documented load flake: one failure that passes on a re-run is that).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/browser/launch.rs src-tauri/src/browser/mod.rs src-tauri/tests/browser_launch.rs src-tauri/tests/browser_live.rs src-tauri/tests/fixtures/autorun-live.html
git commit -q -F - <<'EOF'
test(v2): Auto Run's driver is checked against a real headless browser

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

- [ ] **Step 8: What the user still has to see for themselves**

The live tests prove the driver against a fixture. They do not prove it against the real application, in a visible window, on a slow day. In the task report, list for the user: open Auto Run in the dev app, import `src-tauri/tests/fixtures/autorun-sample-scripts-pms.json`, run a case and confirm the highlight still shows before each click, the mouse visibly lands where the outline was, a deliberately wrong selector fails after about 15 s with a reason and a "View screenshot" link that opens, and an old string-selector script still runs.


---

### Task 9: Auto Run's assistant tools exist in development builds only, behind a switch in AI Bridge

Added during execution at the user's request: "I also need the auto run tools available only for the dev version of the application and togglable in AI Bridge."

Today `get_autorun_guide` and `save_autorun_script` are in `HIDDEN_TOOLS`: never listed, never callable, no switch, in every build. After this task they are DEVELOPMENT-ONLY tools. In a release build nothing changes (not listed, a direct call is refused with "not available", no switch is shown, the bridge routes refuse). In a development build they behave like any other switchable tool: listed and callable unless the person has switched them off in the AI Bridge tab, where they appear as ONE switch.

**Files:**
- Modify: `src-tauri/src/ai_tools.rs` (`HIDDEN_TOOLS` becomes `DEV_ONLY_TOOLS`; `dev_build()`; `effective_disabled_for`)
- Modify: `src-tauri/src/mcp.rs` (the call-time refusal wording)
- Modify: `src-tauri/src/ai_bridge.rs` (the two autorun routes refuse outside a development build)
- Modify: `src/lib/mcpTools.ts` (mirror; the pair row; dev gating), `src/screens/AiBridge.tsx` only if its "How it works" text or row rendering needs it
- Test: `src-tauri/tests/ai_tools.rs`, `src-tauri/tests/tcm_mcp.rs`, `src-tauri/tests/ai_bridge.rs` or `autorun_bridge.rs`, `src/lib/mcpTools.test.ts`, `src/screens/AiBridge.test.tsx`

**Interfaces:**
- Produces (Rust, `v2_lib::ai_tools`): `pub const DEV_ONLY_TOOLS: &[&str] = &["get_autorun_guide", "save_autorun_script"];`, `pub fn dev_build() -> bool` (`cfg!(debug_assertions)`: true for `tauri dev` and for `cargo test`, false for `tauri build`), `pub fn effective_disabled_for(disabled: &[String], dev: bool) -> Vec<String>`, and `pub fn effective_disabled(disabled: &[String]) -> Vec<String>` = `effective_disabled_for(disabled, dev_build())`. `HIDDEN_TOOLS` is removed; nothing may keep referring to it.
- Produces (TypeScript, `src/lib/mcpTools.ts`): `export const DEV_ONLY_TOOLS = ["get_autorun_guide", "save_autorun_script"] as const;`, `export const DEV_BUILD: boolean = import.meta.env.DEV;`, the pair `["get_autorun_guide", "save_autorun_script"]` in `TOOL_PAIRS` with the row label `Auto Run scripts` and summary `Read the script-writing guide and save browser scripts for a PBI's cases. Development builds only.`. `HIDDEN_TOOLS` is removed.

**Rules:**
- `effective_disabled_for(list, false)` (release): the dev-only tools come first, always, exactly as `HIDDEN_TOOLS` did; then the person's list minus core tools and duplicates. The old behaviour, unchanged.
- `effective_disabled_for(list, true)` (development): the dev-only tools are NOT added; they are off only when the person's list names them. Core tools still cannot be switched off.
- They default to ON in a development build, like every other switchable tool (the saved setting is the DISABLED list).
- `mcp.rs` call-time refusal: a dev-only tool refused in a release build says ``The `<name>` tool is not available.``; a dev-only tool switched off in a development build gets the ordinary "switched off in Test Case Manager ... AI Bridge tab" sentence.
- `ai_bridge.rs`: `GET /autorun-guide` and `POST /autorun-script` answer 404 with `not available in this build` outside a development build, before doing anything else. Factor the decision so it can be tested for both values without a release build (for example a small function taking `dev: bool`).
- Frontend: `visibleTools()` leaves the dev-only tools out unless `DEV_BUILD`; `loadDisabledTools()` drops dev-only names from a saved list unless `DEV_BUILD` (so a list saved in a dev build cannot carry them into a release build's requests, where they would be meaningless). In a development build the two tools show as ONE row (a pair), positioned where the first of them sits in `MCP_TOOLS`. If the AI Bridge tab has a "How it works" description per tool or per row, the Auto Run row gets one there too, in development builds only, in plain words with no em dashes.
- The mirror test in `src/lib/mcpTools.test.ts` that compares the Rust and TypeScript lists must compare `DEV_ONLY_TOOLS` on both sides.
- Auto Run never calls Azure DevOps; the Auto Run TAB stays a development-build tab (`AUTO_RUN_ENABLED`), unchanged.

- [ ] **Step 1: Read first.** `src-tauri/src/ai_tools.rs` around `CORE_TOOLS`/`HIDDEN_TOOLS`/`effective_disabled`; every use found by `grep -rn "HIDDEN_TOOLS\|effective_disabled" src-tauri/src src-tauri/tests src`; `src-tauri/src/mcp.rs` `disabled()`, `tools_list`, `tools_call`; `src-tauri/src/ai_bridge.rs` the two autorun routes; `src/lib/mcpTools.ts` whole file; `src/screens/AiBridge.tsx` where `visibleRows()` is rendered and the "How it works" section; the existing tests named above, and `src/components/Sidebar.test.tsx` for the `vi.stubEnv("DEV", ...)` + `vi.resetModules()` pattern used to test a build flag.

- [ ] **Step 2: Write the failing tests.**
  - `tests/ai_tools.rs`: `effective_disabled_for(&[], false)` is `["get_autorun_guide", "save_autorun_script"]`; `effective_disabled_for(&[], true)` is empty; in development a list naming `save_autorun_script` disables it; in both, a core tool in the list is ignored and duplicates collapse; `DEV_ONLY_TOOLS` is exactly the two names.
  - `tests/tcm_mcp.rs`: the tests run as a development build, so with nothing disabled the two tools ARE listed (update the counts and the verbatim order the file asserts) and `save_autorun_script` reaches the bridge rather than being refused; with the person's list naming them they are absent and a call is refused with the "switched off" sentence. Add a test of the release rule through whatever seam you introduce (the refusal text for a dev-only tool when `dev` is false contains "not available").
  - bridge test: the route guard answers 404 "not available in this build" for `dev = false` and lets the request through for `dev = true`.
  - `src/lib/mcpTools.test.ts`: with `DEV` stubbed true, `visibleRows()` contains one row labelled `Auto Run scripts` whose `names` are both tools, and toggling it adds and removes both; with `DEV` stubbed false, no row mentions Auto Run and `loadDisabledTools()` strips the two names from a saved list. Use `vi.stubEnv` + `vi.resetModules()` + a fresh `await import("./mcpTools")`, then `vi.unstubAllEnvs()`.
  - `src/screens/AiBridge.test.tsx`: in a development build the `Auto Run scripts` switch is rendered and switching it off sends both names in the disabled list the screen pushes to the backend (find how the existing tests assert that for another row and mirror it).

- [ ] **Step 3: Run them to see them fail**, one command at a time.

- [ ] **Step 4: Implement** the rules above.

- [ ] **Step 5: Run**, one at a time, from `src-tauri/` with `CARGO_TARGET_DIR=target/gate`: `cargo test --test ai_tools`, `cargo test --test tcm_mcp`, `cargo test --test ai_bridge`, `cargo test --test autorun_bridge`, `cargo test --test bindings`; from the repo root: `npx vitest run src/lib/mcpTools.test.ts src/screens/AiBridge.test.tsx src/ui-consistency.test.ts`, `npx tsc --noEmit`. Then `grep -rn "HIDDEN_TOOLS" src-tauri/src src-tauri/tests src` must return nothing.

- [ ] **Step 6: Commit** with a Bash heredoc, subject `feat(v2): Auto Run's assistant tools are offered in development builds only, behind one AI Bridge switch`, ending with the trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
