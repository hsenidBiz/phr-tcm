//! The HTTP/TCP plumbing: the bridge's shared state, the listener, request parsing and the call log lines.

use super::*;

/// Context shared between the TCP loop and the set_bridge_context command.
pub struct BridgeState {
    pub ctx: StdMutex<BridgeContext>,
    pub token: String,
    /// The running app's version (from tauri.conf.json via
    /// `AppHandle::package_info`), reported on `/ping` and in the handshake
    /// file so tcm-mcp can echo the real version without its own Cargo.toml
    /// needing to stay in sync.
    pub version: String,
}
pub type SharedBridge = Arc<BridgeState>;

impl BridgeState {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(ctx: BridgeContext, version: String) -> SharedBridge {
        Arc::new(BridgeState { ctx: StdMutex::new(ctx), token: new_token(), version })
    }
}

/// Optional per-request ADO client factory: the Tauri layer passes one
/// that mints a fresh token; tests pass None (ping/validate only).
pub type ClientFactory =
    Arc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<crate::ado::AdoClient>> + Send>> + Send + Sync>;

/// Where the app announces the running bridge to `v2.exe --mcp` proxies.
pub fn handshake_path() -> std::path::PathBuf {
    std::env::temp_dir().join("tcm-v2-mcp-bridge.json")
}

/// After an answer sent before the request was all read (a body over the cap): half-closes the socket so the
/// caller sees the answer end, then reads and discards what is still coming, at most 1 MiB and 2 s. Closing
/// with unread bytes makes Windows reset the connection, and the caller then reads the reset, not the answer.
pub(crate) async fn drain_unread_request(sock: &mut tokio::net::TcpStream) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _ = sock.shutdown().await;
    let drain = async {
        let mut sink = [0u8; 8192];
        let mut drained = 0usize;
        while drained < 1024 * 1024 {
            match sock.read(&mut sink).await {
                Ok(0) | Err(_) => break,
                Ok(n) => drained += n,
            }
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), drain).await;
}

/// Bind 127.0.0.1:0, optionally write the handshake file, serve forever on
/// the tokio runtime. Returns (port, token). Requests: tiny HTTP/1.1, one
/// request per connection, 64 KiB body cap (and 1 KiB more for the headers). `handshake` is Some in the real
/// app and None in tests - a test run must never clobber a live app's
/// handshake file (it did once: proxies then saw a dead port + test token).
pub async fn start_listener(
    state: SharedBridge,
    make_client: Option<ClientFactory>,
    handshake: Option<std::path::PathBuf>,
) -> Result<(u16, String), String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = state.token.clone();
    if let Some(path) = handshake {
        std::fs::write(
            path,
            serde_json::json!({ "port": port, "token": token, "version": state.version }).to_string(),
        )
        .map_err(|e| e.to_string())?;
    }

    tauri::async_runtime::spawn(async move {
        let mut backoff = crate::note_server::AcceptBackoff::default();
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(pair) => {
                    backoff.succeeded();
                    pair
                }
                Err(e) => {
                    let wait = backoff.failed();
                    crate::applog::warn(format!(
                        "AI bridge could not accept a connection, retrying in {} ms: {e}",
                        wait.as_millis()
                    ));
                    tokio::time::sleep(wait).await;
                    continue;
                }
            };
            let state = Arc::clone(&state);
            let make_client = make_client.clone();
            tauri::async_runtime::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                // 64 KiB for the request plus a kilobyte for its head: the agent
                // headers a proxy now sends must not take room a body had before.
                let mut buf = vec![0u8; 65536 + 1024];
                let mut used = 0usize;
                // Read until headers+body are complete (or the cap). Every
                // read is bounded: a client that opens a connection, sends
                // half a request and stops used to hold this task and its
                // socket for the life of the process, one per attempt.
                let deadline = std::time::Duration::from_secs(15);
                let parsed = loop {
                    let read = tokio::time::timeout(deadline, sock.read(&mut buf[used..])).await;
                    let Ok(Ok(n)) = read else { return };
                    if n == 0 { return; }
                    used += n;
                    match parse_http(&buf[..used]) {
                        Parsed::Complete { method, target, token, body, proxy_version, tool, agent } => {
                            break (method, target, token, body, proxy_version, tool, agent)
                        }
                        // Answer rather than close in silence: the proxy on
                        // the other end is waiting on a response, and "the
                        // connection went away" tells it nothing it can act
                        // on or show the user.
                        Parsed::Malformed => {
                            let _ = sock
                                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                                .await;
                            return;
                        }
                        Parsed::Incomplete => {}
                    }
                    if used >= buf.len() {
                        let _ = sock
                            .write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                        drain_unread_request(&mut sock).await;
                        return;
                    }
                };
                let (method, target, tok, body, proxy_version, tool, agent) = parsed;
                let (status, payload) = if tok.as_deref() != Some(state.token.as_str()) {
                    (401, String::new())
                } else {
                    note_proxy_version(proxy_version.as_deref(), &state.version);
                    let ctx = state.ctx.lock().unwrap().clone();
                    let client = match &make_client {
                        Some(f) => f().await,
                        None => None,
                    };
                    // Who is calling is known only after the token check: a
                    // request without the token registers nobody.
                    let caller = agent.caller();
                    let label = note_call_for_tool(&caller, &agent, tool.as_deref(), crate::autorun::sessions::now_ms());
                    let (status, payload) =
                        route_timed(&caller, &ctx, client.as_ref(), &method, &target, &body, &state.version).await;
                    if let Some(line) = tool_call_line_for(tool.as_deref(), label.as_deref(), &target, status, &payload) {
                        crate::applog::debug(line);
                    }
                    (status, payload)
                };
                let reason = match status {
                    200 => "OK",
                    202 => "Accepted",
                    400 => "Bad Request",
                    401 => "Unauthorized",
                    403 => "Forbidden",
                    409 => "Conflict",
                    500 => "Internal Server Error",
                    502 => "Bad Gateway",
                    503 => "Unavailable",
                    _ => "Not Found",
                };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len(),
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    Ok((port, token))
}

