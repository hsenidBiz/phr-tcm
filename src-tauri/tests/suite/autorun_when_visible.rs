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
use v2_lib::browser::actions::{execute_with, Action, NOT_SHOWN, SHOWN_SKIPPED};

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
    // One guard may sit inside another (see `a_guard_inside_a_guard_is_one_level_only`),
    // but it holds plain actions too.
    assert_eq!(
        refusal(guarded(json!([guarded(json!([{ "kind": "expect_visible", "selector": { "css": "#x" } }]))]))),
        "when_visible then 1: when_visible \"then\" cannot hold expect_visible - a guarded step tidies up and checks nothing, so put the check after it"
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
        json!({ "kind": "expect_download", "name": "report.csv" }),
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
    let off = NavFile { direct_urls: false, modules: vec![], save_words: vec![], protected: vec![] };
    assert_eq!(check_no_addresses(&off, &[sc]).unwrap_err(), format!("case 7: {}", no_address(2)));
}

fn step(actions: Vec<Action>) -> StepScript {
    StepScript { step_number: 1, actions, unchecked: None, cleanup: false, because: Vec::new() }
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

/// A browser that sets `cancel` once `after` clicks have landed.
struct StopsAfterClicks {
    inner: ScriptedDriver,
    clicks: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    after: usize,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl v2_lib::browser::cdp::Driver for StopsAfterClicks {
    async fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, v2_lib::browser::cdp::CdpError> {
        let answer = self.inner.call(method, params).await;
        if self.clicks.load(Ordering::SeqCst) >= self.after {
            self.cancel.store(true, Ordering::SeqCst);
        }
        answer
    }
    async fn wait_event(
        &mut self,
        method: &str,
        limit: std::time::Duration,
    ) -> Result<v2_lib::browser::cdp::Event, v2_lib::browser::cdp::CdpError> {
        self.inner.wait_event(method, limit).await
    }
    fn forget_events(&mut self) {
        self.inner.forget_events();
    }
    fn take_dialogs(&mut self) -> Vec<String> {
        self.inner.take_dialogs()
    }
    fn set_deadline(&mut self, deadline: Option<std::time::Instant>) {
        self.inner.set_deadline(deadline);
    }
}

/// A replay's stop asked while a guard's `then` runs is heard before the
/// guard's next action, as between a step's own actions: the guard ends
/// as stopped - cut between two clicks, not inside a wait, so the replay
/// says it stopped after that action (`stopped_inside` stays false). An
/// unattended run (no stop before each action) runs the guard to its end,
/// as it always has.
#[tokio::test]
async fn a_replays_stop_is_heard_between_a_guards_then_actions() {
    use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, StepProgress, AFTER_STOP, NEEDS_SCRIPT_AREA};
    use v2_lib::autorun::lease::Held;
    let guard = || {
        action(json!({ "kind": "when_visible", "selector": { "css": "#banner" }, "within_ms": 200,
                       "then": [ { "kind": "click", "selector": { "css": "#banner" } },
                                 { "kind": "click", "selector": { "css": "#banner" } } ] }))
    };
    for (replay, clicks) in [(true, 1), (false, 2)] {
        let dir = tempfile::tempdir().unwrap();
        let (inner, state) = stateful_app(false, None);
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut d = StopsAfterClicks { inner, clicks: state.clicks.clone(), after: 1, cancel: cancel.clone() };
        let (mut acc, mut held) = (None, Held::supervised());
        let progress = StepProgress::new();
        let mut run = InRun { cancel: Some(&cancel), stop_before_each_action: replay, progress: Some(&progress), ..Default::default() };
        let out = run_step_in_run(&mut d, dir.path(), "Acme", "Web", &step(vec![guard()]), &quick(), &mut acc, &mut held, None, AreaRoute::Unknown(NEEDS_SCRIPT_AREA), &mut run)
            .await
            .unwrap();
        assert_eq!(state.clicks.load(Ordering::SeqCst), clicks, "replay: {replay}");
        if replay {
            assert_eq!(out[0].detail, AFTER_STOP, "{:?}", out[0]);
            let now = progress.now();
            assert!(!now.stopped_inside, "a stop between the guard's clicks was said as inside its wait");
            assert_eq!(now.failed, 0, "a stop is no failure");
            assert_eq!(
                v2_lib::autorun::replay_to::stopped_on_request(77, 1, now.index, now.stopped_inside, None),
                "the replay of case 77 was stopped on request after action 1 of step 1"
            );
        } else {
            assert!(out[0].ok, "{:?}", out[0]);
        }
    }
}

#[tokio::test]
async fn with_addresses_switched_off_a_guarded_navigate_never_reaches_the_browser() {
    let dir = tempfile::tempdir().unwrap();
    save_nav(dir.path(), "Acme", "Web", &NavFile { direct_urls: false, modules: vec![], save_words: vec![], protected: vec![] }).unwrap();
    let (mut d, _state) = stateful_app(false, None);
    let mut acc: Option<String> = None;
    let a = action(guarded(json!([{ "kind": "navigate", "url": "/hr/home" }])));
    let after = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![a, after]), &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok && out[0].detail.contains(&no_address(1)), "{:?}", out[0]);
    assert_eq!(out[1].detail, "not run: this step opened a page by address, which this project does not allow");
    assert!(d.calls_to("Page.navigate").is_empty());
}

