//! What the application refused: the requests an action started that the
//! application answered with a 4xx, and the message and trace id its body
//! carries.

use crate::common::ScriptedDriver;
use serde_json::{json, Value};
use v2_lib::autorun::api_checks::shown_body;
use v2_lib::autorun::refusals::{app_refused_line, counts, message_and_trace, refused_since, RefusedRequest, APP_REFUSED};
use v2_lib::browser::cdp::{CdpError, Driver, Event};
use v2_lib::browser::net_record::{NetEntry, NetState};

fn ev(method: &str, params: Value) -> Event {
    Event { method: method.to_string(), params }
}

fn entry(kind: &str, status: Option<u16>) -> NetEntry {
    NetEntry {
        seq: 0,
        id: "r0".to_string(),
        method: "POST".to_string(),
        path_query: "/api/eval-rules?x=1".to_string(),
        status,
        mime: Some("application/json".to_string()),
        state: NetState::Finished,
        redirect: None,
        kind: Some(kind.to_string()),
        same_site: true,
        polling: false,
    }
}

#[test]
fn an_aspnet_problem_body_gives_its_title_and_trace_id() {
    let body = r#"{"title":"One or more validation errors occurred.","errors":{"Weightage":["Weightage must total 100."]},"traceId":"00-4bf9-00"}"#;
    let (message, trace) = message_and_trace(400, Some(body));
    assert_eq!(message, "One or more validation errors occurred.");
    assert_eq!(trace.as_deref(), Some("00-4bf9-00"));
}