/// What a buffer of bytes off the socket amounts to so far.
///
/// The old version returned an Option, which conflated the two ways of not
/// having a request: "keep reading" and "this will never be one". A
/// malformed header read as "keep reading", so the connection sat there
/// until the 64 KB cap - or, if the client simply stopped sending, until
/// the end of the process, holding a task and a socket per attempt.
pub enum Parsed {
    Complete {
        method: String,
        target: String,
        token: Option<String>,
        body: String,
        /// The `x-tcm-proxy-version` header: the build of the MCP proxy that
        /// sent this. None from a proxy older than the header.
        proxy_version: Option<String>,
        /// The `x-tcm-tool` header: the MCP tool this request was made for
        /// (`mcp::TOOL_HEADER`). None outside a tool call.
        tool: Option<String>,
        /// The `x-tcm-agent*` headers (`mcp::AGENT_HEADER` and its three
        /// companions), each None when absent or unreadable. A bad value is
        /// ignored, never a reason to refuse the request.
        agent: crate::autorun::agents::AgentHeaders,
    },
    Incomplete,
    Malformed,
}

/// Parse a request from the raw BYTES.
///
/// Offsets used to come from `String::from_utf8_lossy(raw)` and then index
/// `raw`. Those are not the same string: every invalid byte becomes a
/// three-byte U+FFFD, so one bad byte anywhere in the headers slid the body
/// offset and the request was read from the wrong place. The head is found
/// by byte, and only then decoded.
pub fn parse_http(raw: &[u8]) -> Parsed {
    let Some(head_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        // No blank line yet. A request line is short; if this much has
        // arrived without one, it is not HTTP.
        return if raw.len() > 16 * 1024 { Parsed::Malformed } else { Parsed::Incomplete };
    };
    let Ok(head) = std::str::from_utf8(&raw[..head_end]) else {
        return Parsed::Malformed; // headers are not text
    };
    let mut lines = head.lines();
    let Some(mut req) = lines.next().map(str::split_whitespace) else {
        return Parsed::Malformed;
    };
    let (Some(method), Some(target)) = (req.next(), req.next()) else {
        return Parsed::Malformed;
    };
    let mut token = None;
    let mut proxy_version = None;
    let mut tool = None;
    let mut agent = crate::autorun::agents::AgentHeaders::default();
    let mut content_len = 0usize;
    for line in lines {
        // A header we cannot read is not a reason to reject the request -
        // only Content-Length has to be right, because it decides where
        // the body ends.
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        if k.eq_ignore_ascii_case("x-bridge-token") {
            token = Some(v.to_string());
        }
        if k.eq_ignore_ascii_case(crate::mcp::PROXY_VERSION_HEADER) {
            proxy_version = Some(v.to_string());
        }
        if k.eq_ignore_ascii_case(crate::mcp::TOOL_HEADER) {
            tool = Some(v.to_string());
        }
        if k.eq_ignore_ascii_case(crate::mcp::AGENT_HEADER) {
            agent.id = Some(v.to_string());
        }
        if k.eq_ignore_ascii_case(crate::mcp::AGENT_PID_HEADER) {
            // Not a number, or 0 (no process has it): left out, the request
            // is still served.
            agent.pid = v.parse().ok().filter(|p| *p != 0);
        }
        if k.eq_ignore_ascii_case(crate::mcp::AGENT_CLIENT_HEADER) {
            agent.client = Some(v.to_string());
        }
        if k.eq_ignore_ascii_case(crate::mcp::AGENT_NAME_HEADER) {
            agent.name = Some(v.to_string());
        }
        if k.eq_ignore_ascii_case("content-length") {
            match v.parse() {
                Ok(n) => content_len = n,
                Err(_) => return Parsed::Malformed,
            }
        }
    }
    let body_start = head_end + 4;
    let Some(end) = body_start.checked_add(content_len).filter(|e| *e <= raw.len()) else {
        return Parsed::Incomplete; // body still arriving
    };
    Parsed::Complete {
        method: method.to_string(),
        target: target.to_string(),
        token,
        body: String::from_utf8_lossy(&raw[body_start..end]).to_string(),
        proxy_version,
        tool,
        agent,
    }
}