/// A page whose `#late` banner shows up only once something OTHER than it
/// has been looked for - that is, just after a guard on it gave up, however
/// many looks the guard took. No clock: the look count decides, the way
/// `common::stalling_menu_app` lets its click count decide. Every other css
/// locator is there from the start and clickable. Each click is logged by
/// the locator it was aimed at.
fn late_banner_app() -> (ScriptedDriver, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let mut shown = false;
    let clicked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = clicked.clone();
    let mut last_selector = String::new();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => json!({ "result": { "value": common::ready_probe() } }),
            "Runtime.callFunctionOn"
                if f == v2_lib::browser::locator::VISIBLE_JS || f == v2_lib::browser::input::SHOWN_JS || f == v2_lib::browser::input::HAS_FOCUS_JS =>
            {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last_selector = params["arguments"][0]["value"].as_str().unwrap().to_string();
                shown |= last_selector != "#late";
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = last_selector != "#late" || shown;
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
    let (mut d, clicked) = late_banner_app();
    let mut acc: Option<String> = None;
    let guard = action(json!({ "kind": "when_visible", "selector": { "css": "#late" }, "within_ms": 40,
                               "then": [ { "kind": "click", "selector": { "css": "#late" } } ] }));
    let first = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(first[0].ok, "{:?}", first[0]);
    assert_eq!(first[0].detail, format!("#late {NOT_SHOWN}"));

    // The banner shows up as the next step begins: its first look is for
    // `#save`. The step clicks what it names, and the banner is really up
    // by then - the check after the click sees it.
    let save = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let banner_up = action(json!({ "kind": "expect_visible", "selector": { "css": "#late" } }));
    let next = StepScript { step_number: 2, actions: vec![save, banner_up], unchecked: None, cleanup: false, because: Vec::new() };
    let second = run_step(&mut d, dir.path(), "Acme", "Web", &next, &quick(), &mut acc).await.unwrap();
    assert!(second[0].ok, "the next step runs as usual: {:?}", second[0]);
    assert!(second[1].ok, "the banner did show up late: {:?}", second[1]);
    assert_eq!(*clicked.lock().unwrap(), vec!["#save".to_string()], "the late banner is never clicked");
}

/// A page where `.pair` matches two visible elements (a dialog's OK and
/// Cancel) and every other css locator matches one. Each click is logged by
/// the locator it was aimed at.
fn two_buttons_app() -> (ScriptedDriver, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let clicked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = clicked.clone();
    let mut last_selector = String::new();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => json!({ "result": { "value": common::ready_probe() } }),
            "Runtime.callFunctionOn"
                if f == v2_lib::browser::locator::VISIBLE_JS || f == v2_lib::browser::input::SHOWN_JS || f == v2_lib::browser::input::HAS_FOCUS_JS =>
            {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last_selector = params["arguments"][0]["value"].as_str().unwrap().to_string();
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let one = |i: u32| json!({ "name": i.to_string(), "value": { "objectId": format!("el{i}") } });
                let found = if last_selector == ".pair" { vec![one(0), one(1)] } else { vec![one(0)] };
                json!({ "result": found })
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

/// Two visible matches are visible by any reading: the guard used to wait
/// out its whole `within_ms` and report "not shown, skipped" while the
/// dialog sat on the screen.
#[tokio::test]
async fn a_guard_whose_locator_matches_two_visible_elements_counts_as_shown_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, clicked) = two_buttons_app();
    let mut acc: Option<String> = None;
    let guard = action(json!({ "kind": "when_visible", "selector": { "css": ".pair" }, "within_ms": 5000,
                               "then": [ { "kind": "click", "selector": { "css": "#close" } } ] }));
    let started = std::time::Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(!out[0].detail.contains(NOT_SHOWN), "{}", out[0].detail);
    assert!(out[0].detail.starts_with(".pair showed: "), "{}", out[0].detail);
    assert_eq!(*clicked.lock().unwrap(), vec!["#close".to_string()]);
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "waited {:?}", started.elapsed());
}

