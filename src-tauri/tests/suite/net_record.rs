//! The network record: a bounded list of the page's requests, in the order
//! they started, for a script step that checks what the page asked its
//! server and what it was answered (`expect_response`).

use crate::common::ScriptedDriver;
use serde_json::json;
use std::collections::VecDeque;
use v2_lib::browser::cdp::{Cdp, Driver, Event, Transport};
use v2_lib::browser::net_record::{NetRecord, NetRedirect, NetState, MAX_REQUESTS};

fn ev(method: &str, params: serde_json::Value) -> Event {
    Event { method: method.to_string(), params }
}

fn sent(id: &str, method: &str, url: &str) -> Event {
    ev("Network.requestWillBeSent", json!({ "requestId": id, "request": { "url": url, "method": method } }))
}

fn answered(id: &str, status: u64, mime: &str) -> Event {
    ev("Network.responseReceived", json!({ "requestId": id, "response": { "status": status, "mimeType": mime } }))
}

fn finished(id: &str) -> Event {
    ev("Network.loadingFinished", json!({ "requestId": id }))
}

fn failed(id: &str, why: &str) -> Event {
    ev("Network.loadingFailed", json!({ "requestId": id, "errorText": why, "canceled": false }))
}

#[test]
fn a_request_is_recorded_without_its_host() {
    let mut rec = NetRecord::default();
    rec.observe(&sent("1", "POST", "https://hr.example/hr/pmsv10/Cycle?handler=Save&id=7#top"));
    rec.observe(&sent("2", "GET", "https://hr.example"));
    rec.observe(&sent("3", "GET", "http://hr.example:8080?x=1"));
    let all = rec.since(0);
    assert_eq!(all.len(), 3, "{all:?}");
    assert_eq!(all[0].id, "1");
    assert_eq!(all[0].method, "POST");
    assert_eq!(all[0].path_query, "/hr/pmsv10/Cycle?handler=Save&id=7");
    assert_eq!(all[0].status, None);
    assert_eq!(all[0].mime, None);
    assert_eq!(all[0].state, NetState::Pending);
    assert_eq!(all[1].path_query, "/");
    assert_eq!(all[2].path_query, "/?x=1");
    assert!(all.iter().all(|e| !e.path_query.contains("hr.example")), "{all:?}");
}

#[test]
fn status_and_mime_arrive_with_the_response() {
    let mut rec = NetRecord::default();
    rec.observe(&sent("1", "GET", "https://hr.example/hr/api/menu"));
    rec.observe(&answered("1", 404, "application/json"));
    let e = &rec.since(0)[0];
    assert_eq!(e.status, Some(404));
    assert_eq!(e.mime.as_deref(), Some("application/json"));
    assert_eq!(e.state, NetState::Pending, "answered is not finished: the body may still be arriving");
}

#[test]
fn finished_and_failed_are_recorded() {
    let mut rec = NetRecord::default();
    rec.observe(&sent("1", "GET", "https://hr.example/a"));
    rec.observe(&sent("2", "GET", "https://hr.example/b"));
    rec.observe(&answered("1", 200, "text/html"));
    rec.observe(&finished("1"));
    rec.observe(&failed("2", "net::ERR_CONNECTION_RESET"));
    let all = rec.since(0);
    assert_eq!(all[0].state, NetState::Finished);
    assert_eq!(all[1].state, NetState::Failed("net::ERR_CONNECTION_RESET".to_string()));
    // Events for a request it never saw start are not an error.
    rec.observe(&finished("99"));
    rec.observe(&answered("98", 200, "text/html"));
    assert_eq!(rec.since(0).len(), 2);
}

/// A redirect as Chrome reports it: the same request id sent again, to
/// `to`, carrying what `from` answered.
fn redirected(id: &str, status: u64, from: &str, to: &str, method: &str) -> Event {
    ev(
        "Network.requestWillBeSent",
        json!({ "requestId": id, "type": "XHR", "request": { "url": to, "method": method },
                "redirectResponse": { "url": from, "status": status, "mimeType": "text/html" } }),
    )
}

