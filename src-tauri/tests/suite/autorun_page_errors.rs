//! Page errors as a check: a script's `page_errors` (`fail` or `flag`) and
//! `ignore_page_errors`, judged once per step on the uncaught script errors
//! and 5xx answers every tab met since the step before. The browser is a
//! fake that emits the protocol's own events when a call is made; the
//! driver's page error book reads them as `Cdp` reads a real browser's.

use crate::common;

use common::{FakePage, ScriptedDriver};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use v2_lib::autorun::api_checks::GET_FN;
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::page_errors::{check_phrases, check_saved, EMPTY_PHRASE, LONG_PHRASE, TOO_MANY};
use v2_lib::autorun::replay::{run_selection, Browsers};
use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
use v2_lib::autorun::{store, CaseScript, LocalRun, PageErrors, StepScript};
use v2_lib::browser::actions::ActionOutcome;
use v2_lib::browser::cdp::Event;
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn step(n: i32, actions: Value) -> StepScript {
    serde_json::from_value(json!({ "step_number": n, "actions": actions })).expect("a step")
}

fn ev(method: &str, params: Value) -> Event {
    Event { method: method.to_string(), params }
}

/// An uncaught script error, as `Runtime.exceptionThrown` says it.
fn thrown(message: &str) -> Vec<Event> {
    vec![ev("Runtime.exceptionThrown", json!({ "exceptionDetails": { "text": "Uncaught", "exception": { "description": message } } }))]
}

/// A request and its answer.
fn answered(id: &str, method: &str, url: &str, status: u16) -> Vec<Event> {
    vec![
        ev("Network.requestWillBeSent", json!({ "requestId": id, "request": { "method": method, "url": url } })),
        ev("Network.responseReceived", json!({ "requestId": id, "response": { "status": status, "url": url } })),
    ]
}

const CLICK: &str = "Input.dispatchMouseEvent";

/// A page whose click makes it meet these events.
fn clicking_meets(events: Vec<Event>) -> ScriptedDriver {
    let mut d = FakePage::default().driver();
    for e in events {
        d.on_call_events.push((CLICK.to_string(), e));
    }
    d
}

fn click_then_check() -> StepScript {
    step(1, json!([{ "kind": "click", "selector": "#go" }, { "kind": "check_text", "value": "Saved" }]))
}

/// One step, run as a run runs it, in `mode` with `ignore`. Its outcomes
/// and how many a flag counted.
async fn run(d: &mut ScriptedDriver, s: &StepScript, mode: Option<PageErrors>, ignore: &[&str]) -> (Vec<ActionOutcome>, u32) {
    let dir = tempfile::tempdir().unwrap();
    let mut account = None;
    let mut held = Held::supervised();
    let mut r = InRun {
        page_errors: mode,
        ignore_page_errors: ignore.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    let out = run_step_in_run(d, dir.path(), "Acme", "Web", s, &quick(), &mut account, &mut held, None, AreaRoute::Unknown(NEEDS_SCRIPT_AREA), &mut r)
        .await
        .unwrap();
    (out, r.page_errors_seen)
}

const SCRIPT_ERROR: &str = "TypeError: cycle is undefined at https://hr.example/app.js?v=3";
const SCRIPT_SAID: &str = "the page had an error: TypeError: cycle is undefined at /app.js";
const SAVE_SAID: &str = "a request was answered 503: POST /api/Save";

fn save_failed() -> Vec<Event> {
    answered("r1", "POST", "https://hr.example/api/Save?token=hunter2", 503)
}

// ---- Each kind, in each mode ------------------------------------------------------

#[tokio::test]
async fn a_script_error_fails_the_step_in_fail_mode() {
    let mut d = clicking_meets(thrown(SCRIPT_ERROR));
    let (out, seen) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &[]).await;
    assert!(out[0].ok);
    assert!(!out[1].ok);
    assert_eq!(out[1].detail, SCRIPT_SAID);
    assert_eq!(seen, 0);
    assert!(!out[1].detail.contains("hr.example") && !out[1].detail.contains("v=3"));
}

#[tokio::test]
async fn a_5xx_fails_the_step_in_fail_mode_by_its_method_and_path() {
    let mut d = clicking_meets(save_failed());
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &[]).await;
    assert_eq!(out[1].detail, SAVE_SAID);
    assert!(!out[1].detail.contains("hunter2"));
    // A 4xx is not a page error.
    let mut d = clicking_meets(answered("r2", "GET", "https://hr.example/api/List", 404));
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &[]).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
}

