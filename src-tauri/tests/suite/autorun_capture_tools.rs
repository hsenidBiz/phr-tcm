//! The assistant's capture tools at the bridge: list, search, the request,
//! the response and its body, the filter, the export to TCM's captures
//! folder and the import from it - each over a scripted browser holding a
//! capture, or over an imported set with no browser at all.
//!
//! Every answer is redacted (`capture_redact`): the tests put a password, a
//! token and a session value in and look for them in what comes out.

use crate::common::ScriptedDriver;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use v2_lib::ai_bridge::{
    capture_tool_begin, capture_tool_in, route, BridgeContext, CapturePhase, CaptureTool, DiscoveryBrowser, DiscoveryParts,
};
use v2_lib::autorun::capture_tools::{
    file_stem, ImportedSets, BODY_PAGE_CHARS, IMPORTS_FULL, LIVE_IS_RESERVED, NOTHING_CAPTURED_YET,
};
use v2_lib::autorun::lease::Held;
use v2_lib::browser::cdp::Event;
use v2_lib::browser::net_capture::NetCapture;
use v2_lib::browser::net_record::NetRecord;
use v2_lib::commands::autorun::{DiscoveryState, GuardOwner, NO_BROWSER_OPEN};

const SITE: &str = "https://hr.example";
const PAGE: &str = "https://hr.example/hr/page";
const PASSWORD: &str = "Hunter2!xyz";
const TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhdmluIn0.Zk3qT9vLm2Xw8pRt";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("tcm-capture-tools-{nanos}-{}", N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The supervised browser as the bridge sees it.
struct Browser {
    d: ScriptedDriver,
    lease: Held,
    account: Option<String>,
    discovery: Option<DiscoveryState>,
    guarded_case: Option<i32>,
    guard_owner: Option<GuardOwner>,
}

impl DiscoveryBrowser for Browser {
    type D = ScriptedDriver;
    fn parts(&mut self) -> DiscoveryParts<'_, ScriptedDriver> {
        DiscoveryParts {
            driver: &mut self.d,
            lease: &mut self.lease,
            signed_in: &mut self.account,
            discovery: &mut self.discovery,
            guarded_case: &mut self.guarded_case,
            guard_owner: &mut self.guard_owner,
            overrides: None,
        }
    }
    fn close(self) {}
}

fn browser_with(cap: NetCapture) -> Option<Browser> {
    Some(Browser {
        d: ScriptedDriver::new(|_, _| Ok(json!({}))).with_capture(cap),
        lease: Held::supervised(),
        account: Some("hr.admin".into()),
        discovery: None,
        guarded_case: None,
        guard_owner: None,
    })
}

/// Feeds a capture as `Cdp` does: the record sees each event first.
struct Feeder {
    rec: NetRecord,
    cap: NetCapture,
    wall: f64,
}

impl Feeder {
    /// A capture whose site a sign-in confirmed.
    fn confirmed() -> Self {
        let mut cap = NetCapture::default();
        cap.set_app_site(Some("https://hr.example/Account/Login"));
        cap.confirm_app_site("https://hr.example/hr/home");
        Feeder { rec: NetRecord::default(), cap, wall: 1_760_000_000.0 }
    }

    /// A capture whose site is only provisional: what it keeps is held back.
    fn provisional() -> Self {
        let mut cap = NetCapture::default();
        cap.set_app_site(Some("https://hr.example/Account/Login"));
        Feeder { rec: NetRecord::default(), cap, wall: 1_760_000_000.0 }
    }

    fn feed(&mut self, method: &str, params: Value) {
        let e = Event { method: method.to_string(), params };
        self.rec.observe(&e);
        self.cap.observe("S-main", "main", PAGE, &e, &self.rec);
    }

    /// One whole request: asked, answered, finished, its body read.
    #[allow(clippy::too_many_arguments)]
    fn request(
        &mut self,
        id: &str,
        kind: &str,
        method: &str,
        path: &str,
        req_headers: Value,
        post: Option<&str>,
        status: u64,
        mime: &str,
        resp_headers: Value,
        body: Option<&str>,
    ) {
        self.wall += 1.0;
        let mut request = json!({ "url": format!("{SITE}{path}"), "method": method, "headers": req_headers });
        if let Some(p) = post {
            request["postData"] = json!(p);
            request["hasPostData"] = json!(true);
        }
        self.feed(
            "Network.requestWillBeSent",
            json!({ "requestId": id, "type": kind, "documentURL": PAGE, "request": request, "timestamp": 100.0, "wallTime": self.wall }),
        );
        self.feed(
            "Network.responseReceived",
            json!({ "requestId": id, "type": kind, "response": { "status": status, "mimeType": mime, "headers": resp_headers } }),
        );
        self.feed("Network.loadingFinished", json!({ "requestId": id, "timestamp": 100.25, "encodedDataLength": 321 }));
        while let Some(read) = self.cap.next_read() {
            let answer = body.map(|b| json!({ "body": b, "base64Encoded": false }));
            self.cap.answered(&read, answer.as_ref());
        }
    }

    /// A plain GET answered with JSON.
    fn get(&mut self, id: &str, path: &str, body: &str) {
        self.request(id, "XHR", "GET", path, json!({ "Accept": "application/json" }), None, 200, "application/json", json!({}), Some(body));
    }
}

