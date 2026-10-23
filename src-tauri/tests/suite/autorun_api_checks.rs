//! `expect_response`: the step checks a request the page itself made while
//! the step ran - found in the network record after the step's mark,
//! judged on its status and (optionally) a partial JSON match, and
//! reported in sentences that never carry a query string, a host, a header
//! or a token.

use crate::common::{quick, FakePage, ScriptedDriver};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use v2_lib::api_templates::exec::encode_query;
use v2_lib::autorun::api_checks::{
    all_taken, api_request, expect_response, judge, pick, pick_in_order, which_request, Pick, StepRequests, GET_FN,
};
use v2_lib::autorun::patterns::{classify, ErrorClass};
use v2_lib::autorun::recipe::SignInRecipe;
use v2_lib::autorun::runner::run_step;
use v2_lib::autorun::StepScript;
use v2_lib::browser::actions::{Action, ActionOutcome, ApiExpect, BROWSER_SILENT};
use v2_lib::browser::cdp::{CdpError, Driver, Event};
use v2_lib::browser::net_record::{NetEntry, NetState};
use v2_lib::browser::timing::Timing;

// ---------------------------------------------------------------- helpers

fn ev(method: &str, params: Value) -> Event {
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
    ev("Network.loadingFailed", json!({ "requestId": id, "errorText": why, "canceled": why == "net::ERR_ABORTED" }))
}

fn entry(seq: u64, method: &str, path_query: &str, status: Option<u16>, state: NetState) -> NetEntry {
    NetEntry {
        seq,
        id: format!("r{seq}"),
        method: method.to_string(),
        path_query: path_query.to_string(),
        status,
        mime: Some("application/json".to_string()),
        state,
        redirect: None,
        kind: Some("XHR".to_string()),
        same_site: true,
        polling: false,
        started_ms: None,
    }
}

fn expect(method: Option<&str>, url_contains: &str, status: u16, json: Option<Value>, timeout_ms: Option<u32>) -> Action {
    Action::ExpectResponse {
        method: method.map(str::to_string),
        url_contains: url_contains.to_string(),
        status,
        json,
        timeout_ms,
        stray: Default::default(),
    }
}

/// A browser that answers every call with `{}` except the body request,
/// which `body` answers, and keeps a network record of what it emits.
fn browser(body: impl FnMut() -> Result<Value, CdpError> + Send + 'static) -> ScriptedDriver {
    let mut body = body;
    ScriptedDriver::new(move |method, _| match method {
        "Network.getResponseBody" => body(),
        _ => Ok(json!({})),
    })
    .with_net_record()
}

/// These events appear when the step first looks (its first light call).
fn on_first_look(d: &mut ScriptedDriver, events: Vec<Event>) {
    for e in events {
        d.on_call_events.push(("Runtime.evaluate".to_string(), e));
    }
}

fn save_finished(status: u64) -> Vec<Event> {
    vec![
        sent("7", "POST", "https://hr.example/hr/pmsv10/PerformanceCycle/Save?handler=x"),
        answered("7", status, "application/json"),
        finished("7"),
    ]
}

fn short() -> Timing {
    quick()
}

/// A sentence of a check that took the step's only matching request, sent before any action was noted.
fn at_start(sentence: &str) -> String {
    format!("{sentence}: the step's matching request 1 of 1, sent as the step began")
}

async fn check(d: &mut ScriptedDriver, a: &Action) -> ActionOutcome {
    let mark = d.net_mark();
    let out = expect_response(d, a, &mut StepRequests::new(mark, vec!["expect_response".into()]), &short()).await;
    assert!(d.deadline_was_cleared(), "the wait loop must hand its deadline back: {:?}", d.deadlines);
    out
}

// ------------------------------------------------------------------- pick

#[test]
fn pick_filters_by_method_and_matches_the_pattern_without_case() {
    let entries = vec![
        entry(0, "GET", "/hr/PerformanceCycle/Save", Some(200), NetState::Finished),
        entry(1, "POST", "/hr/PerformanceCycle/Save?handler=x", Some(200), NetState::Finished),
    ];
    match pick(&entries, Some(" post "), "/performancecycle/SAVE") {
        Pick::Finished(e) => assert_eq!(e.seq, 1),
        other => panic!("{other:?}"),
    }
    match pick(&entries, Some("get"), "/PerformanceCycle/Save") {
        Pick::Finished(e) => assert_eq!(e.seq, 0),
        other => panic!("{other:?}"),
    }
    match pick(&entries, Some("PUT"), "/PerformanceCycle/Save") {
        Pick::None { seen } => assert_eq!(seen, 2),
        other => panic!("{other:?}"),
    }
    // The query is part of what is matched.
    assert!(matches!(pick(&entries, None, "handler=X"), Pick::Finished(e) if e.seq == 1));
}

#[test]
fn pick_never_matches_the_host() {
    // The record holds no host, and a pattern naming one matches nothing.
    let entries = vec![entry(0, "GET", "/hr/menu", Some(200), NetState::Finished)];
    assert!(matches!(pick(&entries, None, "hr.example"), Pick::None { seen: 1 }));
    assert!(matches!(pick(&entries, None, "https://hr.example/hr/menu"), Pick::None { seen: 1 }));
}

#[test]
fn the_most_recent_finished_match_wins() {
    let entries = vec![
        entry(0, "POST", "/a/Save", Some(500), NetState::Finished),
        entry(1, "POST", "/a/Save", Some(200), NetState::Finished),
        entry(2, "POST", "/a/Save", None, NetState::Failed("net::ERR_FAILED".into())),
        entry(3, "POST", "/a/Save", None, NetState::Pending),
    ];
    assert!(matches!(pick(&entries, None, "/a/save"), Pick::Finished(e) if e.seq == 1));
    // No finished one: the most recent failed one.
    let failing = vec![
        entry(0, "POST", "/a/Save", None, NetState::Failed("one".into())),
        entry(1, "POST", "/a/Save", None, NetState::Failed("two".into())),
        entry(2, "POST", "/a/Save", None, NetState::Pending),
    ];
    assert!(matches!(pick(&failing, None, "/a/save"), Pick::Failed(e) if e.seq == 1));
}

#[test]
fn a_match_still_going_is_pending_only() {
    let entries = vec![
        entry(0, "GET", "/other", Some(200), NetState::Finished),
        entry(1, "POST", "/a/Save", Some(200), NetState::Pending),
    ];
    assert!(matches!(pick(&entries, None, "/a/Save"), Pick::PendingOnly(e) if e.seq == 1));
    assert!(matches!(pick(&[], None, "/a/Save"), Pick::None { seen: 0 }));
}

#[test]
fn pick_in_order_takes_the_oldest_untaken_match() {
    let entries = vec![
        entry(0, "GET", "/other", Some(200), NetState::Finished),
        entry(1, "POST", "/a/Save", Some(500), NetState::Finished),
        entry(2, "POST", "/a/Save", Some(200), NetState::Finished),
        entry(3, "POST", "/a/Save", None, NetState::Pending),
    ];
    let taken = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // The oldest, where `pick` takes the most recent finished one.
    assert!(matches!(pick_in_order(&entries, None, "/a/save", &[]), Pick::Finished(e) if e.seq == 1));
    assert!(matches!(pick_in_order(&entries, None, "/a/save", &taken(&["r1"])), Pick::Finished(e) if e.seq == 2));
    assert!(matches!(pick_in_order(&entries, None, "/a/save", &taken(&["r1", "r2"])), Pick::PendingOnly(e) if e.seq == 3));
    assert!(matches!(pick_in_order(&entries, None, "/a/save", &taken(&["r1", "r2", "r3"])), Pick::None { seen: 4 }));
    // Taking is by the request, not by the pattern: another pattern still sees it.
    assert!(matches!(pick_in_order(&entries, Some("get"), "/other", &taken(&["r1"])), Pick::Finished(e) if e.seq == 0));
    // An older request still going is skipped once a newer match has finished: the oldest that
    // FINISHED is taken, and the check waits only while none has.
    let slow_first = vec![
        entry(0, "GET", "/a/List", None, NetState::Pending),
        entry(1, "GET", "/a/List", Some(200), NetState::Finished),
    ];
    assert!(matches!(pick_in_order(&slow_first, None, "/a/list", &[]), Pick::Finished(e) if e.seq == 1));
    let both_going = vec![
        entry(0, "GET", "/a/List", None, NetState::Pending),
        entry(1, "GET", "/a/List", None, NetState::Pending),
    ];
    assert!(matches!(pick_in_order(&both_going, None, "/a/list", &[]), Pick::PendingOnly(e) if e.seq == 0));
    // Of several that finished, the oldest still left; the older pending one is next once it finishes.
    let two_done = vec![
        entry(0, "GET", "/a/List", None, NetState::Pending),
        entry(1, "GET", "/a/List", Some(200), NetState::Finished),
        entry(2, "GET", "/a/List", Some(201), NetState::Finished),
    ];
    assert!(matches!(pick_in_order(&two_done, None, "/a/list", &[]), Pick::Finished(e) if e.seq == 1));
    assert!(matches!(pick_in_order(&two_done, None, "/a/list", &["r1".to_string()]), Pick::Finished(e) if e.seq == 2));
    assert!(matches!(
        pick_in_order(&two_done, None, "/a/list", &["r1".to_string(), "r2".to_string()]),
        Pick::PendingOnly(e) if e.seq == 0
    ));
    // A request that failed is judged when none left has finished, and a newer one that finished wins over it.
    let failing = vec![
        entry(0, "GET", "/a/List", None, NetState::Failed("net::ERR_FAILED".into())),
        entry(1, "GET", "/a/List", Some(200), NetState::Finished),
    ];
    assert!(matches!(pick_in_order(&failing, None, "/a/list", &[]), Pick::Finished(e) if e.seq == 1));
    let failed_then_going = vec![
        entry(0, "GET", "/a/List", None, NetState::Pending),
        entry(1, "GET", "/a/List", None, NetState::Failed("net::ERR_FAILED".into())),
    ];
    assert!(matches!(pick_in_order(&failed_then_going, None, "/a/list", &[]), Pick::Failed(e) if e.seq == 1));
    assert!(matches!(pick_in_order(&[], None, "/a/list", &[]), Pick::None { seen: 0 }));
}