#[tokio::test]
async fn in_flag_mode_the_step_is_judged_as_usual_and_each_error_is_listed() {
    let mut events = thrown(SCRIPT_ERROR);
    events.extend(save_failed());
    let mut d = clicking_meets(events);
    let (out, seen) = run(&mut d, &click_then_check(), Some(PageErrors::Flag), &[]).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(seen, 2);
    assert_eq!(out[1].detail, format!("page contains Saved (page errors: {SCRIPT_SAID}; {SAVE_SAID})"));
}

#[tokio::test]
async fn several_errors_in_one_step_name_the_first_and_count_the_rest() {
    let mut events = thrown("Error: one");
    events.extend(thrown("Error: two"));
    events.extend(save_failed());
    let mut d = clicking_meets(events);
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &[]).await;
    assert_eq!(out[1].detail, "the page had an error: Error: one (and 2 more)");
}

/// A step that already failed keeps its own failure; the errors follow it.
#[tokio::test]
async fn a_step_that_already_failed_keeps_its_failure_with_the_errors_noted() {
    let mut d = FakePage { body_has_text: false, ..FakePage::default() }.driver();
    d.on_call_events.push((CLICK.to_string(), thrown("Error: one").remove(0)));
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &[]).await;
    assert_eq!(out[1].detail, "page does NOT contain Saved (page errors: the page had an error: Error: one)");
    assert_eq!(
        v2_lib::autorun::patterns::classify(&out[1].detail, None),
        v2_lib::autorun::patterns::ErrorClass::PageTextMissing
    );
}

// ---- The run's own requests ---------------------------------------------------------

/// A page whose in-page GET (`api_request`'s fetch) is answered 500, while
/// the page's own poll to another path is answered 502 at the same time.
fn api_page() -> ScriptedDriver {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" if params["functionDeclaration"] == GET_FN => Ok(json!({ "result": { "value": {
            "status": 500, "text": "", "sameOrigin": true, "redirected": false, "finalPath": "/api/cycles/42"
        } } })),
        _ => Ok(json!({})),
    })
    .with_net_record();
    let mut events = answered("own-1", "GET", "https://hr.example/api/cycles/42?include=rules", 500);
    events.extend(answered("page-1", "GET", "https://hr.example/api/poll", 502));
    for e in events {
        d.on_call_events.push(("Runtime.callFunctionOn".to_string(), e));
    }
    d
}

#[tokio::test]
async fn an_api_requests_own_5xx_is_not_counted() {
    let mut d = api_page();
    let s = step(1, json!([{ "kind": "api_request", "path": "/api/cycles/42", "query": { "include": "rules" }, "expect": { "status": 500 } }]));
    let (out, seen) = run(&mut d, &s, Some(PageErrors::Flag), &[]).await;
    assert!(out[0].ok, "{out:?}");
    // The page's own poll is still counted; the run's fetch is not.
    assert_eq!(seen, 1);
    assert!(out[0].detail.ends_with("(page errors: a request was answered 502: GET /api/poll)"), "{}", out[0].detail);
}

// ---- Ignore phrases -------------------------------------------------------------------

#[tokio::test]
async fn ignore_phrases_pass_over_known_noise_by_message_or_path() {
    let mut events = thrown("ResizeObserver loop limit exceeded");
    events.extend(answered("t1", "POST", "https://hr.example/api/Telemetry/Send?k=1", 500));
    let mut d = clicking_meets(events.clone());
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &["resizeobserver", "TELEMETRY"]).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");

    // Only the path is looked in, never the query.
    let mut d = clicking_meets(answered("t2", "GET", "https://hr.example/api/List?source=telemetry", 500));
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &["telemetry"]).await;
    assert_eq!(out[1].detail, "a request was answered 500: GET /api/List");

    // Noise ignored, a real error still counted.
    events.extend(thrown("Error: real"));
    let mut d = clicking_meets(events);
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &["resizeobserver", "telemetry"]).await;
    assert_eq!(out[1].detail, "the page had an error: Error: real");
}

// ---- When they count ------------------------------------------------------------------