/// The person reads the real problem - an ambiguous locator - in the
/// guarded click's own sentence, not a timing story.
#[tokio::test]
async fn a_guarded_click_on_an_ambiguous_locator_fails_with_matched() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, clicked) = two_buttons_app();
    let mut acc: Option<String> = None;
    let guard = action(json!({ "kind": "when_visible", "selector": { "css": ".pair" }, "within_ms": 5000,
                               "then": [ { "kind": "click", "selector": { "css": ".pair" } } ] }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.starts_with(".pair showed: "), "{}", out[0].detail);
    assert!(out[0].detail.contains("matched 2"), "{}", out[0].detail);
    assert!(clicked.lock().unwrap().is_empty());
}

/// A `within_ms` shorter than one answer from a slow page used to end
/// before any look had completed, which reads as the browser having gone
/// silent - a harness failure, and a retried case - for a page that was
/// merely slow and had no banner at all.
#[tokio::test]
async fn a_tiny_within_ms_on_a_slow_page_is_not_a_harness_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = true;
    let mut d = ScriptedDriver::new(move |method, params| {
        if method == "Runtime.evaluate" && params["expression"] == "document" {
            if std::mem::take(&mut first) {
                // The page's first answer did not come within its call's time.
                std::thread::sleep(std::time::Duration::from_millis(30));
                return Err(v2_lib::browser::cdp::CdpError::Timeout { what: method.to_string(), ms: 250 });
            }
            return Ok(json!({ "result": { "objectId": "doc" } }));
        }
        Ok(match method {
            "Runtime.callFunctionOn" => json!({ "result": { "objectId": "arr" } }),
            "Runtime.getProperties" => json!({ "result": [] }),
            "Runtime.evaluate" => json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } }),
            _ => json!({}),
        })
    });
    let mut acc: Option<String> = None;
    let guard = action(json!({ "kind": "when_visible", "selector": { "css": "#banner" }, "within_ms": 1,
                               "then": [ { "kind": "click", "selector": { "css": "#banner" } } ] }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(!out[0].harness, "{:?}", out[0]);
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(out[0].detail, format!("#banner {NOT_SHOWN}"));
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

/// A page where `#gone` is in the page and drawn, but outside the window
/// even once scrolled to (a closed cookie banner moved off the page): the
/// click's own probe says `onscreen: false`, as `SHOWN_JS` does, though the
/// plain visibility rule still says it is drawn. `#here` is in view. Every
/// other css locator is there and clickable. Each click is logged by the
/// locator it was aimed at.
fn off_the_page_app() -> (ScriptedDriver, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let clicked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = clicked.clone();
    let mut last_selector = String::new();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let off = last_selector == "#gone";
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => {
                let mut probe = common::ready_probe();
                if off {
                    probe["onscreen"] = json!(false);
                    probe["hit"] = json!(false);
                }
                json!({ "result": { "value": probe } })
            }
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::SHOWN_JS => json!({ "result": { "value": !off } }),
            "Runtime.callFunctionOn"
                if f == v2_lib::browser::locator::VISIBLE_JS || f == v2_lib::browser::input::HAS_FOCUS_JS =>
            {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last_selector = params["arguments"][0]["value"].as_str().unwrap().to_string();
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => json!({ "result": [ { "name": "0", "value": { "objectId": "el" } } ] }),
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

/// Tool errors #71: a guard on a close button left in the page, outside
/// the visible part of it, said "showed" and its click then waited 15 s
/// for the button and failed the step. Shown is what a click needs, so
/// the guard is skipped and the step carries on.
#[tokio::test]
async fn an_element_outside_the_visible_page_is_not_shown_and_the_step_carries_on() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, clicked) = off_the_page_app();
    let mut acc: Option<String> = None;
    let guard = action(json!({ "kind": "when_visible", "selector": { "css": "#gone" }, "within_ms": 40,
                               "then": [ { "kind": "click", "selector": { "css": "#gone" } } ] }));
    let after = action(json!({ "kind": "click", "selector": { "css": "#save" } }));
    let started = std::time::Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard, after]), &quick(), &mut acc).await.unwrap();
    assert_eq!(out.len(), 2);
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(out[0].detail, format!("#gone {NOT_SHOWN}"));
    assert!(out[1].ok, "the step carries on: {:?}", out[1]);
    assert_eq!(*clicked.lock().unwrap(), vec!["#save".to_string()], "the guarded click never ran");
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "waited {:?}", started.elapsed());
}