#[test]
fn a_cancelled_request_is_passed_over_when_a_later_one_matches() {
    let aborted = || NetState::Failed("net::ERR_ABORTED".into());
    // A debounced search: the first request cancelled, the second answered.
    let debounced = vec![
        entry(0, "GET", "/api/Search?q=ab", None, aborted()),
        entry(1, "GET", "/api/Search?q=abc", Some(200), NetState::Finished),
    ];
    assert!(matches!(pick_in_order(&debounced, None, "/api/search", &[]), Pick::Finished(e) if e.seq == 1));
    // The later one still going: it is the one waited for, never "was cancelled".
    let resent = vec![
        entry(0, "GET", "/api/Search?q=ab", None, aborted()),
        entry(1, "GET", "/api/Search?q=abc", None, NetState::Pending),
    ];
    assert!(matches!(pick_in_order(&resent, None, "/api/search", &[]), Pick::PendingOnly(e) if e.seq == 1));
    // Once the later one is taken, the cancelled one is not left for the next check.
    assert!(matches!(
        pick_in_order(&debounced, None, "/api/search", &["r1".to_string()]),
        Pick::None { seen: 2 }
    ));
    // A lone cancelled request is still judged, and fails as it does today.
    let lone = vec![entry(0, "GET", "/api/Search?q=ab", None, aborted())];
    assert!(matches!(pick_in_order(&lone, None, "/api/search", &[]), Pick::Failed(e) if e.seq == 0));
}

#[test]
fn which_request_names_the_place_and_the_action() {
    let entries = vec![
        entry(0, "GET", "/other", Some(200), NetState::Finished),
        entry(1, "POST", "/a/Save", Some(200), NetState::Finished),
        entry(2, "POST", "/a/Save", Some(200), NetState::Finished),
        entry(3, "POST", "/a/Save", Some(200), NetState::Finished),
    ];
    let kinds = ["click", "fill", "press_key", "expect_response"].map(String::from).to_vec();
    let mut step = StepRequests::new(0, kinds.clone());
    for seen in [0, 1, 2, 4] {
        step.action_began(seen);
    }
    let said = |i: usize| which_request(&entries, &entries[i], Some("post"), "/a/save", &step);
    assert_eq!(said(1), ": the step's matching request 1 of 3, sent after action 2 (fill) began");
    assert_eq!(said(2), ": the step's matching request 2 of 3, sent after action 3 (press_key) began");
    assert_eq!(said(3), ": the step's matching request 3 of 3, sent after action 3 (press_key) began");
    // Before the first action had begun, or when no action was noted.
    let mut early = StepRequests::new(0, kinds.clone());
    early.action_began(2);
    assert_eq!(
        which_request(&entries, &entries[1], Some("post"), "/a/save", &early),
        ": the step's matching request 1 of 3, sent as the step began"
    );
    let bare = StepRequests::new(0, kinds);
    assert_eq!(
        which_request(&entries, &entries[0], None, "/other", &bare),
        ": the step's matching request 1 of 1, sent as the step began"
    );
}

#[test]
fn all_taken_names_the_path_only() {
    assert_eq!(
        all_taken(" /a/Save?token=abc ", 100, 2),
        "no request matching \"/a/Save\" left in 0.1 s: an earlier expect_response of this step took each of the 2 that matched"
    );
}

// ------------------------------------------------------------------ judge

#[test]
fn judge_passes_the_right_status_and_says_each_failure_in_its_own_sentence() {
    let ok = entry(0, "post", "/hr/Cycle/Save?id=7", Some(200), NetState::Finished);
    assert_eq!(judge(&ok, 200, None, None), Ok(()));

    let wrong = entry(0, "POST", "/hr/Cycle/Save?id=7", Some(500), NetState::Finished);
    assert_eq!(judge(&wrong, 200, None, None).unwrap_err(), "POST /hr/Cycle/Save answered 500, expected 200");

    let broke = entry(0, "POST", "/hr/Cycle/Save?id=7", None, NetState::Failed("net::ERR_CONNECTION_RESET".into()));
    assert_eq!(judge(&broke, 200, None, None).unwrap_err(), "POST /hr/Cycle/Save failed: net::ERR_CONNECTION_RESET");

    let cancelled = entry(0, "POST", "/hr/Cycle/Save?id=7", None, NetState::Failed("net::ERR_ABORTED".into()));
    assert_eq!(judge(&cancelled, 200, None, None).unwrap_err(), "POST /hr/Cycle/Save was cancelled by the page");
}

#[test]
fn judge_checks_json_as_a_partial_match() {
    let ok = entry(0, "POST", "/hr/Cycle/Save", Some(200), NetState::Finished);
    let want = json!({ "success": true, "data": { "id": 7 } });
    let body = r#"{ "success": true, "extra": 1, "data": { "id": 7, "name": "Q4" } }"#;
    assert_eq!(judge(&ok, 200, Some(&want), Some(body)), Ok(()));

    let differs = r#"{ "success": false, "data": { "id": 7 } }"#;
    assert_eq!(
        judge(&ok, 200, Some(&want), Some(differs)).unwrap_err(),
        "the response to POST /hr/Cycle/Save: expected success = true, got false"
    );
    let nested = r#"{ "success": true, "data": { "id": 8 } }"#;
    assert_eq!(
        judge(&ok, 200, Some(&want), Some(nested)).unwrap_err(),
        "the response to POST /hr/Cycle/Save: expected id = 7, got 8"
    );
}

#[test]
fn judge_says_when_the_body_is_not_json_or_is_gone() {
    let ok = entry(0, "POST", "/hr/Cycle/Save?x=1", Some(200), NetState::Finished);
    let want = json!({ "success": true });
    assert_eq!(
        judge(&ok, 200, Some(&want), Some("<html><body>Sign in</body></html>")).unwrap_err(),
        "the response to POST /hr/Cycle/Save was not JSON"
    );
    assert_eq!(judge(&ok, 200, Some(&want), None).unwrap_err(), "the response body was no longer available");
    // Status is judged before the body.
    let wrong = entry(0, "POST", "/hr/Cycle/Save", Some(302), NetState::Finished);
    assert_eq!(
        judge(&wrong, 200, Some(&want), Some("<html>")).unwrap_err(),
        "POST /hr/Cycle/Save answered 302, expected 200"
    );
}

// ------------------------------------------------------ against a browser

#[tokio::test]
async fn a_request_made_during_the_step_that_answered_right_passes() {
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, save_finished(200));
    let out = check(&mut d, &expect(Some("post"), "/performancecycle/save", 200, None, None)).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.detail, at_start("POST /hr/pmsv10/PerformanceCycle/Save answered 200"));
    assert!(d.calls_to("Network.getResponseBody").is_empty(), "no body is read without a JSON check");
}

#[tokio::test]
async fn a_json_check_reads_the_body_plain_or_base64() {
    let mut d = browser(|| Ok(json!({ "body": "{\"success\":true,\"id\":7}", "base64Encoded": false })));
    on_first_look(&mut d, save_finished(200));
    let out = check(&mut d, &expect(None, "/Save", 200, Some(json!({ "success": true })), None)).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(d.calls_to("Network.getResponseBody"), vec![json!({ "requestId": "7" })]);

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(br#"{"success":true}"#);
    let mut d = browser(move || Ok(json!({ "body": encoded.clone(), "base64Encoded": true })));
    on_first_look(&mut d, save_finished(200));
    let out = check(&mut d, &expect(None, "/Save", 200, Some(json!({ "success": true })), None)).await;
    assert!(out.ok, "{out:?}");
}

#[tokio::test]
async fn no_request_at_all_says_so_after_the_timeout() {
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, vec![sent("1", "GET", "https://hr.example/hr/menu"), answered("1", 200, "text/html"), finished("1")]);
    let out = check(&mut d, &expect(None, "/Save", 200, None, Some(300))).await;
    assert!(!out.ok);
    assert_eq!(out.detail, "no request matching \"/Save\" in 0.3 s (this step made 1 request)");

    // Two: plural.
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, vec![sent("1", "GET", "https://hr.example/a"), sent("2", "GET", "https://hr.example/b")]);
    let out = check(&mut d, &expect(None, "/Save", 200, None, Some(100))).await;
    assert_eq!(out.detail, "no request matching \"/Save\" in 0.1 s (this step made 2 requests)");
    assert!(!out.harness);
}