/// What the page met after one step was judged, before the next began, is
/// the next step's.
#[tokio::test]
async fn errors_between_steps_count_against_the_next_step() {
    let mut d = FakePage::default().driver();
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &[]).await;
    assert!(out.iter().all(|o| o.ok));
    // Read while nothing ran.
    for e in thrown("Error: between") {
        d.page_errors.observe(&e);
    }
    let second = step(2, json!([{ "kind": "check_text", "value": "Saved" }]));
    let (out, _) = run(&mut d, &second, Some(PageErrors::Fail), &[]).await;
    assert_eq!(out[0].detail, "the page had an error: Error: between");
    // Judged once: the step after has none.
    let (out, _) = run(&mut d, &second, Some(PageErrors::Fail), &[]).await;
    assert!(out[0].ok);
}

#[tokio::test]
async fn with_no_page_errors_set_nothing_changes() {
    let mut d = clicking_meets(thrown(SCRIPT_ERROR));
    let (out, seen) = run(&mut d, &click_then_check(), None, &[]).await;
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(out[1].detail, "page contains Saved");
    assert_eq!(seen, 0);
    // Read and let go, so a later step that does look is not handed them.
    assert!(d.page_errors.is_empty());
}

/// Page text is cut to 200 characters in every sentence.
#[tokio::test]
async fn messages_and_paths_are_cut_to_200_characters() {
    let long = "x".repeat(300);
    let mut events = thrown(&format!("Error: {long}"));
    events.extend(answered("p", "GET", &format!("https://hr.example/{long}?q=1"), 500));
    let mut d = clicking_meets(events);
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Flag), &[]).await;
    let note = out[1].detail.split(" (page errors: ").nth(1).unwrap().trim_end_matches(')');
    let (script, request) = note.split_once("; ").unwrap();
    assert_eq!(script.strip_prefix("the page had an error: ").unwrap().chars().count(), 200);
    assert_eq!(request.strip_prefix("a request was answered 500: GET ").unwrap().chars().count(), 200);
}

// ---- Watched runs -------------------------------------------------------------------------

/// A watched browser goes from case to case: what the page met before case
/// B's first step is not B's, but what it met between B's steps is.
#[tokio::test]
async fn a_watched_case_starts_with_a_clean_slate_and_counts_between_its_steps() {
    use v2_lib::autorun::runner::supervised_step_begins;
    let mut d = FakePage::default().driver();
    let mut tabs_case = Some(1);
    let feed = |d: &mut ScriptedDriver| {
        for e in answered("x", "POST", "https://hr.example/api/Save", 500) {
            d.page_errors.observe(&e);
        }
    };
    let check = |n: i32| step(n, json!([{ "kind": "check_text", "value": "Saved" }]));

    // Case A left a 500 behind; B's first step does not inherit it.
    feed(&mut d);
    supervised_step_begins(&mut d, &mut tabs_case, 2, true).await;
    let (out, _) = run(&mut d, &check(1), Some(PageErrors::Fail), &[]).await;
    assert!(out[0].ok, "{out:?}");

    // Between B's steps 1 and 2: counted against step 2.
    feed(&mut d);
    supervised_step_begins(&mut d, &mut tabs_case, 2, false).await;
    let (out, _) = run(&mut d, &check(2), Some(PageErrors::Fail), &[]).await;
    assert_eq!(out[0].detail, "a request was answered 500: POST /api/Save");

    // Another case, even started on a later step, starts clean too.
    feed(&mut d);
    supervised_step_begins(&mut d, &mut tabs_case, 3, false).await;
    let (out, _) = run(&mut d, &check(4), Some(PageErrors::Fail), &[]).await;
    assert!(out[0].ok, "{out:?}");

    // And running a case's first step again starts it clean as well.
    feed(&mut d);
    supervised_step_begins(&mut d, &mut tabs_case, 3, true).await;
    let (out, _) = run(&mut d, &check(1), Some(PageErrors::Fail), &[]).await;
    assert!(out[0].ok, "{out:?}");
}

// ---- The smaller rules ------------------------------------------------------------------------

/// A bare path in a message loses its query, as a whole address does.
#[test]
fn a_path_in_a_message_loses_its_query() {
    use v2_lib::browser::page_errors::scrubbed;
    assert_eq!(scrubbed("Failed to fetch /api/x?token=abc"), "Failed to fetch /api/x");
    assert_eq!(scrubbed("see api/x?token=abc#frag"), "see api/x");
    assert_eq!(scrubbed("at https://hr.example/app.js?v=3"), "at /app.js");
    assert_eq!(scrubbed("a/b and c?d stay"), "a/b and c?d stay");
}

