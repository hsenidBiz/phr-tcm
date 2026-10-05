//! `when_visible` in a case script: dismiss something only if it shows up
//! (a cookie banner, PeoplesHR's "Another active session" prompt). If the
//! target becomes visible within `within_ms`, its `then` actions run;
//! otherwise the step passes with "not shown, skipped". Its `then` holds
//! plain actions only, so nothing inside it counts toward the floor.

use crate::common;

use common::{quick, stateful_app, ScriptedDriver};
use serde_json::json;
use std::sync::atomic::Ordering;
use v2_lib::autorun::floor::{check_floor, Expected};
use v2_lib::autorun::nav::{check_no_addresses, no_address, save_nav, NavFile};
use v2_lib::autorun::runner::run_step;
use v2_lib::autorun::{CaseScript, StepScript};
use v2_lib::browser::actions::{execute_with, Action, NOT_SHOWN};

fn action(v: serde_json::Value) -> Action {
    serde_json::from_value(v).expect("the test wrote an action that does not parse")
}

fn refusal(v: serde_json::Value) -> String {
    action(v).validate().expect_err("this when_visible should have been refused")
}

/// The cookie banner example from the guide and the spec: the ×, never
/// Accept All, so a run records no consent.
fn cookie_banner() -> serde_json::Value {
    json!({ "kind": "when_visible", "selector": { "css": "#btnCookieClose" }, "within_ms": 4000,
            "then": [ { "kind": "click", "selector": { "css": "#btnCookieClose" } } ] })
}

fn guarded(then: serde_json::Value) -> serde_json::Value {
    json!({ "kind": "when_visible", "selector": { "css": "#banner" }, "then": then })
}

#[test]
fn the_cookie_banner_example_is_accepted_and_is_not_a_check() {
    let a = action(cookie_banner());
    assert_eq!(a.validate(), Ok(()));
    assert!(!a.is_check(), "a guarded click is a tidy-up, not an assertion");
    assert_eq!(serde_json::to_value(&a).unwrap(), cookie_banner(), "it goes back out unchanged");

    // `within_ms` is optional (2000 when left out) and is not written back.
    let session = json!({ "kind": "when_visible", "selector": { "role": "button", "name": "Continue here" },
                          "then": [ { "kind": "click", "selector": { "role": "button", "name": "Continue here" } } ] });
    let b = action(session.clone());
    assert_eq!(b.validate(), Ok(()));
    assert_eq!(serde_json::to_value(&b).unwrap(), session);
}

#[test]
fn then_holds_plain_actions_only() {
    let click = json!({ "kind": "click", "selector": { "css": "#x" } });
    assert_eq!(
        refusal(guarded(json!([guarded(json!([click.clone()]))]))),
        "when_visible \"then\" cannot hold another when_visible"
    );
    assert_eq!(
        refusal(guarded(json!([{ "kind": "sign_in", "account": "hr.admin" }]))),
        "when_visible \"then\" cannot hold sign_in - change who is signed in as an action of its own"
    );
    for check in [
        json!({ "kind": "expect_visible", "selector": { "css": "#x" } }),
        json!({ "kind": "check_text", "value": "Saved" }),
        json!({ "kind": "api_request", "path": "/api/x" }),
        json!({ "kind": "expect_response", "url_contains": "/api/x" }),
    ] {
        let kind = check["kind"].as_str().unwrap().to_string();
        assert_eq!(
            refusal(guarded(json!([check]))),
            format!("when_visible \"then\" cannot hold {kind} - a guarded step tidies up and checks nothing, so put the check after it")
        );
    }
    // The inner action's own rules still apply, and the refusal says where.
    assert_eq!(
        refusal(guarded(json!([click.clone(), { "kind": "check_url", "contains": "" }]))),
        "when_visible \"then\" cannot hold check_url - a guarded step tidies up and checks nothing, so put the check after it"
    );
    assert_eq!(
        refusal(guarded(json!([click, { "kind": "navigate", "url": "javascript:alert(1)" }]))),
        "when_visible then 2: navigate needs an http, https or file address, not \"javascript:alert(1)\""
    );
}