#[test]
fn a_redirect_keeps_its_place() {
    let mut rec = NetRecord::default();
    let save = "https://hr.example/hr/Cycle/Save?id=7";
    rec.observe(&sent("1", "POST", save));
    rec.observe(&sent("2", "GET", "https://hr.example/hr/api/menu"));
    rec.observe(&redirected("1", 302, save, "https://hr.example/Account/Login?ReturnUrl=%2Fhr%2Fsecret", "GET"));
    let all = rec.since(0);
    assert_eq!(all.len(), 2, "a redirect is the same request, not a new one: {all:?}");
    assert!(all[0].seq < all[1].seq, "it keeps the place it started in");
    assert_eq!(rec.mark(), 2, "a redirect takes no new number");
    // What the page asked is what matching sees: the first method and address.
    let e = &all[0];
    assert_eq!(e.id, "1");
    assert_eq!(e.method, "POST");
    assert_eq!(e.path_query, "/hr/Cycle/Save?id=7");
    assert_eq!(e.status, None, "the new hop has not been answered yet");
    assert_eq!(e.mime, None);
    assert_eq!(e.state, NetState::Pending);
    assert_eq!(
        e.redirect,
        Some(NetRedirect { status: 302, to: "/Account/Login".to_string(), other_site: false }),
        "what it answered, and the path (no host, no query) it was sent to"
    );

    // The entry follows the later hops to the end, and keeps the FIRST
    // redirect - the one the request itself answered.
    rec.observe(&redirected(
        "1",
        301,
        "https://hr.example/Account/Login?ReturnUrl=%2Fhr%2Fsecret",
        "https://hr.example/Account/SignIn",
        "GET",
    ));
    rec.observe(&answered("1", 200, "text/html"));
    rec.observe(&finished("1"));
    let e = &rec.since(0)[0];
    assert_eq!(e.method, "POST");
    assert_eq!(e.path_query, "/hr/Cycle/Save?id=7");
    assert_eq!(e.status, Some(200));
    assert_eq!(e.mime.as_deref(), Some("text/html"));
    assert_eq!(e.state, NetState::Finished);
    assert_eq!(e.redirect.as_ref().map(|r| (r.status, r.to.as_str())), Some((302, "/Account/Login")));
    assert_eq!(rec.since(0)[1].redirect, None, "a request never redirected has none");

    // A hop that then fails is followed too.
    rec.observe(&sent("3", "GET", "https://hr.example/hr/a"));
    rec.observe(&redirected("3", 302, "https://hr.example/hr/a", "https://hr.example/hr/b", "GET"));
    rec.observe(&failed("3", "net::ERR_CONNECTION_RESET"));
    let e = &rec.since(0)[2];
    assert_eq!(e.state, NetState::Failed("net::ERR_CONNECTION_RESET".to_string()));
    assert_eq!(e.path_query, "/hr/a");
}

#[test]
fn a_redirect_to_another_site_is_marked_so_without_its_host() {
    let mut rec = NetRecord::default();
    let me = "https://hr.example/hr/api/me";
    rec.observe(&sent("1", "GET", me));
    rec.observe(&redirected(
        "1",
        302,
        me,
        "https://login.microsoftonline.com/common/oauth2/authorize?client_id=abc&state=xyz",
        "GET",
    ));
    let e = &rec.since(0)[0];
    assert_eq!(
        e.redirect,
        Some(NetRedirect { status: 302, to: "/common/oauth2/authorize".to_string(), other_site: true })
    );
    // The same host on another port or scheme is another site too; the
    // same host in other letters is not.
    rec.observe(&sent("2", "GET", me));
    rec.observe(&redirected("2", 307, me, "http://hr.example/hr/api/me", "GET"));
    rec.observe(&sent("3", "GET", me));
    rec.observe(&redirected("3", 307, me, "https://HR.example/hr/api/v2/me", "GET"));
    let all = rec.since(0);
    assert!(all[1].redirect.as_ref().unwrap().other_site);
    assert!(!all[2].redirect.as_ref().unwrap().other_site);
    assert_eq!(all[2].redirect.as_ref().unwrap().to, "/hr/api/v2/me");
}

