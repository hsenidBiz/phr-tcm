//! HAR 1.2 for the capture store: a redacted export that reads back, and an
//! import that validates the shape and redacts whatever it takes in.

use serde_json::{json, Value};
use v2_lib::autorun::har::{from_har, to_har};
use v2_lib::browser::net_capture::{Body, Captured, Redirect, MAX_CAPTURED};

const PASSWORD: &str = "Hunter2-Pa55w0rd!";

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
}

fn search(id: u64) -> Captured {
    Captured {
        id,
        request_id: format!("R{id}"),
        tab: "main".into(),
        method: "POST".into(),
        url_path_query: "/api/employees/search?page=2&size=50".into(),
        kind: Some("XHR".into()),
        status: Some(200),
        mime: Some("application/json".into()),
        req_headers: pairs(&[("content-type", "application/json"), ("x-requested-with", "XMLHttpRequest")]),
        resp_headers: pairs(&[("content-type", "application/json; charset=utf-8")]),
        req_body: Some(Body::Text(r#"{"name":"avin","dept":["hr","it"]}"#.into())),
        resp_body: Some(Body::Text(r#"{"rows":[{"id":1},{"id":2}],"next":"https://hr.example/api/x?page=3"}"#.into())),
        resp_size: Some(812),
        started_ms: 1_760_000_000_123,
        duration_ms: Some(48),
        failed: None,
        redirect: None,
    }
}

fn redirected(id: u64) -> Captured {
    Captured {
        id,
        request_id: format!("R{id}"),
        method: "GET".into(),
        url_path_query: "/hr/old".into(),
        kind: Some("Document".into()),
        status: Some(200),
        mime: Some("text/html".into()),
        resp_body: Some(Body::TooBig),
        req_body: None,
        started_ms: 1_760_000_001_000,
        duration_ms: None,
        redirect: Some(Redirect { status: 302, to_path: "/hr/new".into(), other_site: false }),
        ..Default::default()
    }
}

fn failed(id: u64) -> Captured {
    Captured {
        id,
        request_id: format!("R{id}"),
        method: "GET".into(),
        url_path_query: "/api/slow".into(),
        kind: Some("Fetch".into()),
        started_ms: 1_760_000_002_000,
        failed: Some("net::ERR_CONNECTION_RESET".into()),
        resp_body: Some(Body::Dropped),
        req_body: Some(Body::NotText),
        ..Default::default()
    }
}

#[test]
fn a_har_round_trips() {
    let all = [search(1), redirected(2), failed(3)];
    let refs: Vec<&Captured> = all.iter().collect();
    let har = to_har(&refs);

    assert_eq!(har["log"]["version"], "1.2");
    assert_eq!(har["log"]["creator"]["name"], "TCM");
    let first = &har["log"]["entries"][0];
    assert_eq!(first["startedDateTime"], "2025-10-09T08:53:20.123Z");
    assert_eq!(first["time"], 48);
    assert_eq!(first["request"]["method"], "POST");
    assert_eq!(first["request"]["url"], "/api/employees/search?page=2&size=50");
    assert_eq!(first["request"]["queryString"], json!([{ "name": "page", "value": "2" }, { "name": "size", "value": "50" }]));
    assert_eq!(first["request"]["headers"][0], json!({ "name": "content-type", "value": "application/json" }));
    assert_eq!(first["request"]["postData"]["mimeType"], "application/json");
    assert_eq!(first["request"]["postData"]["text"], r#"{"name":"avin","dept":["hr","it"]}"#);
    assert_eq!(first["response"]["status"], 200);
    assert_eq!(first["response"]["content"]["mimeType"], "application/json");
    assert!(first["response"]["content"]["text"].as_str().unwrap().contains("https://hr.example/api/x?page=3"));

    // Through text and back, as a file would.
    let reread: Value = serde_json::from_str(&har.to_string()).unwrap();
    let back = from_har(&reread).expect("our own HAR reads back");
    assert_eq!(back, all.to_vec());
}

#[test]
fn an_export_is_redacted_like_a_read() {
    let mut c = search(1);
    c.url_path_query = format!("/signin?user=avin&password={PASSWORD}");
    c.req_headers = pairs(&[("content-type", "application/x-www-form-urlencoded"), ("x-csrf-token", "(hidden)")]);
    c.req_body = Some(Body::Text(format!("Input.Username=avin&Input.Password={PASSWORD}")));
    c.resp_body = Some(Body::Text(json!({ "id": "x", "access_token": PASSWORD, "note": "ok" }).to_string()));
    let har = to_har(&[&c]).to_string();
    assert!(!har.contains(PASSWORD), "{har}");
    assert!(har.contains("avin") && har.contains("ok"));
    assert!(har.contains("[redacted]"));
}

#[test]
fn a_har_from_elsewhere_with_secrets_is_redacted_on_import() {
    let jwt = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    let foreign = json!({
        "log": {
            "version": "1.2",
            "creator": { "name": "WebInspector", "version": "537.36" },
            "entries": [
                {
                    "startedDateTime": "2026-10-19T09:30:00.000+05:30",
                    "time": 120.6,
                    "_resourceType": "xhr",
                    "request": {
                        "method": "post",
                        "url": format!("https://hr.example:8443/connect/token?client_secret={PASSWORD}&scope=openid#frag"),
                        "headers": [
                            { "name": "Cookie", "value": format!(".ASPXAUTH={PASSWORD}") },
                            { "name": "Authorization", "value": format!("Bearer {jwt}") },
                            { "name": "Content-Type", "value": "application/x-www-form-urlencoded" },
                            { "name": "X-Api-Key", "value": PASSWORD },
                            { "name": "User-Agent", "value": "Mozilla" }
                        ],
                        "cookies": [ { "name": "a", "value": PASSWORD } ],
                        "postData": { "mimeType": "application/x-www-form-urlencoded", "params": [
                            { "name": "grant_type", "value": "password" },
                            { "name": "username", "value": "avin" },
                            { "name": "password", "value": PASSWORD }
                        ] }
                    },
                    "response": {
                        "status": 200,
                        "headers": [
                            { "name": "Set-Cookie", "value": format!("sid={PASSWORD}") },
                            { "name": "Content-Type", "value": "application/json" }
                        ],
                        "content": { "mimeType": "application/json", "size": 90,
                            "text": json!({ "who": jwt, "refresh_token": PASSWORD, "scope": "openid" }).to_string() }
                    }
                },
                {
                    "startedDateTime": "2026-10-19T04:00:01Z",
                    "time": -1,
                    "request": { "method": "GET", "url": "https://hr.example/static/logo.png", "headers": [] },
                    "response": { "status": 200, "headers": [],
                        "content": { "mimeType": "image/png", "size": 4, "encoding": "base64", "text": "iVBORw0=" } }
                }
            ]
        }
    });
    let imported = from_har(&foreign).unwrap();
    assert_eq!(imported.len(), 2);
    let all = format!("{imported:?}");
    assert!(!all.contains(PASSWORD), "{all}");
    assert!(!all.contains("eyJ") && !all.contains("SflKx"), "{all}");

    let c = &imported[0];
    assert_eq!((c.id, c.method.as_str()), (1, "POST"));
    assert_eq!(c.url_path_query, "/connect/token?client_secret=[redacted]&scope=openid");
    assert_eq!(c.kind.as_deref(), Some("XHR"));
    assert_eq!(c.status, Some(200));
    assert_eq!(c.duration_ms, Some(121));
    assert_eq!(c.started_ms, 1_792_382_400_000);
    // Only allow-listed headers: no cookie, no authorization, no User-Agent; a secret x- header is hidden.
    let names: Vec<&str> = c.req_headers.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["content-type", "x-api-key"]);
    assert_eq!(c.req_headers[1].1, "(hidden)");
    assert_eq!(c.resp_headers, pairs(&[("content-type", "application/json")]));
    let Some(Body::Text(req)) = &c.req_body else { panic!("{:?}", c.req_body) };
    assert_eq!(req, "grant_type=password&username=avin&password=[redacted]");
    let Some(Body::Text(resp)) = &c.resp_body else { panic!("{:?}", c.resp_body) };
    assert!(resp.contains("\"scope\":\"openid\"") && resp.contains("[redacted]"), "{resp}");
    // A picture is never text.
    assert_eq!(imported[1].resp_body, Some(Body::NotText));
    assert_eq!(imported[1].id, 2);
}

#[test]
fn an_import_refuses_what_is_not_a_har_in_a_short_sentence() {
    let ok = json!({ "startedDateTime": "2026-10-19T04:00:01Z", "request": { "method": "GET", "url": "/a" }, "response": { "status": 200 } });
    let har = |entries: Value| json!({ "log": { "version": "1.2", "entries": entries } });
    assert!(from_har(&har(json!([ok.clone()]))).is_ok());

    let cases: Vec<(Value, &str)> = vec![
        (json!([1, 2]), "no log"),
        (json!({ "log": {} }), "no log.entries"),
        (json!({ "log": { "entries": "x" } }), "no log.entries"),
        (json!({ "log": { "version": "2.0", "entries": [] } }), "not a HAR 1.x"),
        (har(json!([ok.clone(), { "request": { "method": "GET", "url": "/a" }, "response": { "status": 200 } }])), "entry 2: no valid startedDateTime"),
        (har(json!([{ "startedDateTime": "2026-10-19T04:00:01Z", "request": { "url": "/a" }, "response": { "status": 200 } }])), "entry 1: no request method"),
        (har(json!([{ "startedDateTime": "2026-10-19T04:00:01Z", "request": { "method": "GET" }, "response": { "status": 200 } }])), "entry 1: no request url"),
        (har(json!([{ "startedDateTime": "2026-10-19T04:00:01Z", "request": { "method": "GET", "url": "/a" }, "response": {} }])), "entry 1: no response status"),
    ];
    for (doc, expect) in cases {
        let err = from_har(&doc).expect_err(expect);
        assert!(err.contains(expect), "{err} vs {expect}");
        assert!(err.len() < 80, "{err}");
    }
    // A message never quotes a value from the file.
    let secret = json!({ "log": { "entries": [ { "startedDateTime": PASSWORD, "request": { "method": "GET", "url": "/a" }, "response": { "status": 200 } } ] } });
    assert!(!from_har(&secret).unwrap_err().contains(PASSWORD));
}

#[test]
fn an_import_keeps_the_newest_requests_within_the_cap() {
    let entries: Vec<Value> = (0..MAX_CAPTURED + 5)
        .map(|i| json!({ "startedDateTime": "2026-10-19T04:00:01Z", "request": { "method": "GET", "url": format!("/n/{i}") }, "response": { "status": 200 } }))
        .collect();
    let got = from_har(&json!({ "log": { "version": "1.2", "entries": entries } })).unwrap();
    assert_eq!(got.len(), MAX_CAPTURED);
    assert_eq!(got[0].url_path_query, "/n/5");
    assert_eq!((got[0].id, got.last().unwrap().id), (1, MAX_CAPTURED as u64));
}
