//! The connector (`v2.exe --mcp`) relaying each message to the running app and answering itself when it
//! cannot: what is relayed, what is not, what makes it fall back, and the session it names.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use v2_lib::mcp::{self, relay_or_local, RelayError, SharedCall, SharedRelay};
use v2_lib::mcp_session::SessionBook;

const LIST: &str = r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#;
const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#;

fn local_answer(msg: &str) -> Option<String> {
    mcp::handle_message(msg, "1.0.0", &|_m: &str, _p: &str, _b: &str| Ok((200, "{}".to_string())))
}

#[test]
fn initialize_says_the_list_can_change() {
    let v: serde_json::Value = serde_json::from_str(&local_answer(INIT).unwrap()).unwrap();
    assert_eq!(v["result"]["capabilities"]["tools"]["listChanged"], true);
}

#[test]
fn a_relayed_answer_is_used_as_is() {
    let reply = r#"{"jsonrpc":"2.0","id":7,"result":{"tools":[]}}"#;
    let body = serde_json::json!({ "reply": serde_json::from_str::<serde_json::Value>(reply).unwrap() }).to_string();
    let sent = Mutex::new(Vec::new());
    let local_calls = Mutex::new(0);
    let got = relay_or_local(
        LIST,
        "sess-1",
        &|b: &str, _t: Duration| {
            sent.lock().unwrap().push(b.to_string());
            Ok((200, body.clone()))
        },
        &|_m: &str| {
            *local_calls.lock().unwrap() += 1;
            None
        },
    );
    let got: serde_json::Value = serde_json::from_str(&got.unwrap()).unwrap();
    assert_eq!(got, serde_json::from_str::<serde_json::Value>(reply).unwrap());
    assert_eq!(*local_calls.lock().unwrap(), 0);
    let sent: serde_json::Value = serde_json::from_str(&sent.lock().unwrap()[0]).unwrap();
    assert_eq!(sent["session"], "sess-1");
    assert_eq!(sent["message"]["method"], "tools/list");
}

