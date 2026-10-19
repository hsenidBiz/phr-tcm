//! The capture store: the supervised browser's own requests and responses
//! (same-site XHR, fetch and documents, polls left out), with allow-listed
//! headers and bodies read at `loadingFinished`, within budgets.
//!
//! The store is fed events directly where it can be (`feed`), with a
//! network record beside it as `Cdp` keeps one per tab; the routing, the
//! body reads and a new tab go through a `Cdp` over a fake browser socket.

use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;
use v2_lib::browser::cdp::{Cdp, Driver, Event, Transport};
use v2_lib::browser::net_capture::{
    Body, CaptureFilter, NetCapture, MAX_BODY_BYTES, MAX_CAPTURED, MAX_REQUEST_BODY_BYTES, MAX_TOTAL_BODY_BYTES,
};
use v2_lib::browser::net_record::NetRecord;

const SITE: &str = "https://hr.example";
const MAIN: &str = "S-main";

fn ev(method: &str, params: Value) -> Event {
    Event { method: method.to_string(), params }
}

/// The page at `SITE` asks for `path` as a request of this type.
fn asked(id: &str, kind: &str, method: &str, url: &str, headers: Value, post: Option<&str>) -> Value {
    let mut request = json!({ "url": url, "method": method, "headers": headers });
    if let Some(p) = post {
        request["postData"] = json!(p);
        request["hasPostData"] = json!(true);
    }
    json!({
        "requestId": id, "type": kind, "documentURL": format!("{SITE}/hr/page"),
        "request": request, "timestamp": 100.0, "wallTime": 1_760_000_000.0
    })
}

fn answered(id: &str, status: u64, mime: &str, headers: Value) -> Value {
    json!({ "requestId": id, "type": "XHR", "response": { "status": status, "mimeType": mime, "headers": headers } })
}

fn finished(id: &str, size: u64) -> Value {
    json!({ "requestId": id, "timestamp": 100.25, "encodedDataLength": size })
}

/// Feed one event to the record and then the capture, in the order `Cdp`
/// does, on `main`.
fn feed(rec: &mut NetRecord, cap: &mut NetCapture, method: &str, params: Value) {
    let e = ev(method, params);
    rec.observe(&e);
    cap.observe(MAIN, "main", &e, rec);
}

/// One whole request: asked, answered and finished.
fn whole(rec: &mut NetRecord, cap: &mut NetCapture, id: &str, kind: &str, url: &str, mime: &str) {
    feed(rec, cap, "Network.requestWillBeSent", asked(id, kind, "GET", url, json!({}), None));
    feed(rec, cap, "Network.responseReceived", answered(id, 200, mime, json!({})));
    feed(rec, cap, "Network.loadingFinished", finished(id, 10));
}

/// Answer every queued read from `bodies` (by request id); one not there
/// is gone, as Chrome says when it no longer holds a body.
fn serve(cap: &mut NetCapture, bodies: &HashMap<String, String>) {
    while let Some(read) = cap.next_read() {
        let answer = bodies.get(&read.request_id).map(|b| json!({ "body": b, "postData": b, "base64Encoded": false }));
        cap.answered(&read, answer.as_ref());
    }
}

fn kept_paths(cap: &NetCapture) -> Vec<String> {
    let (all, _) = cap.list(0, MAX_CAPTURED);
    all.iter().rev().map(|c| c.url_path_query.clone()).collect()
}

/// A fake browser socket: answers every command at once on its session,
/// `Network.getResponseBody` / `getRequestPostData` from `bodies` / `posts`
/// (a missing one refused, as Chrome refuses a body it dropped).
struct FakeBrowser {
    incoming: VecDeque<String>,
    sent: Vec<Value>,
    bodies: HashMap<String, String>,
    posts: HashMap<String, String>,
}

