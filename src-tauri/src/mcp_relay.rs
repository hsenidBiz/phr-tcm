//! The running app answering MCP messages a `v2.exe --mcp` connector relays to it (`POST /mcp`), and
//! naming the version of the tool list it would answer (`GET /mcp/list-version`). The answering is
//! `mcp.rs`'s own code, run in this process: one `SessionBook` per session the connector names, and
//! the connector's identity in force for the bridge calls the answer makes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::mcp::{self, BridgeCall, RelayedIdentity};
use crate::mcp_session::{SessionBook, IDLE_RESET_MS};

pub const RELAY_PATH: &str = "/mcp";
pub const LIST_VERSION_PATH: &str = "/mcp/list-version";

/// What a relay request without the connector's id is answered with.
pub const NEEDS_ID: &str = "a relayed message needs the connector's id";

/// What `GET /mcp/list-version` answers (503) when the tool list could not be read.
pub const NO_TOOL_LIST: &str = "the tool list could not be read just now";

/// Most sessions kept at once; past that the least recently used goes.
const MAX_SESSIONS: usize = 64;

struct Entry {
    book: Arc<SessionBook>,
    last_used_ms: u64,
}

/// Per-process state of the relay (what each connector session was told), not a cache.
static SESSIONS: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The book of `session`, made on first use. Entries idle for `IDLE_RESET_MS` are dropped first; a new
/// session past `MAX_SESSIONS` takes the place of the least recently used one.
fn book_for(session: &str, now_ms: u64) -> Arc<SessionBook> {
    let mut guard = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    map.retain(|_, e| now_ms.saturating_sub(e.last_used_ms) < IDLE_RESET_MS);
    if !map.contains_key(session) && map.len() >= MAX_SESSIONS {
        if let Some(oldest) = map.iter().min_by_key(|(_, e)| e.last_used_ms).map(|(k, _)| k.clone()) {
            map.remove(&oldest);
        }
    }
    let entry = map.entry(session.to_string()).or_insert_with(|| Entry { book: Arc::new(SessionBook::new()), last_used_ms: now_ms });
    entry.last_used_ms = now_ms;
    entry.book.clone()
}

fn valid_session(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A name for the log: lowercase letters, digits, `_` and `/` up to 64, or None. Message text is the
/// assistant's own, so anything else is not named.
fn loggable(name: &str) -> Option<&str> {
    let shaped = !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'/');
    shaped.then_some(name)
}

/// Answers one relayed message. `body` is `{"session": ..., "message": <json-rpc object>}`; the answer is
/// `{"reply": <the json-rpc reply, or null for a notification>}`. Runs on the calling thread, with `id`
/// in force for every bridge call `call` makes from here.
pub fn answer(body: &str, id: RelayedIdentity, call: BridgeCall) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "the relay body is not JSON".to_string())?;
    let session = v["session"]
        .as_str()
        .filter(|s| valid_session(s))
        .ok_or("the relay needs a session of 1 to 64 letters, digits and hyphens")?;
    let message = v.get("message").filter(|m| m.is_object()).ok_or("the relay needs a message")?;
    let method = message["method"].as_str().and_then(loggable).unwrap_or("?");
    let tool = (method == "tools/call").then(|| message["params"]["name"].as_str().and_then(loggable)).flatten().unwrap_or("-");
    crate::applog::info(format!("mcp relay: {method} {tool}"));

    let now = now_ms();
    let book = book_for(session, now);
    // As `serve_lines` ticks the session of the agent the message names.
    if let Ok((_, agent)) = mcp::take_agent_arg(&message["params"]["arguments"]) {
        book.tick(agent.as_deref(), now);
    }
    let text = message.to_string();
    let reply = mcp::with_relayed_identity(id, || mcp::handle_message_shared(&text, mcp::PROXY_VERSION, call, &book));
    let reply = match reply {
        Some(r) => serde_json::from_str::<serde_json::Value>(&r).map_err(|_| "the answer was not JSON".to_string())?,
        None => serde_json::Value::Null,
    };
    Ok(serde_json::json!({ "reply": reply }).to_string())
}

/// 16 lowercase hex digits naming the tool list the app would answer now: a hash of the `result` of a
/// `tools/list` answered with no session and no identity at all (the read is the app's own, so it
/// registers no assistant). Err when the app's `/tools` answer could not be read: a failure is no version.
pub fn list_version(call: BridgeCall) -> Result<String, String> {
    let msg = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let read = std::cell::Cell::new(false);
    let watched = |m: &str, p: &str, b: &str| {
        let answer = call(m, p, b);
        if p == "/tools" {
            read.set(matches!(answer, Ok((200, _))));
        }
        answer
    };
    let reply = mcp::with_no_identity(|| mcp::handle_message(msg, mcp::PROXY_VERSION, &watched)).unwrap_or_default();
    if !read.get() {
        return Err(NO_TOOL_LIST.to_string());
    }
    let result = serde_json::from_str::<serde_json::Value>(&reply).map(|v| v["result"].to_string()).unwrap_or_default();
    Ok(Sha256::digest(result.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect())
}

/// The two relay routes, for a request the listener has already token-checked: None for any other
/// path. Blocking, so the listener runs it on a blocking thread. `port` and `token` are the app's own
/// bridge, which the answer's bridge calls loop back to (the routes read the caller from the request's
/// headers, which `send_to_bridge_within` fills from the relayed identity).
pub fn route(
    port: u16,
    token: &str,
    method: &str,
    path: &str,
    body: &str,
    agent: &crate::autorun::agents::AgentHeaders,
) -> Option<(u16, String)> {
    let call = |m: &str, p: &str, b: &str| mcp::send_to_bridge(port, token, m, p, b);
    match (method, path) {
        ("POST", RELAY_PATH) => {
            let Some(agent_id) = agent.id.clone().filter(|i| crate::autorun::agents::valid_id(i)) else {
                return Some((400, serde_json::json!({ "error": NEEDS_ID }).to_string()));
            };
            let id = RelayedIdentity { agent_id, pid: agent.pid.map(|p| p.to_string()), client: agent.client.clone() };
            Some(match answer(body, id, &call) {
                Ok(reply) => (200, reply),
                Err(why) => (400, serde_json::json!({ "error": why }).to_string()),
            })
        }
        ("GET", LIST_VERSION_PATH) => Some(match list_version(&call) {
            Ok(version) => (200, serde_json::json!({ "version": version }).to_string()),
            Err(why) => (503, serde_json::json!({ "error": why }).to_string()),
        }),
        _ => None,
    }
}
