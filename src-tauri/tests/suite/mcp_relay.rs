//! The app answering MCP messages a connector relays to it (`mcp_relay`): the answer equals the
//! connector's own, the connector's identity rides on every call the answer makes, sessions keep
//! their role notes apart, and the two routes sit behind the bridge's token.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use v2_lib::autorun::agents::AgentHeaders;
use v2_lib::autorun::roles::{self, Role};
use v2_lib::mcp::{self, RelayedIdentity};
use v2_lib::mcp_relay;

const TOOLS_ON: &str = r#"{"disabled":[],"modules":["db_access","writing_style","auto_run","api_templates","playwright","crystal_reports"],"unlocked":true}"#;

/// A stand-in for the app's bridge: answers `/tools` with a body the test can change, a role's text
/// for `/autorun-role`, and for every other path the calling agent's id (so an answer shows who the
/// app saw). Records each request's head.
struct Listener {
    port: u16,
    tools: Arc<Mutex<String>>,
    seen: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
}

impl Listener {
    fn start() -> Listener {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let tools = Arc::new(Mutex::new(TOOLS_ON.to_string()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (t, s, st) = (tools.clone(), seen.clone(), stop.clone());
        std::thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (t, s) = (t.clone(), s.clone());
                        std::thread::spawn(move || serve(stream, &t, &s));
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
                }
            }
        });
        Listener { port, tools, seen, stop }
    }

    fn send(&self) -> impl Fn(&str, &str, &str) -> Result<(u16, String), String> + '_ {
        move |m, p, b| mcp::send_to_bridge(self.port, "tok", m, p, b)
    }

    fn tools_answer(&self, body: &str) {
        *self.tools.lock().unwrap() = body.to_string();
    }

    fn requests(&self) -> Vec<AgentHeaders> {
        self.seen.lock().unwrap().iter().map(|raw| agent_of(raw)).collect()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn agent_of(raw: &[u8]) -> AgentHeaders {
    match v2_lib::ai_bridge::parse_http(raw) {
        v2_lib::ai_bridge::Parsed::Complete { agent, .. } => agent,
        _ => panic!("not a whole request: {}", String::from_utf8_lossy(raw)),
    }
}

fn serve(mut stream: std::net::TcpStream, tools: &Mutex<String>, seen: &Mutex<Vec<Vec<u8>>>) {
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(n) if n > 0 => raw.extend_from_slice(&chunk[..n]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&raw).to_string();
    let agent = agent_of(&raw);
    seen.lock().unwrap().push(raw);
    let body = if head.starts_with("GET /tools ") {
        tools.lock().unwrap().clone()
    } else if head.starts_with("GET /autorun-role") {
        "ROLE RULES".to_string()
    } else {
        format!("seen {}", agent.id.unwrap_or_default())
    };
    let _ = stream.write_all(
        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes(),
    );
}

fn identity(id: &str) -> RelayedIdentity {
    RelayedIdentity { agent_id: id.repeat(32), pid: Some("4242".into()), client: Some("claude-code".into()) }
}

fn relay(session: &str, message: serde_json::Value) -> String {
    serde_json::json!({ "session": session, "message": message }).to_string()
}

fn reply_of(answer: &str) -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(answer).unwrap()["reply"].clone()
}

fn call(id: u32, tool: &str, args: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": tool, "arguments": args } })
}