#[test]
fn a_null_reply_is_no_answer() {
    let got = relay_or_local(LIST, "s", &|_b: &str, _t: Duration| Ok((200, r#"{"reply":null}"#.to_string())), &|_m: &str| Some("local".into()));
    assert_eq!(got, None);
}

#[test]
fn an_app_without_the_route_is_answered_locally() {
    let got = relay_or_local(LIST, "s", &|_b: &str, _t: Duration| Ok((404, "unknown route".to_string())), &local_answer);
    assert_eq!(got, local_answer(LIST));
    assert!(got.is_some());
}

#[test]
fn a_closed_app_is_answered_locally() {
    let got = relay_or_local(LIST, "s", &|_b: &str, _t: Duration| Err(RelayError::NotSent("handshake file missing - is the app running?".to_string())), &local_answer);
    assert_eq!(got, local_answer(LIST));
}

#[test]
fn every_other_failure_is_answered_locally() {
    for (status, body) in [(413u16, "too large"), (500, "boom"), (503, "busy"), (400, "no"), (200, "not json"), (200, r#"{"other":1}"#)] {
        let got = relay_or_local(LIST, "s", &|_b: &str, _t: Duration| Ok((status, body.to_string())), &|_m: &str| Some("local".into()));
        assert_eq!(got.as_deref(), Some("local"), "{status} {body}");
    }
}

#[test]
fn notifications_are_not_relayed() {
    for msg in [
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}"#,
    ] {
        let relayed = Mutex::new(0);
        let got = relay_or_local(
            msg,
            "s",
            &|_b: &str, _t: Duration| {
                *relayed.lock().unwrap() += 1;
                Ok((200, r#"{"reply":{"x":1}}"#.to_string()))
            },
            &|_m: &str| Some("local".into()),
        );
        assert_eq!(got, None, "{msg}");
        assert_eq!(*relayed.lock().unwrap(), 0, "{msg}");
    }
}

#[test]
fn initialize_is_never_relayed() {
    let relayed = Mutex::new(0);
    let got = relay_or_local(
        INIT,
        "s",
        &|_b: &str, _t: Duration| {
            *relayed.lock().unwrap() += 1;
            Ok((200, r#"{"reply":{"x":1}}"#.to_string()))
        },
        &local_answer,
    );
    assert_eq!(got, local_answer(INIT));
    assert_eq!(*relayed.lock().unwrap(), 0);
}

#[test]
fn relay_timeout_is_not_shorter_than_the_bridge_timeout() {
    assert!(mcp::RELAY_TIMEOUT >= Duration::from_secs(300));
}

/// `serve_lines_with` over `lines`: the output, and every body the relay was sent.
fn serve(lines: &[&str]) -> (String, Vec<serde_json::Value>) {
    let sent: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
    let log = sent.clone();
    let relay: SharedRelay = Arc::new(move |b: &str, _t: Duration| {
        log.lock().unwrap().push(serde_json::from_str(b).unwrap());
        Ok((200, r#"{"reply":{"jsonrpc":"2.0","id":7,"result":{}}}"#.to_string()))
    });
    let call: SharedCall = Arc::new(|_m: &str, _p: &str, _b: &str| Ok((200, "{}".to_string())));
    let out = Arc::new(Mutex::new(Vec::<u8>::new()));
    let input = std::io::Cursor::new(lines.join("\n").into_bytes());
    mcp::serve_lines_with(input, out.clone(), "1.0.0".to_string(), call, Arc::new(SessionBook::new()), Some(relay));
    let text = String::from_utf8(out.lock().unwrap().clone()).unwrap();
    let sent = sent.lock().unwrap().clone();
    (text, sent)
}

#[test]
fn a_new_initialize_starts_a_new_session() {
    let (_out, sent) = serve(&[LIST, INIT, LIST, LIST]);
    assert_eq!(sent.len(), 3, "initialize is not relayed: {sent:?}");
    let ids: Vec<&str> = sent.iter().map(|s| s["session"].as_str().unwrap()).collect();
    assert_eq!(ids[1], ids[2], "one session between initializes");
    assert_ne!(ids[0], ids[1], "a re-initialize starts clean");
    assert!(ids.iter().all(|s| (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')));
}

#[test]
fn the_connector_answers_initialize_and_relays_the_rest_through_the_loop() {
    let (out, sent) = serve(&[INIT, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, LIST]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    let init: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(init["result"]["capabilities"]["tools"]["listChanged"], true);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["message"]["method"], "tools/list");
}

#[test]
fn a_tool_call_is_relayed_on_its_own_thread() {
    let (out, sent) = serve(&[r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_guide","arguments":{}}}"#]);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["message"]["params"]["name"], "get_guide");
    assert!(out.contains("\"id\":7"), "{out}");
}

/// A stand-in bridge that records the first line of each request and answers 200 `{"reply":null}`.
struct Listener {
    port: u16,
    hits: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Listener {
    fn start() -> Listener {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.set_nonblocking(true).unwrap();
        let port = l.local_addr().unwrap().port();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (h, s) = (hits.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                match l.accept() {
                    Ok((mut stream, _)) => {
                        let h = h.clone();
                        std::thread::spawn(move || {
                            stream.set_nonblocking(false).unwrap();
                            stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                            let mut raw = Vec::new();
                            let mut chunk = [0u8; 4096];
                            while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
                                match stream.read(&mut chunk) {
                                    Ok(n) if n > 0 => raw.extend_from_slice(&chunk[..n]),
                                    _ => return,
                                }
                            }
                            h.lock().unwrap().push(String::from_utf8_lossy(&raw).lines().next().unwrap_or("").to_string());
                            let body = r#"{"reply":null}"#;
                            let _ = stream.write_all(
                                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes(),
                            );
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Listener { port, hits, stop }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[test]
fn a_relay_after_an_app_restart_uses_the_new_handshake() {
    let dir = tempfile::tempdir().unwrap();
    let handshake = dir.path().join("handshake.json");
    let (a, b) = (Listener::start(), Listener::start());
    let write = |port: u16| std::fs::write(&handshake, format!(r#"{{"port":{port},"token":"tok","version":"1.0.0"}}"#)).unwrap();
    let body = r#"{"session":"s","message":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;

    write(a.port);
    assert_eq!(mcp::relay_to_app_at(&handshake, body, Duration::from_secs(10)).unwrap().0, 200);
    write(b.port);
    assert_eq!(mcp::relay_to_app_at(&handshake, body, Duration::from_secs(10)).unwrap().0, 200);

    assert_eq!(a.hits.lock().unwrap().len(), 1);
    assert_eq!(b.hits.lock().unwrap().len(), 1);
    assert!(b.hits.lock().unwrap()[0].starts_with("POST /mcp "), "{:?}", b.hits.lock().unwrap());
}

#[test]
fn a_missing_handshake_and_a_refused_connection_are_not_sent() {
    let dir = tempfile::tempdir().unwrap();
    let handshake = dir.path().join("handshake.json");
    let body = r#"{"session":"s","message":{}}"#;
    assert!(matches!(mcp::relay_to_app_at(&handshake, body, Duration::from_secs(2)), Err(RelayError::NotSent(_))));
    // A port nothing listens on: bind, note the port, close.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    std::fs::write(&handshake, format!(r#"{{"port":{port},"token":"tok"}}"#)).unwrap();
    let got = mcp::relay_to_app_at(&handshake, body, Duration::from_secs(30));
    assert!(matches!(got, Err(RelayError::NotSent(_))), "{got:?}");
}

#[test]
fn a_silent_app_is_a_may_have_run_failure() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let dir = tempfile::tempdir().unwrap();
    let handshake = dir.path().join("handshake.json");
    std::fs::write(&handshake, format!(r#"{{"port":{port},"token":"tok"}}"#)).unwrap();
    let got = mcp::relay_to_app_at(&handshake, r#"{"session":"s","message":{}}"#, Duration::from_millis(300));
    assert!(matches!(got, Err(RelayError::MaySent(_))), "{got:?}");
    drop(l);
}

const CALL: &str = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"save_setup","arguments":{}}}"#;

/// What `relay_or_local` does with `CALL` when the relay answers `answer`: its reply, and whether `local` ran.
fn call_with(answer: Result<(u16, String), RelayError>) -> (Option<serde_json::Value>, bool) {
    let local_ran = Mutex::new(false);
    let got = relay_or_local(CALL, "s", &|_b: &str, _t: Duration| answer.clone(), &|_m: &str| {
        *local_ran.lock().unwrap() = true;
        Some("local".into())
    });
    let ran = *local_ran.lock().unwrap();
    (got.and_then(|g| serde_json::from_str(&g).ok()), ran)
}

#[test]
fn a_call_the_app_may_have_run_is_answered_with_an_error_not_run_again() {
    for answer in [
        Err(RelayError::MaySent("operation timed out".into())),
        Ok((500, "boom".to_string())),
        Ok((503, "busy".to_string())),
        Ok((200, "not json".to_string())),
        Ok((200, r#"{"other":1}"#.to_string())),
        Ok((418, "x".to_string())),
    ] {
        let (reply, local_ran) = call_with(answer.clone());
        assert!(!local_ran, "{answer:?}");
        let reply = reply.unwrap_or_else(|| panic!("{answer:?}"));
        assert_eq!(reply["id"], 9, "{answer:?}");
        assert_eq!(reply["error"]["message"], mcp::CALL_UNCONFIRMED);
        assert!(!reply.to_string().contains("http"), "{reply}");
    }
}

#[test]
fn a_call_the_app_certainly_did_not_run_is_answered_locally() {
    for answer in [
        Err(RelayError::NotSent("handshake file missing".into())),
        Ok((404, "unknown route".to_string())),
        Ok((401, "x".to_string())),
        Ok((403, "x".to_string())),
        Ok((413, "x".to_string())),
        Ok((400, r#"{"error":"the relay needs a message"}"#.to_string())),
    ] {
        let (reply, local_ran) = call_with(answer.clone());
        assert!(local_ran, "{answer:?}");
        assert_eq!(reply, None, "{answer:?}"); // "local" is not JSON; it only shows local answered
    }
}

#[test]
fn a_list_is_still_answered_locally_whatever_went_wrong() {
    for answer in [Err(RelayError::MaySent("timed out".into())), Ok((500, "boom".to_string())), Ok((200, "x".to_string()))] {
        let got = relay_or_local(LIST, "s", &|_b: &str, _t: Duration| answer.clone(), &|_m: &str| Some("local".into()));
        assert_eq!(got.as_deref(), Some("local"), "{answer:?}");
    }
}

#[test]
fn a_call_waits_long_and_everything_else_briefly() {
    assert_eq!(mcp::RELAY_QUICK_TIMEOUT, Duration::from_secs(10));
    assert!(mcp::RELAY_QUICK_TIMEOUT < mcp::RELAY_TIMEOUT);
    for (msg, want) in [(CALL, mcp::RELAY_TIMEOUT), (LIST, mcp::RELAY_QUICK_TIMEOUT), (r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#, mcp::RELAY_QUICK_TIMEOUT)] {
        let seen = Mutex::new(None);
        relay_or_local(msg, "s", &|_b: &str, t: Duration| {
            *seen.lock().unwrap() = Some(t);
            Ok((200, r#"{"reply":null}"#.to_string()))
        }, &|_m: &str| None);
        assert_eq!(*seen.lock().unwrap(), Some(want), "{msg}");
    }
}