/// The same page: an element in view is shown, and its `then` runs.
#[tokio::test]
async fn an_element_in_view_is_shown_and_its_then_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, clicked) = off_the_page_app();
    let mut acc: Option<String> = None;
    let guard = action(json!({ "kind": "when_visible", "selector": { "css": "#here" }, "within_ms": 40,
                               "then": [ { "kind": "click", "selector": { "css": "#here" } } ] }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.starts_with("#here showed: clicked"), "{}", out[0].detail);
    assert_eq!(*clicked.lock().unwrap(), vec!["#here".to_string()]);
}

/// "Shown" is the click's own rule, carried whole: `SHOWN_JS` holds the
/// probe a click waits on, and answers by its `visible` and `onscreen`.
#[test]
fn shown_is_the_clicks_own_visible_and_onscreen() {
    use v2_lib::browser::input::{PROBE_JS, SHOWN_JS};
    assert!(SHOWN_JS.contains(PROBE_JS), "SHOWN_JS must carry PROBE_JS itself, not a copy of its geometry");
    assert!(SHOWN_JS.contains("p.visible && p.onscreen"), "{SHOWN_JS}");
}

/// Only a script's guard asks the click's rule. The runner's own one-look
/// checks (a pop-up before a click, a loader, a sign-in prompt) and the
/// sign-in recipe's prompt keep the plain visibility rule, which never
/// scrolls: a loader wait that scrolled a far-off spinner into view on
/// every look would move the page under the script.
#[tokio::test]
async fn the_runners_own_looks_never_ask_the_scrolling_rule() {
    use v2_lib::browser::actions::{shown_now, shows_up};
    let (mut d, _clicked) = off_the_page_app();
    assert_eq!(shown_now(&mut d, &serde_json::from_value(json!({ "css": "#gone" })).unwrap()).await.unwrap(), Some(true));
    let target = serde_json::from_value(json!({ "css": "#gone" })).unwrap();
    assert!(shows_up(&mut d, &target, 40, &quick()).await.unwrap());
    let asked_shown = d
        .calls_to("Runtime.callFunctionOn")
        .iter()
        .filter(|p| p["functionDeclaration"] == v2_lib::browser::input::SHOWN_JS)
        .count();
    assert_eq!(asked_shown, 0, "only a script's when_visible guard asks SHOWN_JS");
}

// ---------------------------------------------------------------------
// `when_hidden`, and one guard inside a guard (backlog 72)
// ---------------------------------------------------------------------

fn hidden(selector: &str, then: serde_json::Value) -> serde_json::Value {
    json!({ "kind": "when_hidden", "selector": { "css": selector }, "within_ms": 100, "then": then })
}

fn click_json(selector: &str) -> serde_json::Value {
    json!({ "kind": "click", "selector": { "css": selector } })
}

#[test]
fn when_hidden_is_accepted_is_no_check_and_goes_back_out_unchanged() {
    let w = json!({ "kind": "when_hidden", "selector": { "css": "#financial" }, "within_ms": 3000,
                    "then": [ click_json("#apply") ] });
    let a = action(w.clone());
    assert_eq!(a.kind(), "when_hidden");
    assert_eq!(a.validate(), Ok(()));
    assert!(!a.is_check(), "a guard is a tidy-up, not an assertion");
    assert_eq!(serde_json::to_value(&a).unwrap(), w);
}