fn text_of(reply: &serde_json::Value) -> String {
    reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn a_relayed_tools_list_equals_the_local_answer() {
    let _agents = crate::serial::agents();
    let app = Listener::start();
    let message = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let relayed = mcp_relay::answer(&relay("s-tools-list", message.clone()), identity("a"), &app.send()).unwrap();
    let local = mcp::handle_message(&message.to_string(), mcp::PROXY_VERSION, &app.send()).unwrap();
    let local: serde_json::Value = serde_json::from_str(&local).unwrap();
    assert!(local["result"]["tools"].as_array().is_some_and(|t| !t.is_empty()));
    assert_eq!(reply_of(&relayed), local);
}

#[test]
fn a_relayed_call_carries_the_connectors_identity() {
    let _agents = crate::serial::agents();
    let app = Listener::start();
    let id = RelayedIdentity { agent_id: "a".repeat(32), pid: Some("4242".into()), client: Some("claude-code".into()) };
    let answer = mcp_relay::answer(&relay("s-identity", call(2, "get_current_pbi", serde_json::json!({}))), id, &app.send()).unwrap();
    assert_eq!(text_of(&reply_of(&answer)), format!("seen {}", "a".repeat(32)));
    let requests = app.requests();
    assert!(requests.len() >= 2, "the policy read and the route: {}", requests.len());
    for seen in requests {
        assert_eq!(seen.id.as_deref(), Some("a".repeat(32).as_str()));
        assert_eq!(seen.pid, Some(4242));
        assert_eq!(seen.client.as_deref(), Some("claude-code"));
    }
    // Nothing is left in force on this thread.
    let after = mcp::send_to_bridge(app.port, "tok", "GET", "/guide", "");
    assert!(after.is_ok());
    let last = app.requests().pop().unwrap();
    assert_eq!(last.id.as_deref(), Some(mcp::proxy_agent_id()));
}

#[test]
fn two_relays_at_once_keep_their_own_identity() {
    let _agents = crate::serial::agents();
    let app = Listener::start();
    for round in 0..20 {
        std::thread::scope(|scope| {
            let runs: Vec<_> = ["1", "2"]
                .into_iter()
                .map(|who| {
                    let app = &app;
                    scope.spawn(move || {
                        let body = relay("s-concurrent", call(round, "get_current_pbi", serde_json::json!({})));
                        let answer = mcp_relay::answer(&body, identity(who), &app.send()).unwrap();
                        (who, text_of(&reply_of(&answer)))
                    })
                })
                .collect();
            for run in runs {
                let (who, text) = run.join().unwrap();
                assert_eq!(text, format!("seen {}", who.repeat(32)), "round {round}");
            }
        });
    }
    for seen in app.requests() {
        let id = seen.id.unwrap();
        assert!(id == "1".repeat(32) || id == "2".repeat(32), "{id}");
    }
}

#[test]
fn sessions_keep_role_notes_apart() {
    let _agents = crate::serial::agents();
    let app = Listener::start();
    let ask = |session: &str, message| text_of(&reply_of(&mcp_relay::answer(&relay(session, message), identity("a"), &app.send()).unwrap()));
    // `role` on get_guide marks the role read in s1.
    ask("s-roles-1", call(1, "get_guide", serde_json::json!({ "topic": "autorun", "role": "healer" })));
    let in_s1 = ask("s-roles-1", call(2, "get_autorun_failures", serde_json::json!({})));
    assert!(in_s1.contains(&roles::pointer(Role::Healer)), "{in_s1}");
    assert!(!in_s1.contains("ROLE RULES"), "{in_s1}");
    // s2 has been told nothing: the rules come ahead of its answer.
    let in_s2 = ask("s-roles-2", call(3, "get_autorun_failures", serde_json::json!({})));
    assert!(in_s2.starts_with("ROLE RULES"), "{in_s2}");
}

#[test]
fn a_bad_body_or_session_is_refused() {
    let app = Listener::start();
    let message = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let answer = |body: String| mcp_relay::answer(&body, identity("a"), &app.send());
    assert!(answer("not json".into()).is_err());
    assert!(answer(serde_json::json!({ "session": "s1" }).to_string()).is_err());
    assert!(answer(serde_json::json!({ "session": "s1", "message": "tools/list" }).to_string()).is_err());
    assert!(answer(relay("", message.clone())).is_err());
    assert!(answer(relay(&"a".repeat(65), message.clone())).is_err());
    assert!(answer(relay("a b", message.clone())).is_err());
    assert!(answer(relay(&"a".repeat(64), message)).is_ok());
    assert!(app.requests().len() <= 2, "a refused body reached the app");
}

#[test]
fn a_message_with_no_id_gets_a_null_reply() {
    let app = Listener::start();
    let message = serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let answer = mcp_relay::answer(&relay("s-no-id", message), identity("a"), &app.send()).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&answer).unwrap(), serde_json::json!({ "reply": null }));
}

