//! The network record: a bounded list of the page's requests, in the order
//! they started, for a script step that checks what the page asked its
//! server and what it was answered (`expect_response`).

use crate::common::ScriptedDriver;
use serde_json::json;
use std::collections::VecDeque;
use v2_lib::browser::cdp::{Cdp, Driver, Event, Transport};
use v2_lib::browser::net_record::{NetRecord, NetState, MAX_REQUESTS};

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

#[test]
fn a_redirect_keeps_its_place() {
    let mut rec = NetRecord::default();
    rec.observe(&sent("1", "GET", "https://hr.example/hr"));
    rec.observe(&sent("2", "GET", "https://hr.example/hr/api/menu"));
    rec.observe(&sent("1", "GET", "https://hr.example/hr/home/index?tab=2"));
    let all = rec.since(0);
    assert_eq!(all.len(), 2, "a redirect is the same request, not a new one: {all:?}");
    assert_eq!(all[0].id, "1");
    assert_eq!(all[0].path_query, "/hr/home/index?tab=2");
    assert!(all[0].seq < all[1].seq, "it keeps the place it started in");
    assert_eq!(rec.mark(), 2, "a redirect takes no new number");
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