#[test]
fn when_hidden_has_the_same_limits_and_then_rules() {
    let with = |ms: u32| json!({ "kind": "when_hidden", "selector": { "css": "#b" }, "within_ms": ms, "then": [ click_json("#b") ] });
    assert_eq!(action(with(10_000)).validate(), Ok(()));
    assert_eq!(refusal(with(10_001)), "when_hidden waits at most 10000 ms, not 10001");
    assert_eq!(refusal(with(0)), "within_ms must be more than 0");
    let bare = |then: serde_json::Value| json!({ "kind": "when_hidden", "selector": { "css": "#b" }, "then": then });
    assert_eq!(
        refusal(bare(json!([]))),
        "when_hidden has nothing in \"then\" - say what to do when it is not on the page"
    );
    assert_eq!(
        refusal(bare(json!([{ "kind": "expect_visible", "selector": { "css": "#x" } }]))),
        "when_hidden \"then\" cannot hold expect_visible - a guarded step tidies up and checks nothing, so put the check after it"
    );
    assert_eq!(
        refusal(bare(json!([{ "kind": "sign_in", "account": "hr.admin" }]))),
        "when_hidden \"then\" cannot hold sign_in - change who is signed in as an action of its own"
    );
    assert_eq!(
        refusal(bare(json!([{ "kind": "click", "selector": { "css": "#x" } }, { "kind": "navigate", "url": "javascript:alert(1)" }]))),
        "when_hidden then 2: navigate needs an http, https or file address, not \"javascript:alert(1)\""
    );
}

const ONE_LEVEL: &str = "a guard inside a guard's then cannot hold another guard - one level of nesting";

#[test]
fn a_guard_inside_a_guard_is_one_level_only() {
    let inner_visible = guarded(json!([click_json("#x")]));
    let inner_hidden = hidden("#gone", json!([click_json("#x")]));
    // Either kind inside either kind validates.
    for outer in [
        guarded(json!([inner_hidden.clone()])),
        guarded(json!([click_json("#a"), inner_visible.clone()])),
        hidden("#o", json!([inner_visible.clone()])),
        hidden("#o", json!([inner_hidden.clone()])),
    ] {
        assert_eq!(action(outer.clone()).validate(), Ok(()), "{outer}");
        assert!(!action(outer).is_check());
    }
    // A guard in the inner one's then is the second level.
    for deep in [
        guarded(json!([guarded(json!([inner_hidden.clone()]))])),
        hidden("#o", json!([hidden("#i", json!([inner_visible.clone()]))])),
        hidden("#o", json!([click_json("#a"), guarded(json!([click_json("#b"), inner_hidden.clone()]))])),
    ] {
        assert_eq!(refusal(deep), ONE_LEVEL);
    }
    // The inner guard's own then keeps the rules.
    assert_eq!(
        refusal(hidden("#o", json!([hidden("#i", json!([{ "kind": "sign_in", "account": "a" }]))]))),
        "when_hidden then 1: when_hidden \"then\" cannot hold sign_in - change who is signed in as an action of its own"
    );
    assert_eq!(
        refusal(guarded(json!([hidden("#i", json!([]))]))),
        "when_visible then 1: when_hidden has nothing in \"then\" - say what to do when it is not on the page"
    );
}

#[test]
fn a_loop_still_holds_a_guard_with_plain_actions_only() {
    let looped = |then: serde_json::Value| json!({ "kind": "repeat_while_visible", "selector": { "css": ".row" }, "max": 3, "then": then });
    assert_eq!(action(looped(json!([guarded(json!([click_json("#x")]))]))).validate(), Ok(()));
    let err = refusal(looped(json!([hidden("#i", json!([click_json("#x")]))])));
    assert!(err.contains("when_hidden"), "{err}");
    let err = refusal(looped(json!([guarded(json!([hidden("#i", json!([click_json("#x")]))]))])));
    assert!(err.contains("when_hidden"), "{err}");
}