#[tokio::test]
async fn a_path_query_never_reaches_the_sentence() {
    let mut d = clicking_meets(thrown("TypeError: Failed to fetch /api/x?token=abc"));
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &[]).await;
    assert_eq!(out[1].detail, "the page had an error: TypeError: Failed to fetch /api/x");
}

/// The outcome a page error failed is pictured, as a failed action is.
#[tokio::test]
async fn a_step_failed_by_page_errors_is_pictured() {
    let fake = FakePage::default();
    let mut d = ScriptedDriver::new(move |method, params| match method {
        "Page.captureScreenshot" => Ok(json!({ "data": "/9j/4AAQ" })),
        _ => fake.answer(method, params),
    });
    d.on_call_events.push((CLICK.to_string(), thrown("Error: one").remove(0)));
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &[]).await;
    assert!(!out[1].ok);
    assert!(out[1].screenshot.is_some(), "{out:?}");
    assert!(out[0].screenshot.is_none());
}

/// A step with no actions still fails: it is given an outcome to say so.
#[tokio::test]
async fn a_step_with_no_actions_still_fails_in_fail_mode() {
    let mut d = FakePage::default().driver();
    for e in thrown("Error: lonely") {
        d.page_errors.observe(&e);
    }
    let (out, _) = run(&mut d, &step(1, json!([])), Some(PageErrors::Fail), &[]).await;
    assert_eq!(out.len(), 1);
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, "the page had an error: Error: lonely");
    // In flag mode there is nothing to list it on, and nothing fails.
    for e in thrown("Error: lonely") {
        d.page_errors.observe(&e);
    }
    let (out, seen) = run(&mut d, &step(1, json!([])), Some(PageErrors::Flag), &[]).await;
    assert!(out.is_empty());
    assert_eq!(seen, 1);
}

/// The ignore phrases apply first: what they cover is not in the count.
#[tokio::test]
async fn and_n_more_counts_only_what_the_phrases_leave() {
    let mut events = thrown("Error: real");
    events.extend(thrown("ResizeObserver loop limit exceeded"));
    events.extend(thrown("Error: also real"));
    let mut d = clicking_meets(events);
    let (out, _) = run(&mut d, &click_then_check(), Some(PageErrors::Fail), &["resizeobserver"]).await;
    assert_eq!(out[1].detail, "the page had an error: Error: real (and 1 more)");
}

// ---- What a script may say --------------------------------------------------------------

#[test]
fn ignore_phrases_are_refused_where_the_script_is_saved() {
    let phrases = |n: usize, text: &str| (0..n).map(|_| text.to_string()).collect::<Vec<_>>();
    assert_eq!(check_phrases(&phrases(10, "noise")), Ok(()));
    assert_eq!(check_phrases(&phrases(11, "noise")), Err(TOO_MANY.to_string()));
    assert_eq!(TOO_MANY, "ignore_page_errors holds at most 10 phrases");
    assert_eq!(check_phrases(&["a".into(), " ".into()]), Err(EMPTY_PHRASE.to_string()));
    assert_eq!(EMPTY_PHRASE, "ignore_page_errors: a phrase cannot be empty");
    assert_eq!(check_phrases(&["y".repeat(120)]), Ok(()));
    assert_eq!(check_phrases(&["y".repeat(121)]), Err(LONG_PHRASE.to_string()));
    let script: CaseScript =
        serde_json::from_value(json!({ "case_id": 5, "title": "t", "steps": [], "ignore_page_errors": [""] })).unwrap();
    assert_eq!(check_saved(&[script]), Err(format!("case 5: {EMPTY_PHRASE}")));
}