#[tokio::test]
async fn a_matching_request_that_never_finishes_says_it_had_not_finished() {
    // Review focus 1: a hung save is not "no request".
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, vec![sent("7", "POST", "https://hr.example/hr/Cycle/Save?id=1"), answered("7", 200, "application/json")]);
    let out = check(&mut d, &expect(None, "/Save", 200, None, Some(200))).await;
    assert!(!out.ok);
    assert_eq!(out.detail, at_start("POST /hr/Cycle/Save had not finished after 0.2 s"));
}

#[tokio::test]
async fn a_failed_and_a_cancelled_request_each_have_their_sentence() {
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, vec![sent("7", "POST", "https://hr.example/hr/Cycle/Save"), failed("7", "net::ERR_CONNECTION_RESET")]);
    let out = check(&mut d, &expect(None, "/Save", 200, None, None)).await;
    assert_eq!(out.detail, at_start("POST /hr/Cycle/Save failed: net::ERR_CONNECTION_RESET"));

    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, vec![sent("7", "POST", "https://hr.example/hr/Cycle/Save"), failed("7", "net::ERR_ABORTED")]);
    let out = check(&mut d, &expect(None, "/Save", 200, None, None)).await;
    assert_eq!(out.detail, at_start("POST /hr/Cycle/Save was cancelled by the page"));
}

#[tokio::test]
async fn a_wrong_status_names_both_and_shows_the_body_when_one_was_read() {
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, save_finished(500));
    let out = check(&mut d, &expect(Some("POST"), "/Save", 200, None, None)).await;
    assert_eq!(out.detail, at_start("POST /hr/pmsv10/PerformanceCycle/Save answered 500, expected 200"));

    let mut d = browser(|| Ok(json!({ "body": "{\"error\":\"boom\"}", "base64Encoded": false })));
    on_first_look(&mut d, save_finished(500));
    let out = check(&mut d, &expect(Some("POST"), "/Save", 200, Some(json!({ "success": true })), None)).await;
    assert_eq!(
        out.detail,
        "POST /hr/pmsv10/PerformanceCycle/Save answered 500, expected 200: the step's matching request 1 of 1, sent as the step began - the response began: {\"error\":\"boom\"}"
    );
}

/// An unexpected 400 is read even with no JSON asked for: whether it said
/// why is what tells a refused request from a transient one (spec §6).
#[tokio::test]
async fn an_unexpected_400_shows_its_body_so_an_empty_one_can_be_told_apart() {
    let mut d = browser(|| Ok(json!({ "body": "{\"error\":\"Name is required\"}", "base64Encoded": false })));
    on_first_look(&mut d, save_finished(400));
    let out = check(&mut d, &expect(Some("POST"), "/Save", 200, None, None)).await;
    assert_eq!(
        out.detail,
        "POST /hr/pmsv10/PerformanceCycle/Save answered 400, expected 200: the step's matching request 1 of 1, sent as the step began - the response began: {\"error\":\"Name is required\"}"
    );

    let mut d = browser(|| Ok(json!({ "body": "", "base64Encoded": false })));
    on_first_look(&mut d, save_finished(400));
    let out = check(&mut d, &expect(Some("POST"), "/Save", 200, None, None)).await;
    assert_eq!(out.detail, at_start("POST /hr/pmsv10/PerformanceCycle/Save answered 400, expected 200"));

    // An expected 400 is a pass, and its body is still never read.
    let mut d = browser(|| panic!("the body of an expected answer is not read"));
    on_first_look(&mut d, save_finished(400));
    let out = check(&mut d, &expect(Some("POST"), "/Save", 400, None, None)).await;
    assert!(out.ok, "{out:?}");
}

#[tokio::test]
async fn a_body_chrome_no_longer_holds_or_that_is_not_json_fails_plainly() {
    // Review focus 2.
    let mut d = browser(|| {
        Err(CdpError::Protocol {
            method: "Network.getResponseBody".into(),
            message: "No resource with given identifier found".into(),
        })
    });
    on_first_look(&mut d, save_finished(200));
    let out = check(&mut d, &expect(None, "/Save", 200, Some(json!({ "success": true })), None)).await;
    assert!(!out.ok && !out.harness, "{out:?}");
    assert_eq!(out.detail, at_start("the response body was no longer available"));

    let mut d = browser(|| Ok(json!({ "body": "<html>\n  <title>Sign in</title>\n</html>", "base64Encoded": false })));
    on_first_look(&mut d, save_finished(200));
    let out = check(&mut d, &expect(None, "/Save", 200, Some(json!({ "success": true })), None)).await;
    assert!(!out.ok);
    assert_eq!(
        out.detail,
        "the response to POST /hr/pmsv10/PerformanceCycle/Save was not JSON: the step's matching request 1 of 1, sent as the step began - the response began: <html> <title>Sign in</title> </html>"
    );
}

#[tokio::test]
async fn an_anti_forgery_token_in_the_body_is_scrubbed_from_the_excerpt() {
    let body = r#"{"success":false,"__RequestVerificationToken":"CfDJ8secretvalue"}"#;
    let mut d = browser(move || Ok(json!({ "body": body, "base64Encoded": false })));
    on_first_look(&mut d, save_finished(200));
    let out = check(&mut d, &expect(None, "/Save", 200, Some(json!({ "success": true })), None)).await;
    assert!(!out.ok);
    assert!(!out.detail.contains("CfDJ8secretvalue"), "{}", out.detail);
    assert!(out.detail.starts_with("the response to POST /hr/pmsv10/PerformanceCycle/Save: expected success = true, got false"));
}

#[tokio::test]
async fn no_outcome_ever_carries_the_query_string() {
    // Review focus 4: `?access_token=...` stays in the record, for
    // matching, and never reaches a sentence.
    let secret = "https://hr.example/hr/Cycle/Save?access_token=abc123";
    let finished_with = |status: u64| vec![sent("7", "POST", secret), answered("7", status, "application/json"), finished("7")];
    let mut details = vec![];

    for (events, a, body) in [
        (finished_with(200), expect(None, "/Save?access_token=abc123", 200, None, None), None),
        (finished_with(500), expect(None, "/Save", 200, None, None), None),
        (finished_with(200), expect(None, "access_token", 200, Some(json!({ "ok": true })), None), Some("{\"ok\":false}")),
        (finished_with(200), expect(None, "/Save", 200, Some(json!({ "ok": true })), None), Some("<html>")),
        (finished_with(200), expect(None, "/Save", 200, Some(json!({ "ok": true })), None), None),
        (vec![sent("7", "POST", secret)], expect(None, "/Save", 200, None, Some(100)), None),
        (vec![sent("7", "POST", secret), failed("7", "net::ERR_FAILED")], expect(None, "/Save", 200, None, None), None),
        (vec![sent("7", "POST", secret), failed("7", "net::ERR_ABORTED")], expect(None, "/Save", 200, None, None), None),
        (vec![], expect(None, "/Other?access_token=abc123#frag", 200, None, Some(100)), None),
    ] {
        let mut d = browser(move || match body {
            Some(b) => Ok(json!({ "body": b, "base64Encoded": false })),
            None => Err(CdpError::Protocol { method: "Network.getResponseBody".into(), message: "gone".into() }),
        });
        on_first_look(&mut d, events);
        details.push(check(&mut d, &a).await.detail);
    }
    assert_eq!(details[0], at_start("POST /hr/Cycle/Save answered 200"));
    assert_eq!(details[8], "no request matching \"/Other\" in 0.1 s (this step made 0 requests)");
    for detail in &details {
        assert!(!detail.contains("access_token") && !detail.contains("abc123") && !detail.contains("hr.example"), "{detail}");
    }
}

#[tokio::test]
async fn a_browser_that_stops_answering_is_a_harness_failure() {
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed)).with_net_record();
    let out = check(&mut d, &expect(None, "/Save", 200, None, Some(100))).await;
    assert!(out.harness, "{out:?}");
    assert!(out.detail.starts_with(BROWSER_SILENT), "{}", out.detail);
}