#[test]
fn within_ms_and_then_have_limits() {
    let click = json!([{ "kind": "click", "selector": { "css": "#banner" } }]);
    let with = |ms: u32| json!({ "kind": "when_visible", "selector": { "css": "#banner" }, "within_ms": ms, "then": click });
    assert_eq!(action(with(10_000)).validate(), Ok(()));
    assert_eq!(refusal(with(10_001)), "when_visible waits at most 10000 ms, not 10001");
    assert_eq!(refusal(with(0)), "within_ms must be more than 0");
    assert_eq!(refusal(guarded(json!([]))), "when_visible has nothing in \"then\" - say what to do when it shows up");
}

/// Nothing inside a guard counts toward the expected-result floor, even a
/// check that slipped past validation.
#[test]
fn the_floor_never_counts_a_check_inside_a_guard() {
    let sc: CaseScript = serde_json::from_value(json!({ "case_id": 1, "title": "t", "steps": [
        { "step_number": 1, "actions": [ guarded(json!([{ "kind": "expect_visible", "selector": { "css": "#x" } }])) ] }
    ] }))
    .unwrap();
    let expected = vec![Expected { step_number: 1, expected: "A toast says Saved".into(), shared: false }];
    let out = check_floor(&sc, &expected);
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].contains("checks nothing there"), "{}", out[0]);
}

/// With addresses switched off, a `navigate` hidden inside a guard is
/// refused at save the same as one written plainly.
#[test]
fn a_navigate_inside_a_guard_is_still_an_address() {
    let sc: CaseScript = serde_json::from_value(json!({ "case_id": 7, "title": "t", "steps": [
        { "step_number": 2, "actions": [ guarded(json!([{ "kind": "navigate", "url": "/hr/home" }])) ] }
    ] }))
    .unwrap();
    let off = NavFile { direct_urls: false, modules: vec![] };
    assert_eq!(check_no_addresses(&off, &[sc]).unwrap_err(), format!("case 7: {}", no_address(2)));
}

fn step(actions: Vec<Action>) -> StepScript {
    StepScript { step_number: 1, actions, unchecked: None }
}

#[tokio::test]
async fn a_target_that_shows_up_gets_its_then_actions() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, None);
    let mut acc: Option<String> = None;
    let a = action(json!({ "kind": "when_visible", "selector": { "css": "#banner" }, "within_ms": 200,
                           "then": [ { "kind": "click", "selector": { "css": "#banner" } } ] }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![a]), &quick(), &mut acc).await.unwrap();
    assert_eq!(out.len(), 1, "one outcome per action, as always");
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 1);
    assert!(out[0].detail.contains("clicked"), "the then action's own outcome is recorded: {}", out[0].detail);
}

#[tokio::test]
async fn a_target_that_never_shows_up_is_skipped_and_the_step_passes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, None);
    let mut acc: Option<String> = None;
    // `#other-session` never appears against this fake.
    let a = action(json!({ "kind": "when_visible", "selector": { "css": "#other-session" }, "within_ms": 40,
                           "then": [ { "kind": "click", "selector": { "css": "#other-session" } } ] }));
    let after = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![a, after]), &quick(), &mut acc).await.unwrap();
    assert_eq!(out.len(), 2);
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(NOT_SHOWN, "not shown, skipped");
    assert!(out[0].detail.ends_with(NOT_SHOWN), "{}", out[0].detail);
    assert!(out[0].screenshot.is_none());
    assert!(out[1].ok, "the run moves on: {:?}", out[1]);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 1, "only #save");
}