#[test]
fn only_the_last_400_are_kept() {
    assert_eq!(MAX_REQUESTS, 400);
    let mut rec = NetRecord::default();
    for i in 0..(MAX_REQUESTS + 50) {
        rec.observe(&sent(&i.to_string(), "GET", &format!("https://hr.example/w/{i}")));
    }
    let all = rec.since(0);
    assert_eq!(all.len(), MAX_REQUESTS);
    assert_eq!(all[0].id, "50", "the oldest go first");
    assert_eq!(all.last().unwrap().id, (MAX_REQUESTS + 49).to_string());
    // A request that has dropped out is no longer updated, and a late
    // event for it does not touch what is kept.
    rec.observe(&finished("3"));
    rec.observe(&finished("60"));
    let all = rec.since(0);
    assert_eq!(all.len(), MAX_REQUESTS);
    assert_eq!(all[10].id, "60");
    assert_eq!(all[10].state, NetState::Finished);
    assert!(all.iter().all(|e| e.id != "3"));
}

#[test]
fn since_returns_only_newer_entries() {
    let mut rec = NetRecord::default();
    assert_eq!(rec.mark(), 0);
    rec.observe(&sent("1", "GET", "https://hr.example/before"));
    let mark = rec.mark();
    assert_eq!(mark, 1);
    assert!(rec.since(mark).is_empty());
    rec.observe(&sent("2", "GET", "https://hr.example/after/one"));
    rec.observe(&sent("3", "GET", "https://hr.example/after/two"));
    // An older request finishing after the mark is still older.
    rec.observe(&finished("1"));
    let newer = rec.since(mark);
    assert_eq!(newer.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["2", "3"]);
    assert_eq!(rec.since(0).len(), 3);
    assert!(rec.since(rec.mark()).is_empty());
}

#[test]
fn non_http_requests_are_ignored() {
    let mut rec = NetRecord::default();
    rec.observe(&sent("1", "GET", "data:image/png;base64,AAAA"));
    rec.observe(&sent("2", "GET", "blob:https://hr.example/1234"));
    rec.observe(&sent("3", "GET", "chrome-extension://abc/x.js"));
    rec.observe(&sent("4", "GET", "wss://hr.example/hub"));
    assert!(rec.since(0).is_empty());
    assert_eq!(rec.mark(), 0);
    // Other events are ignored too, and nothing panics on odd params.
    rec.observe(&ev("Network.dataReceived", json!({ "requestId": "1" })));
    rec.observe(&ev("Page.lifecycleEvent", json!({ "name": "load" })));
    rec.observe(&ev("Network.requestWillBeSent", json!(null)));
    assert!(rec.since(0).is_empty());
}

fn sent_as(id: &str, kind: Option<&str>, url: &str) -> Event {
    let mut params = json!({ "requestId": id, "request": { "url": url, "method": "GET" } });
    if let Some(kind) = kind {
        params["type"] = json!(kind);
    }
    ev("Network.requestWillBeSent", params)
}

/// Only what a page asks its server is kept - not the pictures, scripts,
/// styles and fonts a heavy page loads, which would push a step's own
/// requests out of the 400.
#[test]
fn only_documents_xhr_fetch_and_other_requests_are_kept() {
    let mut rec = NetRecord::default();
    rec.observe(&sent_as("save", Some("XHR"), "https://hr.example/hr/Cycle/Save"));
    for i in 0..MAX_REQUESTS {
        for kind in ["Image", "Script", "Stylesheet", "Font"] {
            rec.observe(&sent_as(&format!("{kind}{i}"), Some(kind), &format!("https://cdn.example/{kind}/{i}")));
        }
    }
    assert_eq!(rec.mark(), 1, "they take no number");
    let all = rec.since(0);
    assert_eq!(all.len(), 1, "{:?}", all.iter().map(|e| &e.id).collect::<Vec<_>>());
    assert_eq!(all[0].id, "save", "and do not push the step's own request out");
    // Later events for a request never recorded are ignored, a redirect too.
    rec.observe(&answered("Image3", 200, "image/png"));
    rec.observe(&finished("Image3"));
    rec.observe(&ev(
        "Network.requestWillBeSent",
        json!({ "requestId": "Image3", "type": "Image", "request": { "url": "https://cdn.example/b", "method": "GET" },
                "redirectResponse": { "url": "https://cdn.example/a", "status": 302 } }),
    ));
    assert_eq!(rec.since(0).len(), 1);

    for (id, kind) in [("d", Some("Document")), ("f", Some("Fetch")), ("o", Some("Other")), ("n", None)] {
        rec.observe(&sent_as(id, kind, "https://hr.example/hr/x"));
    }
    let ids: Vec<String> = rec.since(0).into_iter().map(|e| e.id).collect();
    assert_eq!(ids, vec!["save", "d", "f", "o", "n"], "a missing type counts as Other");
}

