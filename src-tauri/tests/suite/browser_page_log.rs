//! The page log: what a run's page was doing, for a failure to explain
//! itself - which requests failed, were refused or never finished, and
//! what the page wrote to its console.

use serde_json::json;
use std::collections::VecDeque;
use std::time::Duration;
use v2_lib::browser::cdp::{Cdp, Event, Transport};
use v2_lib::browser::page_log::{without_query, PageLog};

fn ev(method: &str, params: serde_json::Value) -> Event {
    Event { method: method.to_string(), params }
}

fn sent(log: &mut PageLog, id: &str, method: &str, url: &str) {
    assert!(log.observe(&ev("Network.requestWillBeSent", json!({ "requestId": id, "request": { "url": url, "method": method } }))));
}

fn answered(log: &mut PageLog, id: &str, status: u64) {
    assert!(log.observe(&ev("Network.responseReceived", json!({ "requestId": id, "response": { "status": status } }))));
}

fn finished(log: &mut PageLog, id: &str) {
    assert!(log.observe(&ev("Network.loadingFinished", json!({ "requestId": id }))));
}

#[test]
fn a_finished_request_is_not_news_and_an_empty_page_says_nothing() {
    let mut log = PageLog::default();
    assert!(log.report().is_empty());
    sent(&mut log, "1", "GET", "https://hr.example/hr/home/index");
    answered(&mut log, "1", 200);
    finished(&mut log, "1");
    assert!(log.report().is_empty());
}

#[test]
fn failed_refused_and_unfinished_requests_are_reported_in_the_order_they_were_sent() {
    let mut log = PageLog::default();
    sent(&mut log, "1", "POST", "https://hr.example/hr/pmsv10/PerformanceCycle?handler=Save");
    answered(&mut log, "1", 500);
    finished(&mut log, "1");
    sent(&mut log, "2", "GET", "https://hr.example/hr/pmsv10/updatehub?token=SECRET");
    sent(&mut log, "3", "GET", "https://hr.example/hr/api/menu");
    assert!(log.observe(&ev(
        "Network.loadingFailed",
        json!({ "requestId": "3", "errorText": "net::ERR_CONNECTION_RESET", "canceled": false })
    )));
    let lines = log.report();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0], "request answered 500: POST https://hr.example/hr/pmsv10/PerformanceCycle");
    assert!(lines[1].starts_with("request still waiting after "), "{}", lines[1]);
    assert!(lines[1].ends_with("s: GET https://hr.example/hr/pmsv10/updatehub"), "{}", lines[1]);
    assert_eq!(lines[2], "request failed (net::ERR_CONNECTION_RESET): GET https://hr.example/hr/api/menu");
    assert!(!lines.iter().any(|l| l.contains("SECRET") || l.contains('?')), "no query string survives: {lines:?}");
}

#[test]
fn a_request_the_page_called_off_is_not_a_failure() {
    let mut log = PageLog::default();
    sent(&mut log, "1", "GET", "https://hr.example/hr/search");
    assert!(log.observe(&ev("Network.loadingFailed", json!({ "requestId": "1", "errorText": "net::ERR_ABORTED", "canceled": true }))));
    assert!(log.report().is_empty(), "{:?}", log.report());
}

#[test]
fn a_redirect_follows_the_same_request_to_its_new_address() {
    let mut log = PageLog::default();
    sent(&mut log, "1", "GET", "https://hr.example/hr");
    sent(&mut log, "1", "GET", "https://hr.example/hr/home/index");
    let lines = log.report();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].ends_with("GET https://hr.example/hr/home/index (after 1 redirect)"), "{}", lines[0]);
}

/// Times come from the browser's own clock (`timestamp`), first hop to
/// finish: a redirect does not restart them.
fn at_time(log: &mut PageLog, method: &str, params: serde_json::Value) {
    assert!(log.observe(&ev(method, params)));
}