/// The app-log line for one MCP tool call, or None for a request that is
/// not one to log. The proxy names the tool (`mcp::TOOL_HEADER`) on every
/// request it makes for a call: first the `/tools` read that decides
/// whether the tool is offered, then - unless that refused it - the tool's
/// own route. So a call that reaches the app is logged once: "refused" on
/// the `/tools` read when this very answer switches the tool off (decided
/// as the proxy decides it, `mcp::tool_policy_from`), otherwise "ok" or the
/// HTTP status of the route's answer. Only the tool's name and that outcome
/// - never an argument, the answer's text, an id or a path - and only for a
/// name this server offers.
///
/// What is NOT logged is what the proxy refuses before it asks the app
/// anything, because the app never sees those calls and there is no
/// channel for them: an argument-shape refusal (a mix of arguments, a
/// `get_guide` part sent with another topic, a quirk or cookie shape), a
/// `get_guide` topic the proxy refuses, and a call to a retired tool name.
pub fn tool_call_line(tool: Option<&str>, target: &str, status: u16, payload: &str) -> Option<String> {
    tool_call_line_for(tool, None, target, status, payload)
}

/// `tool_call_line` for a caller with a label: "AI tool {tool} ({label}):
/// {outcome}". With no label it is exactly `tool_call_line`'s line. The label
/// is the registry's ("Assistant 2"), never an id or a pid.
pub fn tool_call_line_for(
    tool: Option<&str>,
    label: Option<&str>,
    target: &str,
    status: u16,
    payload: &str,
) -> Option<String> {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let tool = tool?;
    if !NAMES.get_or_init(crate::mcp::all_tool_names).iter().any(|t| t == tool) {
        return None;
    }
    let path = target.split_once('?').map_or(target, |(p, _)| p);
    let outcome = if path == "/tools" {
        let (off, _) = crate::mcp::tool_policy_from(Ok((status, payload.to_string())));
        if !off.iter().any(|o| o == tool) {
            return None;
        }
        "refused".to_string()
    } else if status < 400 {
        "ok".to_string()
    } else {
        format!("tool error, HTTP {status}")
    };
    Some(match label {
        Some(label) => format!("AI tool {tool} ({label}): {outcome}"),
        None => format!("AI tool {tool}: {outcome}"),
    })
}

/// Logs - once per proxy version, not once per call - an MCP proxy that is
/// not this app's build. The assistant hears it from the proxy itself; this
/// is where a person hears it: Settings → Logs, which a bug report ships.
/// A proxy older than the header says nothing and is not flagged here.
pub(crate) fn note_proxy_version(proxy: Option<&str>, app: &str) {
    // The versions already reported. A log de-duplication, not cached data.
    static WARNED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let Some(proxy) = proxy else { return };
    let Some(warning) = crate::mcp::version_warning(proxy, app) else { return };
    let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if warned.iter().any(|v| v == proxy) {
        return;
    }
    warned.push(proxy.to_string());
    crate::applog::warn(format!("AI bridge: {warning}"));
}