impl Transport for FakeBrowser {
    async fn send(&mut self, text: String) -> Result<(), String> {
        let v: Value = serde_json::from_str(&text).expect("the client sent a frame that is not JSON");
        let id = v["id"].as_u64().expect("every frame has an id");
        let method = v["method"].as_str().unwrap_or("");
        let rid = v["params"]["requestId"].as_str().unwrap_or("");
        let gone = json!({ "id": id, "error": { "code": -32000, "message": "No resource with given identifier found" } });
        let mut reply = match method {
            "Target.attachToTarget" => json!({ "id": id, "result": { "sessionId": MAIN } }),
            "Network.getResponseBody" => match self.bodies.get(rid) {
                Some(b) => json!({ "id": id, "result": { "body": b, "base64Encoded": false } }),
                None => gone,
            },
            "Network.getRequestPostData" => match self.posts.get(rid) {
                Some(b) => json!({ "id": id, "result": { "postData": b } }),
                None => gone,
            },
            _ => json!({ "id": id, "result": {} }),
        };
        if let Some(s) = v.get("sessionId") {
            reply["sessionId"] = s.clone();
        }
        self.sent.push(v);
        self.incoming.push_back(reply.to_string());
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, String>> {
        match self.incoming.pop_front() {
            Some(f) => Some(Ok(f)),
            None => {
                std::future::pending::<()>().await;
                None
            }
        }
    }
}

/// A supervised browser's connection: `main` driven over the browser's own
/// socket, capturing.
async fn browser() -> Cdp<FakeBrowser> {
    let fake = FakeBrowser { incoming: VecDeque::new(), sent: vec![], bodies: HashMap::new(), posts: HashMap::new() };
    let mut cdp = Cdp::over(fake);
    cdp.drive_first_page("T-main", "https://hr.example/home").await.expect("main was not attached");
    cdp.capture_requests();
    cdp
}

fn on(cdp: &mut Cdp<FakeBrowser>, session: &str, method: &str, params: Value) {
    let f = json!({ "method": method, "params": params, "sessionId": session });
    cdp.transport_mut().incoming.push_back(f.to_string());
}

async fn settle(cdp: &mut Cdp<FakeBrowser>) {
    cdp.pump(Duration::from_millis(50)).await;
}

fn reads_sent(cdp: &Cdp<FakeBrowser>) -> Vec<(String, String)> {
    cdp.transport()
        .sent
        .iter()
        .filter(|f| f["method"].as_str().is_some_and(|m| m.starts_with("Network.get")))
        .map(|f| (f["method"].as_str().unwrap_or("").to_string(), f["sessionId"].as_str().unwrap_or("").to_string()))
        .collect()
}

#[tokio::test]
async fn a_same_site_xhr_is_kept_with_its_bodies() {
    let mut cdp = browser().await;
    assert!(Driver::capture(&cdp).is_some(), "the supervised browser captures");
    cdp.transport_mut().bodies.insert("r1".into(), r#"{"saved":true,"id":7}"#.into());
    cdp.transport_mut().posts.insert("r1".into(), r#"{"name":"Cycle 1"}"#.into());
    cdp.transport_mut().bodies.insert("r2".into(), "UserName=a".into());
    // r1's body is not in the event (hasPostData, no postData): read.
    let mut sent = asked("r1", "XHR", "POST", "https://hr.example/hr/api/Cycle/Save?x=1", json!({
        "Content-Type": "application/json", "Accept": "application/json", "X-Requested-With": "XMLHttpRequest"
    }), None);
    sent["request"]["hasPostData"] = json!(true);
    on(&mut cdp, MAIN, "Network.requestWillBeSent", sent);
    on(&mut cdp, MAIN, "Network.responseReceived", answered("r1", 200, "application/json", json!({ "Content-Type": "application/json; charset=utf-8", "Content-Length": "21" })));
    on(&mut cdp, MAIN, "Network.loadingFinished", finished("r1", 321));
    // r2 carries its body in the event: nothing to read for the request.
    on(&mut cdp, MAIN, "Network.requestWillBeSent", asked("r2", "Fetch", "POST", "https://hr.example/hr/api/Echo", json!({}), Some("a=1&b=2")));
    on(&mut cdp, MAIN, "Network.responseReceived", answered("r2", 201, "text/plain", json!({})));
    on(&mut cdp, MAIN, "Network.loadingFinished", finished("r2", 50));
    settle(&mut cdp).await;

    let cap = Driver::capture(&cdp).expect("capturing");
    let (all, total) = cap.list(0, 10);
    assert_eq!(total, 2);
    assert_eq!(all[0].request_id, "r2", "newest first");
    let c = cap.get(all[1].id).expect("got by id");
    assert_eq!(c.request_id, "r1");
    assert_eq!(c.tab, "main");
    assert_eq!(c.method, "POST");
    assert_eq!(c.url_path_query, "/hr/api/Cycle/Save?x=1", "path and query, never the host");
    assert_eq!(c.kind.as_deref(), Some("XHR"));
    assert_eq!(c.status, Some(200));
    assert_eq!(c.mime.as_deref(), Some("application/json"));
    assert_eq!(c.req_body, Some(Body::Text(r#"{"name":"Cycle 1"}"#.into())), "read with getRequestPostData");
    assert_eq!(c.resp_body, Some(Body::Text(r#"{"saved":true,"id":7}"#.into())), "read at loadingFinished");
    assert_eq!(c.resp_size, Some(321));
    assert_eq!(c.started_ms, 1_760_000_000_000);
    assert_eq!(c.duration_ms, Some(250));
    assert_eq!(c.failed, None);
    assert!(c.req_headers.contains(&("content-type".into(), "application/json".into())), "{:?}", c.req_headers);
    assert!(c.req_headers.contains(&("x-requested-with".into(), "XMLHttpRequest".into())), "{:?}", c.req_headers);
    assert!(c.resp_headers.contains(&("content-length".into(), "21".into())), "{:?}", c.resp_headers);
    assert_eq!(all[0].req_body, Some(Body::Text("a=1&b=2".into())));
    assert_eq!(all[0].resp_body, Some(Body::Text("UserName=a".into())));
    let reads = reads_sent(&cdp);
    assert!(reads.iter().all(|(_, s)| s == MAIN), "read on the tab's own session: {reads:?}");
    assert_eq!(reads.iter().filter(|(m, _)| m == "Network.getRequestPostData").count(), 1, "{reads:?}");
}

#[tokio::test]
async fn a_body_the_browser_dropped_is_marked_dropped_and_a_picture_is_not_text() {
    let mut cdp = browser().await;
    on(&mut cdp, MAIN, "Network.requestWillBeSent", asked("r1", "XHR", "GET", "https://hr.example/api/list", json!({}), None));
    on(&mut cdp, MAIN, "Network.responseReceived", answered("r1", 200, "application/json", json!({})));
    on(&mut cdp, MAIN, "Network.loadingFinished", finished("r1", 10));
    on(&mut cdp, MAIN, "Network.requestWillBeSent", asked("r2", "XHR", "GET", "https://hr.example/api/photo", json!({}), None));
    on(&mut cdp, MAIN, "Network.responseReceived", answered("r2", 200, "image/png", json!({})));
    on(&mut cdp, MAIN, "Network.loadingFinished", finished("r2", 10));
    settle(&mut cdp).await;
    let cap = Driver::capture(&cdp).unwrap();
    let (all, _) = cap.list(0, 10);
    assert_eq!(all[1].resp_body, Some(Body::Dropped));
    assert_eq!(all[0].resp_body, Some(Body::NotText));
    assert_eq!(reads_sent(&cdp).len(), 1, "a picture's body is never read");
}

#[test]
fn polls_other_sites_images_are_not_kept() {
    let (mut rec, mut cap) = (NetRecord::default(), NetCapture::default());
    whole(&mut rec, &mut cap, "1", "XHR", "https://hr.example/api/kept", "application/json");
    // The same method and path again within two seconds: both are polls.
    whole(&mut rec, &mut cap, "2", "XHR", "https://hr.example/api/notifications?n=1", "application/json");
    whole(&mut rec, &mut cap, "3", "XHR", "https://hr.example/api/notifications?n=2", "application/json");
    whole(&mut rec, &mut cap, "4", "XHR", "https://cdn.other.example/api/x", "application/json");
    whole(&mut rec, &mut cap, "5", "Image", "https://hr.example/logo.png", "image/png");
    whole(&mut rec, &mut cap, "6", "Script", "https://hr.example/app.js", "application/javascript");
    whole(&mut rec, &mut cap, "7", "Stylesheet", "https://hr.example/app.css", "text/css");
    whole(&mut rec, &mut cap, "8", "Document", "https://hr.example/hr/page", "text/html");
    whole(&mut rec, &mut cap, "9", "Fetch", "https://hr.example/api/also", "application/json");
    whole(&mut rec, &mut cap, "10", "Other", "https://hr.example/api/other", "application/json");
    assert_eq!(kept_paths(&cap), ["/api/kept", "/hr/page", "/api/also"]);
    serve(&mut cap, &HashMap::new());
    assert_eq!(cap.queued_reads(), 0);
}

#[test]
fn a_redirect_keeps_its_first_hop() {
    let (mut rec, mut cap) = (NetRecord::default(), NetCapture::default());
    feed(&mut rec, &mut cap, "Network.requestWillBeSent", asked("1", "Document", "POST", "https://hr.example/Account/Login?ReturnUrl=%2F", json!({}), Some("x=1")));
    let mut hop = asked("1", "Document", "GET", "https://hr.example/hr/home", json!({}), None);
    hop["redirectResponse"] = json!({ "url": "https://hr.example/Account/Login?ReturnUrl=%2F", "status": 302, "mimeType": "text/html", "headers": {} });
    feed(&mut rec, &mut cap, "Network.requestWillBeSent", hop);
    let mut hop2 = asked("1", "Document", "GET", "https://hr.example/hr/home2", json!({}), None);
    hop2["redirectResponse"] = json!({ "url": "https://hr.example/hr/home", "status": 301, "headers": {} });
    feed(&mut rec, &mut cap, "Network.requestWillBeSent", hop2);
    feed(&mut rec, &mut cap, "Network.responseReceived", answered("1", 200, "text/html", json!({})));
    feed(&mut rec, &mut cap, "Network.loadingFinished", finished("1", 10));
    let (all, total) = cap.list(0, 10);
    assert_eq!(total, 1, "one request, however many hops");
    let c = all[0];
    assert_eq!(c.method, "POST", "what the page asked");
    assert_eq!(c.url_path_query, "/Account/Login?ReturnUrl=%2F");
    assert_eq!(c.redirect.as_deref(), Some("302 /hr/home"), "the first hop");
    assert_eq!(c.status, Some(200), "the final answer");
    assert_eq!(c.req_body, Some(Body::Text("x=1".into())));
}

#[test]
fn budgets_drop_oldest_bodies_and_requests() {
    let (mut rec, mut cap) = (NetRecord::default(), NetCapture::default());
    // A long discovery: 1500 requests, 50 MB of JSON in all.
    let json_body = format!("{{\"rows\":\"{}\"}}", "x".repeat(50 * 1024 * 1024 / 1500));
    let mut bodies = HashMap::new();
    for i in 0..1500 {
        let id = format!("r{i}");
        whole(&mut rec, &mut cap, &id, "XHR", &format!("https://hr.example/api/item/{i}"), "application/json");
        bodies.insert(id, json_body.clone());
        serve(&mut cap, &bodies);
        assert!(cap.body_bytes() <= MAX_TOTAL_BODY_BYTES, "over the budget at {i}: {}", cap.body_bytes());
    }
    let (all, total) = cap.list(0, MAX_CAPTURED);
    assert_eq!(total, MAX_CAPTURED, "at most 1000 requests");
    assert_eq!(all.last().unwrap().url_path_query, "/api/item/500", "the oldest went first");
    assert_eq!(all[0].url_path_query, "/api/item/1499");
    assert_eq!(all[0].resp_body, Some(Body::Text(json_body.clone())), "the newest body is kept");
    assert_eq!(all.last().unwrap().resp_body, Some(Body::Dropped), "the oldest bodies went first; the request stays");
    let kept: usize = all.iter().filter(|c| matches!(c.resp_body, Some(Body::Text(_)))).count();
    assert!(kept > 0 && kept < MAX_CAPTURED, "{kept}");
    // Every kept body is newer than every dropped one.
    let first_dropped = all.iter().position(|c| c.resp_body == Some(Body::Dropped)).unwrap();
    assert!(all[first_dropped..].iter().all(|c| c.resp_body == Some(Body::Dropped)));

    // One body over 2 MB is not kept; a request body over 256 KB neither.
    let big = "y".repeat(MAX_BODY_BYTES + 1);
    whole(&mut rec, &mut cap, "big", "XHR", "https://hr.example/api/big", "application/json");
    serve(&mut cap, &HashMap::from([("big".to_string(), big)]));
    assert_eq!(cap.list(0, 1).0[0].resp_body, Some(Body::TooBig));
    let post = "z".repeat(MAX_REQUEST_BODY_BYTES + 1);
    feed(&mut rec, &mut cap, "Network.requestWillBeSent", asked("bigpost", "XHR", "POST", "https://hr.example/api/upload", json!({}), Some(&post)));
    assert_eq!(cap.list(0, 1).0[0].req_body, Some(Body::TooBig));
    // Reported as received, never read when the browser says it is over.
    feed(&mut rec, &mut cap, "Network.requestWillBeSent", asked("huge", "XHR", "GET", "https://hr.example/api/huge", json!({}), None));
    feed(&mut rec, &mut cap, "Network.responseReceived", answered("huge", 200, "application/json", json!({})));
    feed(&mut rec, &mut cap, "Network.loadingFinished", finished("huge", (MAX_BODY_BYTES as u64) * 2));
    assert_eq!(cap.queued_reads(), 0);
    assert_eq!(cap.list(0, 1).0[0].resp_body, Some(Body::TooBig));
    assert!(cap.body_bytes() <= MAX_TOTAL_BODY_BYTES);
}

#[test]
fn a_filter_decides_what_is_kept_from_now_on() {
    let (mut rec, mut cap) = (NetRecord::default(), NetCapture::default());
    whole(&mut rec, &mut cap, "1", "XHR", "https://hr.example/api/before", "application/json");
    cap.set_filter(CaptureFilter { include_types: vec!["fetch".into()], exclude_types: vec![], path_contains: None });
    assert_eq!(cap.filter().include_types, ["fetch"]);
    whole(&mut rec, &mut cap, "2", "XHR", "https://hr.example/api/xhr", "application/json");
    whole(&mut rec, &mut cap, "3", "Fetch", "https://hr.example/api/fetch", "application/json");
    cap.set_filter(CaptureFilter { include_types: vec![], exclude_types: vec!["Document".into()], path_contains: Some("/API/".into()) });
    whole(&mut rec, &mut cap, "4", "Document", "https://hr.example/api/page", "text/html");
    whole(&mut rec, &mut cap, "5", "XHR", "https://hr.example/other/x", "application/json");
    whole(&mut rec, &mut cap, "6", "XHR", "https://hr.example/hr/api/y", "application/json");
    assert_eq!(kept_paths(&cap), ["/api/before", "/api/fetch", "/hr/api/y"], "what was kept before stays");
    let last = cap.list(0, 1).0[0].id;
    cap.set_filter(CaptureFilter::default());
    cap.clear();
    assert_eq!(cap.list(0, 10).1, 0);
    assert_eq!(cap.body_bytes(), 0);
    whole(&mut rec, &mut cap, "7", "XHR", "https://hr.example/api/after", "application/json");
    assert_eq!(kept_paths(&cap), ["/api/after"]);
    assert!(cap.list(0, 10).0[0].id > last, "an id is never handed out twice");
}

#[test]
fn cookie_and_authorization_headers_are_never_kept() {
    let (mut rec, mut cap) = (NetRecord::default(), NetCapture::default());
    let headers = json!({
        "Cookie": ".ASPXAUTH=abc", "Authorization": "Bearer hunter2", "Proxy-Authorization": "Basic aHVudGVy",
        "X-Session-Id": "s3cr3t", "X-Auth-Token": "t0k3n", "X-CSRF-TOKEN": "csrf-value", "X-Tenant": "acme",
        "Content-Type": "application/json", "User-Agent": "Edge", "Referer": "https://hr.example/hr/page?token=1"
    });
    feed(&mut rec, &mut cap, "Network.requestWillBeSent", asked("1", "XHR", "POST", "https://hr.example/api/x", headers, Some("{}")));
    feed(&mut rec, &mut cap, "Network.responseReceived", answered("1", 200, "application/json", json!({
        "Set-Cookie": "session=abc; HttpOnly", "set-cookie": "a=b", "Content-Type": "application/json", "X-Powered-By": "ASP.NET",
        "WWW-Authenticate": "Bearer"
    })));
    let c = cap.list(0, 1).0[0];
    let names: Vec<&str> = c.req_headers.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["content-type", "x-csrf-token", "x-tenant"], "{:?}", c.req_headers);
    assert!(c.req_headers.contains(&("x-csrf-token".into(), "[redacted]".into())), "its name only: {:?}", c.req_headers);
    let resp: Vec<&str> = c.resp_headers.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(resp, ["content-type", "x-powered-by"], "{:?}", c.resp_headers);
    let all = format!("{:?}", c);
    for secret in ["abc", "hunter2", "aHVudGVy", "s3cr3t", "t0k3n", "csrf-value"] {
        assert!(!all.contains(secret), "{secret} kept: {all}");
    }
}

#[tokio::test]
async fn a_new_tab_is_captured() {
    let mut cdp = browser().await;
    let popup = "S-pop";
    cdp.transport_mut().bodies.insert("p1".into(), "[1,2,3]".into());
    let attached = json!({ "method": "Target.attachedToTarget", "params": {
        "sessionId": popup,
        "targetInfo": { "targetId": "T-pop", "type": "page", "url": "https://hr.example/hr/report", "attached": true },
        "waitingForDebugger": true
    } });
    cdp.transport_mut().incoming.push_back(attached.to_string());
    settle(&mut cdp).await;
    assert!(
        cdp.transport().sent.iter().any(|f| f["method"] == "Network.enable" && f["sessionId"] == popup),
        "the new tab's network events are switched on"
    );
    on(&mut cdp, popup, "Network.requestWillBeSent", asked("p1", "XHR", "GET", "https://hr.example/hr/api/report", json!({}), None));
    on(&mut cdp, popup, "Network.responseReceived", answered("p1", 200, "application/json", json!({})));
    on(&mut cdp, popup, "Network.loadingFinished", finished("p1", 7));
    settle(&mut cdp).await;
    let cap = Driver::capture(&cdp).unwrap();
    let (all, total) = cap.list(0, 10);
    assert_eq!(total, 1);
    assert_eq!(all[0].url_path_query, "/hr/api/report");
    assert_eq!(all[0].tab, "", "a tab the page opened has no name yet");
    assert_eq!(all[0].resp_body, Some(Body::Text("[1,2,3]".into())));
    assert!(reads_sent(&cdp).iter().all(|(_, s)| s == popup), "read on the popup's own session");
}

#[tokio::test]
async fn a_browser_not_capturing_reads_nothing() {
    let fake = FakeBrowser { incoming: VecDeque::new(), sent: vec![], bodies: HashMap::new(), posts: HashMap::new() };
    let mut cdp = Cdp::over(fake);
    cdp.drive_first_page("T-main", "https://hr.example/home").await.unwrap();
    on(&mut cdp, MAIN, "Network.requestWillBeSent", asked("r1", "XHR", "GET", "https://hr.example/api/list", json!({}), None));
    on(&mut cdp, MAIN, "Network.responseReceived", answered("r1", 200, "application/json", json!({})));
    on(&mut cdp, MAIN, "Network.loadingFinished", finished("r1", 10));
    settle(&mut cdp).await;
    assert!(Driver::capture(&cdp).is_none(), "an unattended run's browser is not captured");
    assert!(reads_sent(&cdp).is_empty());
}