#[tokio::test]
async fn an_invalid_expect_response_is_refused_before_it_waits() {
    let mut d = browser(|| Ok(json!({})));
    let out = expect_response(
        &mut d,
        &expect(None, "  ", 200, None, None),
        &mut StepRequests::new(0, vec!["expect_response".into()]),
        &short(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.starts_with("this action cannot run: "), "{}", out.detail);
    assert!(d.calls.is_empty());
}

// -------------------------------------------------------- redirects (I1)

/// A redirect as Chrome reports it: the same request id sent again, to
/// `to`, carrying what `from` answered.
fn redirected(id: &str, status: u64, from: &str, to: &str) -> Event {
    ev(
        "Network.requestWillBeSent",
        json!({ "requestId": id, "type": "XHR", "request": { "url": to, "method": "GET" },
                "redirectResponse": { "url": from, "status": status, "mimeType": "text/html" } }),
    )
}

const SAVE: &str = "https://hr.example/hr/pmsv10/PerformanceCycle/Save?handler=x";

#[tokio::test]
async fn a_save_redirected_to_sign_in_says_so_promptly() {
    // The session ended: the save is answered 302 to the sign-in page,
    // which is still loading. The request WAS made - not "no request".
    for json in [None, Some(json!({ "success": true }))] {
        let mut d = browser(|| Ok(json!({ "body": "<html>sign in</html>", "base64Encoded": false })));
        on_first_look(
            &mut d,
            vec![
                sent("7", "POST", SAVE),
                redirected("7", 302, SAVE, "https://hr.example/Account/Login?ReturnUrl=%2Fhr%2Fpmsv10%3Faccess_token%3Dabc"),
            ],
        );
        let began = std::time::Instant::now();
        let out = check(&mut d, &expect(Some("POST"), "/PerformanceCycle/Save", 200, json.clone(), Some(5_000))).await;
        assert!(
            began.elapsed() < std::time::Duration::from_millis(2_500),
            "judged when the redirect arrived, not at the timeout: {out:?}"
        );
        assert!(!out.ok && !out.harness, "{out:?}");
        assert_eq!(out.detail, at_start("POST /hr/pmsv10/PerformanceCycle/Save was redirected to /Account/Login"));
        assert!(d.calls_to("Network.getResponseBody").is_empty(), "a redirect's body is never read");
        assert_eq!(classify(&out.detail, None), ErrorClass::Api);
    }
}

#[tokio::test]
async fn a_redirect_to_another_site_says_so_without_its_host() {
    let mut d = browser(|| Ok(json!({})));
    on_first_look(
        &mut d,
        vec![
            sent("7", "POST", SAVE),
            redirected("7", 302, SAVE, "https://login.microsoftonline.com/common/oauth2/authorize?client_id=abc&state=xyz"),
        ],
    );
    let out = check(&mut d, &expect(None, "/Save", 200, None, Some(1_000))).await;
    assert_eq!(
        out.detail,
        at_start("POST /hr/pmsv10/PerformanceCycle/Save was redirected to /common/oauth2/authorize (on another site)")
    );
    for leak in ["microsoftonline", "client_id", "abc", "xyz", "hr.example", "handler"] {
        assert!(!out.detail.contains(leak), "{leak}: {}", out.detail);
    }
}

#[tokio::test]
async fn a_post_expected_to_answer_302_passes_post_redirect_get() {
    let mut d = browser(|| Ok(json!({})));
    on_first_look(
        &mut d,
        vec![
            sent("7", "POST", SAVE),
            redirected("7", 302, SAVE, "https://hr.example/hr/pmsv10/PerformanceCycle/Index"),
            answered("7", 200, "text/html"),
            finished("7"),
        ],
    );
    let out = check(&mut d, &expect(Some("POST"), "/PerformanceCycle/Save", 302, None, Some(1_000))).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.detail, at_start("POST /hr/pmsv10/PerformanceCycle/Save answered 302"));

    // A redirect has no JSON to check: asked for, it fails - without reading
    // the page it went on to.
    let mut d = browser(|| Ok(json!({ "body": "{\"success\":true}", "base64Encoded": false })));
    on_first_look(
        &mut d,
        vec![
            sent("7", "POST", SAVE),
            redirected("7", 302, SAVE, "https://hr.example/hr/pmsv10/PerformanceCycle/Index"),
            answered("7", 200, "application/json"),
            finished("7"),
        ],
    );
    let out = check(&mut d, &expect(Some("POST"), "/Save", 302, Some(json!({ "success": true })), Some(1_000))).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(out.detail, at_start("POST /hr/pmsv10/PerformanceCycle/Save was redirected to /hr/pmsv10/PerformanceCycle/Index"));
    assert!(d.calls_to("Network.getResponseBody").is_empty());

    // Expected another 3xx than the one it answered.
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, vec![sent("7", "POST", SAVE), redirected("7", 302, SAVE, "https://hr.example/hr/Index")]);
    let out = check(&mut d, &expect(None, "/Save", 303, None, Some(1_000))).await;
    assert_eq!(out.detail, at_start("POST /hr/pmsv10/PerformanceCycle/Save was redirected to /hr/Index"));
}

#[tokio::test]
async fn a_redirect_target_that_matches_the_pattern_is_no_false_pass() {
    // No method given; the save is redirected to the cycle list, whose
    // path ALSO contains the pattern. The save did not answer 200.
    let mut d = browser(|| Ok(json!({})));
    on_first_look(
        &mut d,
        vec![
            sent("7", "POST", SAVE),
            redirected("7", 302, SAVE, "https://hr.example/hr/pmsv10/PerformanceCycle/Index"),
            answered("7", 200, "text/html"),
            finished("7"),
        ],
    );
    let out = check(&mut d, &expect(None, "/PerformanceCycle", 200, None, Some(1_000))).await;
    assert!(!out.ok, "{out:?}");
    assert_eq!(out.detail, at_start("POST /hr/pmsv10/PerformanceCycle/Save was redirected to /hr/pmsv10/PerformanceCycle/Index"));
}

// ------------------------------------------- misplaced expectations (I2)

fn parsed(v: Value) -> Result<Action, String> {
    let a: Action = serde_json::from_value(v).map_err(|e| e.to_string())?;
    a.validate()?;
    Ok(a)
}

#[test]
fn an_api_request_written_like_an_expect_response_is_refused() {
    for key in ["status", "json", "method", "url_contains", "timeout_ms"] {
        let mut v = json!({ "kind": "api_request", "path": "/api/cycles/42" });
        v[key] = match key {
            "status" => json!(200),
            "json" => json!({ "name": "Q4 Cycle" }),
            "timeout_ms" => json!(5000),
            _ => json!("GET"),
        };
        let err = parsed(v).unwrap_err();
        assert!(err.contains("api_request takes status and json under \"expect\""), "{key}: {err}");
    }
    // The sibling's whole shape too.
    let err = parsed(json!({ "kind": "api_request", "path": "/api/cycles/42", "status": 200, "json": { "name": "Q4 Cycle" } }))
        .unwrap_err();
    assert!(err.contains("under \"expect\""), "{err}");
}

#[test]
fn an_expect_response_written_like_an_api_request_is_refused() {
    let err = parsed(json!({ "kind": "expect_response", "url_contains": "/Save", "expect": { "status": 201 } })).unwrap_err();
    assert!(err.contains("expect_response takes status and json directly, not under \"expect\""), "{err}");
}

#[test]
fn a_misspelt_expectation_key_is_refused() {
    let err = parsed(json!({ "kind": "api_request", "path": "/api/x", "expect": { "jsn": { "a": 1 } } })).unwrap_err();
    assert!(err.contains("jsn"), "{err}");
    let err = parsed(json!({ "kind": "api_request", "path": "/api/x", "expect": { "status": 200, "Json": {} } })).unwrap_err();
    assert!(err.contains("Json"), "{err}");
}

#[test]
fn the_right_shapes_are_still_accepted() {
    let a = parsed(json!({
        "kind": "api_request", "path": "/api/cycles/42", "query": { "include": "rules" },
        "expect": { "status": 201, "json": { "name": "Q4 Cycle" } }
    }))
    .unwrap();
    assert!(matches!(&a, Action::ApiRequest { expect, .. } if expect.status == 201 && expect.json.is_some()));
    // Written back exactly as it came - nothing extra.
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        json!({ "kind": "api_request", "path": "/api/cycles/42", "query": { "include": "rules" },
                "expect": { "status": 201, "json": { "name": "Q4 Cycle" } } })
    );
    let full = json!({
        "kind": "expect_response", "method": "POST", "url_contains": "/Save",
        "status": 201, "json": { "success": true }, "timeout_ms": 5000
    });
    let e = parsed(full.clone()).unwrap();
    assert_eq!(serde_json::to_value(&e).unwrap(), full);
    parsed(json!({ "kind": "expect_response", "url_contains": "/Save" })).unwrap();
    parsed(json!({ "kind": "api_request", "path": "/api/x" })).unwrap();
}

#[test]
fn an_old_script_of_another_kind_with_an_extra_key_still_loads() {
    for v in [
        json!({ "kind": "click", "selector": { "css": "#save" }, "note": "from an older app" }),
        json!({ "kind": "expect_visible", "selector": { "css": "#ok" }, "status": 200 }),
        json!({ "kind": "navigate", "url": "https://hr.example/", "expect": { "status": 200 } }),
    ] {
        parsed(v.clone()).unwrap_or_else(|e| panic!("{v}: {e}"));
    }
}

// -------------------------------------------------------------- the runner

fn step(actions: Vec<Action>) -> StepScript {
    StepScript { step_number: 1, actions, unchecked: None, cleanup: false, because: Vec::new() }
}

async fn run(d: &mut ScriptedDriver, actions: Vec<Action>) -> Vec<ActionOutcome> {
    let dir = tempfile::tempdir().unwrap();
    let mut acc: Option<String> = None;
    run_step(d, dir.path(), "Acme", "Web", &step(actions), &quick(), &mut acc).await.unwrap()
}

#[tokio::test]
async fn requests_made_before_the_step_began_are_not_the_steps() {
    let mut d = browser(|| Ok(json!({})));
    // Already recorded when the step starts...
    for e in save_finished(200) {
        d.net.as_mut().unwrap().observe(&e);
    }
    // ...and already sent, but not yet read, when it starts: the runner
    // reads what is waiting before it takes the mark.
    on_first_look(&mut d, vec![sent("8", "POST", "https://hr.example/hr/x/Save"), answered("8", 200, "application/json"), finished("8")]);
    let out = run(&mut d, vec![expect(None, "/Save", 200, None, Some(100))]).await;
    assert_eq!(out.len(), 1);
    assert!(!out[0].ok, "{:?}", out[0]);
    assert_eq!(out[0].detail, "no request matching \"/Save\" in 0.1 s (this step made 0 requests)");
}