struct Frames(VecDeque<String>);

impl Transport for Frames {
    async fn send(&mut self, _text: String) -> Result<(), String> {
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, String>> {
        self.0.pop_front().map(Ok)
    }
}

fn frame_of(e: &Event) -> String {
    json!({ "method": e.method, "params": e.params }).to_string()
}

/// A navigation or an upload inside the same step drops the event buffer
/// (`forget_events`), and the record of the step's requests must survive it.
#[tokio::test]
async fn forgetting_events_keeps_the_record() {
    let mut frames: VecDeque<String> = VecDeque::new();
    frames.push_back(frame_of(&sent("1", "GET", "https://hr.example/hr/before")));
    frames.push_back(r#"{"id":1,"result":{}}"#.into());
    frames.push_back(frame_of(&sent("2", "POST", "https://hr.example/hr/api/save?token=x")));
    frames.push_back(frame_of(&answered("2", 200, "application/json")));
    frames.push_back(frame_of(&finished("2")));
    frames.push_back(r#"{"method":"Page.lifecycleEvent","params":{"name":"load"}}"#.into());
    frames.push_back(r#"{"id":2,"result":{}}"#.into());
    let mut cdp = Cdp::over(Frames(frames));

    assert_eq!(Driver::net_mark(&cdp), 0);
    cdp.call("Runtime.evaluate", json!({ "expression": "1" })).await.unwrap();
    let mark = Driver::net_mark(&cdp);
    assert_eq!(mark, 1);
    cdp.call("Runtime.evaluate", json!({ "expression": "2" })).await.unwrap();
    Driver::forget_events(&mut cdp);

    let got = Driver::net_since(&cdp, mark);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].method, "POST");
    assert_eq!(got[0].path_query, "/hr/api/save?token=x");
    assert_eq!(got[0].status, Some(200));
    assert_eq!(got[0].state, NetState::Finished);
    // The page log still sees the same events: it is fed after the record,
    // and the one request that never finished is still news to it.
    let log = cdp.page_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].ends_with("GET https://hr.example/hr/before"), "{}", log[0]);
}

/// The suite's fake driver keeps a record too, fed from the events it
/// emits, so the steps built on it can be tested without a browser.
#[tokio::test]
async fn the_scripted_driver_keeps_a_record_through_a_forget() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({}))).with_net_record();
    d.on_call_events.push(("Page.navigate".into(), sent("1", "GET", "https://hr.example/hr/home?x=1")));
    d.on_call_events.push(("Page.navigate".into(), finished("1")));
    d.on_call_events.push(("Page.navigate".into(), ev("Page.lifecycleEvent", json!({ "name": "load" }))));
    let mark = d.net_mark();
    d.call("Page.navigate", json!({ "url": "https://hr.example/hr/home?x=1" })).await.unwrap();
    assert_eq!(d.events.len(), 1, "network events go to the record, not the buffer: {:?}", d.events);
    d.forget_events();
    let got = d.net_since(mark);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].path_query, "/hr/home?x=1");
    assert_eq!(got[0].state, NetState::Finished);

    // Without a record, the trait's defaults: nothing kept.
    let plain = ScriptedDriver::new(|_, _| Ok(json!({})));
    assert_eq!(plain.net_mark(), 0);
    assert!(plain.net_since(0).is_empty());
}