#[test]
fn the_list_version_changes_with_the_tools_answer() {
    let _agents = crate::serial::agents();
    let app = Listener::start();
    let first = mcp_relay::list_version(&app.send()).unwrap();
    assert_eq!(first.len(), 16);
    assert!(first.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{first}");
    assert_eq!(mcp_relay::list_version(&app.send()).unwrap(), first);
    app.tools_answer(r#"{"disabled":["get_tags"],"modules":["db_access","writing_style","auto_run","api_templates","playwright","crystal_reports"],"unlocked":true}"#);
    let second = mcp_relay::list_version(&app.send()).unwrap();
    assert_ne!(second, first);
    assert_eq!(mcp_relay::list_version(&app.send()).unwrap(), second);
}

/// The two routes on the real listener: refused without the token like any route, answered with it,
/// and the answer's own bridge calls loop back to the same listener.
#[test]
fn the_relay_routes_sit_behind_the_token() {
    let _agents = crate::serial::agents();
    let _log = crate::serial::log_tail();
    v2_lib::autorun::agents::clear();
    let shared = v2_lib::ai_bridge::BridgeState::new(Default::default(), "0.0.0-test".into());
    let (port, token) =
        tauri::async_runtime::block_on(v2_lib::ai_bridge::start_listener(Arc::clone(&shared), None, None)).unwrap();
    let message = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let body = relay("route-1", message);
    let id = "b".repeat(32);
    let post = |token: &str, with_id: bool| {
        let client = reqwest::blocking::Client::new();
        let mut req = client.post(format!("http://127.0.0.1:{port}/mcp")).header("x-bridge-token", token).body(body.clone());
        if with_id {
            req = req.header(mcp::AGENT_HEADER, &id);
        }
        let resp = req.send().unwrap();
        (resp.status().as_u16(), resp.text().unwrap())
    };
    assert_eq!(post("wrong", true).0, 401);
    let (status, text) = post(&token, false);
    assert_eq!(status, 400);
    assert!(text.contains(mcp_relay::NEEDS_ID), "{text}");
    let (status, text) = post(&token, true);
    assert_eq!(status, 200, "{text}");
    let reply: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(reply["reply"]["result"]["tools"].as_array().is_some_and(|t| !t.is_empty()), "{text}");
    let get = |token: &str| {
        let resp = reqwest::blocking::Client::new()
            .get(format!("http://127.0.0.1:{port}/mcp/list-version"))
            .header("x-bridge-token", token)
            .send()
            .unwrap();
        (resp.status().as_u16(), resp.text().unwrap())
    };
    assert_eq!(get("wrong").0, 401);
    let (status, text) = get(&token);
    assert_eq!(status, 200, "{text}");
    let version = serde_json::from_str::<serde_json::Value>(&text).unwrap()["version"].as_str().unwrap().to_string();
    assert_eq!(version.len(), 16);
    // The app's own read of its tool list registered nobody: only the connector's `b` agent is live
    // (from the POST above), never the app's own proxy id.
    let live = v2_lib::autorun::agents::live();
    assert_eq!(live.len(), 1, "{live:?}");
    // The relay left the log one line for the message, never its body.
    assert!(v2_lib::applog::recent(6000).iter().any(|l| l.message == "mcp relay: tools/list -"));
}

/// `list_version` alone, against the real listener: the app reading its own tool list registers no
/// assistant, which would never leave.
#[test]
fn reading_the_list_version_registers_no_assistant() {
    let _agents = crate::serial::agents();
    v2_lib::autorun::agents::clear();
    let shared = v2_lib::ai_bridge::BridgeState::new(Default::default(), "0.0.0-test".into());
    let (port, token) =
        tauri::async_runtime::block_on(v2_lib::ai_bridge::start_listener(Arc::clone(&shared), None, None)).unwrap();
    let answer = reqwest::blocking::Client::new()
        .get(format!("http://127.0.0.1:{port}/mcp/list-version"))
        .header("x-bridge-token", &token)
        .send()
        .unwrap();
    assert_eq!(answer.status().as_u16(), 200);
    assert!(v2_lib::autorun::agents::live().is_empty(), "{:?}", v2_lib::autorun::agents::live());
}

#[test]
fn a_failing_tools_read_gives_no_version() {
    let failing = |_: &str, _: &str, _: &str| -> Result<(u16, String), String> { Err("down".into()) };
    assert_eq!(mcp_relay::list_version(&failing), Err(mcp_relay::NO_TOOL_LIST.to_string()));
    let refused = |_: &str, _: &str, _: &str| -> Result<(u16, String), String> { Ok((500, "{}".into())) };
    assert!(mcp_relay::list_version(&refused).is_err());
    // The route says so with a 503 and no version, when the app's own bridge cannot be reached.
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let (status, text) = mcp_relay::route(dead, "tok", "GET", mcp_relay::LIST_VERSION_PATH, "", &AgentHeaders::default()).unwrap();
    assert_eq!(status, 503, "{text}");
    assert!(!text.contains("version"), "{text}");
}