#[test]
fn each_and_targets_reach_the_inner_guard() {
    let a = action(guarded(json!([hidden("#gone", json!([click_json("#x")]))])));
    let kinds: Vec<String> = a.each().iter().map(|x| x.kind()).collect();
    assert_eq!(kinds, ["when_visible", "when_hidden", "click"]);
    let judged: Vec<String> = a.judging().iter().map(|x| x.kind()).collect();
    assert_eq!(judged, ["when_visible", "when_hidden", "click"]);
    let named: Vec<String> = a.targets().iter().map(|t| t.describe()).collect();
    assert_eq!(named, ["#banner", "#gone", "#x"]);
}

#[tokio::test]
async fn when_hidden_runs_its_then_when_the_target_never_shows() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, None);
    let mut acc: Option<String> = None;
    // `#other-session` never appears against this fake.
    let a = action(hidden("#other-session", json!([click_json("#apply")])));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![a]), &quick(), &mut acc).await.unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 1);
    assert!(out[0].detail.starts_with("#other-session not shown: clicked"), "{}", out[0].detail);
    let trace = out[0].guard.clone().expect("a guard leaves its trace");
    assert!(trace.acted && trace.inner.is_empty(), "{trace:?}");
}

#[tokio::test]
async fn when_hidden_skips_when_the_target_shows() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, None);
    let mut acc: Option<String> = None;
    let a = action(hidden("#financial", json!([click_json("#apply")])));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![a]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(SHOWN_SKIPPED, "on the page, skipped");
    assert_eq!(out[0].detail, format!("#financial {SHOWN_SKIPPED}"));
    assert_eq!(state.clicks.load(Ordering::SeqCst), 0, "the then never ran");
    assert!(!out[0].guard.clone().expect("a guard leaves its trace").acted);
}

/// A page where `#late` is on the page only from `after_ms` on, by the
/// clock. Every other css locator is there from the start and clickable.
fn appears_after(after_ms: u64) -> (ScriptedDriver, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let began = std::time::Instant::now();
    let clicked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = clicked.clone();
    let mut last_selector = String::new();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => json!({ "result": { "value": common::ready_probe() } }),
            "Runtime.callFunctionOn"
                if f == v2_lib::browser::locator::VISIBLE_JS || f == v2_lib::browser::input::SHOWN_JS || f == v2_lib::browser::input::HAS_FOCUS_JS =>
            {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last_selector = params["arguments"][0]["value"].as_str().unwrap().to_string();
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = last_selector != "#late" || began.elapsed() >= std::time::Duration::from_millis(after_ms);
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

/// It never acts on a first empty look: the target comes up 400 ms into a
/// 1000 ms window, so the guard is skipped, and it took that long to say so.
#[tokio::test]
async fn when_hidden_waits_the_whole_window_before_acting() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, clicked) = appears_after(400);
    let mut acc: Option<String> = None;
    let guard = action(json!({ "kind": "when_hidden", "selector": { "css": "#late" }, "within_ms": 1000,
                               "then": [ click_json("#apply") ] }));
    let started = std::time::Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(out[0].detail, format!("#late {SHOWN_SKIPPED}"));
    assert!(clicked.lock().unwrap().is_empty(), "the then ran on a first empty look");
    assert!(started.elapsed() >= std::time::Duration::from_millis(350), "answered after {:?}", started.elapsed());
    assert!(started.elapsed() < std::time::Duration::from_millis(900), "waited the window out though it showed: {:?}", started.elapsed());

    // Never shown inside a short window: the then runs.
    let (mut d, clicked) = appears_after(5000);
    let guard = action(json!({ "kind": "when_hidden", "selector": { "css": "#late" }, "within_ms": 600,
                               "then": [ click_json("#apply") ] }));
    let started = std::time::Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(*clicked.lock().unwrap(), vec!["#apply".to_string()]);
    assert!(started.elapsed() >= std::time::Duration::from_millis(550), "acted before the window ran out: {:?}", started.elapsed());
}