/// The sign-in's own request and its answer, each holding a secret.
fn signed_in_capture() -> NetCapture {
    let mut f = Feeder::confirmed();
    f.request(
        "r1",
        "XHR",
        "POST",
        "/api/login?returnUrl=%2Fhr%2Fhome",
        json!({
            "Content-Type": "application/x-www-form-urlencoded",
            "Cookie": "ASP.NET_SessionId=s3cr3tSessionValue",
            "Authorization": format!("Bearer {TOKEN}"),
            "X-Request-Id": "r-1",
        }),
        Some(&format!("user=avin&password={PASSWORD}")),
        200,
        "application/json",
        json!({ "Set-Cookie": "auth=zzz", "Content-Type": "application/json", "X-Trace": "t-9" }),
        Some(&format!(r#"{{"access_token":"{TOKEN}","name":"Avin","next":"/hr/home"}}"#)),
    );
    f.get("r2", "/api/leave/types?page=2", r#"{"types":["Annual","Casual"]}"#);
    f.request("r3", "Document", "GET", "/hr/home", json!({}), None, 200, "text/html", json!({}), Some("<html><body>Home</body></html>"));
    f.cap
}

fn call(slot: &mut Option<Browser>, imported: &mut ImportedSets, root: Option<&Path>, tool: CaptureTool, args: Value) -> (u16, String) {
    capture_tool_in(slot, imported, root, tool, &args.to_string())
}

fn live(slot: &mut Option<Browser>, tool: CaptureTool, args: Value) -> (u16, Value) {
    let (status, out) = call(slot, &mut BTreeMap::new(), None, tool, args);
    (status, serde_json::from_str(&out).unwrap_or(Value::String(out)))
}

fn assert_no_secret(text: &str) {
    for secret in [PASSWORD, TOKEN, "s3cr3tSessionValue", "auth=zzz", "Bearer"] {
        assert!(!text.contains(secret), "{secret} shows in: {text}");
    }
}

// ------------------------------------------------------------------ list

#[test]
fn list_is_newest_first_paged_and_redacted() {
    let mut slot = browser_with(signed_in_capture());
    let (status, v) = live(&mut slot, CaptureTool::List, json!({}));
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["set"], "live");
    assert_eq!(v["total"], 3);
    let ids: Vec<u64> = v["requests"].as_array().unwrap().iter().map(|r| r["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, vec![3, 2, 1]);
    let first = &v["requests"][2];
    assert_eq!(first["method"], "POST");
    assert_eq!(first["path"], "/api/login?returnUrl=%2Fhr%2Fhome");
    assert_eq!(first["status"], 200);
    assert_eq!(first["type"], "XHR");
    let shown = r#"{"access_token":"[redacted]","name":"Avin","next":"/hr/home"}"#;
    assert_eq!(first["response_size"].as_u64(), Some(shown.len() as u64), "the redacted body's length: {first}");
    assert!(first["request_size"].as_u64().unwrap() > 0);
    assert!(first["started"].as_str().unwrap().starts_with("2025-"), "{first}");
    assert_eq!(first["duration_ms"], 250);
    assert_no_secret(&v.to_string());
}

#[test]
fn list_pages_at_25_by_default_and_100_at_most() {
    let mut f = Feeder::confirmed();
    for i in 1..=130 {
        f.get(&format!("q{i}"), &format!("/api/item/{i}"), "{}");
    }
    let mut slot = browser_with(f.cap);
    let (_, v) = live(&mut slot, CaptureTool::List, json!({}));
    assert_eq!((v["total"].as_u64(), v["returned"].as_u64(), v["next_offset"].as_u64()), (Some(130), Some(25), Some(25)));
    assert_eq!(v["requests"][0]["id"], 130);
    let (_, v) = live(&mut slot, CaptureTool::List, json!({ "limit": 500 }));
    assert_eq!(v["returned"], 100);
    let (_, v) = live(&mut slot, CaptureTool::List, json!({ "offset": "120", "limit": 25 }));
    assert_eq!(v["returned"], 10);
    assert_eq!(v["next_offset"], Value::Null);
    assert_eq!(v["requests"][0]["id"], 10);
    let (status, _) = live(&mut slot, CaptureTool::List, json!({ "offset": -1 }));
    assert_eq!(status, 400);
}

#[test]
fn a_query_secret_is_hidden_in_the_path() {
    let mut f = Feeder::confirmed();
    f.get("t1", "/api/reset?token=Ab3dEf9GhIjKlMnOpQrStUvWxYz012345&page=2", "{}");
    let mut slot = browser_with(f.cap);
    let (_, v) = live(&mut slot, CaptureTool::List, json!({}));
    let path = v["requests"][0]["path"].as_str().unwrap();
    assert!(!path.contains("Ab3dEf9GhIjKlMnOpQrStUvWxYz012345"), "{path}");
    assert!(path.contains("page=2"), "{path}");
}

#[test]
fn with_no_browser_a_live_read_is_404() {
    for tool in [
        CaptureTool::List,
        CaptureTool::Search,
        CaptureTool::Request,
        CaptureTool::Response,
        CaptureTool::ResponseBody,
        CaptureTool::GetFilter,
        CaptureTool::SetFilter,
    ] {
        let (status, out) = call(&mut None, &mut BTreeMap::new(), None, tool, json!({ "id": 1 }));
        assert_eq!((status, out.as_str()), (404, NO_BROWSER_OPEN), "{tool:?}");
    }
    let dir = TempDir::new();
    let (status, out) = call(&mut None, &mut BTreeMap::new(), Some(dir.path()), CaptureTool::Export, json!({ "name": "x" }));
    assert_eq!((status, out.as_str()), (404, NO_BROWSER_OPEN));
}

/// Through the real route, with no browser open in this test process.
#[tokio::test]
async fn the_route_answers_no_browser_is_open() {
    let ctx = BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() };
    let (status, out) = route(&ctx, None, "POST", "/autorun-requests", "{}", "1.0.0").await;
    assert_eq!((status, out.as_str()), (404, NO_BROWSER_OPEN));
    let (status, out) = route(&ctx, None, "GET", "/autorun-capture", "", "1.0.0").await;
    assert_eq!((status, out.as_str()), (404, NO_BROWSER_OPEN));
}

#[test]
fn before_the_site_is_confirmed_nothing_shows_and_it_says_why() {
    let mut f = Feeder::provisional();
    f.get("p1", "/api/leave/types", r#"{"types":["Annual"]}"#);
    let mut slot = browser_with(f.cap);
    let (status, v) = live(&mut slot, CaptureTool::List, json!({}));
    assert_eq!(status, 200);
    assert_eq!(v["total"], 0);
    assert_eq!(v["note"], NOTHING_CAPTURED_YET);
    assert!(NOTHING_CAPTURED_YET.contains("Sign in through a replay or a discovery"));
    let (_, v) = live(&mut slot, CaptureTool::Search, json!({ "path": "leave" }));
    assert_eq!(v["total"], 0, "held back never shows in a search: {v}");
    assert_eq!(v["note"], NOTHING_CAPTURED_YET);
    let (status, out) = live(&mut slot, CaptureTool::Request, json!({ "id": 1 }));
    assert_eq!((status, out), (404, Value::String(NOTHING_CAPTURED_YET.to_string())));
    let (_, v) = live(&mut slot, CaptureTool::GetFilter, json!({}));
    assert_eq!(v["site_confirmed"], false);
    assert_eq!(v["note"], NOTHING_CAPTURED_YET);
    let dir = TempDir::new();
    let (status, out) = call(&mut slot, &mut BTreeMap::new(), Some(dir.path()), CaptureTool::Export, json!({ "name": "early" }));
    assert_eq!((status, out.as_str()), (404, NOTHING_CAPTURED_YET));
}

// ---------------------------------------------------------------- search

#[test]
fn search_matches_every_filter_given() {
    let mut slot = browser_with(signed_in_capture());
    let ids = |v: &Value| -> Vec<u64> { v["requests"].as_array().unwrap().iter().map(|r| r["id"].as_u64().unwrap()).collect() };
    let (_, v) = live(&mut slot, CaptureTool::Search, json!({ "path": "LEAVE" }));
    assert_eq!(ids(&v), vec![2]);
    let (_, v) = live(&mut slot, CaptureTool::Search, json!({ "method": "post" }));
    assert_eq!(ids(&v), vec![1]);
    let (_, v) = live(&mut slot, CaptureTool::Search, json!({ "type": "document" }));
    assert_eq!(ids(&v), vec![3]);
    let (_, v) = live(&mut slot, CaptureTool::Search, json!({ "status": "2xx" }));
    assert_eq!(v["total"], 3);
    let (_, v) = live(&mut slot, CaptureTool::Search, json!({ "status": 404 }));
    assert_eq!(v["total"], 0);
    assert!(v.get("note").is_none(), "a search that matched nothing is not an empty capture: {v}");
    let (_, v) = live(&mut slot, CaptureTool::Search, json!({ "response_contains": "casual" }));
    assert_eq!(ids(&v), vec![2]);
    let (_, v) = live(&mut slot, CaptureTool::Search, json!({ "request_contains": "user=avin", "method": "POST" }));
    assert_eq!(ids(&v), vec![1]);
    let (status, _) = live(&mut slot, CaptureTool::Search, json!({ "status": "abc" }));
    assert_eq!(status, 400);
    // A type the capture never keeps is refused with the ones it does, not
    // answered with nothing.
    let mut slot = browser_with(signed_in_capture());
    let (status, out) = call(&mut slot, &mut BTreeMap::new(), None, CaptureTool::Search, json!({ "type": "xhrr" }));
    assert_eq!((status, out.as_str()), (400, "type: the capture keeps only these types: XHR, Fetch, Document"));
}

/// A search runs over the redacted text: asking for a secret's value can
/// never confirm it.
#[test]
fn searching_for_a_secret_never_finds_it() {
    let mut slot = browser_with(signed_in_capture());
    for args in [
        json!({ "request_contains": PASSWORD }),
        json!({ "request_contains": "Hunter2" }),
        json!({ "response_contains": TOKEN }),
        json!({ "response_contains": "Zk3qT9vLm2Xw8pRt" }),
    ] {
        let (_, v) = live(&mut slot, CaptureTool::Search, args.clone());
        assert_eq!(v["total"], 0, "{args} found {v}");
    }
}

// ------------------------------------------------- request and response

#[test]
fn a_request_shows_allow_listed_headers_and_a_redacted_body() {
    let mut slot = browser_with(signed_in_capture());
    let (status, v) = live(&mut slot, CaptureTool::Request, json!({ "id": 1 }));
    assert_eq!(status, 200, "{v}");
    let text = v.to_string();
    assert_no_secret(&text);
    let names: Vec<&str> = v["headers"].as_array().unwrap().iter().map(|h| h["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"content-type"), "{names:?}");
    assert!(names.contains(&"x-request-id"), "{names:?}");
    assert!(!names.iter().any(|n| *n == "cookie" || *n == "authorization"), "{names:?}");
    let body = v["body"]["text"].as_str().unwrap();
    assert!(body.contains("user=avin"), "{body}");
    assert!(body.contains("password="), "{body}");
    assert_eq!(v["tab"], "main");
    let (status, _) = live(&mut slot, CaptureTool::Request, json!({}));
    assert_eq!(status, 400, "an id is needed");
    let (status, out) = live(&mut slot, CaptureTool::Request, json!({ "id": 99 }));
    assert_eq!(status, 404);
    assert!(out.as_str().unwrap().contains("No request 99"), "{out}");
}

#[test]
fn a_request_with_no_body_says_so() {
    let mut slot = browser_with(signed_in_capture());
    let (_, v) = live(&mut slot, CaptureTool::Request, json!({ "id": 2 }));
    assert_eq!(v["body"]["kept"], false);
    assert!(v["body"]["why"].as_str().unwrap().contains("None sent"), "{v}");
}

#[test]
fn a_response_shows_status_headers_timing_and_whether_its_body_is_kept() {
    let mut slot = browser_with(signed_in_capture());
    let (status, v) = live(&mut slot, CaptureTool::Response, json!({ "id": 1 }));
    assert_eq!(status, 200, "{v}");
    assert_no_secret(&v.to_string());
    assert_eq!(v["status"], 200);
    assert_eq!(v["mime"], "application/json");
    assert_eq!(v["size"].as_u64(), v["body"]["chars"].as_u64(), "the redacted body's length");
    assert_eq!(v["duration_ms"], 250);
    assert_eq!(v["body"]["kept"], true);
    assert!(v["body"]["chars"].as_u64().unwrap() > 0);
    let names: Vec<&str> = v["headers"].as_array().unwrap().iter().map(|h| h["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"x-trace"), "{names:?}");
    assert!(!names.contains(&"set-cookie"), "{names:?}");
}

#[test]
fn a_response_body_is_redacted() {
    let mut slot = browser_with(signed_in_capture());
    let (status, v) = live(&mut slot, CaptureTool::ResponseBody, json!({ "id": 1 }));
    assert_eq!(status, 200, "{v}");
    let text = v["text"].as_str().unwrap();
    assert_no_secret(text);
    assert!(text.contains("\"name\":\"Avin\""), "{text}");
    assert!(text.contains("/hr/home"), "addresses inside a body are kept: {text}");
}

#[test]
fn a_long_response_body_comes_in_pages_of_20000_characters() {
    let mut f = Feeder::confirmed();
    let long = "leave day ".repeat(4_500);
    assert_eq!(long.chars().count(), 45_000);
    f.request("big", "XHR", "GET", "/api/report", json!({}), None, 200, "text/plain", json!({}), Some(&long));
    let mut slot = browser_with(f.cap);
    let (_, v) = live(&mut slot, CaptureTool::ResponseBody, json!({ "id": 1 }));
    assert_eq!(v["chars"], 45_000);
    assert_eq!(v["text"].as_str().unwrap().chars().count(), BODY_PAGE_CHARS);
    assert_eq!(v["next_offset"], 20_000);
    let (_, v) = live(&mut slot, CaptureTool::ResponseBody, json!({ "id": 1, "offset": 40_000 }));
    assert_eq!(v["text"].as_str().unwrap().chars().count(), 5_000);
    assert_eq!(v["next_offset"], Value::Null);
    let (status, _) = live(&mut slot, CaptureTool::ResponseBody, json!({ "id": 1, "offset": 50_000 }));
    assert_eq!(status, 400);
}

#[test]
fn a_body_that_was_not_kept_says_why() {
    let mut f = Feeder::confirmed();
    f.request("img", "XHR", "GET", "/api/photo", json!({}), None, 200, "image/png", json!({}), None);
    f.request("gone", "XHR", "GET", "/api/gone", json!({}), None, 200, "application/json", json!({}), None);
    let mut slot = browser_with(f.cap);
    let (status, out) = live(&mut slot, CaptureTool::ResponseBody, json!({ "id": 1 }));
    assert_eq!(status, 404);
    assert!(out.as_str().unwrap().contains("Not text"), "{out}");
    let (status, out) = live(&mut slot, CaptureTool::ResponseBody, json!({ "id": 2 }));
    assert_eq!(status, 404);
    assert!(out.as_str().unwrap().contains("gone"), "{out}");
}

// ---------------------------------------------------------------- filter

#[test]
fn the_filter_is_set_whole_and_read_back() {
    let mut slot = browser_with(signed_in_capture());
    let (status, v) = live(&mut slot, CaptureTool::SetFilter, json!({ "include_types": ["xhr", "fetch"], "path_contains": "/api/" }));
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["include_types"], json!(["XHR", "Fetch"]));
    assert_eq!(v["path_contains"], "/api/");
    assert!(v["said"].as_str().unwrap().contains("from now on"));
    let (_, v) = live(&mut slot, CaptureTool::GetFilter, json!({}));
    assert_eq!(v["include_types"], json!(["XHR", "Fetch"]));
    assert_eq!(v["exclude_types"], json!([]));
    assert_eq!(v["kept"], 3, "what was kept stays");
    assert_eq!(v["site_confirmed"], true);
    let (status, out) = live(&mut slot, CaptureTool::SetFilter, json!({ "include_types": ["Image"] }));
    assert_eq!(status, 400);
    assert!(out.as_str().unwrap().contains("XHR, Fetch, Document"), "{out}");
    let (status, _) = live(&mut slot, CaptureTool::SetFilter, json!({ "path_contains": "x".repeat(201) }));
    assert_eq!(status, 400);
    // Everything left out: every request is kept again.
    let (_, v) = live(&mut slot, CaptureTool::SetFilter, json!({}));
    assert_eq!((v["include_types"].clone(), v["path_contains"].clone()), (json!([]), Value::Null));
}

#[test]
fn the_filter_decides_what_is_kept_from_then_on() {
    let mut f = Feeder::confirmed();
    let mut slot = {
        f.get("a", "/api/one", "{}");
        let cap = std::mem::take(&mut f.cap);
        browser_with(cap)
    };
    let (status, _) = live(&mut slot, CaptureTool::SetFilter, json!({ "exclude_types": ["XHR"] }));
    assert_eq!(status, 200);
    // Feed the store the browser holds now.
    let cap = slot.as_mut().unwrap().d.capture.take().unwrap();
    f.cap = cap;
    f.get("b", "/api/two", "{}");
    f.request("c", "Document", "GET", "/hr/next", json!({}), None, 200, "text/html", json!({}), Some("<p>x</p>"));
    slot.as_mut().unwrap().d.capture = Some(std::mem::take(&mut f.cap));
    let (_, v) = live(&mut slot, CaptureTool::List, json!({}));
    let paths: Vec<&str> = v["requests"].as_array().unwrap().iter().map(|r| r["path"].as_str().unwrap()).collect();
    assert_eq!(paths, vec!["/hr/next", "/api/one"]);
}

// ---------------------------------------------------------------- export

#[test]
fn the_export_is_written_to_the_captures_folder_and_holds_no_secret() {
    let dir = TempDir::new();
    let mut slot = browser_with(signed_in_capture());
    let mut imported = BTreeMap::new();
    let (status, out) = call(&mut slot, &mut imported, Some(dir.path()), CaptureTool::Export, json!({ "name": "leave flow" }));
    assert_eq!(status, 200, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["file"], "leave flow.har");
    assert_eq!(v["requests"], 3);
    assert_eq!(v["replaced"], false);
    let file = dir.path().join("captures").join("leave flow.har");
    let text = std::fs::read_to_string(&file).unwrap();
    assert_no_secret(&text);
    let har: Value = serde_json::from_str(&text).unwrap();
    let entries = har["log"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["request"]["url"], format!("{SITE}/api/login?returnUrl=%2Fhr%2Fhome"), "oldest first, on the confirmed site");
    assert!(!out.contains(&dir.path().display().to_string()), "the answer names the file, not the folder's path: {out}");
    // A second export of the name replaces it.
    let (status, out) = call(&mut slot, &mut imported, Some(dir.path()), CaptureTool::Export, json!({ "name": "leave flow.har" }));
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["replaced"], true);
}

#[test]
fn an_export_name_is_a_plain_file_stem() {
    let long = "n".repeat(81);
    let longest = "n".repeat(80);
    for bad in ["", "..", "../x", "a/b", "a\\b", "C:x", "..\\secret", "x..y", ".hidden", "con", "COM1", "a?b", long.as_str()] {
        assert!(file_stem(bad).is_err(), "{bad:?} was taken");
    }
    for good in ["leave flow", "run-2026.10.19", "a_b", longest.as_str()] {
        assert_eq!(file_stem(good).as_deref(), Ok(good), "{good:?}");
    }
    assert_eq!(file_stem("cycle.HAR").as_deref(), Ok("cycle"));

    let dir = TempDir::new();
    let mut slot = browser_with(signed_in_capture());
    for bad in ["../out", "a/b", "..\\out", long.as_str()] {
        let (status, _) = call(&mut slot, &mut BTreeMap::new(), Some(dir.path()), CaptureTool::Export, json!({ "name": bad }));
        assert_eq!(status, 400, "{bad}");
    }
    let (status, _) = call(&mut slot, &mut BTreeMap::new(), Some(dir.path()), CaptureTool::Export, json!({}));
    assert_eq!(status, 400, "a name is needed");
    assert!(!dir.path().join("out.har").exists());
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing written for a refused name");
}

// ---------------------------------------------------------------- import

/// A HAR saved from another tool: cookies, authorization, a password and a
/// token, all of which must stay out.
fn foreign_har() -> Value {
    json!({ "log": { "version": "1.2", "creator": { "name": "Edge", "version": "1" }, "entries": [
        {
            "startedDateTime": "2026-10-19T08:00:00.000Z",
            "time": 120,
            "request": {
                "method": "POST",
                "url": "https://hr.example/api/login?session=s3cr3tSessionValue",
                "headers": [
                    { "name": "Cookie", "value": "ASP.NET_SessionId=s3cr3tSessionValue" },
                    { "name": "Authorization", "value": format!("Bearer {TOKEN}") },
                    { "name": "Content-Type", "value": "application/x-www-form-urlencoded" },
                ],
                "postData": { "mimeType": "application/x-www-form-urlencoded", "text": format!("user=avin&password={PASSWORD}") },
            },
            "response": {
                "status": 200,
                "headers": [{ "name": "Set-Cookie", "value": "auth=zzz" }, { "name": "Content-Type", "value": "application/json" }],
                "content": { "mimeType": "application/json", "text": format!(r#"{{"token":"{TOKEN}","ok":true}}"#) },
                "bodySize": 40,
            },
        },
        {
            "startedDateTime": "2026-10-19T08:00:01.000Z",
            "time": 30,
            "request": { "method": "GET", "url": "https://hr.example/api/leave/types", "headers": [] },
            "response": { "status": 200, "headers": [], "content": { "mimeType": "application/json", "text": "{\"types\":[\"Annual\"]}" } },
        },
    ] } })
}

fn put_har(root: &Path, name: &str, har: &Value) {
    let dir = root.join("captures");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.har")), har.to_string()).unwrap();
}

#[test]
fn an_import_is_read_with_from_and_needs_no_browser() {
    let dir = TempDir::new();
    put_har(dir.path(), "from edge", &foreign_har());
    let mut imported = BTreeMap::new();
    let (status, out) = call(&mut None, &mut imported, Some(dir.path()), CaptureTool::Import, json!({ "name": "from edge" }));
    assert_eq!(status, 200, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!((v["set"].as_str(), v["requests"].as_u64()), (Some("from edge"), Some(2)));
    assert!(imported.contains_key("from edge"));

    let from = |tool, mut args: Value, imported: &mut ImportedSets| {
        args["from"] = json!("from edge");
        let (status, out) = call(&mut None, imported, None, tool, args);
        assert_eq!(status, 200, "{tool:?}: {out}");
        assert_no_secret(&out);
        serde_json::from_str::<Value>(&out).unwrap()
    };
    let list = from(CaptureTool::List, json!({}), &mut imported);
    assert_eq!(list["set"], "from edge");
    assert_eq!(list["requests"][0]["path"], "/api/leave/types", "newest first");
    let found = from(CaptureTool::Search, json!({ "method": "POST" }), &mut imported);
    assert_eq!(found["total"], 1);
    let id = found["requests"][0]["id"].clone();
    let req = from(CaptureTool::Request, json!({ "id": id.clone() }), &mut imported);
    let names: Vec<&str> = req["headers"].as_array().unwrap().iter().map(|h| h["name"].as_str().unwrap()).collect();
    assert!(!names.iter().any(|n| *n == "cookie" || *n == "authorization"), "{names:?}");
    from(CaptureTool::Response, json!({ "id": id.clone() }), &mut imported);
    let body = from(CaptureTool::ResponseBody, json!({ "id": id }), &mut imported);
    assert!(body["text"].as_str().unwrap().contains("\"ok\":true"), "{body}");
}

#[test]
fn an_unknown_set_is_named_as_not_loaded() {
    let (status, out) = call(&mut None, &mut BTreeMap::new(), None, CaptureTool::List, json!({ "from": "nope" }));
    assert_eq!(status, 400);
    assert!(out.contains("import_autorun_requests"), "{out}");
    let (status, _) = call(&mut None, &mut BTreeMap::new(), None, CaptureTool::List, json!({ "from": "../x" }));
    assert_eq!(status, 400);
}

#[test]
fn an_import_reads_only_from_the_captures_folder() {
    let dir = TempDir::new();
    // A HAR beside the folder, and one a level up: neither is reachable.
    std::fs::write(dir.path().join("outside.har"), foreign_har().to_string()).unwrap();
    std::fs::create_dir_all(dir.path().join("captures")).unwrap();
    let mut imported = BTreeMap::new();
    for bad in ["../outside", "..\\outside", "captures/../../outside", "C:\\outside", "/outside", ".."] {
        let (status, out) = call(&mut None, &mut imported, Some(dir.path()), CaptureTool::Import, json!({ "name": bad }));
        assert_eq!(status, 400, "{bad}: {out}");
    }
    let (status, out) = call(&mut None, &mut imported, Some(dir.path()), CaptureTool::Import, json!({ "name": "outside" }));
    assert_eq!(status, 404, "{out}");
    assert!(imported.is_empty());
}

#[test]
fn an_import_that_is_not_a_har_is_refused() {
    let dir = TempDir::new();
    let folder = dir.path().join("captures");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("junk.har"), "not json").unwrap();
    std::fs::write(folder.join("empty.har"), "{}").unwrap();
    let mut imported = BTreeMap::new();
    let (status, out) = call(&mut None, &mut imported, Some(dir.path()), CaptureTool::Import, json!({ "name": "junk" }));
    assert_eq!((status, out.as_str()), (400, "not a HAR file: not JSON"));
    let (status, _) = call(&mut None, &mut imported, Some(dir.path()), CaptureTool::Import, json!({ "name": "empty" }));
    assert_eq!(status, 400);
    let (status, _) = call(&mut None, &mut imported, None, CaptureTool::Import, json!({ "name": "junk" }));
    assert_eq!(status, 503, "no Auto Run folder");
}

/// An export read back by an import is the same requests, still redacted.
#[test]
fn an_export_reads_back_as_an_import() {
    let dir = TempDir::new();
    let mut slot = browser_with(signed_in_capture());
    let mut imported = BTreeMap::new();
    let (status, _) = call(&mut slot, &mut imported, Some(dir.path()), CaptureTool::Export, json!({ "name": "round" }));
    assert_eq!(status, 200);
    let (status, _) = call(&mut None, &mut imported, Some(dir.path()), CaptureTool::Import, json!({ "name": "round" }));
    assert_eq!(status, 200);
    let (_, out) = call(&mut None, &mut imported, None, CaptureTool::List, json!({ "from": "round" }));
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["total"], 3);
    assert_no_secret(&out);
    let (_, filter) = live(&mut slot, CaptureTool::GetFilter, json!({}));
    assert_eq!(filter["imported_sets"], json!([]), "a call's own map is what it reads");
    let (_, out) = call(&mut slot, &mut imported, None, CaptureTool::GetFilter, json!({}));
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["imported_sets"], json!(["round"]));
}

// ------------------------------------------------------- review round 1

/// A token-shaped path segment a sign-in redirects to: 40 characters or
/// more, the length a path segment is judged a token at (`capture_redact`).
const RESET_TOKEN: &str = "Ab3dEf9GhIjKlMnOpQrStUvWx7Yz0aB1cD2eF3gH4iJ5kL6m";

/// The sign-in posts its form and is redirected to a reset page whose path
/// carries a token.
fn redirected_capture() -> NetCapture {
    let mut f = Feeder::confirmed();
    f.wall += 1.0;
    let asked = |path: &str, method: &str, wall: f64| {
        json!({ "requestId": "rd", "type": "Document", "documentURL": PAGE, "timestamp": 100.0, "wallTime": wall,
                "request": { "url": format!("{SITE}{path}"), "method": method, "headers": {} } })
    };
    let first = asked("/account/login", "POST", f.wall);
    f.feed("Network.requestWillBeSent", first);
    let mut hop = asked(&format!("/account/reset/{RESET_TOKEN}"), "GET", f.wall);
    hop["redirectResponse"] = json!({ "url": format!("{SITE}/account/login"), "status": 302, "mimeType": "text/html", "headers": {} });
    f.feed("Network.requestWillBeSent", hop);
    f.feed("Network.responseReceived", json!({ "requestId": "rd", "type": "Document", "response": { "status": 200, "mimeType": "text/html", "headers": {} } }));
    f.feed("Network.loadingFinished", json!({ "requestId": "rd", "timestamp": 100.25, "encodedDataLength": 10 }));
    while let Some(read) = f.cap.next_read() {
        f.cap.answered(&read, Some(&json!({ "body": "<p>reset</p>", "base64Encoded": false })));
    }
    f.cap
}

#[test]
fn an_export_never_writes_a_redirects_token() {
    let cap = redirected_capture();
    let raw = cap.list(0, 1).0[0].redirect.clone().expect("the redirect was kept");
    assert!(raw.to_path.contains(RESET_TOKEN), "the store keeps the raw path: {raw:?}");
    let dir = TempDir::new();
    let mut slot = browser_with(cap);
    let (status, out) = call(&mut slot, &mut BTreeMap::new(), Some(dir.path()), CaptureTool::Export, json!({ "name": "reset" }));
    assert_eq!(status, 200, "{out}");
    let text = std::fs::read_to_string(dir.path().join("captures").join("reset.har")).unwrap();
    assert!(!text.contains(RESET_TOKEN), "{text}");
    let har: Value = serde_json::from_str(&text).unwrap();
    let to = har["log"]["entries"][0]["response"]["redirectURL"].as_str().unwrap();
    assert!(to.starts_with("/account/reset/"), "{to}");
    let (_, v) = live(&mut slot, CaptureTool::Response, json!({ "id": 1 }));
    assert!(!v.to_string().contains(RESET_TOKEN), "{v}");
}

/// A size is the redacted text's, never the raw length that would give a
/// password's length away.
#[test]
fn sizes_are_the_redacted_bodies_lengths() {
    let mut slot = browser_with(signed_in_capture());
    let raw_request = format!("user=avin&password={PASSWORD}").len();
    let (_, req) = live(&mut slot, CaptureTool::Request, json!({ "id": 1 }));
    let shown_request = req["body"]["text"].as_str().unwrap().len();
    assert_ne!(shown_request, raw_request);
    let (_, list) = live(&mut slot, CaptureTool::List, json!({}));
    let line = &list["requests"][2];
    assert_eq!(line["request_size"].as_u64(), Some(shown_request as u64), "{line}");
    let (_, body) = live(&mut slot, CaptureTool::ResponseBody, json!({ "id": 1 }));
    let shown_response = body["text"].as_str().unwrap().len() as u64;
    assert_eq!(line["response_size"].as_u64(), Some(shown_response), "{line}");
    let (_, resp) = live(&mut slot, CaptureTool::Response, json!({ "id": 1 }));
    assert_eq!(resp["size"].as_u64(), Some(shown_response));

    let dir = TempDir::new();
    let (status, _) = call(&mut slot, &mut BTreeMap::new(), Some(dir.path()), CaptureTool::Export, json!({ "name": "sizes" }));
    assert_eq!(status, 200);
    let har: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("captures").join("sizes.har")).unwrap()).unwrap();
    let e = &har["log"]["entries"][0];
    let post = e["request"]["postData"]["text"].as_str().unwrap();
    assert_eq!(e["request"]["bodySize"].as_u64(), Some(post.len() as u64));
    let content = e["response"]["content"]["text"].as_str().unwrap();
    assert_eq!(e["response"]["content"]["size"].as_u64(), Some(content.len() as u64));
    assert_eq!(e["response"]["bodySize"].as_u64(), Some(content.len() as u64));
}

#[test]
fn at_most_five_sets_are_imported() {
    let dir = TempDir::new();
    let mut imported = BTreeMap::new();
    for i in 1..=6 {
        put_har(dir.path(), &format!("set{i}"), &foreign_har());
    }
    for i in 1..=5 {
        let (status, out) = call(&mut None, &mut imported, Some(dir.path()), CaptureTool::Import, json!({ "name": format!("set{i}") }));
        assert_eq!(status, 200, "{out}");
    }
    let (status, out) = call(&mut None, &mut imported, Some(dir.path()), CaptureTool::Import, json!({ "name": "set6" }));
    assert_eq!((status, out.as_str()), (409, IMPORTS_FULL));
    assert_eq!(IMPORTS_FULL, "Five captures are loaded; a sign-in clears them, or restart TCM.");
    assert_eq!(imported.len(), 5);
    // A set already loaded may be loaded again: it replaces itself.
    let (status, out) = call(&mut None, &mut imported, Some(dir.path()), CaptureTool::Import, json!({ "name": "set3" }));
    assert_eq!(status, 200, "{out}");
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["replaced"], true);
}

/// The bridge's own imported sets live in the Rust cache's memory-only
/// tier: an import keeps a set there, a read with no browser finds it, a
/// refused change keeps nothing, the five-set cap holds, and a sign-in or
/// out forgets them all.
#[test]
fn the_bridges_imported_sets_live_in_the_rust_cache() {
    use v2_lib::autorun::capture_tools::{read_import, store_import, MAX_IMPORTED_SETS};
    use v2_lib::commands::autorun::{change_imported_captures, forget_imported_captures, imported_captures};
    // The bridge's capture routes read the same sets.
    let _a = crate::serial::autorun();
    forget_imported_captures();
    let dir = TempDir::new();
    for i in 1..=MAX_IMPORTED_SETS + 1 {
        put_har(dir.path(), &format!("set{i}"), &foreign_har());
    }
    let import = |name: &str| {
        let (stem, entries) = read_import(dir.path(), &json!({ "name": name })).unwrap();
        change_imported_captures(|sets| store_import(sets, stem, entries))
    };
    let (status, out) = import("set1");
    assert_eq!(status, 200, "{out}");
    assert_eq!(imported_captures().keys().collect::<Vec<_>>(), ["set1"]);
    let mut sets = imported_captures();
    let (status, out) = call(&mut None, &mut sets, None, CaptureTool::List, json!({ "from": "set1" }));
    assert_eq!(status, 200, "read with no browser open: {out}");
    assert_no_secret(&out);

    let (status, _) = change_imported_captures(|sets| {
        sets.clear();
        (409, IMPORTS_FULL.to_string())
    });
    assert_eq!(status, 409);
    assert_eq!(imported_captures().len(), 1, "a refused change keeps nothing");

    for i in 2..=MAX_IMPORTED_SETS {
        let (status, out) = import(&format!("set{i}"));
        assert_eq!(status, 200, "{out}");
    }
    let (status, out) = import(&format!("set{}", MAX_IMPORTED_SETS + 1));
    assert_eq!((status, out.as_str()), (409, IMPORTS_FULL));
    assert_eq!(imported_captures().len(), MAX_IMPORTED_SETS);

    forget_imported_captures();
    assert!(imported_captures().is_empty(), "a sign-in or out forgets them");
}

/// Eight imports at once, each of its own set: the cap still holds -
/// exactly five are kept and three refused - because the imports are
/// taken one at a time (`change_imported_captures`).
#[test]
fn imports_at_once_never_pass_the_five_set_cap() {
    use v2_lib::autorun::capture_tools::{read_import, store_import, MAX_IMPORTED_SETS};
    use v2_lib::commands::autorun::{change_imported_captures, forget_imported_captures, imported_captures};
    // The bridge's capture routes read the same sets.
    let _a = crate::serial::autorun();
    forget_imported_captures();
    let dir = TempDir::new();
    let parsed: Vec<_> = (1..=8)
        .map(|i| {
            let name = format!("race{i}");
            put_har(dir.path(), &name, &foreign_har());
            read_import(dir.path(), &json!({ "name": name })).unwrap()
        })
        .collect();
    let start = std::sync::Arc::new(std::sync::Barrier::new(parsed.len()));
    let threads: Vec<_> = parsed
        .into_iter()
        .map(|(stem, entries)| {
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                change_imported_captures(|sets| store_import(sets, stem, entries))
            })
        })
        .collect();
    let answers: Vec<(u16, String)> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let kept = answers.iter().filter(|(status, _)| *status == 200).count();
    let refused: Vec<_> = answers.iter().filter(|(status, _)| *status != 200).collect();
    assert_eq!(kept, MAX_IMPORTED_SETS, "{answers:?}");
    assert_eq!(refused.len(), 3, "{answers:?}");
    assert!(refused.iter().all(|(status, said)| *status == 409 && said == IMPORTS_FULL), "{refused:?}");
    assert_eq!(imported_captures().len(), MAX_IMPORTED_SETS);
    forget_imported_captures();
}

#[test]
fn live_is_never_an_imported_or_exported_name() {
    let dir = TempDir::new();
    put_har(dir.path(), "live", &foreign_har());
    let mut imported = BTreeMap::new();
    for name in ["live", "LIVE", "live.har"] {
        let (status, out) = call(&mut None, &mut imported, Some(dir.path()), CaptureTool::Import, json!({ "name": name }));
        assert_eq!((status, out.as_str()), (400, LIVE_IS_RESERVED), "{name}");
    }
    assert!(imported.is_empty());
    let mut slot = browser_with(signed_in_capture());
    let (status, out) = call(&mut slot, &mut imported, Some(dir.path()), CaptureTool::Export, json!({ "name": "live" }));
    assert_eq!((status, out.as_str()), (400, LIVE_IS_RESERVED));
}

/// A read takes what it needs out of its set and is answered after: the
/// redaction and matching need neither the browser nor the imported sets.
#[test]
fn a_read_is_answered_after_its_set_is_let_go() {
    let mut slot = browser_with(signed_in_capture());
    let mut imported = BTreeMap::new();
    for (tool, args) in [
        (CaptureTool::Search, json!({ "response_contains": "casual" })),
        (CaptureTool::List, json!({ "limit": 2 })),
        (CaptureTool::ResponseBody, json!({ "id": 1 })),
    ] {
        let phase = capture_tool_begin(&mut slot, &mut imported, None, tool, &args.to_string());
        assert!(matches!(phase, CapturePhase::Read(..)), "{tool:?} answered under the lock");
        let (status, out) = phase.finish();
        assert_eq!(status, 200, "{tool:?}: {out}");
        assert_no_secret(&out);
    }
    let phase = capture_tool_begin(&mut slot, &mut imported, None, CaptureTool::Search, r#"{"response_contains":"casual"}"#);
    slot = None;
    let (_, out) = phase.finish();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["total"], 1, "{v}");
    assert!(slot.is_none());
    // A list's page keeps the whole set's count.
    let mut slot = browser_with(signed_in_capture());
    let (_, v) = live(&mut slot, CaptureTool::List, json!({ "limit": 2 }));
    assert_eq!((v["total"].as_u64(), v["returned"].as_u64(), v["next_offset"].as_u64()), (Some(3), Some(2), Some(2)));
    // An export takes its requests out the same way.
    let dir = TempDir::new();
    let phase = capture_tool_begin(&mut slot, &mut imported, Some(dir.path()), CaptureTool::Export, r#"{"name":"later"}"#);
    assert!(matches!(phase, CapturePhase::Export(..)));
    assert!(!dir.path().join("captures").join("later.har").exists(), "nothing written under the lock");
    assert_eq!(phase.finish().0, 200);
    assert!(dir.path().join("captures").join("later.har").exists());
}
