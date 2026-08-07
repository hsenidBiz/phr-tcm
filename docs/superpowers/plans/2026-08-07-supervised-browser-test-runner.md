# Supervised Browser Test Runner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new "Auto Run" screen that drives a real, visible Edge window through a test case's steps while a human watches, and records the verdict the human chooses — entirely on this machine, with nothing written to Azure DevOps.

**Architecture:** The Rust side launches the installed Edge with a debugging port and a throwaway profile, then speaks the Chrome DevTools Protocol over a WebSocket. Each test case gets an *action script* — a small JSON list of typed actions per step — stored locally beside the app's other data. The frontend executes one step at a time, shows what each action did, and the human marks the case Passed/Failed/Blocked. Runs are written to local JSON files and read back in a results view. No ADO command is called anywhere in this feature.

**Tech Stack:** Rust (Tauri 2, tokio, serde, reqwest, tokio-tungstenite), React 19 + TypeScript, Tailwind, vitest, tauri-specta bindings.

## Global Constraints

- **Nothing in this feature may call Azure DevOps.** No `commands.*` from `ado/`, no test-run recording, no bug filing. Reading the case list through the existing `pbi_test_cases_full` command is the *only* ADO read permitted, and it is read-only.
- The browser must be **visible (headed)**, never headless — the human is the oracle and has to see what happened.
- The app already runs unit tests inside `src/` (`[lib] test = false` was removed); new integration tests still go in `src-tauri/tests/` to match the dominant pattern.
- Tests that need a real browser must be marked `#[ignore]` with a reason, matching the existing precedent in `tests/updater.rs` (`ignored, hits github.com`).
- `specta` forbids `u64` across IPC. Use `i32`, `u32`, `f64`, or `String`.
- `src/bindings.ts` is generated — **never hand-edit it**. Regenerate with `cargo test --test bindings`.
- Commits use a Bash heredoc (`git commit -F - <<'EOF'`), never PowerShell message flags.
- Every task ends green: `cargo test` and `npx vitest run` both pass, `npx tsc --noEmit` clean.

---

### Task 1: Find and launch Edge with a debugging port

**Files:**
- Create: `v2/src-tauri/src/browser/mod.rs`
- Create: `v2/src-tauri/src/browser/launch.rs`
- Modify: `v2/src-tauri/src/lib.rs` (add `pub mod browser;` beside the other module declarations near line 13)
- Test: `v2/src-tauri/tests/browser_launch.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `browser::launch::edge_candidates(program_files: &str, program_files_x86: &str) -> Vec<PathBuf>`
  - `browser::launch::launch_args(port: u16, profile_dir: &Path) -> Vec<String>`
  - `browser::launch::free_port() -> Result<u16, String>`
  - `browser::launch::LaunchedBrowser { pub child: std::process::Child, pub port: u16, pub profile_dir: PathBuf }`
  - `browser::launch::launch() -> Result<LaunchedBrowser, String>`

- [ ] **Step 1: Write the failing test**

Create `v2/src-tauri/tests/browser_launch.rs`:

```rust
//! Finding and launching the browser the tests will be watched in. The
//! pure parts - where Edge lives, what arguments it gets, picking a port
//! - are unit tested here; actually starting a browser is an ignored
//! test, like the updater's live download check.

use v2_lib::browser::launch::{edge_candidates, free_port, launch_args};
use std::path::{Path, PathBuf};