#[test]
fn a_message_key_wins_over_title() {
    let (message, trace) = message_and_trace(409, Some(r#"{"message":"Code already exists","title":"Conflict"}"#));
    assert_eq!(message, "Code already exists");
    assert_eq!(trace, None);
}

#[test]
fn errors_alone_give_their_first_entry() {
    assert_eq!(message_and_trace(400, Some(r#"{"errors":{"Name":["Name is required"]}}"#)).0, "Name is required");
    assert_eq!(message_and_trace(400, Some(r#"{"errors":{"Name":"Name is required"}}"#)).0, "Name is required");
    assert_eq!(message_and_trace(400, Some(r#"{"errors":["Name is required","Other"]}"#)).0, "Name is required");
}

#[test]
fn the_other_message_keys_are_read_in_order() {
    assert_eq!(message_and_trace(400, Some(r#"{"error":"Bad code"}"#)).0, "Bad code");
    assert_eq!(message_and_trace(400, Some(r#"{"error":{"message":"Bad code"}}"#)).0, "Bad code");
    assert_eq!(
        message_and_trace(400, Some(r#"{"detail":"Too long","trace_id":"t-9"}"#)),
        ("Too long".to_string(), Some("t-9".to_string()))
    );
    assert_eq!(
        message_and_trace(400, Some(r#"{"Message":"Capital","TraceId":"T1"}"#)),
        ("Capital".to_string(), Some("T1".to_string()))
    );
    assert_eq!(message_and_trace(400, Some(r#"{"message":"","error_description":"Second"}"#)).0, "Second");
}

#[test]
fn a_plain_text_body_gives_its_first_200_characters() {
    let long = "x".repeat(500);
    assert_eq!(message_and_trace(400, Some("Name is required")).0, "Name is required");
    assert_eq!(message_and_trace(400, Some(&long)).0.chars().count(), 200);
}

#[test]
fn an_html_or_missing_body_gives_the_status() {
    assert_eq!(message_and_trace(422, Some("<html><body>nope</body></html>")).0, "422 Unprocessable Entity");
    assert_eq!(message_and_trace(400, None).0, "400 Bad Request");
    assert_eq!(message_and_trace(409, Some("   ")).0, "409 Conflict");
    assert_eq!(message_and_trace(418, None).0, "418");
    assert_eq!(message_and_trace(400, Some(r#"{"unrelated":1}"#)).0, "400 Bad Request");
}

#[test]
fn the_message_is_collapsed_and_capped() {
    let body = json!({ "message": format!("a\n  b {}", "y".repeat(400)) }).to_string();
    let message = message_and_trace(400, Some(&body)).0;
    assert!(message.starts_with("a b y"), "{message}");
    assert_eq!(message.chars().count(), 300);
}

#[test]
fn a_secret_in_the_body_is_never_shown() {
    // A message that holds JSON with a secret in it.
    let inner = r#"{"detail":"bad","token":"abc123secret"}"#;
    let body = json!({ "message": inner, "token": "abc123secret" }).to_string();
    let shown = shown_body(&body);
    assert!(!shown.contains("abc123secret"), "{shown}");
    let (message, _) = message_and_trace(400, Some(&body));
    assert!(!message.contains("abc123secret"), "{message}");
    // A field named like a secret never supplies the message.
    let (message, _) = message_and_trace(400, Some(r#"{"errors":{"Password":["abc123secret"]}}"#));
    assert!(!message.contains("abc123secret"), "{message}");
    assert_eq!(message, "400 Bad Request");
    // Plain text holding a quoted secret member.
    let (message, _) = message_and_trace(400, Some(r#"bad request {"password":"abc123secret"}"#));
    assert!(!message.contains("abc123secret"), "{message}");
}

#[test]
fn which_requests_count() {
    let own = vec!["mine".to_string()];
    assert!(counts(&entry("XHR", Some(400)), &own));
    assert!(counts(&entry("Fetch", Some(422)), &own));
    for s in [401, 403, 404, 500, 200, 399] {
        assert!(!counts(&entry("XHR", Some(s)), &own), "{s}");
    }
    let mut e = entry("XHR", Some(400));
    e.polling = true;
    assert!(!counts(&e, &own), "a poll");
    assert!(!counts(&entry("Document", Some(400)), &own), "a document");
    let mut e = entry("XHR", Some(400));
    e.same_site = false;
    assert!(!counts(&e, &own), "another site");
    let mut e = entry("XHR", Some(400));
    e.id = "mine".to_string();
    assert!(!counts(&e, &own), "TCM's own");
    let mut e = entry("XHR", Some(400));
    e.state = NetState::Pending;
    assert!(!counts(&e, &own), "unfinished");
    let mut e = entry("XHR", Some(400));
    e.kind = None;
    assert!(!counts(&e, &own), "no type");
}

#[test]
fn the_line_names_method_path_status_and_trace() {
    let r = RefusedRequest {
        method: "POST".into(),
        path: "/api/eval-rules".into(),
        status: 400,
        message: "Weightage must total 100.".into(),
        trace_id: Some("00-4bf9-00".into()),
    };
    assert_eq!(
        app_refused_line(&r),
        r#"the app refused: "Weightage must total 100." (POST /api/eval-rules, 400, trace id 00-4bf9-00)"#
    );
    assert!(app_refused_line(&r).starts_with(APP_REFUSED));
    let r = RefusedRequest { trace_id: None, ..r };
    assert_eq!(app_refused_line(&r), r#"the app refused: "Weightage must total 100." (POST /api/eval-rules, 400)"#);
}

fn sent(id: &str, method: &str, url: &str, kind: &str) -> Event {
    ev(
        "Network.requestWillBeSent",
        json!({ "requestId": id, "type": kind, "documentURL": "https://hr.example/hr/page",
                "request": { "url": url, "method": method } }),
    )
}

fn refused(id: &str, status: u64) -> Vec<Event> {
    vec![
        ev("Network.responseReceived", json!({ "requestId": id, "response": { "status": status, "mimeType": "application/json" } })),
        ev("Network.loadingFinished", json!({ "requestId": id })),
    ]
}

fn browser(body: Option<&'static str>) -> ScriptedDriver {
    ScriptedDriver::new(move |method, _| match method {
        "Network.getResponseBody" => match body {
            Some(b) => Ok(json!({ "body": b, "base64Encoded": false })),
            None => Err(CdpError::Protocol { method: method.into(), message: "gone".into() }),
        },
        _ => Ok(json!({})),
    })
    .with_net_record()
}

fn feed(d: &mut ScriptedDriver, events: Vec<Event>) {
    let net = d.net.as_mut().expect("a net record");
    for e in &events {
        net.observe(e);
    }
}

#[tokio::test]
async fn refused_since_reads_the_body_of_the_requests_that_count() {
    let mut d = browser(Some(r#"{"message":"Weightage must total 100.","traceId":"00-4bf9-00"}"#));
    let mark = d.net_mark();
    let mut events = vec![sent("1", "POST", "https://hr.example/api/eval-rules?x=1", "XHR")];
    events.extend(refused("1", 400));
    events.push(sent("2", "GET", "https://hr.example/api/menu", "XHR"));
    events.extend(refused("2", 404));
    events.push(sent("3", "POST", "https://hr.example/api/mine", "XHR"));
    events.extend(refused("3", 400));
    feed(&mut d, events);
    let got = refused_since(&mut d, mark, &["3".to_string()]).await;
    assert_eq!(
        got,
        vec![RefusedRequest {
            method: "POST".into(),
            path: "/api/eval-rules".into(),
            status: 400,
            message: "Weightage must total 100.".into(),
            trace_id: Some("00-4bf9-00".into()),
        }]
    );
    assert_eq!(d.calls_to("Network.getResponseBody"), vec![json!({ "requestId": "1" })], "only a counted request's body is read");
}

#[tokio::test]
async fn a_gone_body_gives_the_status_and_at_most_three_are_kept() {
    let mut d = browser(None);
    let mark = d.net_mark();
    let mut events = Vec::new();
    for i in 0..5 {
        let id = format!("{i}");
        events.push(sent(&id, "POST", &format!("https://hr.example/api/r{i}"), "Fetch"));
        events.extend(refused(&id, 409));
    }
    feed(&mut d, events);
    let got = refused_since(&mut d, mark, &[]).await;
    assert_eq!(got.len(), 3);
    assert_eq!(got[0].path, "/api/r0");
    assert_eq!(got[2].path, "/api/r2");
    assert!(got.iter().all(|r| r.message == "409 Conflict" && r.trace_id.is_none()), "{got:?}");
}
