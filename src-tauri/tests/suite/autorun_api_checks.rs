//! `expect_response`: the step checks a request the page itself made while
//! the step ran - found in the network record after the step's mark,
//! judged on its status and (optionally) a partial JSON match, and
//! reported in sentences that never carry a query string, a host, a header
//! or a token.

use crate::common::{quick, ScriptedDriver};
use serde_json::{json, Value};
use v2_lib::autorun::api_checks::{expect_response, judge, pick, Pick};
use v2_lib::autorun::patterns::{classify, ErrorClass};
use v2_lib::autorun::recipe::SignInRecipe;
use v2_lib::autorun::runner::run_step;
use v2_lib::autorun::StepScript;
use v2_lib::browser::actions::{Action, ActionOutcome, BROWSER_SILENT};
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
    }
}

fn expect(method: Option<&str>, url_contains: &str, status: u16, json: Option<Value>, timeout_ms: Option<u32>) -> Action {
    Action::ExpectResponse {
        method: method.map(str::to_string),
        url_contains: url_contains.to_string(),
        status,
        json,
        timeout_ms,
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

async fn check(d: &mut ScriptedDriver, a: &Action) -> ActionOutcome {
    let mark = d.net_mark();
    let out = expect_response(d, a, mark, &short()).await;
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
    assert_eq!(out.detail, "POST /hr/pmsv10/PerformanceCycle/Save answered 200");
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
    assert_eq!(out.detail, "no request matching \"/Save\" in 0.3 s (this step made 1 requests)");
    assert!(!out.harness);
}

#[tokio::test]
async fn a_matching_request_that_never_finishes_says_it_had_not_finished() {
    // Review focus 1: a hung save is not "no request".
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, vec![sent("7", "POST", "https://hr.example/hr/Cycle/Save?id=1"), answered("7", 200, "application/json")]);
    let out = check(&mut d, &expect(None, "/Save", 200, None, Some(200))).await;
    assert!(!out.ok);
    assert_eq!(out.detail, "POST /hr/Cycle/Save had not finished after 0.2 s");
}

#[tokio::test]
async fn a_failed_and_a_cancelled_request_each_have_their_sentence() {
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, vec![sent("7", "POST", "https://hr.example/hr/Cycle/Save"), failed("7", "net::ERR_CONNECTION_RESET")]);
    let out = check(&mut d, &expect(None, "/Save", 200, None, None)).await;
    assert_eq!(out.detail, "POST /hr/Cycle/Save failed: net::ERR_CONNECTION_RESET");

    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, vec![sent("7", "POST", "https://hr.example/hr/Cycle/Save"), failed("7", "net::ERR_ABORTED")]);
    let out = check(&mut d, &expect(None, "/Save", 200, None, None)).await;
    assert_eq!(out.detail, "POST /hr/Cycle/Save was cancelled by the page");
}

#[tokio::test]
async fn a_wrong_status_names_both_and_shows_the_body_when_one_was_read() {
    let mut d = browser(|| Ok(json!({})));
    on_first_look(&mut d, save_finished(500));
    let out = check(&mut d, &expect(Some("POST"), "/Save", 200, None, None)).await;
    assert_eq!(out.detail, "POST /hr/pmsv10/PerformanceCycle/Save answered 500, expected 200");

    let mut d = browser(|| Ok(json!({ "body": "{\"error\":\"boom\"}", "base64Encoded": false })));
    on_first_look(&mut d, save_finished(500));
    let out = check(&mut d, &expect(Some("POST"), "/Save", 200, Some(json!({ "success": true })), None)).await;
    assert_eq!(
        out.detail,
        "POST /hr/pmsv10/PerformanceCycle/Save answered 500, expected 200 - the response began: {\"error\":\"boom\"}"
    );
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
    assert_eq!(out.detail, "the response body was no longer available");

    let mut d = browser(|| Ok(json!({ "body": "<html>\n  <title>Sign in</title>\n</html>", "base64Encoded": false })));
    on_first_look(&mut d, save_finished(200));
    let out = check(&mut d, &expect(None, "/Save", 200, Some(json!({ "success": true })), None)).await;
    assert!(!out.ok);
    assert_eq!(
        out.detail,
        "the response to POST /hr/pmsv10/PerformanceCycle/Save was not JSON - the response began: <html> <title>Sign in</title> </html>"
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
    assert_eq!(details[0], "POST /hr/Cycle/Save answered 200");
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
    let out = expect_response(&mut d, &expect(None, "  ", 200, None, None), 0, &short()).await;
    assert!(!out.ok);
    assert!(out.detail.starts_with("this action cannot run: "), "{}", out.detail);
    assert!(d.calls.is_empty());
}

// -------------------------------------------------------------- the runner

fn step(actions: Vec<Action>) -> StepScript {
    StepScript { step_number: 1, actions, unchecked: None }
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
    assert_eq!(out[1].detail, "GET /hr/api/cycles answered 200");
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
    assert_eq!(out[0].detail, "POST /hr/pmsv10/PerformanceCycle/Save answered 200");
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