#[test]
fn the_new_fields_round_trip_and_are_written_only_when_set() {
    let old = r#"{"case_id":1,"title":"t","steps":[]}"#;
    let script: CaseScript = serde_json::from_str(old).unwrap();
    assert_eq!(script.page_errors, None);
    assert_eq!(serde_json::to_string(&script).unwrap(), old);
    let set = r#"{"case_id":1,"title":"t","steps":[],"page_errors":"flag","ignore_page_errors":["ResizeObserver"]}"#;
    let script: CaseScript = serde_json::from_str(set).unwrap();
    assert_eq!(script.page_errors, Some(PageErrors::Flag));
    assert_eq!(serde_json::to_string(&script).unwrap(), set);
    let fail: CaseScript = serde_json::from_str(r#"{"case_id":1,"title":"t","steps":[],"page_errors":"fail"}"#).unwrap();
    assert_eq!(fail.page_errors, Some(PageErrors::Fail));
    assert!(serde_json::from_str::<CaseScript>(r#"{"case_id":1,"title":"t","steps":[],"page_errors":"warn"}"#).is_err());
}

// ---- A whole run ---------------------------------------------------------------------

struct One(Option<ScriptedDriver>);

impl Browsers for One {
    type D = ScriptedDriver;
    async fn open(&mut self) -> Result<Self::D, String> {
        self.0.take().ok_or_else(|| "no browser".to_string())
    }
    async fn close(&mut self, _d: Self::D) {}
}

async fn run_case(root: &Path, script: &CaseScript, d: ScriptedDriver) -> LocalRun {
    store::save_script(root, script).unwrap();
    let mut run = LocalRun {
        id: "run-p".into(),
        pbi_id: 42,
        started_at: "1700000000000".into(),
        cases: vec![],
        mode: "unattended".into(),
        published: None,
        environment: None,
        resets: vec![],
    };
    let cancel = AtomicBool::new(false);
    run_selection(&mut One(Some(d)), root, "Acme", "Web", &mut run, &[(script.case_id, script.title.clone())], &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    run
}

fn case(mode: &str) -> CaseScript {
    serde_json::from_value(json!({
        "case_id": 1, "title": "c", "page_errors": mode,
        "steps": [{ "step_number": 1, "actions": [{ "kind": "click", "selector": "#go" }, { "kind": "check_text", "value": "Saved" }] }]
    }))
    .unwrap()
}

#[tokio::test]
async fn a_flagged_case_carries_its_count_and_a_failing_one_its_sentence() {
    let mut events = thrown(SCRIPT_ERROR);
    events.extend(save_failed());
    let dir = tempfile::tempdir().unwrap();
    let run = run_case(dir.path(), &case("flag"), clicking_meets(events.clone())).await;
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
    assert_eq!(rec.page_errors_seen, 2);
    let saved = serde_json::to_value(rec).unwrap();
    assert_eq!(saved["page_errors_seen"], 2);
    let back: v2_lib::autorun::CaseRecord = serde_json::from_value(saved).unwrap();
    assert_eq!(back.page_errors_seen, 2);

    let dir = tempfile::tempdir().unwrap();
    let run = run_case(dir.path(), &case("fail"), clicking_meets(events)).await;
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Failed");
    assert_eq!(rec.reason, format!("step 1: {SCRIPT_SAID} (and 1 more)"));
    assert_eq!(rec.page_errors_seen, 0);
    assert!(serde_json::to_value(rec).unwrap().get("page_errors_seen").is_none());
}

/// What the page met before step 1 is no step's error.
#[tokio::test]
async fn errors_before_step_1_are_dropped() {
    let mut d = FakePage::default().driver();
    for e in thrown("Error: while loading") {
        d.page_errors.observe(&e);
    }
    let dir = tempfile::tempdir().unwrap();
    let run = run_case(dir.path(), &case("fail"), d).await;
    assert_eq!(run.cases[0].proposed, "Passed", "{}", run.cases[0].reason);
}

/// The report carries the same badge the review shows, and only for a case
/// that met some.
#[tokio::test]
async fn the_report_shows_the_count_as_a_badge() {
    let mut events = thrown(SCRIPT_ERROR);
    events.extend(save_failed());
    let dir = tempfile::tempdir().unwrap();
    let run = run_case(dir.path(), &case("flag"), clicking_meets(events)).await;
    let html = v2_lib::autorun::report::build_with_downloads(&run, &[], "2 Oct 2026", &|_| false, &|_| None);
    assert!(html.contains("page errors seen: 2"), "{html}");
    assert!(!html.contains("hunter2"), "the query never reaches the report");

    let dir = tempfile::tempdir().unwrap();
    let run = run_case(dir.path(), &case("flag"), FakePage::default().driver()).await;
    let html = v2_lib::autorun::report::build_with_downloads(&run, &[], "2 Oct 2026", &|_| false, &|_| None);
    assert!(!html.contains("page errors seen"));
}