#[tokio::test]
async fn a_then_action_that_fails_fails_the_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, Some("#broken"));
    let mut acc: Option<String> = None;
    let a = action(json!({ "kind": "when_visible", "selector": { "css": "#banner" }, "within_ms": 200,
                           "then": [ { "kind": "click", "selector": { "css": "#broken" } },
                                     { "kind": "click", "selector": { "css": "#banner" } } ] }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![a]), &quick(), &mut acc).await.unwrap();
    assert_eq!(out.len(), 1);
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.contains("#broken"), "{}", out[0].detail);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 0, "the guard stops at its first failure");
}

#[tokio::test]
async fn with_addresses_switched_off_a_guarded_navigate_never_reaches_the_browser() {
    let dir = tempfile::tempdir().unwrap();
    save_nav(dir.path(), "Acme", "Web", &NavFile { direct_urls: false, modules: vec![] }).unwrap();
    let (mut d, _state) = stateful_app(false, None);
    let mut acc: Option<String> = None;
    let a = action(guarded(json!([{ "kind": "navigate", "url": "/hr/home" }])));
    let after = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![a, after]), &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok && out[0].detail.contains(&no_address(1)), "{:?}", out[0]);
    assert_eq!(out[1].detail, "not run: this step opened a page by address, which this project does not allow");
    assert!(d.calls_to("Page.navigate").is_empty());
}

/// A page whose `#late` banner shows up only `after` it was opened; every
/// other css locator is there from the start and clickable. Each click is
/// logged by the locator it was aimed at.
fn late_banner_app(after: std::time::Duration) -> (ScriptedDriver, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let opened = std::time::Instant::now();
    let clicked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = clicked.clone();
    let mut last_selector = String::new();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => json!({ "result": { "value": common::ready_probe() } }),
            "Runtime.callFunctionOn"
                if f == v2_lib::browser::locator::VISIBLE_JS || f == v2_lib::browser::input::HAS_FOCUS_JS =>
            {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last_selector = params["arguments"][0]["value"].as_str().unwrap().to_string();
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = last_selector != "#late" || opened.elapsed() >= after;
                json!({ "result": if there { vec![json!({ "name": "0", "value": { "objectId": "el" } })] } else { vec![] } })
            }
            "Runtime.callFunctionOn" => json!({ "result": { "value": "text" } }),
            "Runtime.evaluate" => json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } }),
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                log.lock().unwrap().push(last_selector.clone());
                json!({})
            }
            _ => json!({}),
        })
    });
    (d, clicked)
}

/// Review focus 1: a banner that shows up just after `within_ms` is not
/// waited for. The guard passes as "not shown, skipped", nothing clicks the
/// banner once it has appeared, and the next step runs as it would have.
#[tokio::test]
async fn a_banner_that_shows_up_after_within_ms_is_skipped_and_the_next_step_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, clicked) = late_banner_app(std::time::Duration::from_millis(150));
    let mut acc: Option<String> = None;
    let guard = action(json!({ "kind": "when_visible", "selector": { "css": "#late" }, "within_ms": 40,
                               "then": [ { "kind": "click", "selector": { "css": "#late" } } ] }));
    let first = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(first[0].ok, "{:?}", first[0]);
    assert_eq!(first[0].detail, format!("#late {NOT_SHOWN}"));

    // The banner is up by the time the next step begins.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let next = StepScript { step_number: 2, actions: vec![save], unchecked: None };
    let second = run_step(&mut d, dir.path(), "Acme", "Web", &next, &quick(), &mut acc).await.unwrap();
    assert!(second[0].ok, "the next step runs as usual: {:?}", second[0]);
    assert_eq!(*clicked.lock().unwrap(), vec!["#save".to_string()], "the late banner is never clicked");
}

/// The executor alone cannot carry one out: its `then` may hold an upload,
/// which only the runner can place.
#[tokio::test]
async fn the_executor_hands_when_visible_to_the_runner() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let out = execute_with(&mut d, &action(cookie_banner()), &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, "when_visible is carried out by the runner");
}