#[tokio::test]
async fn a_request_made_by_an_earlier_action_of_the_same_step_is_checked() {
    // The navigation's requests stay in the record (review focus 5): only
    // the event buffer is cleared before a navigation.
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Page.navigate" => Ok(json!({ "frameId": "F", "loaderId": "L" })),
        _ => Ok(json!({})),
    })
    .with_net_record();
    d.on_call_events.push((
        "Page.navigate".into(),
        ev("Page.lifecycleEvent", json!({ "frameId": "F", "loaderId": "L", "name": "load" })),
    ));
    for e in [sent("9", "GET", "https://hr.example/hr/api/cycles?page=1"), answered("9", 200, "application/json"), finished("9")] {
        d.on_call_events.push(("Page.navigate".into(), e));
    }
    let out = run(
        &mut d,
        vec![
            Action::Navigate { url: "https://hr.example/hr/cycles".into() },
            expect(Some("get"), "/api/cycles", 200, None, Some(300)),
        ],
    )
    .await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(out[1].ok, "{:?}", out[1]);
    assert_eq!(
        out[1].detail,
        "GET /hr/api/cycles answered 200: the step's matching request 1 of 1, sent after action 1 (navigate) began"
    );
}

/// A browser whose page sends these requests only once it has been asked
/// `after` things - so they start after whatever the step did first.
struct Later {
    inner: ScriptedDriver,
    after: usize,
    events: Vec<Event>,
}

impl Driver for Later {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        let out = self.inner.call(method, params).await;
        if self.inner.calls.len() == self.after {
            for e in self.events.drain(..) {
                self.inner.net.as_mut().unwrap().observe(&e);
            }
        }
        out
    }
    async fn wait_event(&mut self, method: &str, limit: std::time::Duration) -> Result<Event, CdpError> {
        self.inner.wait_event(method, limit).await
    }
    fn forget_events(&mut self) {
        self.inner.forget_events()
    }
    fn take_dialogs(&mut self) -> Vec<String> {
        self.inner.take_dialogs()
    }
    fn net_mark(&self) -> u64 {
        self.inner.net_mark()
    }
    fn net_since(&self, mark: u64) -> Vec<NetEntry> {
        self.inner.net_since(mark)
    }
    fn set_deadline(&mut self, deadline: Option<std::time::Instant>) {
        self.inner.set_deadline(deadline)
    }
}

#[tokio::test]
async fn a_lone_tried_expect_response_is_carried_out_by_the_runner() {
    // The assistant's "try" is a step of one through `run_step`: its mark
    // is taken as it starts, and a request the page sends while it waits
    // is the one checked.
    let mut d = Later { inner: browser(|| Ok(json!({}))), after: 2, events: save_finished(200) };
    let dir = tempfile::tempdir().unwrap();
    let mut acc: Option<String> = None;
    let lone = step(vec![expect(None, "/Save", 200, None, Some(300))]);
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &lone, &quick(), &mut acc).await.unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].ok, "{:?}", out[0]);
    // The page sent it while the check waited: after the check's own action began.
    assert_eq!(
        out[0].detail,
        "POST /hr/pmsv10/PerformanceCycle/Save answered 200: the step's matching request 1 of 1, sent after action 1 (expect_response) began"
    );
}

fn click_option() -> Action {
    serde_json::from_value(json!({ "kind": "click", "selector": { "css": "#option" } })).unwrap()
}

/// A page whose click on the option makes these `GET /hr/api/List` requests, each answered 200.
fn clicking_sends(urls: &[&str]) -> ScriptedDriver {
    let mut d = FakePage::default().driver().with_net_record();
    for (i, url) in urls.iter().enumerate() {
        let id = format!("L{i}");
        for e in [sent(&id, "GET", url), answered(&id, 200, "application/json"), finished(&id)] {
            d.on_call_events.push(("Input.dispatchMouseEvent".into(), e));
        }
    }
    d
}

const FILTER_URL: &str = "https://hr.example/hr/api/List?StatusFilter=Draft";

#[tokio::test]
async fn an_expect_response_after_a_filter_click_in_the_same_step_matches_the_filters_request() {
    let mut d = clicking_sends(&[FILTER_URL]);
    let out = run(&mut d, vec![click_option(), expect(Some("GET"), "StatusFilter=Draft", 200, None, Some(300))]).await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(out[1].ok, "{:?}", out[1]);
    assert_eq!(
        out[1].detail,
        "GET /hr/api/List answered 200: the step's matching request 1 of 1, sent after action 1 (click) began"
    );
}

#[tokio::test]
async fn two_checks_on_one_pattern_take_two_requests_in_order() {
    let mut d = clicking_sends(&[FILTER_URL, "https://hr.example/hr/api/List?StatusFilter=Draft&page=2"]);
    let again = expect(Some("GET"), "/api/List", 200, None, Some(300));
    let out = run(&mut d, vec![click_option(), again.clone(), again]).await;
    assert!(out[1].ok && out[2].ok, "{:?}", out);
    assert_eq!(
        out[1].detail,
        "GET /hr/api/List answered 200: the step's matching request 1 of 2, sent after action 1 (click) began"
    );
    assert_eq!(
        out[2].detail,
        "GET /hr/api/List answered 200: the step's matching request 2 of 2, sent after action 1 (click) began"
    );
}

#[tokio::test]
async fn a_check_with_every_match_taken_says_so() {
    let mut d = clicking_sends(&[FILTER_URL, "https://hr.example/hr/api/List?StatusFilter=Draft&page=2"]);
    let again = expect(Some("GET"), "/api/List", 200, None, Some(100));
    let out = run(&mut d, vec![click_option(), again.clone(), again.clone(), again]).await;
    assert!(out[1].ok && out[2].ok, "{:?}", out);
    assert!(!out[3].ok, "{:?}", out[3]);
    assert_eq!(out[3].detail, all_taken("/api/List", 100, 2));
    assert_eq!(classify(&out[3].detail, None), ErrorClass::Api);
}

#[tokio::test]
async fn a_failure_ends_with_the_request_it_judged_before_any_excerpt() {
    let mut d = clicking_sends(&[FILTER_URL]);
    let out = run(&mut d, vec![click_option(), expect(None, "/api/List", 201, None, Some(300))]).await;
    assert!(!out[1].ok);
    assert_eq!(
        out[1].detail,
        "GET /hr/api/List answered 200, expected 201: the step's matching request 1 of 1, sent after action 1 (click) began"
    );
    assert_eq!(classify(&out[1].detail, None), ErrorClass::Api);
}

/// A click that sends one request that never finishes (a fetch whose body the page never reads stays
/// going in the browser's record) and then one that is answered.
fn clicking_sends_one_going_and_one_answered() -> ScriptedDriver {
    let mut d = FakePage::default().driver().with_net_record();
    for e in [
        sent("L0", "GET", "https://hr.example/hr/api/List?page=1"),
        sent("L1", "GET", "https://hr.example/hr/api/List?page=2"),
        answered("L1", 200, "application/json"),
        finished("L1"),
    ] {
        d.on_call_events.push(("Input.dispatchMouseEvent".into(), e));
    }
    d
}

#[tokio::test]
async fn an_older_request_still_going_does_not_hold_up_a_newer_one_that_has_answered() {
    let mut d = clicking_sends_one_going_and_one_answered();
    let began = std::time::Instant::now();
    let out = run(&mut d, vec![click_option(), expect(Some("GET"), "/api/List", 200, None, Some(5000))]).await;
    assert!(out[1].ok, "{:?}", out[1]);
    assert_eq!(
        out[1].detail,
        "GET /hr/api/List answered 200: the step's matching request 2 of 2, sent after action 1 (click) began"
    );
    assert!(began.elapsed() < std::time::Duration::from_millis(3000), "it waited for the older request: {:?}", began.elapsed());
}

#[tokio::test]
async fn a_check_waits_only_while_no_matching_request_has_finished() {
    let mut d = clicking_sends_one_going_and_one_answered();
    let again = expect(Some("GET"), "/api/List", 200, None, Some(100));
    let out = run(&mut d, vec![click_option(), again.clone(), again]).await;
    assert!(out[1].ok, "{:?}", out[1]);
    // The next check takes the one left, which never finished.
    assert!(!out[2].ok, "{:?}", out[2]);
    assert_eq!(
        out[2].detail,
        "GET /hr/api/List had not finished after 0.1 s: the step's matching request 1 of 2, sent after action 1 (click) began"
    );
}

#[tokio::test]
async fn a_debounced_search_that_cancels_and_resends_judges_the_resent_request() {
    // The first request is cancelled before the page resends it: a moment later the resent one is
    // answered. The check does not stop at the cancelled one.
    let mut d = browser(|| Ok(json!({})));
    on_first_look(
        &mut d,
        vec![sent("1", "GET", "https://hr.example/hr/api/Search?q=ab"), failed("1", "net::ERR_ABORTED")],
    );
    d.on_idle_events.extend([
        sent("2", "GET", "https://hr.example/hr/api/Search?q=abc"),
        answered("2", 200, "application/json"),
        finished("2"),
    ]);
    let out = check(&mut d, &expect(Some("GET"), "/api/Search", 200, None, Some(5000))).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.detail, "GET /hr/api/Search answered 200: the step's matching request 2 of 2, sent as the step began");

    // One that is never resent is still said to have been cancelled - at the end of the wait, not at once.
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, vec![sent("1", "GET", "https://hr.example/hr/api/Search?q=ab"), failed("1", "net::ERR_ABORTED")]);
    let began = std::time::Instant::now();
    let out = check(&mut d, &expect(Some("GET"), "/api/Search", 200, None, None)).await;
    assert_eq!(out.detail, at_start("GET /hr/api/Search was cancelled by the page"));
    assert!(began.elapsed() >= std::time::Duration::from_millis(100), "judged at once: {:?}", began.elapsed());
}