/// A page where `#folded` is in the document the whole time but never
/// shown (display:none, or off the window): visibility and "shown to click"
/// both read false for it, and every css locator finds it.
fn folded_away() -> (ScriptedDriver, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let clicked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = clicked.clone();
    let mut last_selector = String::new();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => json!({ "result": { "value": common::ready_probe() } }),
            "Runtime.callFunctionOn" if f == v2_lib::browser::locator::VISIBLE_JS || f == v2_lib::browser::input::SHOWN_JS => {
                json!({ "result": { "value": last_selector != "#folded" } })
            }
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::HAS_FOCUS_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if params["arguments"][0]["value"].is_string() && params["objectId"] == "doc" => {
                last_selector = params["arguments"][0]["value"].as_str().unwrap().to_string();
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                let there = last_selector != "#nothing";
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

/// A `when_hidden` acts only when nothing matches at all: an element that is
/// there but collapsed counts as there, so the guard is skipped; nothing
/// matching for the whole window runs it.
#[tokio::test]
async fn when_hidden_skips_an_element_that_is_there_but_not_shown() {
    let dir = tempfile::tempdir().unwrap();
    let mut acc: Option<String> = None;
    let (mut d, clicked) = folded_away();
    let guard = action(json!({ "kind": "when_hidden", "selector": { "css": "#folded" }, "within_ms": 600,
                               "then": [ click_json("#apply") ] }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(out[0].detail, format!("#folded {SHOWN_SKIPPED}"));
    assert!(clicked.lock().unwrap().is_empty(), "the then ran though the element is on the page");

    let (mut d, clicked) = folded_away();
    let guard = action(json!({ "kind": "when_hidden", "selector": { "css": "#nothing" }, "within_ms": 600,
                               "then": [ click_json("#apply") ] }));
    let started = std::time::Instant::now();
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![guard]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(*clicked.lock().unwrap(), vec!["#apply".to_string()]);
    assert!(started.elapsed() >= std::time::Duration::from_millis(550), "acted before the window ran out: {:?}", started.elapsed());
}

#[tokio::test]
async fn a_nested_guard_runs_inside_its_outer_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut acc: Option<String> = None;
    // Outer shows, inner target absent: both clicks happen, in order.
    let (mut d, state) = stateful_app(false, None);
    let nested = action(guarded(json!([click_json("#one"), hidden("#other-session", json!([click_json("#two")]))])));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![nested.clone()]), &quick(), &mut acc).await.unwrap();
    assert_eq!(out.len(), 1, "one outcome for the whole outer guard");
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 2);
    assert!(out[0].detail.starts_with("#banner showed: "), "{}", out[0].detail);
    assert!(out[0].detail.contains("#other-session not shown: "), "the outer detail lists the inner one's: {}", out[0].detail);
    let trace = out[0].guard.clone().unwrap();
    assert!(trace.acted);
    assert_eq!(trace.inner.len(), 1, "{trace:?}");
    assert!(trace.inner[0].acted);

    // Inner target present: the inner is skipped, the outer still acted.
    let (mut d, state) = stateful_app(false, None);
    let skipped = action(guarded(json!([click_json("#one"), hidden("#there", json!([click_json("#two")]))])));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![skipped]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 1);
    assert!(out[0].detail.contains(&format!("#there {SHOWN_SKIPPED}")), "{}", out[0].detail);
    let trace = out[0].guard.clone().unwrap();
    assert!(trace.acted && trace.inner.len() == 1 && !trace.inner[0].acted, "{trace:?}");

    // An outer guard that does not show never looks at the inner one.
    let (mut d, state) = stateful_app(false, None);
    let off = action(json!({ "kind": "when_visible", "selector": { "css": "#other-session" }, "within_ms": 40,
                             "then": [ hidden("#other-session", json!([click_json("#two")])) ] }));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![off]), &quick(), &mut acc).await.unwrap();
    assert!(out[0].ok && out[0].detail.ends_with(NOT_SHOWN), "{:?}", out[0]);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 0);
    let trace = out[0].guard.clone().unwrap();
    assert!(!trace.acted && trace.inner.is_empty(), "{trace:?}");
}

#[tokio::test]
async fn a_failing_inner_guard_fails_the_outer_one() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, state) = stateful_app(false, Some("#broken"));
    let mut acc: Option<String> = None;
    let a = action(guarded(json!([hidden("#other-session", json!([click_json("#broken")])), click_json("#after")])));
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step(vec![a]), &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.contains("#broken"), "{}", out[0].detail);
    assert_eq!(state.clicks.load(Ordering::SeqCst), 0, "the outer stops at the inner's failure");
}

#[tokio::test]
async fn the_executor_hands_when_hidden_to_the_runner() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let out = execute_with(&mut d, &action(hidden("#x", json!([click_json("#x")]))), &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, "when_hidden is carried out by the runner");
}