#[test]
fn a_slow_request_that_finished_is_reported_with_its_time_and_a_quick_one_is_not() {
    let mut log = PageLog::default();
    let send = |id: &str, url: &str, t: f64| json!({ "requestId": id, "timestamp": t, "request": { "url": url, "method": "GET" } });
    at_time(&mut log, "Network.requestWillBeSent", send("1", "https://hr.example/hr/PMSV10/PerformanceCycle/Manage?x=1", 100.0));
    at_time(&mut log, "Network.requestWillBeSent", json!({ "requestId": "1", "timestamp": 101.0, "redirectResponse": { "status": 302 }, "request": { "url": "https://hr.example/hr/pmsv10/PerformanceCycle/Manage", "method": "GET" } }));
    at_time(&mut log, "Network.loadingFinished", json!({ "requestId": "1", "timestamp": 114.04 }));
    at_time(&mut log, "Network.requestWillBeSent", send("2", "https://hr.example/hr/home/widgets", 100.0));
    at_time(&mut log, "Network.loadingFinished", json!({ "requestId": "2", "timestamp": 104.9 }));
    at_time(&mut log, "Network.requestWillBeSent", send("3", "https://hr.example/hr/api/menu", 100.0));
    at_time(&mut log, "Network.responseReceived", json!({ "requestId": "3", "response": { "status": 500 } }));
    at_time(&mut log, "Network.loadingFinished", json!({ "requestId": "3", "timestamp": 100.4 }));
    at_time(&mut log, "Network.requestWillBeSent", send("4", "https://hr.example/hr/api/badge", 100.0));
    at_time(&mut log, "Network.loadingFailed", json!({ "requestId": "4", "timestamp": 107.25, "errorText": "net::ERR_TIMED_OUT", "canceled": false }));
    assert_eq!(
        log.report(),
        vec![
            "request took 14s: GET https://hr.example/hr/pmsv10/PerformanceCycle/Manage (after 1 redirect)".to_string(),
            "request answered 500 after 0.4s: GET https://hr.example/hr/api/menu".to_string(),
            "request failed (net::ERR_TIMED_OUT) after 7.2s: GET https://hr.example/hr/api/badge".to_string(),
        ]
    );
}

#[test]
fn a_still_waiting_request_counts_its_redirects() {
    let mut log = PageLog::default();
    sent(&mut log, "1", "GET", "https://hr.example/hr/a");
    sent(&mut log, "1", "GET", "https://hr.example/hr/b");
    sent(&mut log, "1", "GET", "https://hr.example/hr/a");
    let line = &log.report()[0];
    assert!(line.starts_with("request still waiting after "), "{line}");
    assert!(line.ends_with("s: GET https://hr.example/hr/a (after 2 redirects)"), "{line}");
}

#[test]
fn an_uncaught_error_says_which_script_and_line_threw_it() {
    let mut log = PageLog::default();
    at_time(&mut log, "Runtime.exceptionThrown", json!({ "exceptionDetails": {
        "text": "Uncaught", "lineNumber": 41, "columnNumber": 7, "url": "https://hr.example/hr/js/home.js?v=12",
        "exception": { "description": "SyntaxError: Invalid or unexpected token" } } }));
    at_time(&mut log, "Runtime.exceptionThrown", json!({ "exceptionDetails": {
        "text": "Uncaught", "lineNumber": 0, "columnNumber": 3, "url": "", "scriptId": "88",
        "exception": { "description": "SyntaxError: Invalid or unexpected token" } } }));
    at_time(&mut log, "Runtime.consoleAPICalled", json!({ "type": "error", "args": [{ "type": "string", "value": "menu data missing" }],
        "stackTrace": { "callFrames": [{ "url": "https://hr.example/hr/js/menu.js?token=s", "lineNumber": 9, "columnNumber": 0 }] } }));
    assert_eq!(
        log.report(),
        vec![
            "uncaught error: SyntaxError: Invalid or unexpected token (at https://hr.example/hr/js/home.js:42:8)".to_string(),
            "uncaught error: SyntaxError: Invalid or unexpected token (at line 1 of a script the page added itself)".to_string(),
            "console error: menu data missing (at https://hr.example/hr/js/menu.js:10:1)".to_string(),
        ]
    );
}

#[test]
fn data_and_blob_addresses_are_not_requests_worth_reporting() {
    let mut log = PageLog::default();
    sent(&mut log, "1", "GET", "data:image/png;base64,AAAA");
    sent(&mut log, "2", "GET", "blob:https://hr.example/1234");
    assert!(log.report().is_empty());
}