// --------------------------------------------------- patterns and recipes

#[test]
fn every_expect_response_failure_is_in_the_api_class() {
    for detail in [
        "no request matching \"/Save\" in 10 s (this step made 4 requests)",
        "POST /hr/Cycle/Save had not finished after 10 s",
        "POST /hr/Cycle/Save failed: net::ERR_CONNECTION_RESET",
        "POST /hr/Cycle/Save was cancelled by the page",
        "POST /hr/Cycle/Save answered 500, expected 200",
        "POST /hr/Cycle/Save answered 500, expected 200 - the response began: <p>the field is disabled</p> is disabled",
        "the response to POST /hr/Cycle/Save was not JSON - the response began: <html> not found",
        "the response to POST /hr/Cycle/Save: expected name = \"x\", got \"y\"",
        "the response body was no longer available",
        // Ending with the request the check took.
        "POST /hr/Cycle/Save was cancelled by the page: the step's matching request 2 of 3, sent after action 1 (click) began",
        "POST /hr/Cycle/Save had not finished after 10 s: the step's matching request 1 of 1, sent as the step began",
        "POST /hr/Cycle/Save answered 500, expected 200: the step's matching request 1 of 1, sent as the step began - the response began: {}",
        "no request matching \"/Save\" left in 10 s: an earlier expect_response of this step took each of the 2 that matched",
    ] {
        assert_eq!(classify(detail, None), ErrorClass::Api, "{detail}");
        assert_eq!(classify(detail, Some("/hr/Cycle/Save")), ErrorClass::Api, "{detail}");
    }
    assert_eq!(ErrorClass::Api.key(), "api");
}

fn recipe_with(steps: Value, after: Value) -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": steps,
        "after_sign_in": after,
        "signed_in": { "css": "#home" }
    }))
    .unwrap()
}

#[test]
fn a_sign_in_recipe_cannot_check_the_api() {
    let fill = json!({ "kind": "fill", "selector": { "css": "#user" }, "value": "{{username}}" });
    let checks = [
        ("expect_response", json!({ "kind": "expect_response", "url_contains": "/Account/Login" })),
        ("api_request", json!({ "kind": "api_request", "path": "/api/me" })),
        // Nor a downloaded file: that is a case's check too.
        ("expect_download", json!({ "kind": "expect_download", "name": "report.csv" })),
    ];
    for (kind, a) in checks {
        let want = format!("a sign-in recipe cannot contain {kind} - it belongs in a case script");
        let err = recipe_with(json!([fill.clone(), a.clone()]), json!([])).validate().unwrap_err();
        assert_eq!(err, format!("step 2: {want}"));
        let err = recipe_with(json!([fill.clone()]), json!([a.clone()])).validate().unwrap_err();
        assert_eq!(err, format!("after_sign_in step 1: {want}"));
        let inside = json!([{ "kind": "when_visible", "selector": { "css": "#x" }, "within_ms": 100, "then": [a] }]);
        let err = recipe_with(json!([fill.clone()]), inside).validate().unwrap_err();
        assert_eq!(err, format!("after_sign_in step 1: {want}"));
    }
}

// ------------------------------------------------------------ api_request

fn api(path: &str, query: &[(&str, &str)], status: u16, json: Option<Value>) -> Action {
    Action::ApiRequest {
        path: path.to_string(),
        query: query.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        expect: ApiExpect { status, json },
        stray: Default::default(),
    }
}

/// A page whose in-page GET answers `answer` (as `GET_FN` would return it).
fn page_answering(answer: Value) -> ScriptedDriver {
    ScriptedDriver::new(move |method, _| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": answer.clone() } })),
        _ => Ok(json!({})),
    })
}

fn got(status: u16, text: &str) -> Value {
    json!({ "status": status, "contentType": "application/json", "finalPath": "/hr/api/cycles/42?include=rules",
            "redirected": false, "text": text })
}

async fn ask(d: &mut ScriptedDriver, a: &Action) -> ActionOutcome {
    api_request(d, a, &short()).await
}

