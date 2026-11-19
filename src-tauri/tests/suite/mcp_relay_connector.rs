//! The connector (`v2.exe --mcp`) relaying each message to the running app and answering itself when it
//! cannot: what is relayed, what is not, what makes it fall back, and the session it names.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use v2_lib::mcp::{self, relay_or_local, SharedCall, SharedRelay};
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
        &|b: &str| {
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
    let got = relay_or_local(LIST, "s", &|_b: &str| Ok((200, r#"{"reply":null}"#.to_string())), &|_m: &str| Some("local".into()));
    assert_eq!(got, None);
}

#[test]
fn an_app_without_the_route_is_answered_locally() {
    let got = relay_or_local(LIST, "s", &|_b: &str| Ok((404, "unknown route".to_string())), &local_answer);
    assert_eq!(got, local_answer(LIST));
    assert!(got.is_some());
}

#[test]
fn a_closed_app_is_answered_locally() {
    let got = relay_or_local(LIST, "s", &|_b: &str| Err("handshake file missing - is the app running?".to_string()), &local_answer);
    assert_eq!(got, local_answer(LIST));
}

#[test]
fn every_other_failure_is_answered_locally() {
    for (status, body) in [(413u16, "too large"), (500, "boom"), (503, "busy"), (400, "no"), (200, "not json"), (200, r#"{"other":1}"#)] {
        let got = relay_or_local(LIST, "s", &|_b: &str| Ok((status, body.to_string())), &|_m: &str| Some("local".into()));
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
            &|_b: &str| {
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
        &|_b: &str| {
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
    let relay: SharedRelay = Arc::new(move |b: &str| {
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

/// The temp folder pointed at `dir` while it lives, put back on drop. `dir` is never deleted: another
/// test that makes a temp folder meanwhile would find it inside this one.
struct TempAt {
    old: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl TempAt {
    fn new(dir: &std::path::Path) -> TempAt {
        let old = ["TEMP", "TMP"].iter().map(|k| (*k, std::env::var_os(k))).collect();
        for k in ["TEMP", "TMP"] {
            std::env::set_var(k, dir);
        }
        TempAt { old }
    }
}

impl Drop for TempAt {
    fn drop(&mut self) {
        for (k, v) in &self.old {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

#[test]
fn a_relay_after_an_app_restart_uses_the_new_handshake() {
    let _lock = crate::serial::temp_dir_env();
    let dir = std::env::temp_dir().join("tcm-relay-connector-test");
    std::fs::create_dir_all(&dir).unwrap();
    let _temp = TempAt::new(&dir);
    let (a, b) = (Listener::start(), Listener::start());
    let handshake = v2_lib::ai_bridge::handshake_path();
    assert!(handshake.starts_with(&dir), "{handshake:?}");
    let write = |port: u16| std::fs::write(&handshake, format!(r#"{{"port":{port},"token":"tok","version":"1.0.0"}}"#)).unwrap();
    let body = r#"{"session":"s","message":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;

    write(a.port);
    assert_eq!(mcp::relay_to_app(body).unwrap().0, 200);
    write(b.port);
    assert_eq!(mcp::relay_to_app(body).unwrap().0, 200);
    let _ = std::fs::remove_file(&handshake);

    assert_eq!(a.hits.lock().unwrap().len(), 1);
    assert_eq!(b.hits.lock().unwrap().len(), 1);
    assert!(b.hits.lock().unwrap()[0].starts_with("POST /mcp "), "{:?}", b.hits.lock().unwrap());
}
