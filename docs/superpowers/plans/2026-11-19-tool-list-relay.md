# Tool List Without a Restart Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The running app answers `tools/list` and `tools/call`; the `--mcp` connector relays them and tells the client when the list changes, so updates and module switches need no editor restart.

**Architecture:** The app gains `POST /mcp` (runs today's `mcp::handle_message_shared` in-process, with a loopback `BridgeCall` that carries the relaying connector's identity) and `GET /mcp/list-version`. The connector answers `initialize` itself (now with `tools.listChanged`), relays every other request, falls back to today's local handling when the app is down or too old, and polls the list version to send `notifications/tools/list_changed`.

**Tech Stack:** Rust (Tauri 2 app, `reqwest::blocking` in the connector, the app's existing bridge server in `src-tauri/src/ai_bridge/server.rs`).

**Spec:** `docs/superpowers/specs/2026-11-19-tool-list-without-restart-design.md`

## Global Constraints

- Tool names stay `mcp__tcm__...`; no tool is renamed, added or removed by this work.
- `/mcp` and `/mcp/list-version` sit behind the bridge's existing `x-bridge-token` like every other route. No new port, no new secret, no token in the webview.
- Never log a tool's arguments or a relayed message body; log the method and tool name only.
- No HTTP DELETE anywhere. User-facing text names no URL.
- A closed app keeps today's behaviour: the connector answers from its own code (saved modules, Simple-hidden tools) - tools never vanish because TCM is closed.
- Poll interval: 5 s. Relay timeout: 310 s (today's 300 s bridge timeout plus margin), so no tool call is cut shorter than today.
- Rust tests only in `src-tauri/tests/suite/` (one module per file, `mod` line in `suite/main.rs`); process-wide state under a `suite/serial.rs` lock.
- One build/test command at a time; never set `CARGO_TARGET_DIR`. Commits by bash heredoc. "Auto Test" in user-facing text.

## Review Focus

1. Two editors (two connectors) relaying at the same moment: each tool call reaches the app's routes with its own connector's agent id, pid and client - never the other's, never the app's. (Task 1 test `two_relays_at_once_keep_their_own_identity`.)
2. TCM restarts (new port and token) while a connector keeps running: the next relay reads the new handshake, and the watcher counts down-then-up as a change. (Task 2 `a_relay_after_an_app_restart_uses_the_new_handshake`; Task 3 `down_then_up_is_a_change`.)
3. A new connector with an older app (no `/mcp` route, 404): answers come from the local path and equal today's; the watcher never sends a notice. (Task 2 `an_app_without_the_route_is_answered_locally`; Task 3 `an_old_app_never_triggers_a_notice`.)
4. A tool call that takes minutes (`get_run_results` on a large org) through the relay is not cut short before 300 s. (Task 2 `relay_timeout_is_not_shorter_than_the_bridge_timeout`.)
5. Messages with no id (`notifications/initialized`, `notifications/cancelled`) are never relayed and never answered. (Task 2 `notifications_are_not_relayed`.)

---

### Task 1: The app answers MCP messages (`POST /mcp`, `GET /mcp/list-version`)

**Files:**
- Create: `src-tauri/src/mcp_relay.rs` (`pub mod mcp_relay;` in `lib.rs`)
- Modify: `src-tauri/src/mcp.rs` (relayed identity), `src-tauri/src/ai_bridge/mod.rs` (route table), `src-tauri/src/ai_bridge/server.rs` only if the route needs the request's agent headers passed through
- Test: `src-tauri/tests/suite/mcp_relay.rs`

**Interfaces:**
- Produces in `mcp.rs`:
  - `pub struct RelayedIdentity { pub agent_id: String, pub pid: Option<String>, pub client: Option<String> }`
  - `pub fn with_relayed_identity<T>(id: RelayedIdentity, f: impl FnOnce() -> T) -> T` - a thread-local in force for `f`; `send_to_bridge_within` sends `id`'s values in `AGENT_HEADER` / `AGENT_PID_HEADER` / `AGENT_CLIENT_HEADER` instead of `proxy_agent_id()`, the process id and `client_name()` while it is set.
- Produces in `mcp_relay.rs`:
  - `pub const RELAY_PATH: &str = "/mcp";` and `pub const LIST_VERSION_PATH: &str = "/mcp/list-version";`
  - `pub fn answer(body: &str, id: RelayedIdentity, call: mcp::BridgeCall) -> Result<String, String>` - body `{"session": "<1-64 chars [A-Za-z0-9-]>", "message": <json-rpc object>}`; answers `{"reply": <json-rpc reply string parsed as JSON, or null>}`. A bad body or session id is `Err` (route answers 400 with a short sentence). Runs `mcp::handle_message_shared` with the session's `SessionBook` and `id` in force.
  - `pub fn list_version(call: mcp::BridgeCall) -> String` - 16 lowercase hex chars: a hash of the `result` of a `tools/list` answered with no session and no relayed identity.
  - Sessions: `SessionBook` per session id in a process-wide map; an entry unused for `mcp_session::IDLE_RESET_MS` is dropped on the next `answer`. At most 64 sessions; past that the least recently used goes.
- The route's `BridgeCall` is a loopback to the app's own bridge (its port and `BridgeState::token`), run on a blocking thread (`tokio::task::spawn_blocking` or the bridge's existing blocking pattern) - the builder picks direct dispatch or loopback; loopback is the default because the routes read the agent headers from the request.

- [ ] **Step 1: Write the failing tests** in `src-tauri/tests/suite/mcp_relay.rs`, with a test listener in the style the suite already uses for `send_to_bridge` (it records each request's headers and answers `/tools` like the app):
  - `a_relayed_tools_list_equals_the_local_answer` - `answer({"session":"s1","message":{"jsonrpc":"2.0","id":1,"method":"tools/list"}})`'s `reply` equals `mcp::handle_message` of the same message against the same listener.
  - `a_relayed_call_carries_the_connectors_identity` - inside `answer` with `RelayedIdentity { agent_id: "a".repeat(32), pid: Some("4242".into()), client: Some("claude-code".into()) }`, a `tools/call` of `get_current_pbi` reaches the listener with exactly those three header values.
  - `two_relays_at_once_keep_their_own_identity` - two threads relay at once with different identities; each request the listener sees carries its own thread's id (run 20 rounds).
  - `sessions_keep_role_notes_apart` - a `get_guide` with `role` marks the role read in session `s1` only: the first role-starting call in `s2` still gets the role's rules ahead of its answer.
  - `a_bad_body_or_session_is_refused` - not JSON, missing `message`, session `""`, session of 65 chars, session `"a b"` -> `Err`.
  - `a_message_with_no_id_gets_a_null_reply` - `{"jsonrpc":"2.0","method":"notifications/initialized"}` -> `{"reply":null}`.
  - `the_list_version_changes_with_the_tools_answer` - `/tools` answering `{"disabled":[],"modules":["auto_run"]}` then `{"disabled":["get_bugs"],"modules":["auto_run"]}` gives two different 16-hex versions; the same answer twice gives the same version.
  - Route level (with the real bridge, under the suite's bridge lock): `POST /mcp` without the token is refused like any route; with it, a `tools/list` comes back.
- [ ] **Step 2: Run them to see them fail** - `cd src-tauri && cargo test --test suite mcp_relay::` - FAIL: module `mcp_relay` not found.
- [ ] **Step 3: Implement** `RelayedIdentity` / `with_relayed_identity` in `mcp.rs`, `mcp_relay.rs` as in Interfaces, and the two routes in the bridge's route table (POST `/mcp` reads the agent headers of the incoming request into `RelayedIdentity`; a request without `AGENT_HEADER` is answered 400 "a relayed message needs the connector's id"). Log `mcp relay: <method> <tool name or ->` at info, nothing else of the body.
- [ ] **Step 4: Run them to see them pass** - `cargo test --test suite mcp_relay::` - PASS. Then `cargo test --test bindings` (bindings.ts must not change: no command signature changes) and the full `cargo test --test suite`.
- [ ] **Step 5: Commit** - `feat(v2): the app answers MCP messages relayed by the connector`.

### Task 2: The connector relays, and falls back to answering itself

**Files:**
- Modify: `src-tauri/src/mcp.rs` (`answer_with`'s `initialize`, `serve_lines` / `answer_call_line` path, `run_stdio_proxy`)
- Test: `src-tauri/tests/suite/mcp_relay_connector.rs`

**Interfaces:**
- Consumes: `mcp_relay::RELAY_PATH`, the `{"session","message"}` / `{"reply"}` shapes from Task 1.
- Produces in `mcp.rs`:
  - `initialize` answers `"capabilities": { "tools": { "listChanged": true } }` (local and relayed alike - `initialize` is always answered by the connector).
  - `pub const RELAY_TIMEOUT: Duration = Duration::from_secs(310);`
  - `pub fn relay_or_local(msg: &str, session: &str, relay: &dyn Fn(&str) -> Result<(u16, String), String>, local: &dyn Fn(&str) -> Option<String>) -> Option<String>` - no id: `None`, nothing sent; `initialize`: `local`; anything else: `relay` with the `{"session","message"}` body; a 200 `{"reply": ...}` is the answer (null reply -> `None`); a transport error, a non-200 (404 from an older app included) or an unreadable body -> `local(msg)`.
  - The session id: a fresh `ai_bridge::new_token()` per process, renewed on every `initialize` (so a re-initialize starts clean, as `reset_all` does today).
  - The version warning (`version_warning`) is said only when answering locally.

- [ ] **Step 1: Write the failing tests** in `mcp_relay_connector.rs`:
  - `initialize_says_the_list_can_change` - `handle_message(initialize)` has `result.capabilities.tools.listChanged == true`.
  - `a_relayed_answer_is_used_as_is` - relay returns `(200, {"reply":{"jsonrpc":"2.0","id":7,"result":{"tools":[]}}})` -> that reply; `local` never called.
  - `an_app_without_the_route_is_answered_locally` - relay `(404, "unknown route")` -> `local`'s answer, equal to `handle_message` of the message.
  - `a_closed_app_is_answered_locally` - relay `Err("handshake file missing - is the app running?")` -> `local`.
  - `notifications_are_not_relayed` - `notifications/initialized` and `notifications/cancelled`: `None`, relay never called.
  - `a_relay_after_an_app_restart_uses_the_new_handshake` - the real relay function reads the handshake each call: write handshake A (listener A), relay, rewrite as B (listener B), relay -> second request lands on B.
  - `relay_timeout_is_not_shorter_than_the_bridge_timeout` - `RELAY_TIMEOUT >= Duration::from_secs(300)`.
  - `a_new_initialize_starts_a_new_session` - the session id sent after a second `initialize` differs from the first.
- [ ] **Step 2: Run to see them fail** - `cargo test --test suite mcp_relay_connector::` - FAIL.
- [ ] **Step 3: Implement** `relay_or_local` and wire it into the connector's line handling so tool calls still run on their threads (`MAX_CALLS_IN_FLIGHT` unchanged); the relay posts with `send_to_bridge_within(.., RELAY_TIMEOUT)` reading the handshake each time (as `bridge_request` does). The local path is today's `handle_message_shared` with the connector's own `SessionBook`.
- [ ] **Step 4: Run to see them pass**, then the full `cargo test --test suite` (the existing `tcm_mcp` tests must pass unchanged - they exercise the local path).
- [ ] **Step 5: Commit** - `feat(v2): the connector relays to the app and answers itself when it cannot`.

### Task 3: The connector tells the client when the list changed

**Files:**
- Modify: `src-tauri/src/mcp.rs` (watcher thread started by `run_stdio_proxy` after the first `initialize`)
- Test: `src-tauri/tests/suite/mcp_relay_connector.rs`

**Interfaces:**
- Consumes: `mcp_relay::LIST_VERSION_PATH`, Task 2's output lock (`write_response_line`).
- Produces in `mcp.rs`:
  - `pub const LIST_POLL: Duration = Duration::from_secs(5);`
  - `pub const LIST_CHANGED: &str = r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#;`
  - `pub enum ListSeen { Version(String), Down, Unsupported }` - from a `GET /mcp/list-version`: 200 -> `Version`, transport error -> `Down`, 404 -> `Unsupported`.
  - `pub fn notice_due(last: Option<&ListSeen>, now: &ListSeen) -> bool` - `false` when `last` is `None` (the first reading only records); `false` when either is `Unsupported`; otherwise `true` when they differ (`Down` vs `Version` counts as different).
  - The watcher records the reading taken right after it answers each `tools/list` (relayed or local) as `last`, so a list the client just fetched is never announced again.

- [ ] **Step 1: Write the failing tests:**
  - `notice_due` table: (None, V"a") false; (V"a", V"a") false; (V"a", V"b") true; (V"a", Down) true; (Down, V"a") true (`down_then_up_is_a_change`); (Unsupported, V"a") false; (V"a", Unsupported) false; (Down, Down) false.
  - `an_old_app_never_triggers_a_notice` - a watcher run over readings [Unsupported, Unsupported, Unsupported] writes nothing.
  - `one_change_one_notice` - readings [V"a", V"a", V"b", V"b", V"b"] write `LIST_CHANGED` exactly once, as one whole line, and never in the middle of another reply line (drive the watcher's step function with an in-memory writer behind the same mutex as a reply).
  - `no_notice_before_initialize` - the watcher does not start until an `initialize` has been answered.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement** the watcher as a step function (`fn watch_step(last: &mut Option<ListSeen>, now: ListSeen, out: &Mutex<W>)`) driven by a thread that sleeps `LIST_POLL` between `GET /mcp/list-version` calls, and stops when stdin closes.
- [ ] **Step 4: Run to see them pass**, then the full `cargo test --test suite` and `npm test` (nothing in the webview should change; run it as the gate).
- [ ] **Step 5: Commit** - `feat(v2): the connector tells the client when TCM's tools change`.

Finish: update `docs/backlog.html` (done note: a TCM update or module switch now reaches assistants within seconds after one more restart), changelog line at release: "Assistants now see TCM's tools change within a few seconds of an update or a module being switched, without restarting the editor (after one more restart once this update is installed)."