/// Both Program Files roots are searched, 64-bit first: an Edge in
/// "Program Files" is the current install, the x86 one is the legacy
/// location that some machines still carry.
#[test]
fn edge_is_looked_for_in_both_program_files_roots() {
    let found = edge_candidates(r"C:\Program Files", r"C:\Program Files (x86)");
    assert_eq!(
        found,
        vec![
            PathBuf::from(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
        ]
    );
}

/// The launch has to be debuggable, isolated, and VISIBLE. A headless
/// flag here would defeat the whole feature - the human is the oracle.
#[test]
fn the_launch_is_debuggable_isolated_and_never_headless() {
    let args = launch_args(9333, Path::new(r"C:\tmp\profile-1"));
    assert!(args.contains(&"--remote-debugging-port=9333".to_string()));
    assert!(args.contains(&r"--user-data-dir=C:\tmp\profile-1".to_string()));
    assert!(args.contains(&"--no-first-run".to_string()));
    assert!(
        !args.iter().any(|a| a.contains("headless")),
        "the run must be watchable: {args:?}"
    );
}

/// A port nobody else holds, so two runs (or a stale browser) cannot
/// collide.
#[test]
fn free_port_returns_a_usable_port() {
    let a = free_port().unwrap();
    let b = free_port().unwrap();
    assert!(a > 1024, "expected a high port, got {a}");
    assert!(b > 1024, "expected a high port, got {b}");
}

#[test]
#[ignore = "starts a real Edge window"]
fn launch_starts_a_browser_that_answers_on_its_port() {
    let mut b = v2_lib::browser::launch::launch().unwrap();
    let url = format!("http://127.0.0.1:{}/json/version", b.port);
    let body = reqwest::blocking::get(&url).unwrap().text().unwrap();
    assert!(body.contains("webSocketDebuggerUrl"), "got: {body}");
    let _ = b.child.kill();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd v2/src-tauri && cargo test --test browser_launch`
Expected: FAIL to compile — `could not find 'browser' in 'v2_lib'`.

- [ ] **Step 3: Write minimal implementation**

Create `v2/src-tauri/src/browser/mod.rs`:

```rust
//! Driving a real, visible browser for the supervised test runner.
//!
//! Deliberately NOT a bundled automation framework: the app talks the
//! Chrome DevTools Protocol to the Edge already on the machine. That
//! keeps the installer lean (Velopack ships deltas; a bundled browser
//! would wreck them) and means tests run in the browser people actually
//! use, not a webview stand-in.

pub mod launch;
```

Create `v2/src-tauri/src/browser/launch.rs`:

```rust
//! Where the browser lives and how it is started.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Every place the Edge installers put msedge.exe, 64-bit first. Same
/// shape as `ai_tools::claude_cli_candidates` and for the same reason:
/// PATH is not trustworthy enough to be the only answer.
pub fn edge_candidates(program_files: &str, program_files_x86: &str) -> Vec<PathBuf> {
    vec![
        PathBuf::from(program_files).join(r"Microsoft\Edge\Application\msedge.exe"),
        PathBuf::from(program_files_x86).join(r"Microsoft\Edge\Application\msedge.exe"),
    ]
}

/// The arguments the run needs: a debugging port to drive it through, a
/// throwaway profile so no cookie or extension from yesterday leaks into
/// today's result, and NOTHING that hides the window.
pub fn launch_args(port: u16, profile_dir: &Path) -> Vec<String> {
    vec![
        format!("--remote-debugging-port={port}"),
        format!("--user-data-dir={}", profile_dir.display()),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-popup-blocking".to_string(),
        "about:blank".to_string(),
    ]
}

/// Ask the OS for a port, then let it go: the browser binds it a moment
/// later. The gap is a race in theory and has never been one in
/// practice, and it beats guessing a fixed port that a stale browser
/// might still hold.
pub fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    Ok(port)
}

pub struct LaunchedBrowser {
    pub child: Child,
    pub port: u16,
    pub profile_dir: PathBuf,
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

/// Start Edge. Errors name the thing to fix rather than a code.
pub fn launch() -> Result<LaunchedBrowser, String> {
    let candidates = edge_candidates(
        &env_or("ProgramFiles", r"C:\Program Files"),
        &env_or("ProgramFiles(x86)", r"C:\Program Files (x86)"),
    );
    let exe = candidates
        .iter()
        .find(|p| p.is_file())
        .ok_or_else(|| "Microsoft Edge was not found in either Program Files".to_string())?;

    let port = free_port()?;
    let profile_dir = std::env::temp_dir().join(format!("tcm-autorun-{port}"));
    std::fs::create_dir_all(&profile_dir).map_err(|e| e.to_string())?;

    let child = Command::new(exe)
        .args(launch_args(port, &profile_dir))
        .spawn()
        .map_err(|e| format!("could not start Edge: {e}"))?;

    Ok(LaunchedBrowser { child, port, profile_dir })
}
```

In `v2/src-tauri/src/lib.rs`, add the module beside the others (the block that begins `pub mod branchcheck;`):

```rust
pub mod browser;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd v2/src-tauri && cargo test --test browser_launch`
Expected: PASS — 3 passed, 1 ignored.

- [ ] **Step 5: Commit**

```bash
git add v2/src-tauri/src/browser v2/src-tauri/src/lib.rs v2/src-tauri/tests/browser_launch.rs
git commit -F - <<'EOF'
feat(v2): find and launch a watchable Edge for auto-run

The supervised runner drives the Edge already on the machine rather
than a bundled browser: the installer stays lean and tests run in the
browser people actually use. A throwaway profile per launch keeps
yesterday's cookies out of today's result, and a test pins that no
headless flag ever creeps in - a run nobody can watch is the one thing
this feature must not produce.
EOF
```

---

### Task 2: CDP transport — connect and evaluate

**Files:**
- Create: `v2/src-tauri/src/browser/cdp.rs`
- Modify: `v2/src-tauri/src/browser/mod.rs` (add `pub mod cdp;`)
- Modify: `v2/src-tauri/Cargo.toml` (add `tokio-tungstenite`)
- Test: `v2/src-tauri/tests/browser_cdp.rs`

**Interfaces:**
- Consumes: `browser::launch::LaunchedBrowser` from Task 1.
- Produces:
  - `browser::cdp::frame(id: u64, method: &str, params: serde_json::Value) -> String`
  - `browser::cdp::reply_for(id: u64, raw: &str) -> Option<Result<serde_json::Value, String>>`
  - `browser::cdp::Cdp` with `pub async fn connect(port: u16) -> Result<Cdp, String>` and `pub async fn eval(&mut self, expression: &str) -> Result<serde_json::Value, String>`

- [ ] **Step 1: Write the failing test**

Create `v2/src-tauri/tests/browser_cdp.rs`:

```rust
//! The DevTools wire format. Only two things need to be right: the
//! frame we send, and telling OUR reply apart from the flood of events
//! the browser volunteers.

use v2_lib::browser::cdp::{frame, reply_for};

#[test]
fn a_frame_carries_its_id_method_and_params() {
    let f = frame(7, "Runtime.evaluate", serde_json::json!({ "expression": "1+1" }));
    let v: serde_json::Value = serde_json::from_str(&f).unwrap();
    assert_eq!(v["id"], 7);
    assert_eq!(v["method"], "Runtime.evaluate");
    assert_eq!(v["params"]["expression"], "1+1");
}

/// The socket also carries EVENTS - frames with no id at all. Treating
/// one as an answer would hang the caller on the next reply, or worse,
/// return another request's result.
#[test]
fn events_and_other_ids_are_not_mistaken_for_our_reply() {
    assert!(reply_for(7, r#"{"method":"Page.loadEventFired","params":{}}"#).is_none());
    assert!(reply_for(7, r#"{"id":8,"result":{"x":1}}"#).is_none());
    assert!(reply_for(7, "not json at all").is_none());
}

#[test]
fn our_reply_returns_its_result() {
    let got = reply_for(7, r#"{"id":7,"result":{"result":{"value":42}}}"#).unwrap().unwrap();
    assert_eq!(got["result"]["value"], 42);
}

/// A protocol error must surface as an error, not as an empty success -
/// a silently-empty result reads as "the check found nothing", which is
/// a false negative in a test runner.
#[test]
fn a_protocol_error_is_an_error_with_its_message() {
    let got = reply_for(7, r#"{"id":7,"error":{"code":-32000,"message":"No node found"}}"#)
        .unwrap()
        .unwrap_err();
    assert!(got.contains("No node found"), "got: {got}");
}

#[tokio::test]
#[ignore = "starts a real Edge window"]
async fn eval_round_trips_against_a_real_browser() {
    let mut b = v2_lib::browser::launch::launch().unwrap();
    // Give the browser a moment to bind its port.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let mut cdp = v2_lib::browser::cdp::Cdp::connect(b.port).await.unwrap();
    let v = cdp.eval("1 + 1").await.unwrap();
    assert_eq!(v["result"]["value"], 2);
    let _ = b.child.kill();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd v2/src-tauri && cargo test --test browser_cdp`
Expected: FAIL to compile — `could not find 'cdp' in 'browser'`.

- [ ] **Step 3: Write minimal implementation**

In `v2/src-tauri/Cargo.toml`, under `[dependencies]`, beside the other network crates:

```toml
# The DevTools Protocol is WebSocket-only. Five commands is the whole
# surface this app needs, so a full automation framework (and the Node
# runtime and browser binaries it would drag into the installer) buys
# nothing a socket does not.
tokio-tungstenite = "0.24"
```

In `v2/src-tauri/src/browser/mod.rs`, add:

```rust
pub mod cdp;
```

Create `v2/src-tauri/src/browser/cdp.rs`:

```rust
//! A minimal Chrome DevTools Protocol client: connect to the page's
//! socket and evaluate JavaScript in it. Evaluation alone covers finding
//! elements, clicking, typing, reading text and highlighting, which is
//! everything the supervised runner does.

use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

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

type Socket = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

pub struct Cdp {
    socket: Socket,
    next_id: u64,
}

impl Cdp {
    /// Ask the browser which socket its page is on, then open it.
    pub async fn connect(port: u16) -> Result<Cdp, String> {
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
        Ok(Cdp { socket, next_id: 1 })
    }

    /// Run an expression in the page and return its DevTools result.
    /// `awaitPromise` so an async expression resolves before we answer.
    pub async fn eval(&mut self, expression: &str) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let params = serde_json::json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": true,
        });
        self.socket
            .send(Message::Text(frame(id, "Runtime.evaluate", params)))
            .await
            .map_err(|e| format!("the DevTools socket closed: {e}"))?;

        // Skip past the events the browser volunteers until our own
        // reply arrives.
        while let Some(msg) = self.socket.next().await {
            let msg = msg.map_err(|e| format!("the DevTools socket failed: {e}"))?;
            let Message::Text(raw) = msg else { continue };
            if let Some(answer) = reply_for(id, &raw) {
                return answer;
            }
        }
        Err("the browser closed before answering".to_string())
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd v2/src-tauri && cargo test --test browser_cdp`
Expected: PASS — 4 passed, 1 ignored.

- [ ] **Step 5: Commit**

```bash
git add v2/src-tauri/src/browser v2/src-tauri/Cargo.toml v2/src-tauri/Cargo.lock v2/src-tauri/tests/browser_cdp.rs
git commit -F - <<'EOF'
feat(v2): a minimal DevTools client for the supervised runner

Evaluate-in-page is the whole surface the runner needs, so this is a
socket and a frame builder rather than an automation framework - no
Node runtime, no bundled browser binaries in the installer. The reply
matcher is the part worth testing: DevTools volunteers a flood of
events, and mistaking one for an answer would return another request's
result. A protocol error surfaces as an error, never as an empty
success, because an empty result reads as "the check found nothing".
EOF
```

---

### Task 3: Actions — the typed steps and the JavaScript they become

**Files:**
- Create: `v2/src-tauri/src/browser/actions.rs`
- Modify: `v2/src-tauri/src/browser/mod.rs` (add `pub mod actions;`)
- Test: `v2/src-tauri/tests/browser_actions.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks (the `Evaluator` trait keeps this independent of `Cdp`).
- Produces:
  - `browser::actions::Action` — a serde-tagged enum: `Navigate { url }`, `Click { selector }`, `Fill { selector, value }`, `WaitFor { selector, timeout_ms: u32 }`, `CheckText { value }`, `CheckUrl { contains }`
  - `browser::actions::ActionOutcome { pub ok: bool, pub detail: String }`
  - `browser::actions::js_for(action: &Action) -> String`
  - `browser::actions::highlight_js(selector: &str) -> String`
  - `browser::actions::Evaluator` trait with `async fn eval(&mut self, expression: &str) -> Result<serde_json::Value, String>`
  - `browser::actions::execute<E: Evaluator>(ev: &mut E, action: &Action) -> ActionOutcome`

- [ ] **Step 1: Write the failing test**

Create `v2/src-tauri/tests/browser_actions.rs`:

```rust
//! What each action becomes in the page, and how the executor treats
//! the answer. A fake evaluator stands in for the browser so every rule
//! here is pinned without starting Edge.

use v2_lib::browser::actions::{execute, highlight_js, js_for, Action, ActionOutcome, Evaluator};

/// Returns canned answers in order, and remembers every expression it
/// was asked to run.
struct FakeEval {
    answers: Vec<serde_json::Value>,
    seen: Vec<String>,
}

impl FakeEval {
    fn new(answers: Vec<serde_json::Value>) -> Self {
        FakeEval { answers, seen: vec![] }
    }
}

impl Evaluator for FakeEval {
    async fn eval(&mut self, expression: &str) -> Result<serde_json::Value, String> {
        self.seen.push(expression.to_string());
        if self.answers.is_empty() {
            return Ok(serde_json::json!({ "result": { "value": { "ok": true, "detail": "" } } }));
        }
        Ok(self.answers.remove(0))
    }
}

fn value(ok: bool, detail: &str) -> serde_json::Value {
    serde_json::json!({ "result": { "value": { "ok": ok, "detail": detail } } })
}

/// A selector with a quote in it must not be able to close the string
/// it sits in and run its own code - the scripts are authored by hand
/// and later by an assistant, so neither is trusted input.
#[test]
fn selectors_and_values_are_escaped_into_the_expression() {
    let js = js_for(&Action::Fill {
        selector: r#"input[name="x"]"#.to_string(),
        value: "he said \"hi\"\n</script>".to_string(),
    });
    assert!(js.contains(r#"\"x\""#), "selector not escaped: {js}");
    assert!(!js.contains("</script>"), "raw value leaked into the source: {js}");
}

/// React (and anything else with a controlled input) ignores a plain
/// .value assignment. The native setter plus an input event is what
/// actually moves a modern form.
#[test]
fn fill_uses_the_native_setter_so_controlled_inputs_notice() {
    let js = js_for(&Action::Fill {
        selector: "#user".to_string(),
        value: "kim".to_string(),
    });
    assert!(js.contains("getOwnPropertyDescriptor"), "{js}");
    assert!(js.contains("new Event('input'"), "{js}");
}

/// Test cases are written in prose, so the scripts derived from them
/// need to address things the way the prose does: by their words.
#[test]
fn a_text_selector_is_supported_alongside_css() {
    let js = js_for(&Action::Click { selector: "text=Sign in".to_string() });
    assert!(js.contains("text="), "{js}");
    assert!(js.contains("innerText"), "the text branch must read innerText: {js}");
}

#[test]
fn highlight_outlines_the_element_it_is_about_to_touch() {
    let js = highlight_js("#go");
    assert!(js.contains("outline"), "{js}");
    assert!(js.contains("#go"), "{js}");
}

#[tokio::test]
async fn a_successful_action_reports_what_it_did() {
    let mut ev = FakeEval::new(vec![value(true, "clicked Sign in")]);
    let out = execute(&mut ev, &Action::Click { selector: "text=Sign in".into() }).await;
    assert_eq!(out, ActionOutcome { ok: true, detail: "clicked Sign in".into() });
}

/// The human is the oracle, so the executor's job is to report
/// faithfully - a missing element is a plain false with a reason, not a
/// silent pass and not a crash.
#[tokio::test]
async fn a_missing_element_fails_with_the_reason() {
    let mut ev = FakeEval::new(vec![value(false, "not found: #nope")]);
    let out = execute(&mut ev, &Action::Click { selector: "#nope".into() }).await;
    assert!(!out.ok);
    assert!(out.detail.contains("not found"), "{}", out.detail);
}

/// A dropped socket must not read as a failed assertion about the app
/// under test - it is a failure of the harness, and it says so.
#[tokio::test]
async fn a_transport_error_is_reported_as_a_harness_problem() {
    struct Broken;
    impl Evaluator for Broken {
        async fn eval(&mut self, _e: &str) -> Result<serde_json::Value, String> {
            Err("the DevTools socket closed".to_string())
        }
    }
    let out = execute(&mut Broken, &Action::CheckText { value: "Dashboard".into() }).await;
    assert!(!out.ok);
    assert!(out.detail.contains("browser"), "{}", out.detail);
}

/// Clicking highlights first, so the watcher sees WHERE the click went.
#[tokio::test]
async fn a_click_highlights_before_it_acts() {
    let mut ev = FakeEval::new(vec![value(true, "highlighted"), value(true, "clicked")]);
    let _ = execute(&mut ev, &Action::Click { selector: "#go".into() }).await;
    assert_eq!(ev.seen.len(), 2, "expected highlight then click: {:?}", ev.seen);
    assert!(ev.seen[0].contains("outline"), "first call was not the highlight: {}", ev.seen[0]);
}

/// Waiting polls rather than sleeping a fixed guess, and gives up with
/// a verdict instead of hanging the run.
#[tokio::test]
async fn wait_for_polls_until_it_appears() {
    let mut ev = FakeEval::new(vec![
        value(false, "not found: #late"),
        value(false, "not found: #late"),
        value(true, "found: #late"),
    ]);
    let out = execute(
        &mut ev,
        &Action::WaitFor { selector: "#late".into(), timeout_ms: 2000 },
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(ev.seen.len(), 3);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd v2/src-tauri && cargo test --test browser_actions`
Expected: FAIL to compile — `could not find 'actions' in 'browser'`.

- [ ] **Step 3: Write minimal implementation**

In `v2/src-tauri/src/browser/mod.rs`, add:

```rust
pub mod actions;
```

Create `v2/src-tauri/src/browser/actions.rs`:

```rust
//! The typed steps a script is made of, and the JavaScript each becomes.
//!
//! Every expression returns `{ ok, detail }`: `detail` is written for the
//! human watching, because in this runner the person - not the machine -
//! decides the verdict. An action that cannot tell what happened says so
//! rather than guessing.

/// How long between polls while waiting for an element.
const POLL_MS: u64 = 200;
/// How long the highlight stays on screen before the action fires, so a
/// watcher can see WHERE the click is about to go.
const HIGHLIGHT_MS: u64 = 350;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Navigate { url: String },
    Click { selector: String },
    Fill { selector: String, value: String },
    WaitFor { selector: String, timeout_ms: u32 },
    CheckText { value: String },
    CheckUrl { contains: String },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ActionOutcome {
    pub ok: bool,
    pub detail: String,
}

/// Anything that can run an expression in the page. `Cdp` is the real
/// one; tests supply a fake, which is why the executor's rules can be
/// pinned without starting a browser.
pub trait Evaluator {
    fn eval(
        &mut self,
        expression: &str,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, String>>;
}

/// A JS string literal, escaped by the JSON encoder. Scripts are
/// authored by hand today and by an assistant later, so no value in
/// them is trusted to be quote-free.
fn lit(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Prepended to every expression rather than installed as a global: a
/// page navigation wipes globals, and the runner navigates.
fn find_helper() -> &'static str {
    r#"
const __find = (sel) => {
  if (sel.startsWith('text=')) {
    const want = sel.slice(5).trim().toLowerCase();
    const all = Array.from(document.querySelectorAll(
      'button,a,[role=button],label,input,textarea,select,td,th,li,summary,h1,h2,h3,span,div'));
    // Last match wins: the deepest element containing the words is the
    // control itself, not the panel it sits in.
    const hits = all.filter(e => ((e.innerText || e.value || '') + '').trim().toLowerCase().includes(want));
    return hits.length ? hits[hits.length - 1] : null;
  }
  return document.querySelector(sel);
};
"#
}

fn wrap(body: &str) -> String {
    format!("(() => {{{}{}}})()", find_helper(), body)
}

/// Outline the element the next action will touch.
pub fn highlight_js(selector: &str) -> String {
    let sel = lit(selector);
    wrap(&format!(
        r#"
  const el = __find({sel});
  if (!el) return {{ ok: false, detail: "not found: " + {sel} }};
  el.scrollIntoView({{ block: 'center', behavior: 'instant' }});
  const prev = el.style.outline;
  el.style.outline = '3px solid #7c5cff';
  setTimeout(() => {{ el.style.outline = prev; }}, 1200);
  return {{ ok: true, detail: "highlighted" }};
"#
    ))
}

/// The expression for one action.
pub fn js_for(action: &Action) -> String {
    match action {
        Action::Navigate { url } => {
            let u = lit(url);
            wrap(&format!(
                r#"
  location.href = {u};
  return {{ ok: true, detail: "navigating to " + {u} }};
"#
            ))
        }
        Action::Click { selector } => {
            let sel = lit(selector);
            wrap(&format!(
                r#"
  const el = __find({sel});
  if (!el) return {{ ok: false, detail: "not found: " + {sel} }};
  el.click();
  return {{ ok: true, detail: "clicked " + (el.innerText || el.value || el.tagName).toString().trim().slice(0, 60) }};
"#
            ))
        }
        Action::Fill { selector, value } => {
            let sel = lit(selector);
            let val = lit(value);
            wrap(&format!(
                r#"
  const el = __find({sel});
  if (!el) return {{ ok: false, detail: "not found: " + {sel} }};
  // A plain .value assignment is invisible to a controlled input - the
  // framework's own setter has to be the one called, then told.
  const desc = Object.getOwnPropertyDescriptor(el.constructor.prototype, 'value');
  if (desc && desc.set) desc.set.call(el, {val}); else el.value = {val};
  el.dispatchEvent(new Event('input', {{ bubbles: true }}));
  el.dispatchEvent(new Event('change', {{ bubbles: true }}));
  return {{ ok: true, detail: "filled " + {sel} }};
"#
            ))
        }
        Action::WaitFor { selector, .. } => {
            let sel = lit(selector);
            wrap(&format!(
                r#"
  const el = __find({sel});
  return el ? {{ ok: true, detail: "found: " + {sel} }} : {{ ok: false, detail: "not found: " + {sel} }};
"#
            ))
        }
        Action::CheckText { value } => {
            let v = lit(value);
            wrap(&format!(
                r#"
  const hay = (document.body ? document.body.innerText : '') || '';
  const found = hay.toLowerCase().includes({v}.toLowerCase());
  return {{ ok: found, detail: (found ? "page contains " : "page does NOT contain ") + {v} }};
"#
            ))
        }
        Action::CheckUrl { contains } => {
            let c = lit(contains);
            wrap(&format!(
                r#"
  const found = location.href.includes({c});
  return {{ ok: found, detail: "url is " + location.href }};
"#
            ))
        }
    }
}

/// Pull `{ ok, detail }` back out of a DevTools reply.
fn outcome_from(v: &serde_json::Value) -> ActionOutcome {
    let value = &v["result"]["value"];
    ActionOutcome {
        ok: value["ok"].as_bool().unwrap_or(false),
        detail: value["detail"].as_str().unwrap_or("no detail").to_string(),
    }
}

/// A harness failure, said plainly: the app under test did nothing
/// wrong, the browser connection did.
fn harness_error(e: String) -> ActionOutcome {
    ActionOutcome { ok: false, detail: format!("the browser did not answer: {e}") }
}

/// Run one action. Clicks and fills highlight first so the watcher can
/// see where they landed; waiting polls instead of guessing a sleep.
pub async fn execute<E: Evaluator>(ev: &mut E, action: &Action) -> ActionOutcome {
    if let Action::Click { selector } | Action::Fill { selector, .. } = action {
        match ev.eval(&highlight_js(selector)).await {
            Ok(v) => {
                let out = outcome_from(&v);
                if !out.ok {
                    return out; // the element is not there; do not click blind
                }
            }
            Err(e) => return harness_error(e),
        }
        tokio::time::sleep(std::time::Duration::from_millis(HIGHLIGHT_MS)).await;
    }

    if let Action::WaitFor { timeout_ms, selector } = action {
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(u64::from(*timeout_ms));
        loop {
            match ev.eval(&js_for(action)).await {
                Ok(v) => {
                    let out = outcome_from(&v);
                    if out.ok {
                        return out;
                    }
                }
                Err(e) => return harness_error(e),
            }
            if std::time::Instant::now() >= deadline {
                return ActionOutcome {
                    ok: false,
                    detail: format!("waited {timeout_ms}ms and never saw {selector}"),
                };
            }
            tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
        }
    }

    match ev.eval(&js_for(action)).await {
        Ok(v) => outcome_from(&v),
        Err(e) => harness_error(e),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd v2/src-tauri && cargo test --test browser_actions`
Expected: PASS — 9 passed.

- [ ] **Step 5: Commit**

```bash
git add v2/src-tauri/src/browser v2/src-tauri/tests/browser_actions.rs
git commit -F - <<'EOF'
feat(v2): typed browser actions and the JavaScript they become

Six actions cover what a test case step does: navigate, click, fill,
wait, and two checks. Each returns { ok, detail } written for the
person watching, because the human decides the verdict here - an
action that cannot tell what happened says so rather than guessing.
Three details are pinned by test: values are JSON-escaped into the
expression so a quote cannot close the string it sits in, fill goes
through the native setter (a plain .value assignment is invisible to a
controlled input), and a dropped socket reports as a harness problem
rather than a failed assertion about the app under test.
EOF
```

---

### Task 4: Scripts and local runs on disk

**Files:**
- Create: `v2/src-tauri/src/autorun/mod.rs`
- Create: `v2/src-tauri/src/autorun/store.rs`
- Modify: `v2/src-tauri/src/lib.rs` (add `pub mod autorun;`)
- Test: `v2/src-tauri/tests/autorun_store.rs`

**Interfaces:**
- Consumes: `browser::actions::{Action, ActionOutcome}` from Task 3.
- Produces:
  - `autorun::StepScript { pub step_number: i32, pub actions: Vec<Action> }`
  - `autorun::CaseScript { pub case_id: i32, pub title: String, pub steps: Vec<StepScript> }`
  - `autorun::StepRecord { pub step_number: i32, pub outcomes: Vec<ActionOutcome> }`
  - `autorun::CaseRecord { pub case_id: i32, pub title: String, pub verdict: String, pub note: String, pub steps: Vec<StepRecord> }`
  - `autorun::LocalRun { pub id: String, pub pbi_id: i32, pub started_at: String, pub cases: Vec<CaseRecord> }`
  - `autorun::store::new_run_id() -> String`
  - `autorun::store::save_script(root: &Path, script: &CaseScript) -> Result<(), String>`
  - `autorun::store::load_script(root: &Path, case_id: i32) -> Result<Option<CaseScript>, String>`
  - `autorun::store::save_run(root: &Path, run: &LocalRun) -> Result<(), String>`
  - `autorun::store::list_runs(root: &Path) -> Vec<LocalRun>`

- [ ] **Step 1: Write the failing test**

Create `v2/src-tauri/tests/autorun_store.rs`:

```rust
//! Scripts and results live on THIS machine and nowhere else. These
//! tests pin the round trip and the two rules that matter: a run always
//! records the verdict the human gave (never one the machine inferred),
//! and nothing here has an Azure DevOps shape.

use v2_lib::autorun::store::{list_runs, load_script, new_run_id, save_run, save_script};
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, StepRecord, StepScript};
use v2_lib::browser::actions::{Action, ActionOutcome};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-autorun-test-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn script() -> CaseScript {
    CaseScript {
        case_id: 201,
        title: "Valid login".to_string(),
        steps: vec![StepScript {
            step_number: 1,
            actions: vec![
                Action::Navigate { url: "https://app.invalid/login".to_string() },
                Action::Fill { selector: "#user".to_string(), value: "kim".to_string() },
                Action::Click { selector: "text=Sign in".to_string() },
                Action::CheckText { value: "Dashboard".to_string() },
            ],
        }],
    }
}

#[test]
fn a_script_round_trips_by_case_id() {
    let dir = TempDir::new();
    save_script(dir.path(), &script()).unwrap();
    let back = load_script(dir.path(), 201).unwrap().unwrap();
    assert_eq!(back, script());
}

#[test]
fn a_case_with_no_script_reads_as_none_not_an_error() {
    let dir = TempDir::new();
    assert!(load_script(dir.path(), 999).unwrap().is_none());
}

#[test]
fn run_ids_are_unique() {
    assert_ne!(new_run_id(), {
        std::thread::sleep(std::time::Duration::from_millis(2));
        new_run_id()
    });
}

/// The verdict is whatever the human typed in. Nothing in this module
/// may infer one from the outcomes - that is the whole point of a
/// supervised runner.
#[test]
fn a_run_round_trips_with_the_humans_verdict() {
    let dir = TempDir::new();
    let run = LocalRun {
        id: "run-1".to_string(),
        pbi_id: 42,
        started_at: "1786000000000".to_string(),
        cases: vec![CaseRecord {
            case_id: 201,
            title: "Valid login".to_string(),
            verdict: "Failed".to_string(),
            note: "the dashboard came up but the name was wrong".to_string(),
            steps: vec![StepRecord {
                step_number: 1,
                // Every action succeeded and the human still said Failed.
                outcomes: vec![ActionOutcome { ok: true, detail: "clicked Sign in".to_string() }],
            }],
        }],
    };
    save_run(dir.path(), &run).unwrap();

    let all = list_runs(dir.path());
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].cases[0].verdict, "Failed");
    assert_eq!(all[0].cases[0].steps[0].outcomes[0].ok, true);
}

#[test]
fn runs_come_back_newest_first() {
    let dir = TempDir::new();
    for (id, at) in [("run-1", "1000"), ("run-2", "3000"), ("run-3", "2000")] {
        save_run(
            dir.path(),
            &LocalRun {
                id: id.to_string(),
                pbi_id: 42,
                started_at: at.to_string(),
                cases: vec![],
            },
        )
        .unwrap();
    }
    let ids: Vec<String> = list_runs(dir.path()).into_iter().map(|r| r.id).collect();
    assert_eq!(ids, vec!["run-2", "run-3", "run-1"]);
}

/// An unreadable file is skipped, not a panic and not a lost list - one
/// corrupt run must never hide every other run from the results view.
#[test]
fn a_corrupt_run_file_is_skipped_rather_than_fatal() {
    let dir = TempDir::new();
    save_run(
        dir.path(),
        &LocalRun {
            id: "good".to_string(),
            pbi_id: 1,
            started_at: "1".to_string(),
            cases: vec![],
        },
    )
    .unwrap();
    std::fs::write(dir.path().join("runs").join("broken.json"), "{ not json").unwrap();
    let all = list_runs(dir.path());
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, "good");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd v2/src-tauri && cargo test --test autorun_store`
Expected: FAIL to compile — `could not find 'autorun' in 'v2_lib'`.

- [ ] **Step 3: Write minimal implementation**

Create `v2/src-tauri/src/autorun/mod.rs`:

```rust
//! The supervised runner's own data: the action script for a case, and
//! the record of a run.
//!
//! Both live on THIS machine only. Nothing here is sent to Azure DevOps
//! - the results view in the app is the whole audience while the feature
//! earns trust.

pub mod store;

use crate::browser::actions::{Action, ActionOutcome};

/// The actions that carry out one numbered step of a test case.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StepScript {
    pub step_number: i32,
    pub actions: Vec<Action>,
}

/// How one test case is driven. Keyed by the Azure DevOps case id so a
/// script and its case stay together, but the script never leaves here.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CaseScript {
    pub case_id: i32,
    pub title: String,
    pub steps: Vec<StepScript>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StepRecord {
    pub step_number: i32,
    pub outcomes: Vec<ActionOutcome>,
}

/// One case in a run. `verdict` is the HUMAN's word - "", "Passed",
/// "Failed", "Blocked". The machine never fills it in: the action
/// outcomes are evidence shown to the person, not a vote.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CaseRecord {
    pub case_id: i32,
    pub title: String,
    pub verdict: String,
    pub note: String,
    pub steps: Vec<StepRecord>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LocalRun {
    pub id: String,
    pub pbi_id: i32,
    /// Epoch milliseconds as a string - specta forbids u64 across IPC,
    /// and the frontend formats it anyway.
    pub started_at: String,
    pub cases: Vec<CaseRecord>,
}
```

Create `v2/src-tauri/src/autorun/store.rs`:

```rust
//! Scripts and runs on disk, under the app's own data directory.

use super::{CaseScript, LocalRun};
use std::path::{Path, PathBuf};

fn scripts_dir(root: &Path) -> PathBuf {
    root.join("scripts")
}

fn runs_dir(root: &Path) -> PathBuf {
    root.join("runs")
}

/// Epoch milliseconds, which sorts and reads as a time.
pub fn new_run_id() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("run-{ms}")
}

pub fn save_script(root: &Path, script: &CaseScript) -> Result<(), String> {
    let dir = scripts_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(script).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("case-{}.json", script.case_id)), json)
        .map_err(|e| e.to_string())
}

/// `Ok(None)` for a case nobody has scripted yet - that is the normal
/// state of most cases, not an error.
pub fn load_script(root: &Path, case_id: i32) -> Result<Option<CaseScript>, String> {
    let path = scripts_dir(root).join(format!("case-{case_id}.json"));
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| format!("{} is not a readable script: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_run(root: &Path, run: &LocalRun) -> Result<(), String> {
    let dir = runs_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(run).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{}.json", run.id)), json).map_err(|e| e.to_string())
}

/// Newest first. An unreadable file is skipped: one corrupt run must
/// never hide every other run from the results view.
pub fn list_runs(root: &Path) -> Vec<LocalRun> {
    let Ok(entries) = std::fs::read_dir(runs_dir(root)) else {
        return vec![];
    };
    let mut out: Vec<LocalRun> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<LocalRun>(&s).ok())
        .collect();
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    out
}
```

In `v2/src-tauri/src/lib.rs`, add beside the other modules:

```rust
pub mod autorun;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd v2/src-tauri && cargo test --test autorun_store`
Expected: PASS — 6 passed.

- [ ] **Step 5: Commit**

```bash
git add v2/src-tauri/src/autorun v2/src-tauri/src/lib.rs v2/src-tauri/tests/autorun_store.rs
git commit -F - <<'EOF'
feat(v2): local scripts and run records for the supervised runner

An action script per case and a run record per session, both on this
machine only - nothing here has an Azure DevOps shape, and nothing
here is sent anywhere. The verdict field holds the HUMAN's word and a
test pins that every action can succeed while the person still says
Failed, which is the point of a supervised runner. A corrupt run file
is skipped rather than fatal: one bad file must not hide every other
run from the results view.
EOF
```

---

### Task 5: Tauri commands for a session

**Files:**
- Create: `v2/src-tauri/src/commands/autorun.rs`
- Modify: `v2/src-tauri/src/commands/mod.rs` (add `pub mod autorun;`)
- Modify: `v2/src-tauri/src/lib.rs` (register six commands in the specta builder list, beside `ai_tools::db_server_presets`)
- Modify: `v2/src/bindings.ts` (regenerated, never hand-edited)
- Test: `v2/src-tauri/tests/autorun_commands.rs`

**Interfaces:**
- Consumes: everything from Tasks 1–4.
- Produces these Tauri commands (TypeScript names in brackets):
  - `auto_run_open_browser() -> Result<(), String>` [`autoRunOpenBrowser`]
  - `auto_run_close_browser() -> Result<(), String>` [`autoRunCloseBrowser`]
  - `auto_run_step(step: StepScript) -> Result<Vec<ActionOutcome>, String>` [`autoRunStep`]
  - `auto_run_load_script(app, case_id: i32) -> Result<Option<CaseScript>, String>` [`autoRunLoadScript`]
  - `auto_run_save_script(app, script: CaseScript) -> Result<(), String>` [`autoRunSaveScript`]
  - `auto_run_save_run(app, run: LocalRun) -> Result<(), String>` [`autoRunSaveRun`]
  - `auto_run_list_runs(app) -> Vec<LocalRun>` [`autoRunListRuns`]
- Also produces `autorun::store::root(app: &tauri::AppHandle) -> Result<PathBuf, String>`.

- [ ] **Step 1: Write the failing test**

Create `v2/src-tauri/tests/autorun_commands.rs`:

```rust
//! The session's own rule: a step cannot run without an open browser,
//! and the failure says how to fix it rather than panicking on an
//! absent session.

use v2_lib::commands::autorun::describe_session_error;

#[test]
fn stepping_without_a_browser_says_to_open_one() {
    let msg = describe_session_error();
    assert!(msg.to_lowercase().contains("open"), "unhelpful: {msg}");
    assert!(msg.to_lowercase().contains("browser"), "unhelpful: {msg}");
}
```

Add to `v2/src-tauri/tests/bindings.rs` — no change needed; the existing `export_bindings` test regenerates and the `bindings_never_expose_a_token` test already guards the whole surface.

- [ ] **Step 2: Run test to verify it fails**

Run: `cd v2/src-tauri && cargo test --test autorun_commands`
Expected: FAIL to compile — `could not find 'autorun' in 'commands'`.

- [ ] **Step 3: Write minimal implementation**

Create `v2/src-tauri/src/commands/autorun.rs`:

```rust
//! The supervised runner's IPC surface.
//!
//! NOTHING here calls Azure DevOps. The browser session lives for as
//! long as the screen keeps it open; steps run against it one at a time,
//! driven by the human clicking through.

use crate::autorun::store;
use crate::autorun::{CaseScript, LocalRun, StepScript};
use crate::browser::actions::{execute, ActionOutcome, Evaluator};
use crate::browser::cdp::Cdp;
use crate::browser::launch::{launch, LaunchedBrowser};
use std::path::PathBuf;
use tauri::Manager;

/// The one live session. A second Open replaces the first, so a stray
/// browser can never leave the screen permanently stuck.
static SESSION: tokio::sync::Mutex<Option<Session>> = tokio::sync::Mutex::const_new(None);

struct Session {
    browser: LaunchedBrowser,
    cdp: Cdp,
}

impl Evaluator for Cdp {
    async fn eval(&mut self, expression: &str) -> Result<serde_json::Value, String> {
        Cdp::eval(self, expression).await
    }
}

/// Said when a step arrives with no browser behind it. Pulled out so a
/// test can hold the wording to account - "no session" would tell the
/// person nothing about what to do.
pub fn describe_session_error() -> String {
    "no browser is open - press Open browser first".to_string()
}

/// Where scripts and runs live: beside the app's other data.
pub fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("autorun"))
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_open_browser() -> Result<(), String> {
    let mut slot = SESSION.lock().await;
    if let Some(old) = slot.take() {
        close_session(old);
    }
    let browser = launch()?;
    // The browser needs a moment to bind its port before it will answer.
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let cdp = Cdp::connect(browser.port).await?;
    *slot = Some(Session { browser, cdp });
    crate::applog::info("Auto-run browser opened");
    Ok(())
}

fn close_session(mut s: Session) {
    let _ = s.browser.child.kill();
    let _ = std::fs::remove_dir_all(&s.browser.profile_dir);
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_close_browser() -> Result<(), String> {
    if let Some(s) = SESSION.lock().await.take() {
        close_session(s);
        crate::applog::info("Auto-run browser closed");
    }
    Ok(())
}

/// Run one step's actions in order and report every outcome. Actions
/// after a failure still run: the watcher learns more from "the click
/// worked, the check did not" than from a run that stops at the first
/// red.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_step(step: StepScript) -> Result<Vec<ActionOutcome>, String> {
    let mut slot = SESSION.lock().await;
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    let mut out = Vec::new();
    for action in &step.actions {
        out.push(execute(&mut session.cdp, action).await);
    }
    Ok(out)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_load_script(
    app: tauri::AppHandle,
    case_id: i32,
) -> Result<Option<CaseScript>, String> {
    store::load_script(&root(&app)?, case_id)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_save_script(app: tauri::AppHandle, script: CaseScript) -> Result<(), String> {
    store::save_script(&root(&app)?, &script)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_save_run(app: tauri::AppHandle, run: LocalRun) -> Result<(), String> {
    store::save_run(&root(&app)?, &run)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_list_runs(app: tauri::AppHandle) -> Vec<LocalRun> {
    match root(&app) {
        Ok(r) => store::list_runs(&r),
        Err(_) => vec![],
    }
}

/// A run id the frontend can stamp on a new session.
#[tauri::command]
#[specta::specta]
pub fn auto_run_new_id() -> String {
    store::new_run_id()
}
```

In `v2/src-tauri/src/commands/mod.rs`, add:

```rust
pub mod autorun;
```

In `v2/src-tauri/src/lib.rs`, add to the specta builder's command list, directly after `ai_tools::db_server_presets,`:

```rust
            autorun::auto_run_open_browser,
            autorun::auto_run_close_browser,
            autorun::auto_run_step,
            autorun::auto_run_load_script,
            autorun::auto_run_save_script,
            autorun::auto_run_save_run,
            autorun::auto_run_list_runs,
            autorun::auto_run_new_id,
```

- [ ] **Step 4: Run tests and regenerate bindings**

Run: `cd v2/src-tauri && cargo test --test autorun_commands && cargo test --test bindings && cargo test`
Expected: PASS. `src/bindings.ts` now contains `autoRunStep`, `CaseScript`, `LocalRun`, `Action`, `ActionOutcome`.

Then confirm the frontend still typechecks: `cd v2 && npx tsc --noEmit` → no errors.

- [ ] **Step 5: Commit**

```bash
git add v2/src-tauri/src/commands v2/src-tauri/src/lib.rs v2/src-tauri/tests/autorun_commands.rs v2/src/bindings.ts
git commit -F - <<'EOF'
feat(v2): IPC for a supervised browser session

Open a browser, run one step's actions against it, close it - plus
local script and run storage. Not one call in this module reaches
Azure DevOps. Two deliberate behaviours: a second Open replaces the
first, so a stray browser cannot leave the screen permanently stuck,
and the actions after a failure still run, because the watcher learns
more from "the click worked, the check did not" than from a run that
halts at the first red.
EOF
```

---

### Task 6: The Auto Run screen — sidebar entry and case list

**Files:**
- Modify: `v2/src/components/Sidebar.tsx:23` (extend `Section`), `:34-44` (add to `CASE_ITEMS`)
- Modify: `v2/src/index.css:753` (add the hover animation rule)
- Modify: `v2/src/App.tsx:715` (render the screen)
- Create: `v2/src/screens/AutoRun/index.tsx`
- Test: `v2/src/screens/AutoRun.test.tsx`

**Interfaces:**
- Consumes: `commands.autoRunLoadScript` from Task 5; the existing `commands.pbiTestCasesFull(org, pbiId, moduleRef, preconditionsRef)`.
- Produces: `AutoRun` default export taking `{ org: string; project: string; pbi: PbiHit | null }`.

- [ ] **Step 1: Write the failing test**

Create `v2/src/screens/AutoRun.test.tsx`:

```tsx
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import AutoRun from "./AutoRun";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const PBI = { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" };

function renderAutoRun() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="Web" pbi={PBI} />
    </QueryClientProvider>,
  );
}

const cases = [
  {
    id: 201,
    title: "Valid login",
    tags: "",
    automation_status: "Not Automated",
    steps: [{ action: "Open the login page", expected: "The form is shown" }],
    step_ids: ["2"],
    module_value: "",
    preconditions: "",
    steps_xml: "",
  },
  {
    id: 202,
    title: "Locked account",
    tags: "",
    automation_status: "Not Automated",
    steps: [{ action: "Sign in", expected: "A lockout message appears" }],
    step_ids: ["2"],
    module_value: "",
    preconditions: "",
    steps_xml: "",
  },
];

/// Most cases have no script, and that is the normal state - the screen
/// has to say which are drivable rather than looking broken.
test("lists the PBI's cases and marks which ones have a script", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201
        ? { case_id: 201, title: "Valid login", steps: [{ step_number: 1, actions: [] }] }
        : null;
    }
  });
  renderAutoRun();

  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Locked account")).toBeInTheDocument();
  expect(await screen.findByText("Script ready")).toBeInTheDocument();
  expect(screen.getByText("No script")).toBeInTheDocument();
});

/// The whole feature is local. Saying so on the screen is what stops
/// someone assuming a green run updated Azure DevOps.
test("says plainly that nothing reaches Azure DevOps", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
  });
  renderAutoRun();
  expect(await screen.findByText(/nothing is sent to azure devops/i)).toBeInTheDocument();
});

test("without a PBI it asks for one instead of loading", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="Web" pbi={null} />
    </QueryClientProvider>,
  );
  expect(await screen.findByText(/pick a pbi/i)).toBeInTheDocument();
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd v2 && npx vitest run src/screens/AutoRun.test.tsx`
Expected: FAIL — cannot resolve `./AutoRun`.

- [ ] **Step 3: Write minimal implementation**

Create `v2/src/screens/AutoRun/index.tsx`:

```tsx
// Supervised auto-run: the app drives a real Edge window through a
// case's steps while a person watches and decides the verdict.
//
// LOCAL ONLY. Nothing on this screen writes to Azure DevOps - the case
// list is read from it, and the results stay in this app until the
// feature has earned more trust than that.

import { useQueries, useQuery } from "@tanstack/react-query";
import { commands, type PbiHit } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { unwrap } from "../../lib/ipc";

export default function AutoRun({
  org,
  project,
  pbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
}) {
  const { prefs } = useFieldRefs(org, project);

  const cases = useQuery({
    queryKey: ["autorun-cases", org, pbi?.id, prefs.moduleRef, prefs.preconditionsRef],
    queryFn: () =>
      unwrap(
        commands.pbiTestCasesFull(org, pbi!.id, prefs.moduleRef, prefs.preconditionsRef),
      ),
    enabled: Boolean(org && pbi),
    retry: false,
  });

  // One script lookup per case, so the list can say which are drivable.
  const scripts = useQueries({
    queries: (cases.data ?? []).map((c) => ({
      queryKey: ["autorun-script", c.id],
      queryFn: () => unwrap(commands.autoRunLoadScript(c.id)),
      retry: false,
    })),
  });

  if (!org || !pbi) {
    return <p className="text-sm text-muted">Pick a PBI in the bar above to auto-run its cases.</p>;
  }

  return (
    <div className="max-w-3xl space-y-4">
      <p className="rounded-md border border-accent/40 bg-accent-soft px-3 py-2 text-xs text-muted">
        Runs happen in a real Edge window on this machine and you decide every verdict.
        Nothing is sent to Azure DevOps - results are saved here only.
      </p>

      {cases.isLoading && <p className="text-sm text-muted">Loading test cases…</p>}
      {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}

      <ul className="space-y-1">
        {(cases.data ?? []).map((c, i) => (
          <li
            key={c.id}
            className="flex items-center gap-2 rounded-md border border-border bg-surface px-3 py-2 text-sm"
          >
            <span className="id-mono text-faint">#{c.id}</span>
            <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
            {scripts[i]?.data ? (
              <Badge className="bg-success/15 text-success">Script ready</Badge>
            ) : (
              <Badge className="bg-surface-2 text-faint">No script</Badge>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
```

In `v2/src/components/Sidebar.tsx`, extend the union on line 23:

```tsx
export type Section = "manual" | "import" | "edit" | "view" | "run" | "autorun" | "suites" | "ai" | "settings";
```

Add `Bot`'s neighbour to the lucide import at the top of the same file:

```tsx
  Radar,
```

And add the item to `CASE_ITEMS`, directly after the `run` entry:

```tsx
  // A radar sweep, not a second play button: Run Tests owns the play
  // glyph, and the rail has to stay scannable at 16px.
  { id: "autorun", label: "Auto Run", icon: Radar, tone: "nav-ico nav-ico-autorun" },
```

In `v2/src/index.css`, beside the other nav hover rules (after line 753):

```css
.group:hover .nav-ico-autorun { animation: ico-sway 0.5s ease; }
```

In `v2/src/App.tsx`, add the import beside the other screens:

```tsx
import AutoRun from "./screens/AutoRun";
```

And render it directly before the `{section === "ai" && <AiBridge />}` line:

```tsx
              {section === "autorun" && <AutoRun org={org} project={project} pbi={pbi} />}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd v2 && npx vitest run src/screens/AutoRun.test.tsx && npx tsc --noEmit`
Expected: PASS — 3 passed, no type errors.

- [ ] **Step 5: Commit**

```bash
git add v2/src/screens/AutoRun v2/src/screens/AutoRun.test.tsx v2/src/components/Sidebar.tsx v2/src/index.css v2/src/App.tsx
git commit -F - <<'EOF'
feat(v2): an Auto Run screen listing a PBI's drivable cases

A new tab that reads the PBI's cases and marks which ones have a local
action script - most will not, and the list says so rather than
looking broken. The banner states the rule the whole feature rests on:
this runs in a real browser on this machine, the human decides every
verdict, and nothing reaches Azure DevOps.
EOF
```

---

### Task 7: Script editor per case

**Files:**
- Create: `v2/src/screens/AutoRun/ScriptEditor.tsx`
- Modify: `v2/src/screens/AutoRun/index.tsx` (open the editor from a row)
- Modify: `v2/src/screens/AutoRun.test.tsx` (append the new tests)

**Interfaces:**
- Consumes: `commands.autoRunSaveScript`, `commands.autoRunLoadScript` from Task 5; `CaseScript` from bindings.
- Produces: `ScriptEditor` default export taking `{ caseId: number; title: string; steps: { action: string; expected: string }[]; onClose: () => void }`.

- [ ] **Step 1: Write the failing test**

Append to `v2/src/screens/AutoRun.test.tsx`:

```tsx
import { fireEvent, waitFor } from "@testing-library/react";

/// The script is authored as JSON for now - an assistant will generate
/// these later, and hand-editing is how the format gets proven first.
/// Invalid JSON must be refused at the point of saving, not written and
/// discovered mid-run.
test("the script editor refuses invalid JSON instead of saving it", async () => {
  let saved = 0;
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_save_script") {
      saved++;
      return null;
    }
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Edit script for #201" }));
  const box = await screen.findByLabelText("Action script JSON");
  fireEvent.change(box, { target: { value: "{ not json" } });
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));

  expect(await screen.findByText(/not valid json/i)).toBeInTheDocument();
  expect(saved).toBe(0);
});

test("a valid script is saved for that case id", async () => {
  let payload: Record<string, unknown> | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_save_script") {
      payload = args as Record<string, unknown>;
      return null;
    }
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Edit script for #201" }));
  const box = await screen.findByLabelText("Action script JSON");
  fireEvent.change(box, {
    target: {
      value: JSON.stringify([
        { step_number: 1, actions: [{ kind: "check_text", value: "Dashboard" }] },
      ]),
    },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));

  await waitFor(() => expect(payload).not.toBeNull());
  const script = (payload as unknown as { script: { case_id: number; steps: unknown[] } }).script;
  expect(script.case_id).toBe(201);
  expect(script.steps).toHaveLength(1);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd v2 && npx vitest run src/screens/AutoRun.test.tsx`
Expected: FAIL — no button named "Edit script for #201".

- [ ] **Step 3: Write minimal implementation**

Create `v2/src/screens/AutoRun/ScriptEditor.tsx`:

```tsx
// Authoring the actions for one case, as JSON.
//
// JSON on purpose, for now: an assistant will generate these scripts
// later, and the format has to be proven by hand before anything
// generates it. The case's own steps sit alongside as the reference.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type StepScript } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { unwrap } from "../../lib/ipc";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";

const PLACEHOLDER = `[
  {
    "step_number": 1,
    "actions": [
      { "kind": "navigate", "url": "https://app.example/login" },
      { "kind": "wait_for", "selector": "#user", "timeout_ms": 5000 },
      { "kind": "fill", "selector": "#user", "value": "tester" },
      { "kind": "click", "selector": "text=Sign in" },
      { "kind": "check_text", "value": "Dashboard" }
    ]
  }
]`;

export default function ScriptEditor({
  caseId,
  title,
  steps,
  onClose,
}: {
  caseId: number;
  title: string;
  steps: { action: string; expected: string }[];
  onClose: () => void;
}) {
  const existing = useQuery({
    queryKey: ["autorun-script", caseId],
    queryFn: () => unwrap(commands.autoRunLoadScript(caseId)),
    retry: false,
  });

  const [text, setText] = useState<string | null>(null);
  const [problem, setProblem] = useState("");
  const value =
    text ?? (existing.data ? JSON.stringify(existing.data.steps, null, 2) : "");

  const save = async () => {
    let parsed: StepScript[];
    try {
      parsed = JSON.parse(value || "[]") as StepScript[];
    } catch (e) {
      setProblem(`That is not valid JSON: ${(e as Error).message}`);
      return;
    }
    if (!Array.isArray(parsed)) {
      setProblem("That is not valid JSON: the script must be an array of steps.");
      return;
    }
    setProblem("");
    const r = await commands.autoRunSaveScript({ case_id: caseId, title, steps: parsed });
    if (r.status === "error") {
      toast.error(`Could not save the script: ${r.error}`);
      return;
    }
    toast.success("Script saved.");
    onClose();
  };

  return (
    <Modal onClose={onClose} className="w-full max-w-3xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">
        <span className="id-mono text-faint">#{caseId}</span> {title}
      </h2>

      <div className="grid gap-3 lg:grid-cols-2">
        <div className="space-y-1">
          <span className="text-xs font-medium text-muted">The case's steps</span>
          <ol className="max-h-64 space-y-1 overflow-y-auto text-xs text-muted">
            {steps.map((s, i) => (
              <li key={i} className="rounded border border-border/60 px-2 py-1">
                <span className="text-text">
                  {i + 1}. {s.action}
                </span>
                {s.expected && <div className="text-faint">→ {s.expected}</div>}
              </li>
            ))}
          </ol>
        </div>

        <label className="block text-xs text-muted">
          Action script JSON
          <Textarea
            aria-label="Action script JSON"
            className="mt-1 h-64 w-full font-mono text-xs"
            placeholder={PLACEHOLDER}
            value={value}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
      </div>

      {problem && <p className="text-xs text-danger">{problem}</p>}

      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" onClick={save}>
          <IconConfirm aria-hidden />
          Save script
        </Button>
      </div>
    </Modal>
  );
}
```

In `v2/src/screens/AutoRun/index.tsx`, add the state and the row button. Add to the imports:

```tsx
import { useState } from "react";
import ScriptEditor from "./ScriptEditor";
import { Button } from "../../components/ui/button";
import { IconEdit } from "../../lib/actionIcons";
```

Add inside the component, above the `if (!org || !pbi)` guard:

```tsx
  const [editing, setEditing] = useState<number | null>(null);
```

Add the button inside the `<li>`, after the Badge block:

```tsx
            <Button
              size="sm"
              variant="outline"
              aria-label={`Edit script for #${c.id}`}
              onClick={() => setEditing(c.id)}
            >
              <IconEdit aria-hidden />
              Script
            </Button>
```

And render the editor just before the closing `</div>` of the component's return:

```tsx
      {editing != null &&
        (() => {
          const c = (cases.data ?? []).find((x) => x.id === editing);
          if (!c) return null;
          return (
            <ScriptEditor
              caseId={c.id}
              title={c.title}
              steps={c.steps}
              onClose={() => setEditing(null)}
            />
          );
        })()}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd v2 && npx vitest run src/screens/AutoRun.test.tsx && npx tsc --noEmit`
Expected: PASS — 5 passed, no type errors.

- [ ] **Step 5: Commit**

```bash
git add v2/src/screens/AutoRun v2/src/screens/AutoRun.test.tsx
git commit -F - <<'EOF'
feat(v2): author a case's action script by hand

JSON on purpose: an assistant will generate these later, and the
format has to be proven by hand before anything generates it. The
case's own steps sit beside the editor as the reference. Invalid JSON
is refused at the point of saving with the parser's own complaint,
rather than being written and discovered mid-run.
EOF
```

---

### Task 8: Run a case — execute steps, watch, and mark the verdict

**Files:**
- Create: `v2/src/screens/AutoRun/RunPane.tsx`
- Modify: `v2/src/screens/AutoRun/index.tsx` (a Run button per scripted row, and the pane)
- Modify: `v2/src/screens/AutoRun.test.tsx` (append the new tests)

**Interfaces:**
- Consumes: `commands.autoRunOpenBrowser`, `autoRunCloseBrowser`, `autoRunStep`, `autoRunSaveRun`, `autoRunNewId` from Task 5.
- Produces: `RunPane` default export taking `{ pbiId: number; caseId: number; title: string; onClose: () => void }`.

- [ ] **Step 1: Write the failing test**

Append to `v2/src/screens/AutoRun.test.tsx`:

```tsx
const scriptFor201 = {
  case_id: 201,
  title: "Valid login",
  steps: [
    { step_number: 1, actions: [{ kind: "check_text", value: "Dashboard" }] },
    { step_number: 2, actions: [{ kind: "click", selector: "text=Sign out" }] },
  ],
};

function mockRunnable(extra?: (cmd: string, args: unknown) => unknown) {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201 ? scriptFor201 : null;
    }
    if (cmd === "auto_run_new_id") return "run-1786000000000";
    if (cmd === "auto_run_open_browser") return null;
    if (cmd === "auto_run_close_browser") return null;
    return extra?.(String(cmd), args);
  });
}

/// The outcomes are EVIDENCE, not a vote: the pane shows what each
/// action reported and leaves the verdict buttons untouched.
test("running a step shows each action's outcome and picks no verdict", async () => {
  mockRunnable((cmd) => {
    if (cmd === "auto_run_step")
      return [{ ok: true, detail: "page contains Dashboard" }];
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run step 1" }));

  expect(await screen.findByText("page contains Dashboard")).toBeInTheDocument();
  // Nothing is pre-selected - the human has not decided yet.
  expect(screen.getByRole("button", { name: "Passed" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  expect(screen.getByRole("button", { name: "Failed" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
});

/// A failed action is reported plainly and STILL does not decide the
/// verdict - the person may know the failure is the harness's fault.
test("a failed action is shown but the human still chooses", async () => {
  mockRunnable((cmd) => {
    if (cmd === "auto_run_step") return [{ ok: false, detail: "not found: #nope" }];
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run step 1" }));

  expect(await screen.findByText("not found: #nope")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Passed" })).toBeEnabled();
});

/// Saving records the human's verdict and the evidence together, into a
/// LOCAL run - and never calls anything that writes to Azure DevOps.
test("saving stores the verdict locally and touches no ADO command", async () => {
  let saved: Record<string, unknown> | null = null;
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    calls.push(String(cmd));
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201 ? scriptFor201 : null;
    }
    if (cmd === "auto_run_new_id") return "run-1786000000000";
    if (cmd === "auto_run_open_browser") return null;
    if (cmd === "auto_run_step") return [{ ok: true, detail: "page contains Dashboard" }];
    if (cmd === "auto_run_save_run") {
      saved = args as Record<string, unknown>;
      return null;
    }
    if (cmd === "auto_run_close_browser") return null;
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run step 1" }));
  await screen.findByText("page contains Dashboard");
  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: "Save result" }));

  await waitFor(() => expect(saved).not.toBeNull());
  const run = (saved as unknown as { run: { cases: { verdict: string }[] } }).run;
  expect(run.cases[0].verdict).toBe("Failed");

  // The guard that matters: no run-recording command was ever invoked.
  expect(calls).not.toContain("start_test_run");
  expect(calls).not.toContain("record_result");
  expect(calls).not.toContain("finish_test_run");
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd v2 && npx vitest run src/screens/AutoRun.test.tsx`
Expected: FAIL — no button named "Run #201".

- [ ] **Step 3: Write minimal implementation**

Create `v2/src/screens/AutoRun/RunPane.tsx`:

```tsx
// Driving one case while a person watches.
//
// The action outcomes are EVIDENCE, never a vote. Nothing here
// pre-selects a verdict: a green run can still be a failure the person
// spotted with their eyes, and a red action can be the harness's fault
// rather than the app's. The human presses the button.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type ActionOutcome, type CaseRecord } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Textarea } from "../../components/ui/input";
import { cn } from "../../lib/cn";
import { unwrap } from "../../lib/ipc";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";

const VERDICTS = ["Passed", "Failed", "Blocked"] as const;

const verdictTone: Record<string, string> = {
  Passed: "bg-success/20 text-success",
  Failed: "bg-danger/20 text-danger",
  Blocked: "bg-warning/20 text-warning",
};

export default function RunPane({
  pbiId,
  caseId,
  title,
  onClose,
}: {
  pbiId: number;
  caseId: number;
  title: string;
  onClose: () => void;
}) {
  const script = useQuery({
    queryKey: ["autorun-script", caseId],
    queryFn: () => unwrap(commands.autoRunLoadScript(caseId)),
    retry: false,
  });

  const [opened, setOpened] = useState(false);
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<Record<number, ActionOutcome[]>>({});
  const [verdict, setVerdict] = useState("");
  const [note, setNote] = useState("");

  const openBrowser = async () => {
    setBusy(true);
    const r = await commands.autoRunOpenBrowser();
    setBusy(false);
    if (r.status === "error") {
      toast.error(`Could not open the browser: ${r.error}`);
      return;
    }
    setOpened(true);
  };

  const runStep = async (stepNumber: number) => {
    const step = script.data?.steps.find((s) => s.step_number === stepNumber);
    if (!step) return;
    setBusy(true);
    const r = await commands.autoRunStep(step);
    setBusy(false);
    if (r.status === "error") {
      toast.error(r.error);
      return;
    }
    setResults((prev) => ({ ...prev, [stepNumber]: r.data }));
  };

  const save = async () => {
    const idRes = await commands.autoRunNewId();
    const record: CaseRecord = {
      case_id: caseId,
      title,
      verdict,
      note,
      steps: (script.data?.steps ?? []).map((s) => ({
        step_number: s.step_number,
        outcomes: results[s.step_number] ?? [],
      })),
    };
    const r = await commands.autoRunSaveRun({
      id: idRes,
      pbi_id: pbiId,
      started_at: String(Date.now()),
      cases: [record],
    });
    if (r.status === "error") {
      toast.error(`Could not save the result: ${r.error}`);
      return;
    }
    toast.success("Result saved on this machine.");
    await commands.autoRunCloseBrowser();
    onClose();
  };

  const close = async () => {
    await commands.autoRunCloseBrowser();
    onClose();
  };

  return (
    <Modal onClose={close} className="w-full max-w-2xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">
        <span className="id-mono text-faint">#{caseId}</span> {title}
      </h2>

      {!opened ? (
        <div className="space-y-2">
          <p className="text-xs text-muted">
            A real Edge window opens with a fresh profile. Keep it beside this one and watch
            each step as it runs.
          </p>
          <Button size="sm" disabled={busy} onClick={openBrowser}>
            Open browser
          </Button>
        </div>
      ) : (
        <ul className="max-h-72 space-y-2 overflow-y-auto">
          {(script.data?.steps ?? []).map((s) => (
            <li key={s.step_number} className="rounded-md border border-border p-2">
              <div className="flex items-center gap-2">
                <span className="text-xs font-medium text-muted">Step {s.step_number}</span>
                <span className="text-[11px] text-faint">
                  {s.actions.length} action{s.actions.length === 1 ? "" : "s"}
                </span>
                <Button
                  className="ml-auto"
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() => runStep(s.step_number)}
                >
                  Run step {s.step_number}
                </Button>
              </div>
              {(results[s.step_number] ?? []).map((o, i) => (
                <p
                  key={i}
                  className={cn("mt-1 text-xs", o.ok ? "text-muted" : "text-danger")}
                >
                  {o.detail}
                </p>
              ))}
            </li>
          ))}
        </ul>
      )}

      <div className="space-y-2 border-t border-border pt-3">
        <span className="text-xs font-medium text-muted">Your verdict</span>
        <div className="flex gap-2">
          {VERDICTS.map((v) => (
            <button
              key={v}
              aria-pressed={verdict === v}
              className={cn(
                "rounded-md border border-border px-3 py-1.5 text-xs font-medium transition-colors",
                verdict === v ? verdictTone[v] : "text-muted hover:border-border-strong",
              )}
              onClick={() => setVerdict(v)}
            >
              {v}
            </button>
          ))}
        </div>
        <Textarea
          aria-label="Result note"
          className="h-16 w-full text-xs"
          placeholder="What you saw (optional)"
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
      </div>

      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={close}>
          <IconCancel aria-hidden />
          Close
        </Button>
        <Button size="sm" disabled={!verdict} onClick={save}>
          <IconConfirm aria-hidden />
          Save result
        </Button>
      </div>
    </Modal>
  );
}
```

In `v2/src/screens/AutoRun/index.tsx`, add the import:

```tsx
import RunPane from "./RunPane";
```

Add the state beside `editing`:

```tsx
  const [running, setRunning] = useState<number | null>(null);
```

Add the Run button inside the `<li>`, after the Script button — only for scripted cases:

```tsx
            {scripts[i]?.data && (
              <Button
                size="sm"
                aria-label={`Run #${c.id}`}
                onClick={() => setRunning(c.id)}
              >
                Run
              </Button>
            )}
```

And render the pane beside the editor:

```tsx
      {running != null &&
        (() => {
          const c = (cases.data ?? []).find((x) => x.id === running);
          if (!c) return null;
          return (
            <RunPane
              pbiId={pbi.id}
              caseId={c.id}
              title={c.title}
              onClose={() => setRunning(null)}
            />
          );
        })()}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd v2 && npx vitest run src/screens/AutoRun.test.tsx && npx tsc --noEmit && npx vitest run`
Expected: PASS — 8 in the AutoRun file, whole suite green, no type errors.

- [ ] **Step 5: Commit**

```bash
git add v2/src/screens/AutoRun v2/src/screens/AutoRun.test.tsx
git commit -F - <<'EOF'
feat(v2): run a case step by step and mark it yourself

The pane opens a real Edge window, runs one step at a time, and prints
what every action reported. The outcomes are evidence, never a vote:
no verdict is pre-selected, because a green run can still be a failure
the person saw with their eyes and a red action can be the harness's
fault rather than the app's. Saving writes the human's verdict and the
evidence to a local run file - a test asserts that no run-recording
command is called anywhere in the flow.
EOF
```

---

### Task 9: Local results view

**Files:**
- Create: `v2/src/screens/AutoRun/PastRuns.tsx`
- Modify: `v2/src/screens/AutoRun/index.tsx` (render it under the case list)
- Modify: `v2/src/screens/AutoRun.test.tsx` (append the new tests)

**Interfaces:**
- Consumes: `commands.autoRunListRuns` from Task 5.
- Produces: `PastRuns` default export taking no props.

- [ ] **Step 1: Write the failing test**

Append to `v2/src/screens/AutoRun.test.tsx`:

```tsx
test("past runs list newest first with their verdicts", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs")
      return [
        {
          id: "run-2",
          pbi_id: 42,
          started_at: "1786000200000",
          cases: [
            { case_id: 202, title: "Locked account", verdict: "Passed", note: "", steps: [] },
          ],
        },
        {
          id: "run-1",
          pbi_id: 42,
          started_at: "1786000100000",
          cases: [
            { case_id: 201, title: "Valid login", verdict: "Failed", note: "wrong name", steps: [] },
          ],
        },
      ];
  });
  renderAutoRun();

  expect(await screen.findByText("Locked account")).toBeInTheDocument();
  const rows = await screen.findAllByRole("listitem", { name: /run of/i });
  expect(rows[0]).toHaveTextContent("Passed");
  expect(rows[1]).toHaveTextContent("Failed");
  expect(screen.getByText("wrong name")).toBeInTheDocument();
});

test("no past runs says so rather than showing an empty box", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs") return [];
  });
  renderAutoRun();
  expect(await screen.findByText(/no runs on this machine yet/i)).toBeInTheDocument();
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd v2 && npx vitest run src/screens/AutoRun.test.tsx`
Expected: FAIL — no listitem matching /run of/i.

- [ ] **Step 3: Write minimal implementation**

Create `v2/src/screens/AutoRun/PastRuns.tsx`:

```tsx
// Everything this machine has run. Read straight off disk - these
// results exist nowhere else, which is the whole arrangement while the
// feature earns trust.

import { useQuery } from "@tanstack/react-query";
import { commands } from "../../bindings";
import { cn } from "../../lib/cn";

const verdictTone: Record<string, string> = {
  Passed: "text-success",
  Failed: "text-danger",
  Blocked: "text-warning",
};

/** Epoch milliseconds as a string; the Rust side sends it that way
 * because specta will not carry a u64 across IPC. */
function when(startedAt: string): string {
  const n = Number(startedAt);
  if (!Number.isFinite(n) || n <= 0) return "unknown time";
  return new Date(n).toLocaleString();
}

export default function PastRuns() {
  const runs = useQuery({
    queryKey: ["autorun-runs"],
    queryFn: () => commands.autoRunListRuns(),
    retry: false,
  });

  return (
    <div className="space-y-2">
      <h2 className="text-sm font-semibold text-muted">Past runs (this machine)</h2>
      {(runs.data?.length ?? 0) === 0 && (
        <p className="text-xs text-muted">No runs on this machine yet.</p>
      )}
      <ul className="space-y-1">
        {(runs.data ?? []).flatMap((run) =>
          run.cases.map((c) => (
            <li
              key={`${run.id}-${c.case_id}`}
              aria-label={`Run of ${c.title}`}
              className="rounded-md border border-border bg-surface px-3 py-2 text-sm"
            >
              <div className="flex items-center gap-2">
                <span className="id-mono text-faint">#{c.case_id}</span>
                <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
                <span className={cn("text-xs font-medium", verdictTone[c.verdict] ?? "text-faint")}>
                  {c.verdict || "—"}
                </span>
                <span className="shrink-0 text-[11px] text-faint">{when(run.started_at)}</span>
              </div>
              {c.note && <p className="mt-0.5 text-xs text-muted">{c.note}</p>}
            </li>
          )),
        )}
      </ul>
    </div>
  );
}
```

In `v2/src/screens/AutoRun/index.tsx`, add the import:

```tsx
import PastRuns from "./PastRuns";
```

And render it after the case list `</ul>`:

```tsx
      <PastRuns />
```

- [ ] **Step 4: Run the whole gate**

Run, all from `v2`:

```bash
npx tsc --noEmit
npx vitest run
```

Then: `cd src-tauri && cargo test`

Expected: no type errors, every frontend test passing (10 in the AutoRun file), every Rust test passing.

- [ ] **Step 5: Commit**

```bash
git add v2/src/screens/AutoRun v2/src/screens/AutoRun.test.tsx
git commit -F - <<'EOF'
feat(v2): a local results view for supervised runs

Every run this machine has recorded, newest first, with the verdict
the person gave and the note they left. Read straight off disk -
these results exist nowhere else, which is the whole arrangement until
the runner has earned more trust than that. An empty list says so
rather than showing a bare box.
EOF
```

---

## Deliberately Out of Scope

Named here so a later reader knows they were decided, not forgotten:

- **Any ADO write.** No test-run creation, no result recording, no bug filing. Adding it later means one new command that maps a `LocalRun` onto the existing `start_test_run` / `record_result` / `finish_test_run` protocol — the local shape was designed to make that a small step, not a rewrite.
- **LLM script generation.** The JSON script format exists so an assistant can write it later. Proving the format by hand first is the point of Task 7.
- **Screenshots and video.** The person is watching the real browser, which is better evidence than a still. Worth adding when runs start being reviewed after the fact.
- **Running a whole suite unattended.** Every rule in this plan assumes a human present.
- **Database assertions.** The `phr-db-mcp` connection could verify state behind the UI. That is a genuine multiplier and a separate plan.
- **Chrome and Firefox.** Edge only, matching the app's Windows-first stance.