#[test]
fn console_errors_warnings_and_uncaught_errors_are_kept_and_chatter_is_not() {
    let mut log = PageLog::default();
    let console = |kind: &str, args: serde_json::Value| ev("Runtime.consoleAPICalled", json!({ "type": kind, "args": args }));
    assert!(log.observe(&console("log", json!([{ "type": "string", "value": "menu ready" }]))));
    assert!(log.observe(&console("error", json!([{ "type": "string", "value": "Failed to load" }, { "type": "number", "value": 404 }]))));
    assert!(log.observe(&console("warning", json!([{ "type": "object", "description": "Object" }]))));
    assert!(log.observe(&ev(
        "Runtime.exceptionThrown",
        json!({ "exceptionDetails": { "text": "Uncaught", "exception": { "description": "TypeError: x is undefined\n    at https://hr.example/js/app.js?v=9:1:2" } } })
    )));
    assert_eq!(
        log.report(),
        vec![
            "console error: Failed to load 404".to_string(),
            "console warning: Object".to_string(),
            "uncaught error: TypeError: x is undefined at https://hr.example/js/app.js".to_string(),
        ]
    );
}

#[test]
fn a_long_console_message_is_cut_and_a_long_report_says_how_much_more() {
    let mut log = PageLog::default();
    let long = "x".repeat(1000);
    assert!(log.observe(&ev("Runtime.consoleAPICalled", json!({ "type": "error", "args": [{ "type": "string", "value": long }] }))));
    let line = &log.report()[0];
    assert!(line.chars().count() < 340 && line.ends_with("..."), "{}", line.len());

    let mut busy = PageLog::default();
    for i in 0..40 {
        sent(&mut busy, &i.to_string(), "GET", &format!("https://hr.example/hr/widget/{i}"));
    }
    let lines = busy.report();
    assert_eq!(lines.len(), 26, "25 lines, then a count");
    assert_eq!(lines[25], "...and 15 more");
}

#[test]
fn only_the_log_events_are_taken_and_everything_else_is_left_for_the_buffer() {
    let mut log = PageLog::default();
    assert!(log.observe(&ev("Network.dataReceived", json!({ "requestId": "1" }))));
    assert!(!log.observe(&ev("Runtime.bindingCalled", json!({ "name": "__tcm", "payload": "{}" }))));
    assert!(!log.observe(&ev("Runtime.executionContextCreated", json!({}))));
    assert!(!log.observe(&ev("Page.lifecycleEvent", json!({ "name": "load" }))));
}

#[test]
fn an_address_loses_its_query_and_fragment() {
    assert_eq!(without_query("https://h/a/b?token=1#x"), "https://h/a/b");
    assert_eq!(without_query("https://h/a#x?y"), "https://h/a");
    assert_eq!(without_query("https://h/a"), "https://h/a");
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

/// Network events read while a call waits go to the page log, not the
/// event buffer, so a busy page cannot push out the load event a
/// navigation waits for - and the load event is still there afterwards.
#[tokio::test]
async fn the_client_hands_network_events_to_the_page_log_and_keeps_the_rest() {
    let mut frames: VecDeque<String> = VecDeque::new();
    frames.push_back(r#"{"method":"Network.requestWillBeSent","params":{"requestId":"7","request":{"url":"https://hr.example/hr/pmsv10/x?t=1","method":"GET"}}}"#.into());
    for _ in 0..300 {
        frames.push_back(r#"{"method":"Network.dataReceived","params":{"requestId":"7"}}"#.into());
    }
    frames.push_back(r#"{"method":"Page.lifecycleEvent","params":{"name":"load"}}"#.into());
    for _ in 0..300 {
        frames.push_back(r#"{"method":"Network.dataReceived","params":{"requestId":"7"}}"#.into());
    }
    frames.push_back(r#"{"id":1,"result":{}}"#.into());
    let mut cdp = Cdp::over(Frames(frames));
    cdp.call("Runtime.evaluate", json!({ "expression": "1" })).await.unwrap();

    let lines = cdp.page_log();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].ends_with("GET https://hr.example/hr/pmsv10/x"), "{}", lines[0]);
    let load = cdp.wait_event("Page.lifecycleEvent", Duration::from_millis(50)).await.unwrap();
    assert_eq!(load.params["name"], "load");
}