#[test]
fn encode_query_is_the_api_templates_own_encoding() {
    let q: BTreeMap<String, String> =
        [("q", "a b&c"), ("include", "rules"), ("x/y", "1=2")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(encode_query(&q), "include=rules&q=a%20b%26c&x%2Fy=1%3D2");
    assert_eq!(encode_query(&BTreeMap::new()), "");
}

#[tokio::test]
async fn an_api_request_sends_the_path_and_query_from_the_page_and_passes() {
    let mut d = page_answering(got(200, r#"{"name":"Q4 Cycle","id":42,"rules":[]}"#));
    let a = api("/hr/api/cycles/42", &[("include", "rules"), ("q", "a b")], 200, Some(json!({ "name": "Q4 Cycle" })));
    let out = ask(&mut d, &a).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.detail, "GET /hr/api/cycles/42 answered 200");
    let calls = d.calls_to("Runtime.callFunctionOn");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["objectId"], "doc");
    assert_eq!(calls[0]["functionDeclaration"], GET_FN);
    assert_eq!(calls[0]["arguments"][0]["value"], "/hr/api/cycles/42?include=rules&q=a%20b");
    assert_eq!(calls[0]["arguments"][1]["value"], 400, "the run's action timing");
    for want in ["fetch(url", "credentials: \"same-origin\"", "Accept: \"application/json\"", "signal"] {
        assert!(GET_FN.contains(want), "{want}");
    }
    assert!(!GET_FN.contains("method:"), "GET only");

    // No query: just the path.
    let mut d = page_answering(got(200, "{}"));
    assert!(ask(&mut d, &api("/hr/api/me", &[], 200, None)).await.ok);
    assert_eq!(d.calls_to("Runtime.callFunctionOn")[0]["arguments"][0]["value"], "/hr/api/me");
}

#[tokio::test]
async fn an_api_request_says_each_failure_in_its_own_sentence() {
    let path = "/hr/api/cycles/42";
    let mut d = page_answering(got(404, r#"{"error":"no such cycle"}"#));
    let out = ask(&mut d, &api(path, &[], 200, None)).await;
    assert!(!out.ok && !out.harness);
    assert_eq!(out.detail, "GET /hr/api/cycles/42 answered 404, expected 200 - the response began: {\"error\":\"no such cycle\"}");

    let mut d = page_answering(got(200, r#"{"name":"Q3 Cycle"}"#));
    let out = ask(&mut d, &api(path, &[], 200, Some(json!({ "name": "Q4 Cycle" })))).await;
    assert_eq!(
        out.detail,
        "the response to GET /hr/api/cycles/42: expected name = \"Q4 Cycle\", got \"Q3 Cycle\" - the response began: {\"name\":\"Q3 Cycle\"}"
    );

    let mut d = page_answering(got(200, "<html><title>Oops</title></html>"));
    let out = ask(&mut d, &api(path, &[], 200, Some(json!({ "name": "Q4 Cycle" })))).await;
    assert_eq!(
        out.detail,
        "the response to GET /hr/api/cycles/42 was not JSON - the response began: <html><title>Oops</title></html>"
    );

    let mut d = page_answering(json!({ "error": "timeout" }));
    let out = ask(&mut d, &api(path, &[("k", "v")], 200, None)).await;
    assert!(!out.ok && !out.harness);
    assert_eq!(out.detail, "GET /hr/api/cycles/42 failed: timeout");
}

#[tokio::test]
async fn an_api_request_redirected_to_the_sign_in_page_fails_naming_where() {
    // Review focus 3: an expired session answers 200 - with the sign-in page.
    let mut d = page_answering(json!({
        "status": 200, "contentType": "text/html", "redirected": true,
        "finalPath": "/Account/Login?ReturnUrl=%2Fhr%2Fapi%2Fcycles%2F42%3Faccess_token%3Dabc",
        "text": "<html><title>Sign in</title></html>"
    }));
    let out = ask(&mut d, &api("/hr/api/cycles/42", &[("access_token", "abc")], 200, None)).await;
    assert!(!out.ok && !out.harness);
    // The path is enough: a sign-in page echoes its ReturnUrl, query and all.
    assert_eq!(out.detail, "GET /hr/api/cycles/42 was redirected to /Account/Login");

    // A sign-in page that writes the address into its own text.
    let mut d = page_answering(json!({
        "status": 200, "contentType": "text/html", "redirected": true, "sameOrigin": true,
        "finalPath": "/Account/Login?ReturnUrl=x",
        "text": "<form action=\"/Account/Login?ReturnUrl=%2Fhr%2Fapi%3Faccess_token%3Dabc\">"
    }));
    let out = ask(&mut d, &api("/hr/api/cycles/42", &[("access_token", "abc")], 200, None)).await;
    assert_eq!(out.detail, "GET /hr/api/cycles/42 was redirected to /Account/Login");
}

#[tokio::test]
async fn an_answer_from_another_site_fails_even_on_the_same_path() {
    let mut d = page_answering(json!({
        "status": 200, "contentType": "application/json", "redirected": true, "sameOrigin": false,
        "finalPath": "/hr/api/me", "text": "{\"ok\":true}"
    }));
    let out = ask(&mut d, &api("/hr/api/me", &[], 200, None)).await;
    assert!(!out.ok && !out.harness, "{out:?}");
    assert_eq!(out.detail, "GET /hr/api/me was redirected to /hr/api/me on another site");
    assert_eq!(classify(&out.detail, None), ErrorClass::Api);
    assert!(GET_FN.contains("sameOrigin: at.origin === location.origin"));
}

#[tokio::test]
async fn an_unsafe_or_query_carrying_path_is_refused_before_anything_is_sent() {
    for p in [
        "https://evil.example/x",
        "//evil.example/x",
        "/a/../b",
        "api/x",
        "/x?access_token=abc",
        "/x#abc",
        "https://evil.example/x?access_token=abc",
        "//evil.example/x#abc",
    ] {
        let mut d = page_answering(got(200, "{}"));
        let out = ask(&mut d, &api(p, &[], 200, None)).await;
        assert!(!out.ok, "{p}");
        assert!(out.detail.starts_with("this action cannot run: "), "{}", out.detail);
        assert!(!out.detail.contains("abc") && !out.detail.contains("evil.example"), "{}", out.detail);
        assert!(d.calls.is_empty(), "{p}: {:?}", d.calls);
    }
}

#[test]
fn a_path_refusal_never_repeats_a_host() {
    let err = |p: &str| api(p, &[], 200, None).validate().unwrap_err();
    for p in ["https://evil.example/x", "https://evil.example/x?y=1", "//evil.example/x#f", "/a/../b?x=1"] {
        assert_eq!(err(p), "api_request path is not a safe path on this site - give a path such as /api/cycles/42, never an address");
    }
}

#[tokio::test]
async fn no_api_request_outcome_carries_the_query_string_or_host() {
    // Review focus 4.
    let q = [("access_token", "abc123")];
    let path = "/hr/api/cycles/42";
    let mut details = vec![];
    for (answer, json) in [
        (got(200, "{}"), None),
        (got(500, "{}"), None),
        (got(200, r#"{"ok":false}"#), Some(json!({ "ok": true }))),
        (got(200, "<html>"), Some(json!({ "ok": true }))),
        (json!({ "error": "TypeError: Failed to fetch https://hr.example/hr/api/cycles/42?access_token=abc123" }), None),
        (json!({ "error": "Failed to parse URL from /hr/api/cycles/42?access_token=abc123" }), None),
        (json!({ "status": 200, "redirected": true, "finalPath": "/Account/Login?ReturnUrl=x&access_token=abc123", "text": "" }), None),
        (json!({ "nothing": true }), None),
    ] {
        let mut d = page_answering(answer);
        details.push(ask(&mut d, &api(path, &q, 200, json)).await.detail);
    }
    assert_eq!(details[0], "GET /hr/api/cycles/42 answered 200");
    for detail in &details {
        assert!(!detail.contains("access_token") && !detail.contains("abc123") && !detail.contains("hr.example"), "{detail}");
    }
}

#[tokio::test]
async fn an_api_request_whose_browser_stops_answering_is_a_harness_failure() {
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    let out = ask(&mut d, &api("/hr/api/me", &[], 200, None)).await;
    assert!(out.harness, "{out:?}");
    assert!(out.detail.starts_with(BROWSER_SILENT), "{}", out.detail);
}

#[tokio::test]
async fn the_runner_carries_out_an_api_request() {
    let mut d = page_answering(got(200, r#"{"name":"Q4 Cycle"}"#));
    let out = run(&mut d, vec![api("/hr/api/cycles/42", &[], 200, Some(json!({ "name": "Q4 Cycle" })))]).await;
    assert_eq!(out.len(), 1);
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(out[0].detail, "GET /hr/api/cycles/42 answered 200");
}

// ------------------------------------------------- redaction and the caps

#[tokio::test]
async fn secrets_in_a_json_body_never_reach_an_api_request_outcome() {
    let body = r#"{"access_token":"abc","user":{"name":"kim","password":"p"},"Session_Id":"s1","ok":false}"#;
    for want in [
        json!({ "ok": true }),
        json!({ "access_token": "zzz" }),
        json!({ "user": { "password": "x" } }),
        json!({ "user": "kim" }),
    ] {
        let mut d = page_answering(got(200, body));
        let out = ask(&mut d, &api("/hr/api/me", &[], 200, Some(want.clone()))).await;
        assert!(!out.ok);
        for secret in ["\"abc\"", "\"p\"", "s1"] {
            assert!(!out.detail.contains(secret), "{want}: {}", out.detail);
        }
        assert!(out.detail.contains("[redacted]"), "{}", out.detail);
    }
    let mut d = page_answering(got(500, body));
    let out = ask(&mut d, &api("/hr/api/me", &[], 200, None)).await;
    assert!(out.detail.contains("\"name\":\"kim\""), "the rest still shows: {}", out.detail);
    assert!(!out.detail.contains("\"abc\"") && !out.detail.contains("\"p\""), "{}", out.detail);
}

#[tokio::test]
async fn secrets_in_a_json_body_never_reach_an_expect_response_outcome() {
    let body = r#"{"access_token":"abc","user":{"password":"p"},"success":false}"#;
    for want in [json!({ "success": true }), json!({ "user": { "password": "x" } }), json!({ "user": 1 })] {
        let mut d = browser(move || Ok(json!({ "body": body, "base64Encoded": false })));
        on_first_look(&mut d, save_finished(200));
        let out = check(&mut d, &expect(None, "/Save", 200, Some(want.clone()), None)).await;
        assert!(!out.ok);
        assert!(!out.detail.contains("\"abc\"") && !out.detail.contains("\"p\""), "{want}: {}", out.detail);
        assert!(out.detail.contains("[redacted]"), "{}", out.detail);
    }
    // A secret that matches is still a match.
    let mut d = browser(move || Ok(json!({ "body": body, "base64Encoded": false })));
    on_first_look(&mut d, save_finished(200));
    let out = check(&mut d, &expect(None, "/Save", 200, Some(json!({ "access_token": "abc" })), None)).await;
    assert!(out.ok, "{out:?}");
}

#[tokio::test]
async fn a_cut_off_json_body_still_has_its_secrets_hidden() {
    // Over 64 KB: no longer parses, so the plain-text pass hides them.
    let big = format!(r#"{{"sessionToken":"abc","pad":"{}"}}"#, "x".repeat(70_000));
    let mut d = page_answering(json!({ "status": 200, "redirected": false, "finalPath": "/hr/api/me",
        "text": big.chars().take(65_536).collect::<String>(), "over": true }));
    let out = ask(&mut d, &api("/hr/api/me", &[], 200, Some(json!({ "ok": true })))).await;
    assert!(!out.ok);
    assert!(out.detail.starts_with("the response to GET /hr/api/me was over 64 KB"), "{}", out.detail);
    assert!(!out.detail.contains("abc"), "{}", out.detail);
}

#[tokio::test]
async fn a_body_over_64_kb_says_so_for_both_kinds() {
    let big = format!(r#"{{"ok":true,"pad":"{}"}}"#, "x".repeat(70_000));
    let shown = big.clone();
    let mut d = browser(move || Ok(json!({ "body": shown.clone(), "base64Encoded": false })));
    on_first_look(&mut d, save_finished(200));
    let out = check(&mut d, &expect(None, "/Save", 200, Some(json!({ "ok": true })), None)).await;
    assert!(!out.ok);
    assert!(out.detail.starts_with("the response to POST /hr/pmsv10/PerformanceCycle/Save was over 64 KB"), "{}", out.detail);

    let mut d = page_answering(json!({ "status": 200, "redirected": false, "finalPath": "/hr/api/me",
        "text": big.chars().take(65_536).collect::<String>(), "over": true }));
    let out = ask(&mut d, &api("/hr/api/me", &[], 200, Some(json!({ "ok": true })))).await;
    assert!(out.detail.starts_with("the response to GET /hr/api/me was over 64 KB"), "{}", out.detail);
    // Without a JSON check the size does not matter.
    let mut d = page_answering(json!({ "status": 200, "redirected": false, "finalPath": "/hr/api/me",
        "text": "x", "over": true }));
    assert!(ask(&mut d, &api("/hr/api/me", &[], 200, None)).await.ok);
    assert!(GET_FN.contains("65536"));
}

#[tokio::test]
async fn a_body_that_cannot_be_decoded_says_it_could_not_be_read() {
    let mut d = browser(|| Ok(json!({ "body": "not base64 !!", "base64Encoded": true })));
    on_first_look(&mut d, save_finished(200));
    let out = check(&mut d, &expect(None, "/Save", 200, Some(json!({ "ok": true })), None)).await;
    assert!(!out.ok && !out.harness, "{out:?}");
    assert_eq!(out.detail, at_start("the response body could not be read"));
}

// ----------------------------------------------------------- validation

#[test]
fn an_api_request_path_carries_no_query_or_fragment() {
    let err = |p: &str| api(p, &[], 200, None).validate().unwrap_err();
    let want = "api_request path \"/api/x\" must not contain ? or # - put the query in \"query\"";
    assert_eq!(err("/api/x?access_token=abc"), want);
    assert_eq!(err("/api/x#frag"), want);
    assert_eq!(err("/api/x?"), want);
}

#[test]
fn url_contains_is_never_a_full_address() {
    for u in ["https://hr.example/hr/Save", "http://x/y", "HTTPS://X"] {
        assert_eq!(
            expect(None, u, 200, None, None).validate().unwrap_err(),
            "url_contains is a path fragment, not a full address",
        );
    }
    assert!(expect(None, "/hr/Save?handler=x", 200, None, None).validate().is_ok());
}

#[test]
fn the_new_sentences_are_in_the_api_class() {
    for detail in [
        "GET /hr/api/me was redirected to /Account/Login",
        "GET /hr/api/me was redirected to /Account/Login - the response began: <html>",
        "GET /hr/api/me failed: timeout",
        "the response to GET /hr/api/me was over 64 KB - the response began: {\"a\"",
        "the response body could not be read",
    ] {
        assert_eq!(classify(detail, None), ErrorClass::Api, "{detail}");
    }
}

// ------------------------------------------- redaction: no way round it

/// What each kind of check shows of `body` when its JSON check fails.
async fn both_kinds_show(body: &str, over: bool) -> Vec<String> {
    let mut shown = vec![];
    let owned = body.to_string();
    let mut d = browser(move || Ok(json!({ "body": owned.clone(), "base64Encoded": false })));
    on_first_look(&mut d, save_finished(200));
    shown.push(check(&mut d, &expect(None, "/Save", 200, Some(json!({ "ok": true })), None)).await.detail);
    let text: String = body.chars().take(65_536).collect();
    let mut d = page_answering(json!({ "status": 200, "redirected": false, "finalPath": "/hr/api/me", "text": text, "over": over }));
    shown.push(ask(&mut d, &api("/hr/api/me", &[], 200, Some(json!({ "ok": true })))).await.detail);
    shown
}

fn hides(details: &[String], secrets: &[&str]) {
    for detail in details {
        assert!(!detail.is_empty());
        for s in secrets {
            assert!(!detail.contains(s), "{s} shows in: {}", detail.chars().take(300).collect::<String>());
        }
        assert!(detail.contains("[redacted]"), "{}", detail.chars().take(300).collect::<String>());
    }
}

#[tokio::test]
async fn an_object_or_array_under_a_secret_key_is_hidden_in_a_cut_off_body() {
    let pad = "x".repeat(70_000);
    let body = format!(r#"{{"session":{{"id":"s1","inner":{{"v":"s2"}},"note":"a \"}}\" b"}},"pad":"{pad}"}}"#);
    let details = both_kinds_show(&body, true).await;
    for d in &details {
        assert!(d.contains("over 64 KB"), "{}", d.chars().take(200).collect::<String>());
        assert!(d.contains("\"pad\""), "what follows the hidden object still shows");
    }
    hides(&details, &["s1", "s2", "a \\\"}\\\" b"]);

    let body = format!(r#"{{"cookies":["a1",{{"b":"b2"}}],"pad":"{pad}"}}"#);
    hides(&both_kinds_show(&body, true).await, &["a1", "b2"]);

    // Cut off inside the secret object itself: hidden to the end.
    let body = format!(r#"{{"ok":false,"session":{{"id":"s1","pad":"{pad}"}}}}"#);
    hides(&both_kinds_show(&body, true).await, &["s1", "xxxx"]);
}

#[tokio::test]
async fn an_array_under_a_secret_key_is_hidden_in_a_whole_body() {
    let body = r#"{"cookies":["a1",{"b":"b2"}],"tokens":{"x":[1,"t3"]},"ok":false}"#;
    hides(&both_kinds_show(body, false).await, &["a1", "b2", "t3"]);
}

#[tokio::test]
async fn json_inside_a_json_string_is_hidden_too() {
    // The whole body a JSON string that holds JSON.
    let body = serde_json::to_string(r#"{"token":"abc","user":{"password":"p"}}"#).unwrap();
    hides(&both_kinds_show(&body, false).await, &["abc", "\\\"p\\\""]);

    // A field that holds JSON as text, and one that holds JSON-like text.
    let inner = serde_json::to_string(r#"{"token":"abc"}"#).unwrap();
    let body = format!(r#"{{"data":{inner},"log":"saw \"secret\": \"zz9\" then","ok":false}}"#);
    hides(&both_kinds_show(&body, false).await, &["abc", "zz9"]);

    // ...and the same cut off past 64 KB, where nothing parses.
    let body = format!(r#"{{"data":{inner},"log":"saw \"secret\": \"zz9\" then","pad":"{}"}}"#, "x".repeat(70_000));
    hides(&both_kinds_show(&body, true).await, &["abc", "zz9"]);

    // A field mismatch quoting a value that holds JSON as text.
    let body = format!(r#"{{"data":{inner}}}"#);
    let mut d = page_answering(got(200, &body));
    let out = ask(&mut d, &api("/hr/api/me", &[], 200, Some(json!({ "data": 1 })))).await;
    assert!(out.detail.starts_with("the response to GET /hr/api/me: expected data = 1, got "), "{}", out.detail);
    assert!(!out.detail.contains("abc") && out.detail.contains("[redacted]"), "{}", out.detail);
}

// ----------------------------- addresses and tokens in a body (M1 + M2)

/// Every outcome both kinds give for `body`, through each way a body is
/// quoted: a body that is not the JSON asked for (or a field mismatch),
/// and a wrong status (the excerpt).
async fn every_quote_of(body: &str) -> Vec<String> {
    let mut shown = both_kinds_show(body, false).await;
    let owned = body.to_string();
    let mut d = browser(move || Ok(json!({ "body": owned.clone(), "base64Encoded": false })));
    on_first_look(&mut d, save_finished(500));
    shown.push(check(&mut d, &expect(None, "/Save", 200, Some(json!({ "ok": true })), None)).await.detail);
    let mut d = page_answering(got(500, body));
    shown.push(ask(&mut d, &api("/hr/api/me", &[], 200, Some(json!({ "ok": true })))).await.detail);
    shown
}

fn none_of(details: &[String], leaks: &[&str]) {
    for detail in details {
        assert!(!detail.is_empty());
        for leak in leaks {
            assert!(!detail.contains(leak), "{leak} shows in: {detail}");
        }
    }
}

#[tokio::test]
async fn an_html_body_quotes_no_host_and_no_query() {
    let body = r#"<html><script src="https://cdn.example.net/lib/app.js?v=3.1"></script><form action="/Account/Login?ReturnUrl=%2Fhr%2Fapi%3Faccess_token%3Dabc" method="post"></form></html>"#;
    let details = every_quote_of(body).await;
    none_of(&details, &["cdn.example.net", "https://", "?v=", "v=3.1", "ReturnUrl", "access_token", "abc"]);
    // What is left still reads as the page: the paths survive.
    assert!(details.iter().all(|d| d.contains("/lib/app.js") && d.contains("/Account/Login")), "{details:?}");
}

#[tokio::test]
async fn a_json_body_address_loses_its_host_and_query_whole_or_in_a_mismatch() {
    let body = r#"{"ok":false,"redirectUrl":"https://login.example.com/authorize?client_id=abc123&state=xyz789"}"#;
    let details = every_quote_of(body).await;
    none_of(&details, &["login.example.com", "client_id", "abc123", "xyz789"]);
    assert!(details.iter().all(|d| d.contains("/authorize")), "{details:?}");

    // The mismatch sentence quotes the value itself.
    let mut d = page_answering(got(200, body));
    let out = ask(&mut d, &api("/hr/api/me", &[], 200, Some(json!({ "redirectUrl": "/home" })))).await;
    assert!(out.detail.starts_with("the response to GET /hr/api/me: expected redirectUrl = "), "{}", out.detail);
    none_of(&[out.detail.clone()], &["login.example.com", "client_id", "abc123", "xyz789"]);
    assert!(out.detail.contains("/authorize"), "{}", out.detail);
}

#[tokio::test]
async fn a_bare_jwt_or_bearer_token_is_redacted_wherever_it_sits() {
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhbGljZSJ9.c2lnbmF0dXJlLXZhbHVl";
    for body in [
        format!(r#"{{"ok":false,"data":"{jwt}"}}"#),
        r#"{"ok":false,"result":"Bearer abc.DEF-123_xyz"}"#.to_string(),
        format!("<html><p>Bearer {jwt}</p></html>"),
    ] {
        let details = every_quote_of(&body).await;
        none_of(&details, &[jwt, "eyJhbGci", "abc.DEF-123_xyz"]);
        assert!(details.iter().all(|d| d.contains("[redacted]")), "{details:?}");
    }
    // ...and in a field mismatch.
    let body = format!(r#"{{"data":"{jwt}","auth":"Bearer abc.DEF-123_xyz"}}"#);
    for want in [json!({ "data": "x" }), json!({ "auth": "x" })] {
        let mut d = page_answering(got(200, &body));
        let out = ask(&mut d, &api("/hr/api/me", &[], 200, Some(want))).await;
        none_of(&[out.detail.clone()], &[jwt, "eyJhbGci", "abc.DEF-123_xyz"]);
        assert!(out.detail.contains("[redacted]"), "{}", out.detail);
    }
}
