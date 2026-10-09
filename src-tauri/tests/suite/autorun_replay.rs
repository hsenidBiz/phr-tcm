//! The unattended replay engine: a fresh browser per case, a proposed
//! verdict never a real one, and a run file saved after every case so a
//! crash or a stop loses nothing.

use crate::common;

use std::sync::atomic::{AtomicBool, Ordering};
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::recipe::save_recipe;
use std::path::Path;
use v2_lib::autorun::nav::{
    is_setup_problem, nav_path, no_address, no_path, save_nav, NavFile, Route, AFTER_SIGN_IN, NO_ACCOUNT, NO_MODULE,
    TRIED_TWICE, UNREACHED_PREFIX,
};
use v2_lib::autorun::transient::{FIRST_TRY, RETRY_NOT_STARTED, RETRY_PASSED};
use v2_lib::autorun::replay::{
    propose, run_cases, run_cases_checked, run_selection, settle_downloads, Browsers, CaseToRun, MODULE_STEP, PAGE_LOG_NOTE,
    SIGN_IN_STEP,
};
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::preconditions::{PreconditionDb, NOT_CHECKED};
use v2_lib::autorun::runner::run_step_routed;
use v2_lib::autorun::{store, CaseScript, LocalRun, StepRecord};
use v2_lib::browser::actions::ActionOutcome;
use v2_lib::browser::cdp::{CdpError, Event};
use v2_lib::browser::timing::Timing;
use v2_lib::events::ReplayProgress;

/// Hands out one prepared driver per `open`, and remembers how many were
/// opened and closed. `None` in the queue is a browser that will not open.
/// Returned drivers are kept (not dropped) so a test can inspect the calls
/// a case's own browser saw, even after the case gave it back.
struct FakeBrowsers {
    queue: std::collections::VecDeque<Option<common::ScriptedDriver>>,
    opened: usize,
    closed: usize,
    returned: Vec<common::ScriptedDriver>,
}

impl Browsers for FakeBrowsers {
    type D = common::ScriptedDriver;
    async fn open(&mut self) -> Result<Self::D, String> {
        self.opened += 1;
        self.queue.pop_front().flatten().ok_or_else(|| "Edge is not installed".to_string())
    }
    async fn close(&mut self, d: Self::D) {
        self.closed += 1;
        self.returned.push(d);
    }
}

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 20, highlight_ms: 0, lease_wait_ms: 300 }
}

fn new_run(id: &str) -> LocalRun {
    LocalRun { id: id.into(), pbi_id: 42, started_at: "1700000000000".into(), cases: vec![], mode: "unattended".into(), published: None, environment: None, resets: vec![] }
}

fn script(case_id: i32, account: Option<&str>, steps: serde_json::Value) -> CaseScript {
    serde_json::from_value(serde_json::json!({ "case_id": case_id, "title": format!("case {case_id}"), "account": account, "steps": steps })).unwrap()
}

/// A one-step script with a single passing `check_text` - the minimum
/// shape that proposes `Passed`.
fn passing_script(case_id: i32) -> CaseScript {
    script(case_id, None, serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ]))
}

/// A driver for `check_text` actions whose value is literally `"yes"` pass
/// and everything else fails - a page whose truth a test controls one
/// step at a time, unlike `FakePage`'s single static answer.
fn checking_driver() -> common::ScriptedDriver {
    common::ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" if params["expression"] == "document" => {
            Ok(serde_json::json!({ "result": { "objectId": "doc" } }))
        }
        "Runtime.callFunctionOn" => {
            let arg = params["arguments"][0]["value"].as_str().unwrap_or("");
            Ok(serde_json::json!({ "result": { "value": arg == "yes" } }))
        }
        "Page.captureScreenshot" => Ok(serde_json::json!({ "data": "/9j/4AAQ" })),
        _ => Ok(serde_json::json!({})),
    })
}

/// A driver whose very first call goes silent - a harness failure, not a
/// page one.
fn harness_driver() -> common::ScriptedDriver {
    common::ScriptedDriver::new(|method, _| match method {
        "Runtime.evaluate" => Err(CdpError::Closed),
        _ => Ok(serde_json::json!({})),
    })
}

fn navigable_fake_page_driver() -> common::ScriptedDriver {
    let mut d = common::FakePage::default().driver();
    d.on_call_events.push((
        "Page.navigate".into(),
        Event { method: "Page.lifecycleEvent".into(), params: serde_json::json!({ "frameId": "F", "loaderId": "L", "name": "load" }) },
    ));
    d
}

#[tokio::test]
async fn every_case_gets_its_own_browser_and_gives_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    store::save_script(root, &passing_script(2)).unwrap();
    let mut browsers = FakeBrowsers {
        queue: [Some(common::FakePage::default().driver()), Some(common::FakePage::default().driver())].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string()), (2, "case 2".to_string())];
    let cancel = AtomicBool::new(false);
    let res = run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await;
    assert!(res.is_ok(), "{res:?}");
    assert_eq!(browsers.opened, 2);
    assert_eq!(browsers.closed, 2);
    assert_eq!(run.cases.len(), 2);
    assert!(
        run.cases.iter().all(|c| c.proposed == "Passed" && c.verdict.is_empty() && c.note.is_empty()),
        "{:?}",
        run.cases
    );
    assert!(run.cases.iter().all(|c| c.duration_ms.is_some()), "a case that ran should carry its wall time: {:?}", run.cases);
}

/// Every case ran; only the save failed. The error says so rather than
/// claiming the run did not finish.
#[tokio::test]
async fn a_run_that_finished_but_could_not_be_saved_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    // A file where the runs folder belongs: every save of the run fails.
    std::fs::write(root.join("runs"), b"not a folder").unwrap();
    let mut browsers = browsers_of(vec![common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    let res = run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, false, &quick(), &cancel, &mut |_| {})
        .await;
    let e = res.unwrap_err();
    assert!(e.starts_with("the run finished but could not be saved: "), "{e}");
    assert_eq!(run.cases.len(), 1, "the case still ran");
}

#[tokio::test]
async fn a_browser_that_will_not_open_blocks_that_case_and_the_run_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    store::save_script(root, &passing_script(2)).unwrap();
    let mut browsers = FakeBrowsers {
        queue: [None, Some(common::FakePage::default().driver())].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string()), (2, "case 2".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert!(
        run.cases[0].reason.contains("browser did not open") && run.cases[0].reason.contains("Edge is not installed"),
        "{}",
        run.cases[0].reason
    );
    assert_eq!(run.cases[1].proposed, "Passed");
    assert_eq!(browsers.closed, 1);
}

#[tokio::test]
async fn a_failed_step_stops_the_case_but_not_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let case1 = script(
        1,
        None,
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] },
            { "step_number": 2, "actions": [{ "kind": "check_text", "value": "no" }] },
            { "step_number": 3, "actions": [{ "kind": "check_text", "value": "yes" }] },
        ]),
    );
    store::save_script(root, &case1).unwrap();
    store::save_script(root, &passing_script(2)).unwrap();
    let mut browsers = FakeBrowsers {
        queue: [Some(checking_driver()), Some(common::FakePage::default().driver())].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string()), (2, "case 2".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();

    let rec1 = &run.cases[0];
    assert_eq!(rec1.proposed, "Failed");
    assert!(rec1.reason.starts_with("step 2:"), "{}", rec1.reason);
    let step3 = rec1.steps.iter().find(|s| s.step_number == 3).expect("step 3 is still recorded");
    assert!(
        step3.outcomes.iter().all(|o| o.detail == "not run: an earlier step of this case failed"),
        "{:?}",
        step3.outcomes
    );

    let d1 = &browsers.returned[0];
    assert_eq!(
        d1.calls_to("Runtime.callFunctionOn").len(),
        2,
        "step 3 must never touch the driver: {:?}",
        d1.calls
    );

    assert_eq!(run.cases[1].proposed, "Passed");
}

#[tokio::test]
async fn a_failed_sign_in_runs_no_step_and_is_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let case = script(
        1,
        Some("ghost"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] }]),
    );
    store::save_script(root, &case).unwrap();
    let mut browsers = FakeBrowsers {
        queue: [Some(common::FakePage::default().driver())].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();

    let rec = &run.cases[0];
    assert_eq!(rec.steps[0].step_number, SIGN_IN_STEP);
    let step1 = rec.steps.iter().find(|s| s.step_number == 1).expect("the case's own step is still recorded");
    assert!(step1.outcomes.iter().all(|o| o.detail == "not run: the sign-in failed"), "{:?}", step1.outcomes);
    assert_eq!(rec.proposed, "Blocked");
}

#[tokio::test]
async fn a_successful_sign_in_is_step_zero_and_the_account_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    save_accounts(root, &[common::account()]).unwrap();
    let case = script(1, Some("admin"), serde_json::json!([]));
    store::save_script(root, &case).unwrap();
    let (d, _state) = common::stateful_app(false, None);
    let mut browsers = FakeBrowsers { queue: [Some(d)].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();

    let rec = &run.cases[0];
    assert_eq!(rec.steps[0].step_number, 0);
    assert!(rec.steps[0].outcomes.iter().all(|o| o.ok), "{:?}", rec.steps[0].outcomes);
    assert_eq!(rec.account.as_deref(), Some("admin"));
}

#[tokio::test]
async fn with_no_account_picked_a_script_signs_in_as_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    save_accounts(root, &[common::account()]).unwrap();
    store::save_script(root, &script(1, Some("admin"), serde_json::json!([]))).unwrap();
    let (d, _state) = common::stateful_app(false, None);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();

    assert_eq!(run.cases[0].account.as_deref(), Some("admin"));
}

#[tokio::test]
async fn a_harness_failure_is_blocked_and_takes_no_picture() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let case = script(
        1,
        None,
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] }]),
    );
    store::save_script(root, &case).unwrap();
    let mut browsers = FakeBrowsers { queue: [Some(harness_driver())].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();

    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Blocked");
    let step1 = rec.steps.iter().find(|s| s.step_number == 1).unwrap();
    assert!(step1.screenshot.is_none());
    let d = &browsers.returned[0];
    assert!(
        d.calls.iter().all(|(m, _)| m != "Page.captureScreenshot"),
        "a browser that stopped answering must never be asked for a picture: {:?}",
        d.calls
    );
}

#[tokio::test]
async fn every_executed_step_has_a_picture_and_a_skipped_one_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let case = script(
        1,
        None,
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] },
            { "step_number": 2, "actions": [{ "kind": "check_text", "value": "no" }] },
            { "step_number": 3, "actions": [{ "kind": "check_text", "value": "yes" }] },
        ]),
    );
    store::save_script(root, &case).unwrap();
    let mut browsers = FakeBrowsers { queue: [Some(checking_driver())].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();

    let rec = &run.cases[0];
    for n in [1, 2] {
        let step = rec.steps.iter().find(|s| s.step_number == n).unwrap();
        let name = step.screenshot.as_ref().unwrap_or_else(|| panic!("step {n} should have a picture"));
        assert!(root.join("shots").join(name).is_file());
    }
    let step3 = rec.steps.iter().find(|s| s.step_number == 3).unwrap();
    assert!(step3.screenshot.is_none());
}

#[tokio::test]
async fn a_script_that_checks_nothing_proposes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let case = script(
        1,
        None,
        serde_json::json!([
            { "step_number": 1, "actions": [
                { "kind": "navigate", "url": "https://app.example/page" },
                { "kind": "click", "selector": { "css": "#go" } }
            ] },
        ]),
    );
    store::save_script(root, &case).unwrap();
    let mut browsers = FakeBrowsers { queue: [Some(navigable_fake_page_driver())].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();

    let rec = &run.cases[0];
    assert!(rec.steps.iter().all(|s| s.outcomes.iter().all(|o| o.ok)), "{:?}", rec.steps);
    assert_eq!(rec.proposed, "");
    assert!(rec.reason.contains("checks nothing"), "{}", rec.reason);
}

#[tokio::test]
async fn the_run_is_on_disk_after_every_case() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    store::save_script(root, &passing_script(2)).unwrap();
    let mut browsers = FakeBrowsers {
        queue: [Some(common::FakePage::default().driver()), Some(common::FakePage::default().driver())].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string()), (2, "case 2".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |e: ReplayProgress| {
        if e.phase == "done" {
            let on_disk = store::load_run(root, "run-x").unwrap().expect("the run should already be on disk");
            assert_eq!(on_disk.cases.len(), (e.index + 1) as usize);
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_save_that_fails_does_not_stop_the_run_and_is_reported_at_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    store::save_script(root, &passing_script(2)).unwrap();
    // `save_run` needs `root/runs` to be a directory it can create or
    // already use; a plain FILE sitting at that path makes every save
    // fail, the same shape a real disk problem (permissions, a stray
    // file) would take.
    std::fs::write(root.join("runs"), b"not a directory").unwrap();
    let mut browsers = FakeBrowsers {
        queue: [Some(common::FakePage::default().driver()), Some(common::FakePage::default().driver())].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string()), (2, "case 2".to_string())];
    let cancel = AtomicBool::new(false);
    let mut done_count = 0;
    let res = run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |e: ReplayProgress| {
        if e.phase == "done" {
            done_count += 1;
        }
    })
    .await;

    assert!(res.is_err(), "the first save error should be reported at the end");
    assert_eq!(run.cases.len(), 2, "a save that fails must not stop the run");
    assert_eq!(done_count, 2, "both cases still reach done");
}

#[tokio::test]
async fn stopping_leaves_out_what_never_started_and_marks_what_did() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let case1 = script(
        1,
        None,
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] },
            { "step_number": 2, "actions": [{ "kind": "check_text", "value": "yes" }] },
            { "step_number": 3, "actions": [{ "kind": "check_text", "value": "yes" }] },
        ]),
    );
    store::save_script(root, &case1).unwrap();
    store::save_script(root, &passing_script(2)).unwrap();
    let mut browsers = FakeBrowsers {
        queue: [Some(checking_driver()), Some(common::FakePage::default().driver())].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string()), (2, "case 2".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |e: ReplayProgress| {
        if e.phase == "step" && e.step_number == 2 {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .await
    .unwrap();

    assert_eq!(run.cases.len(), 1, "case 2 never started, so it is left out entirely");
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "");
    assert_eq!(rec.reason, "stopped before it finished");
    let step3 = rec.steps.iter().find(|s| s.step_number == 3).unwrap();
    assert!(step3.outcomes.iter().all(|o| o.detail == "not run: the run was stopped"), "{:?}", step3.outcomes);
    assert_eq!(browsers.opened, 1);
}

/// A stop asked for before a case's own sign-in must never let that
/// sign-in run anyway - the case is left completely untouched, not just
/// cut short after touching the browser once.
#[tokio::test]
async fn stopping_before_a_cases_sign_in_leaves_it_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    save_accounts(root, &[common::account()]).unwrap();
    let case = script(
        1,
        Some("admin"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] }]),
    );
    store::save_script(root, &case).unwrap();
    let (d, _state) = common::stateful_app(false, None);
    let mut browsers = FakeBrowsers { queue: [Some(d)].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |e: ReplayProgress| {
        if e.phase == "opening" {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .await
    .unwrap();

    let rec = &run.cases[0];
    assert!(
        rec.steps.iter().all(|s| s.step_number != SIGN_IN_STEP),
        "no sign-in step should ever be recorded: {:?}",
        rec.steps
    );
    assert!(
        rec.steps.iter().all(|s| s.outcomes.iter().all(|o| o.detail == "not run: the run was stopped")),
        "{:?}",
        rec.steps
    );
    assert_eq!(rec.proposed, "");
    assert_eq!(rec.reason, "stopped before it finished");
    let d = &browsers.returned[0];
    assert!(
        d.calls_to("Page.navigate").is_empty(),
        "cancelling before the sign-in must mean no navigation at all: {:?}",
        d.calls
    );
}

#[tokio::test]
async fn a_case_with_no_script_is_recorded_as_such() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut browsers =
        FakeBrowsers { queue: std::collections::VecDeque::new(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cases = vec![(999, "ghost case".to_string())];
    let cancel = AtomicBool::new(false);
    let mut done_steps = None;
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |e: ReplayProgress| {
        if e.phase == "done" {
            done_steps = Some(e.steps);
        }
    })
    .await
    .unwrap();

    assert_eq!(browsers.opened, 0);
    let rec = &run.cases[0];
    assert!(rec.steps.is_empty());
    assert_eq!(rec.reason, "this case has no script on this machine");
    // There is genuinely no script to count, so `steps` on "done" reads 0
    // - never `record.steps.len()`, which would also be 0 here but for
    // the wrong reason (no sign-in step recorded, not "the script has no
    // steps").
    assert_eq!(done_steps, Some(0));
}

#[tokio::test]
async fn progress_tells_the_story_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    save_accounts(root, &[common::account()]).unwrap();
    // `click` (unlike `check_text`) is answered `ok` by `stateful_app` for
    // any selector that is not one of its own special ones, so both steps
    // run regardless of sign-in state - this test is about the ORDER of
    // progress events, not about pass/fail.
    let case = script(
        1,
        Some("admin"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": { "css": "#one" } }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "css": "#two" } }] },
        ]),
    );
    store::save_script(root, &case).unwrap();
    let (d, _state) = common::stateful_app(false, None);
    let mut browsers = FakeBrowsers { queue: [Some(d)].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    let mut phases = vec![];
    let mut totals = vec![];
    let mut step_counts = vec![];
    let mut done_proposed = None;
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |e: ReplayProgress| {
        phases.push(e.phase.clone());
        totals.push(e.total);
        step_counts.push(e.steps);
        if e.phase == "done" {
            done_proposed = Some(e.proposed.clone());
        }
    })
    .await
    .unwrap();

    assert_eq!(phases, vec!["opening", "signing_in", "step", "step", "done"]);
    assert!(totals.iter().all(|&t| t == 1), "{totals:?}");
    // The script's own step count - 2 - on every phase, "done" included,
    // never what the case actually got through.
    assert!(step_counts.iter().all(|&s| s == 2), "{step_counts:?}");
    assert_eq!(done_proposed, Some(run.cases[0].proposed.clone()));
}

#[test]
fn propose_blames_the_browser_before_the_page() {
    let case = script(
        1,
        None,
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }]),
    );
    let ordinary_fail = ActionOutcome::failed("nope");
    let mut harness_fail = ActionOutcome::failed("gone");
    harness_fail.harness = true;
    let steps = vec![StepRecord { step_number: 1, outcomes: vec![ordinary_fail, harness_fail], screenshot: None, downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None }];
    let p = propose(&case, &steps, None, false);
    assert_eq!(p.verdict, "Blocked");
}

// ---- Module paths --------------------------------------------------------

const MENU: &[(&str, &str, &str)] = &[("link", "Leave", "/hr/leave"), ("link", "Apply Leave", "/hr/leave/apply")];

fn leave_nav() -> NavFile {
    serde_json::from_value(serde_json::json!({
        "direct_urls": true,
        "modules": [{
            "module": "Leave",
            "clicks": [
                { "role": "link", "name": "Leave", "exact": true },
                { "role": "link", "name": "Apply Leave", "exact": true }
            ],
            "arrived": "/hr/leave/apply",
            "recorded": "2026-09-24T10:00:00Z"
        }]
    }))
    .unwrap()
}

/// A project with a recipe, the admin account and a path for Leave.
fn menu_project(root: &Path) {
    save_recipe(root, "Acme", "Web", &common::menu_recipe()).unwrap();
    save_accounts(root, &[common::account()]).unwrap();
    save_nav(root, "Acme", "Web", &leave_nav()).unwrap();
}

fn to_run(case_id: i32, module: Option<&str>) -> CaseToRun {
    CaseToRun { case_id, title: format!("case {case_id}"), module: module.map(str::to_string) }
}

fn one_check(account: Option<&str>) -> CaseScript {
    script(1, account, serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] }]))
}

fn browsers_of(drivers: Vec<common::ScriptedDriver>) -> FakeBrowsers {
    FakeBrowsers { queue: drivers.into_iter().map(Some).collect(), opened: 0, closed: 0, returned: vec![] }
}

#[tokio::test]
async fn a_case_signs_in_goes_home_clicks_to_its_module_checks_it_arrived_then_runs_its_steps() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let (d, app) = common::menu_app(MENU, "/hr/welcome", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    let mut phases: Vec<String> = vec![];
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some(" leave "))], None, false, &quick(), &cancel, &mut |p: ReplayProgress| {
        phases.push(p.phase);
    })
    .await
    .unwrap();

    assert_eq!(
        *app.log.lock().unwrap(),
        vec!["navigate /hr/home/index", "click #go", "navigate /hr/home/index", "click Leave", "click Apply Leave", "check yes"]
    );
    let rec = &run.cases[0];
    assert_eq!(rec.steps.iter().map(|s| s.step_number).collect::<Vec<_>>(), vec![SIGN_IN_STEP, MODULE_STEP, 1]);
    let module = &rec.steps[1].outcomes[0];
    assert!(module.ok, "{module:?}");
    assert_eq!(module.detail, "Go to Leave");
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
    assert!(phases.contains(&"module".to_string()), "{phases:?}");
}

/// Fix round 1, I2: spec §6's exemptions (the sign-in recipe's `start_url`
/// and the runner's own go-home step) hold even in a project whose switch
/// is off - only a SCRIPT's own `navigate` is refused. Same shape as the
/// switch-on version above, with `direct_urls: false`.
#[tokio::test]
async fn a_case_with_addresses_switched_off_still_signs_in_goes_home_and_reaches_its_module() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_recipe(root, "Acme", "Web", &common::menu_recipe()).unwrap();
    save_accounts(root, &[common::account()]).unwrap();
    save_nav(root, "Acme", "Web", &NavFile { direct_urls: false, modules: leave_nav().modules, save_words: vec![] }).unwrap();
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let (d, app) = common::menu_app(MENU, "/hr/welcome", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();

    assert_eq!(
        *app.log.lock().unwrap(),
        vec!["navigate /hr/home/index", "click #go", "navigate /hr/home/index", "click Leave", "click Apply Leave", "check yes"]
    );
    let rec = &run.cases[0];
    assert_eq!(rec.steps.iter().map(|s| s.step_number).collect::<Vec<_>>(), vec![SIGN_IN_STEP, MODULE_STEP, 1]);
    assert!(rec.steps[0].outcomes.iter().all(|o| o.ok), "sign-in: {:?}", rec.steps[0]);
    let module = &rec.steps[1].outcomes[0];
    assert!(module.ok, "{module:?}");
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
}

#[tokio::test]
async fn a_case_with_no_module_is_blocked_and_no_browser_opens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let mut browsers = browsers_of(vec![]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, false, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let rec = &run.cases[0];
    assert_eq!(browsers.opened, 0);
    assert_eq!(rec.proposed, "Blocked");
    assert_eq!(rec.reason, NO_MODULE);
    assert!(rec.steps.iter().flat_map(|s| &s.outcomes).all(|o| o.detail == format!("not run: {NO_MODULE}")), "{:?}", rec.steps);
}

#[tokio::test]
async fn a_module_with_no_recorded_path_is_blocked_and_named() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let mut browsers = browsers_of(vec![]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Payroll"))], None, false, &quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(browsers.opened, 0);
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].reason, no_path("Payroll"));
}

/// A project with Leave's path plus a second area under Leave, "Leave
/// home", that stops after the first click.
fn two_leave_areas(root: &Path) {
    menu_project(root);
    let mut nav = leave_nav();
    nav.modules.push(
        serde_json::from_value(serde_json::json!({
            "area": "Leave home",
            "module": "Leave",
            "clicks": [{ "role": "link", "name": "Leave", "exact": true }],
            "arrived": "/hr/leave",
            "recorded": "2026-10-01T10:00:00Z"
        }))
        .unwrap(),
    );
    save_nav(root, "Acme", "Web", &nav).unwrap();
}

fn in_area(area: &str) -> CaseScript {
    let mut sc = one_check(Some("admin"));
    sc.area = Some(area.to_string());
    sc
}

/// Spec §9: the script's `area` decides where the case starts - here a
/// case whose Module has no area of its own name at all.
#[tokio::test]
async fn a_scripts_area_takes_the_case_there_whatever_its_module() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    two_leave_areas(root);
    store::save_script(root, &in_area("leave home")).unwrap();
    let (d, app) = common::menu_app(MENU, "/hr/welcome", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Payroll"))], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(
        *app.log.lock().unwrap(),
        vec!["navigate /hr/home/index", "click #go", "navigate /hr/home/index", "click Leave", "check yes"]
    );
    let rec = &run.cases[0];
    assert_eq!(rec.steps[1].outcomes[0].detail, "Go to Leave home");
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
}

#[tokio::test]
async fn a_script_naming_an_unrecorded_area_is_blocked_and_no_browser_opens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    two_leave_areas(root);
    store::save_script(root, &in_area("Leave balance")).unwrap();
    let mut browsers = browsers_of(vec![]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(browsers.opened, 0);
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].reason, "the area \"Leave balance\" is not recorded - record it in Auto Run, Areas");
}

#[tokio::test]
async fn with_paths_a_case_no_account_applies_to_is_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(None)).unwrap();
    let mut browsers = browsers_of(vec![]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(browsers.opened, 0);
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].reason, NO_ACCOUNT);
}

/// A trip that never reached its module logs what the page was doing -
/// every line, in the application log - and its sentence says where to
/// look. The run itself keeps only the sentence.
#[tokio::test]
async fn a_failed_trip_logs_what_the_page_was_doing_and_says_where() {
    let _tail = crate::serial::log_tail();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &script(4711, Some("admin"), serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] }]))).unwrap();
    let (mut d, _app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    d.page_log = vec![
        "request still waiting after 14s: GET https://hr.example.internal/hr/pmsv10/PerformanceCycle".to_string(),
        "console error: initialData is not defined".to_string(),
    ];
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(4711, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();

    let rec = &run.cases[0];
    let module = &rec.steps.iter().find(|s| s.step_number == MODULE_STEP).unwrap().outcomes[0];
    assert!(module.detail.starts_with("Could not reach module \"Leave\": click 2, link \"Apply Leave\" - "), "{}", module.detail);
    assert!(module.detail.ends_with(PAGE_LOG_NOTE), "{}", module.detail);
    assert!(!module.detail.contains("pmsv10"), "the lines stay out of the run: {}", module.detail);
    assert_eq!(rec.reason, module.detail);
    assert!(is_setup_problem(&rec.reason), "still read as the trip that failed");

    let logged: Vec<String> = v2_lib::applog::recent(6000).into_iter().map(|l| l.message).collect();
    let mine: Vec<&String> = logged.iter().filter(|l| l.starts_with("unattended run, case 4711") && !l.contains(": Took ")).collect();
    // The first try's page goes to the log before the reload clears it,
    // then the second try's, as a failed trip always logged it.
    assert_eq!(mine.len(), 6, "{mine:?}");
    assert!(
        mine[0].starts_with("unattended run, case 4711, first try: Could not reach module \"Leave\"")
            && !mine[0].contains(TRIED_TWICE.trim())
            && mine[0].ends_with("What the page was doing:"),
        "{}",
        mine[0]
    );
    assert_eq!(
        mine[1],
        "unattended run, case 4711, first try, page: request still waiting after 14s: GET https://hr.example.internal/hr/pmsv10/PerformanceCycle"
    );
    assert_eq!(mine[2], "unattended run, case 4711, first try, page: console error: initialData is not defined");
    assert!(
        mine[3].starts_with("unattended run, case 4711: Could not reach module \"Leave\"")
            && mine[3].contains(TRIED_TWICE.trim())
            && mine[3].ends_with("What the page was doing:"),
        "{}",
        mine[3]
    );
    assert_eq!(
        mine[4],
        "unattended run, case 4711, page: request still waiting after 14s: GET https://hr.example.internal/hr/pmsv10/PerformanceCycle"
    );
    assert_eq!(mine[5], "unattended run, case 4711, page: console error: initialData is not defined");
}

#[tokio::test]
async fn a_failed_trip_with_nothing_in_the_page_log_logs_nothing_and_keeps_its_sentence() {
    let _tail = crate::serial::log_tail();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &script(4712, Some("admin"), serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] }]))).unwrap();
    let (d, _app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(4712, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert!(!run.cases[0].reason.contains(PAGE_LOG_NOTE.trim()), "{}", run.cases[0].reason);
    assert!(!v2_lib::applog::recent(6000).iter().any(|l| l.message.starts_with("unattended run, case 4712") && !l.message.contains(": Took ")));
}

#[tokio::test]
async fn a_path_click_that_finds_nothing_blocks_the_case_with_a_picture_and_runs_no_step() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let (d, app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let rec = &run.cases[0];
    let module = &rec.steps.iter().find(|s| s.step_number == MODULE_STEP).unwrap().outcomes[0];
    assert!(!module.ok);
    assert!(module.detail.starts_with("Could not reach module \"Leave\": click 2, link \"Apply Leave\" - "), "{}", module.detail);
    assert!(module.detail.ends_with('.'), "{}", module.detail);
    assert!(module.screenshot.is_some(), "a failed trip to the module keeps a picture");
    let step1 = rec.steps.iter().find(|s| s.step_number == 1).unwrap();
    assert!(step1.outcomes.iter().all(|o| o.detail == "not run: the module screen was not reached"), "{:?}", step1.outcomes);
    assert!(!app.log.lock().unwrap().iter().any(|l| l.starts_with("check")), "no step may run off the module screen");
    assert_eq!(rec.proposed, "Blocked");
    assert_eq!(rec.reason, module.detail);
}

#[tokio::test]
async fn a_path_that_ends_somewhere_else_fails_its_arrival_check() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let (d, _app) = common::menu_app(&[("link", "Leave", "/hr/leave"), ("link", "Apply Leave", "/hr/leave/other")], "/hr/home/index", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Blocked");
    assert_eq!(
        rec.reason,
        "Could not reach module \"Leave\": click 2, link \"Apply Leave\" - the page ended on /hr/leave/other, not /hr/leave/apply. \
         Tried twice, reloading the start page between."
    );
}

/// Spec §5: a trip that stalls reloads the start page and goes once more;
/// the second go reaching the module is all the case records, and its
/// steps run as usual.
#[tokio::test]
async fn a_stalled_trip_reloads_the_start_page_and_the_second_go_reaches_the_module() {
    let _tail = crate::serial::log_tail();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &script(4721, Some("admin"), serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] }]))).unwrap();
    // The first two menu clicks land nowhere: the first trip stalls.
    let (mut d, app) = common::stalling_menu_app(MENU, "/hr/home/index", 0, 2);
    d.page_log = vec!["request still waiting after 14s: GET https://hr.example.internal/hr/menu".to_string()];
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(4721, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();

    assert_eq!(
        *app.log.lock().unwrap(),
        vec![
            "navigate /hr/home/index", "click #go",
            "click Leave", "click Apply Leave",
            "navigate /hr/home/index",
            "click Leave", "click Apply Leave",
            "check yes",
        ]
    );
    let rec = &run.cases[0];
    let modules: Vec<&StepRecord> = rec.steps.iter().filter(|s| s.step_number == MODULE_STEP).collect();
    assert_eq!(modules.len(), 1, "one Go to line, whatever it took");
    assert!(modules[0].outcomes[0].ok, "{:?}", modules[0].outcomes);
    assert_eq!(modules[0].outcomes[0].detail, "Go to Leave");
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
    // What the first try's page was doing is logged before the reload.
    let logged: Vec<String> = v2_lib::applog::recent(6000).into_iter().map(|l| l.message).collect();
    assert!(
        logged.iter().any(|l| l == "unattended run, case 4721, first try, page: request still waiting after 14s: GET https://hr.example.internal/hr/menu"),
        "{logged:?}"
    );
}

/// Review focus 3: one reload, then the report - never a loop. Both goes
/// failing give one Blocked line carrying the "tried twice" sentence.
#[tokio::test]
async fn a_trip_that_fails_twice_is_reported_once_after_one_reload() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let (d, app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();

    let log = app.log.lock().unwrap().clone();
    assert_eq!(log.iter().filter(|l| l.starts_with("navigate")).count(), 2, "the sign-in's load and one reload: {log:?}");
    assert_eq!(log.iter().filter(|l| *l == "click Leave").count(), 2, "two goes: {log:?}");
    let rec = &run.cases[0];
    let modules: Vec<&StepRecord> = rec.steps.iter().filter(|s| s.step_number == MODULE_STEP).collect();
    assert_eq!(modules.len(), 1);
    let module = &modules[0].outcomes[0];
    assert!(!module.ok);
    assert!(module.detail.starts_with("Could not reach module \"Leave\": click 2, link \"Apply Leave\" - "), "{}", module.detail);
    assert!(module.detail.ends_with(TRIED_TWICE), "{}", module.detail);
    assert_eq!(TRIED_TWICE, " Tried twice, reloading the start page between.");
    assert_eq!(rec.proposed, "Blocked");
    assert_eq!(rec.reason, module.detail);
    assert!(is_setup_problem(&rec.reason), "still read as the trip that failed");
    assert_eq!(rec.retried, None, "a module that was not reached is never a transient retry");
    assert_eq!(browsers.opened, 1);
}

/// A mid-script `sign_in`'s trip back gets the same second go.
#[tokio::test]
async fn a_mid_script_sign_ins_stalled_trip_back_goes_once_more() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    let signs_in = script(1, Some("admin"), serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "sign_in", "account": "admin" }, { "kind": "check_text", "value": "yes" }
    ] }]));
    let (mut d, app) = common::stalling_menu_app(MENU, "/hr/home/index", 0, 2);
    let route = Route::new(&common::menu_recipe(), leave_nav().modules[0].clone());
    let mut account = None;
    let mut held = Held::supervised();
    let outcomes = run_step_routed(&mut d, root, "Acme", "Web", &signs_in.steps[0], &quick(), &mut account, &mut held, Some(&route), v2_lib::autorun::runner::AreaRoute::To(&route))
        .await
        .unwrap();
    assert!(outcomes[0].ok && outcomes[0].detail.ends_with("; then Go to Leave"), "{outcomes:?}");
    assert!(outcomes[1].ok, "{outcomes:?}");
    let log = app.log.lock().unwrap().clone();
    assert_eq!(log.iter().filter(|l| *l == "click Leave").count(), 2, "{log:?}");
    assert_eq!(log.last().map(String::as_str), Some("check yes"));
}

// ---- Transient retry (spec §6) --------------------------------------------

/// Case 1, one passing check, its first browser going silent at once.
async fn first_go_silent(second: common::ScriptedDriver, retry: bool) -> (LocalRun, FakeBrowsers) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    let mut browsers = browsers_of(vec![harness_driver(), second]);
    let mut run = new_run("run-t");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, retry, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    (run, browsers)
}

#[tokio::test]
async fn a_transient_failure_runs_once_more_in_a_fresh_browser_and_passes_saying_why() {
    let (run, browsers) = first_go_silent(common::FakePage::default().driver(), true).await;
    assert_eq!((browsers.opened, browsers.closed), (2, 2), "a fresh browser for the second go");
    assert_eq!(run.cases.len(), 1, "one record per case");
    let rec = &run.cases[0];
    let first = rec.retried.clone().expect("a retried case says so");
    assert!(first.starts_with("the browser stopped answering at step 1: "), "{first}");
    assert_eq!(rec.proposed, "Passed");
    assert_eq!(rec.reason, format!("passed on a second try after a transient failure: {first}"));
    assert_eq!(RETRY_PASSED, "passed on a second try after a transient failure: ");
    assert!(rec.steps.iter().flat_map(|s| &s.outcomes).all(|o| o.ok), "only the final go's steps: {:?}", rec.steps);
}

/// Review focus 2: never a third go. A case transient twice is reported
/// once, with both sentences.
#[tokio::test]
async fn a_case_that_fails_transiently_again_is_reported_once_with_both_sentences() {
    let (run, browsers) = first_go_silent(harness_driver(), true).await;
    assert_eq!(browsers.opened, 2, "one retry, never two");
    assert_eq!(run.cases.len(), 1);
    let rec = &run.cases[0];
    let first = rec.retried.clone().unwrap();
    assert_eq!(rec.proposed, "Blocked");
    assert!(rec.reason.starts_with("the browser stopped answering at step 1: "), "{}", rec.reason);
    assert!(rec.reason.ends_with(&format!("{FIRST_TRY}{first})")), "{}", rec.reason);
    assert_eq!(FIRST_TRY, " (first try: ");
}

#[tokio::test]
async fn a_retry_that_fails_otherwise_proposes_what_the_second_go_found() {
    let (run, _) = first_go_silent(checking_driver(), true).await;
    let rec = &run.cases[0];
    let first = rec.retried.clone().unwrap();
    // `checking_driver` passes only "yes"; the script checks "ok".
    assert_eq!(rec.proposed, "Failed");
    assert!(rec.reason.starts_with("step 1: "), "{}", rec.reason);
    assert!(rec.reason.ends_with(&format!("{FIRST_TRY}{first})")), "{}", rec.reason);
}

/// When the second go's browser never opens, there is no second record to
/// keep: the first go's steps and evidence stay the case's record, its
/// sentence stays in `retried`, and the reason says the retry could not
/// start - never "the browser did not open" in place of what really failed.
#[tokio::test]
async fn a_retry_whose_browser_never_opens_keeps_the_first_gos_record() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    let mut browsers =
        FakeBrowsers { queue: vec![Some(harness_driver()), None].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-t");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, true, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!((browsers.opened, browsers.closed), (2, 1), "one retry, whose browser never opened");
    assert_eq!(run.cases.len(), 1);
    let rec = &run.cases[0];
    let first = rec.retried.clone().expect("the first try's sentence is kept");
    assert!(first.starts_with("the browser stopped answering at step 1: "), "{first}");
    assert_eq!(rec.proposed, "Blocked");
    assert_eq!(rec.reason, format!("{first}{RETRY_NOT_STARTED}the browser did not open: Edge is not installed)"));
    assert_eq!(RETRY_NOT_STARTED, " (a second try could not start: ");
    let failed: Vec<&ActionOutcome> = rec.steps.iter().flat_map(|s| &s.outcomes).filter(|o| !o.ok).collect();
    assert!(failed.iter().any(|o| o.harness), "the first go's steps are the record: {:?}", rec.steps);
}

/// A page whose API answers `status` to the script's `api_request`.
fn api_answering(status: u16) -> common::ScriptedDriver {
    common::ScriptedDriver::new(move |method, params| match method {
        "Runtime.evaluate" if params["expression"] == "document" => {
            Ok(serde_json::json!({ "result": { "objectId": "doc" } }))
        }
        "Runtime.callFunctionOn" if params["functionDeclaration"] == v2_lib::autorun::api_checks::GET_FN => {
            Ok(serde_json::json!({ "result": { "value": {
                "status": status, "contentType": "application/json", "finalPath": "/hr/api/cycles/42",
                "sameOrigin": true, "redirected": false, "text": "", "over": false
            } } }))
        }
        "Page.captureScreenshot" => Ok(serde_json::json!({ "data": "/9j/4AAQ" })),
        _ => Ok(serde_json::json!({})),
    })
}

/// End to end, through the sentence `api_request` really writes: a
/// gateway that answers 502 once is a transient failure, and the second
/// go's 200 passes the case, labelled Retried.
#[tokio::test]
async fn an_api_check_that_answers_502_then_200_on_the_retry_passes_retried() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let api = script(1, None, serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "api_request", "path": "/hr/api/cycles/42", "expect": { "status": 200 } }
    ] }]));
    store::save_script(root, &api).unwrap();
    let mut browsers = browsers_of(vec![api_answering(502), api_answering(200)]);
    let mut run = new_run("run-t");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, true, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(browsers.opened, 2, "a fresh browser for the second go");
    let rec = &run.cases[0];
    let first = rec.retried.clone().expect("labelled Retried");
    assert!(first.contains("GET /hr/api/cycles/42 answered 502, expected 200"), "{first}");
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
    assert_eq!(rec.reason, format!("{RETRY_PASSED}{first}"));
}

/// The same through the page's own `fetch` failing outright: the sentence
/// `api_request` writes for a dropped connection is the one retried.
#[tokio::test]
async fn an_api_check_whose_fetch_failed_is_retried() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let api = script(1, None, serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "api_request", "path": "/hr/api/cycles/42", "expect": { "status": 200 } }
    ] }]));
    store::save_script(root, &api).unwrap();
    let dropped = common::ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" if params["expression"] == "document" => {
            Ok(serde_json::json!({ "result": { "objectId": "doc" } }))
        }
        "Runtime.callFunctionOn" => Ok(serde_json::json!({ "result": { "value": { "error": "TypeError: Failed to fetch" } } })),
        _ => Ok(serde_json::json!({})),
    });
    let mut browsers = browsers_of(vec![dropped, api_answering(200)]);
    let mut run = new_run("run-t");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, true, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    let rec = &run.cases[0];
    let first = rec.retried.clone().expect("labelled Retried");
    assert!(first.contains("GET /hr/api/cycles/42 failed: TypeError: Failed to fetch"), "{first}");
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
}

#[tokio::test]
async fn an_assertion_that_fails_is_never_retried() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    let mut browsers = browsers_of(vec![checking_driver(), common::FakePage::default().driver()]);
    let mut run = new_run("run-t");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, true, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(browsers.opened, 1);
    assert_eq!(run.cases[0].proposed, "Failed");
    assert_eq!(run.cases[0].retried, None);
}

#[tokio::test]
async fn with_the_option_off_nothing_is_retried() {
    let (run, browsers) = first_go_silent(common::FakePage::default().driver(), false).await;
    assert_eq!(browsers.opened, 1);
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].retried, None);
    assert!(!run.cases[0].reason.contains("second try"), "{}", run.cases[0].reason);
}

/// A stop asked for during the first go is a stop: no second go.
#[tokio::test]
async fn a_stopped_run_does_not_retry() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    let mut browsers = browsers_of(vec![harness_driver(), common::FakePage::default().driver()]);
    let mut run = new_run("run-t");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, true, &quick(), &cancel, &mut |p: ReplayProgress| {
        if p.phase == "step" {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .await
    .unwrap();
    assert_eq!(browsers.opened, 1, "{:?}", run.cases);
    assert_eq!(run.cases[0].retried, None);
}

/// Review focus 5.
#[tokio::test]
async fn an_address_that_changes_a_moment_after_the_last_click_still_counts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let (d, _app) = common::menu_app(MENU, "/hr/home/index", 3);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(run.cases[0].proposed, "Passed", "{}", run.cases[0].reason);
}

#[tokio::test]
async fn a_sign_in_in_the_middle_of_a_script_goes_back_to_the_module_before_the_next_action() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(
        root,
        &script(1, Some("admin"), serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] },
            { "step_number": 2, "actions": [{ "kind": "sign_in", "account": "admin" }, { "kind": "check_text", "value": "yes" }] }
        ])),
    )
    .unwrap();
    let (d, app) = common::menu_app(MENU, "/hr/home/index", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {}).await.unwrap();

    // The second sign-in may come from the saved session (no form, so no
    // `#go`) - what matters is what happens around it.
    let log: Vec<String> = app.log.lock().unwrap().iter().filter(|l| *l != "click #go").cloned().collect();
    assert_eq!(
        log,
        vec![
            "navigate /hr/home/index", "click Leave", "click Apply Leave", "check yes",
            "navigate /hr/home/index", "click Leave", "click Apply Leave", "check yes",
        ]
    );
    let rec = &run.cases[0];
    let step2 = rec.steps.iter().find(|s| s.step_number == 2).unwrap();
    assert!(step2.outcomes[0].ok && step2.outcomes[0].detail.ends_with("; then Go to Leave"), "{:?}", step2.outcomes);
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
}

#[tokio::test]
async fn a_project_whose_file_has_no_paths_runs_exactly_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_nav(root, "Acme", "Web", &NavFile::default()).unwrap();
    store::save_script(root, &passing_script(1)).unwrap();
    let mut browsers = browsers_of(vec![common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let rec = &run.cases[0];
    assert_eq!(rec.steps.iter().map(|s| s.step_number).collect::<Vec<_>>(), vec![1]);
    assert_eq!(rec.proposed, "Passed");
    assert!(browsers.returned[0].calls_to("Page.navigate").is_empty(), "no trip home without paths");
}

/// Review focus 4.
#[tokio::test]
async fn an_unreadable_module_paths_file_stops_the_run_before_any_browser_opens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    std::fs::create_dir_all(root.join("projects")).unwrap();
    std::fs::write(nav_path(root, "Acme", "Web"), "{ not json").unwrap();
    let mut browsers = browsers_of(vec![common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    let err = run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap_err();
    assert!(err.contains("areas file is not readable"), "{err}");
    assert_eq!(browsers.opened, 0);
    assert!(run.cases.is_empty());
}

// ---- A page's own words are never a failed trip ---------------------------

/// Text a page or a script could carry that reads like the runner's own
/// sentence for a failed trip to the module.
const LOOKALIKE: &str = "Could not reach module \"Payroll\": click 2, link \"Pay\" - gone.";

fn checks_for(account: Option<&str>, value: &str) -> CaseScript {
    script(1, account, serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": value }] }]))
}

/// Review I1.
#[tokio::test]
async fn a_failed_check_whose_value_reads_like_an_unreached_module_is_failed_in_a_project_without_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &checks_for(None, LOOKALIKE)).unwrap();
    let mut browsers = browsers_of(vec![checking_driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(1, "case 1".to_string())], &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Failed", "{}", rec.reason);
    assert_eq!(rec.reason, format!("step 1: page does NOT contain {LOOKALIKE}"));
}

/// Review I1.
#[tokio::test]
async fn a_failed_check_whose_value_reads_like_an_unreached_module_is_failed_in_a_project_with_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &checks_for(Some("admin"), LOOKALIKE)).unwrap();
    let (d, _app) = common::menu_app(MENU, "/hr/home/index", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, false, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let rec = &run.cases[0];
    assert!(rec.steps.iter().find(|s| s.step_number == MODULE_STEP).unwrap().outcomes[0].ok, "{:?}", rec.steps);
    assert_eq!(rec.proposed, "Failed", "{}", rec.reason);
    assert_eq!(rec.reason, format!("step 1: page does NOT contain {LOOKALIKE}"));
}

/// Review I1: an application's own alert that happens to use the sentence.
#[tokio::test]
async fn a_page_dialog_that_reads_like_an_unreached_module_leaves_the_failure_the_pages() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &checks_for(None, "no")).unwrap();
    let mut d = checking_driver();
    d.dialogs.push(format!("alert: {LOOKALIKE}"));
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(1, "case 1".to_string())], &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    let rec = &run.cases[0];
    assert!(rec.reason.contains(LOOKALIKE), "the dialog is reported: {}", rec.reason);
    assert_eq!(rec.proposed, "Failed", "{}", rec.reason);
    assert!(rec.reason.starts_with("step 1: page does NOT contain no"), "{}", rec.reason);
}

/// Fix round 1, I1: a failed `navigate` with a page dialog appended to its
/// detail that happens to read like the runner's own unreached-module
/// sentence is still Failed - the dialog is the page's words, not the
/// runner's, and `execute_in` appends it to any outcome, including this one.
#[test]
fn a_failed_navigate_whose_appended_dialog_reads_like_an_unreached_module_is_failed() {
    let sc = script(1, None, serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/x" }] }]));
    let detail = format!(
        "https://app.example/x did not finish loading within 1ms (the page showed alert: {LOOKALIKE} and it was accepted)"
    );
    let steps = vec![StepRecord { step_number: 1, outcomes: vec![ActionOutcome::failed(detail)], screenshot: None, downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None }];
    let p = propose(&sc, &steps, None, false);
    assert_eq!(p.verdict, "Failed", "{}", p.reason);
}

/// Fix round 1, I1: same as above, but the appended dialog happens to be
/// the address sentence itself - still the page's words, not the runner's,
/// since the runner never appends anything to its own refusal (it never
/// goes through `execute_in`).
#[test]
fn a_failed_navigate_whose_appended_dialog_is_the_address_sentence_itself_is_still_failed() {
    let sc = script(1, None, serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/x" }] }]));
    let detail = format!(
        "https://app.example/x did not finish loading within 1ms (the page showed alert: {} and it was accepted)",
        no_address(1)
    );
    let steps = vec![StepRecord { step_number: 1, outcomes: vec![ActionOutcome::failed(detail)], screenshot: None, downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None }];
    let p = propose(&sc, &steps, None, false);
    assert_eq!(p.verdict, "Failed", "{}", p.reason);
}

/// Fix round 1, I1: a `check_text` whose own scripted value is the address
/// sentence is still a script defect - the same rule Review I1 (Task 2)
/// already pinned for the unreached-module sentence.
#[test]
fn a_check_text_whose_value_is_the_address_sentence_itself_is_failed() {
    let sc = script(1, None, serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": no_address(1) }] }]));
    let detail = format!("page does NOT contain {}", no_address(1));
    let steps = vec![StepRecord { step_number: 1, outcomes: vec![ActionOutcome::failed(detail.clone())], screenshot: None, downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None }];
    let p = propose(&sc, &steps, None, false);
    assert_eq!(p.verdict, "Failed", "{}", p.reason);
    assert_eq!(p.reason, format!("step 1: {detail}"));
}

/// Review I1: the same words are Blocked on a `sign_in` (its trip back to
/// the module) and Failed on any other action.
#[test]
fn only_a_sign_in_whose_trip_back_failed_is_blocked_by_those_words() {
    let unreached = format!(
        "Could not reach module \"Leave\": click 2, link \"Apply Leave\" - no visible match.{AFTER_SIGN_IN}signed in as admin)"
    );
    let steps = vec![StepRecord {
        step_number: 1,
        outcomes: vec![
            ActionOutcome::failed(unreached.clone()),
            ActionOutcome::failed("not run: the module screen was not reached after the sign-in"),
        ],
        screenshot: None,
        downloads: vec![],
        tab: None,
        dialog: None,
        components: Vec::new(), duration_ms: None,
    }];
    let signs_in = script(1, Some("admin"), serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "sign_in", "account": "admin" }, { "kind": "check_text", "value": "yes" }
    ] }]));
    let p = propose(&signs_in, &steps, Some(true), false);
    assert_eq!((p.verdict, p.reason.as_str()), ("Blocked", unreached.as_str()));

    let checks = script(1, Some("admin"), serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "check_text", "value": "x" }, { "kind": "check_text", "value": "yes" }
    ] }]));
    let p = propose(&checks, &steps, Some(true), false);
    assert_eq!(p.verdict, "Failed", "{}", p.reason);
}

/// A failed sign-in never went back to the module, so its words are the
/// sign-in's own, and those can carry a page's dialog. A dialog holding
/// the runner's joiner and sentence further in must not make the case
/// Blocked with the page's words as the reason.
#[test]
fn a_failed_sign_in_whose_dialog_looks_like_a_failed_trip_back_is_not_blocked_by_it() {
    let lookalike = format!("; then {UNREACHED_PREFIX}Leave\": click 1, link \"Leave\" - no visible match.");
    let detail = format!(
        "sign-in stopped at step 1: waited 1ms for the page (the page showed alert: x{lookalike} and it was accepted)"
    );
    let steps = vec![StepRecord { step_number: 1, outcomes: vec![ActionOutcome::failed(detail.clone())], screenshot: None, downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None }];
    let signs_in = script(1, Some("admin"), serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "sign_in", "account": "admin" }
    ] }]));
    let p = propose(&signs_in, &steps, Some(true), false);
    assert!(!p.reason.starts_with(UNREACHED_PREFIX), "{}", p.reason);
    assert_eq!((p.verdict, p.reason), ("Failed", format!("step 1: {detail}")));
}

/// A mid-script `sign_in` whose trip back to the module fails: the rest of
/// the step is not run, and the case is Blocked on the runner's sentence.
#[tokio::test]
async fn a_mid_script_sign_in_whose_trip_back_fails_blocks_the_case() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    let signs_in = script(1, Some("admin"), serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "sign_in", "account": "admin" }, { "kind": "check_text", "value": "yes" }
    ] }]));
    let (mut d, app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    let route = Route::new(&common::menu_recipe(), leave_nav().modules[0].clone());
    let mut account = None;
    let mut held = Held::supervised();
    let outcomes = run_step_routed(&mut d, root, "Acme", "Web", &signs_in.steps[0], &quick(), &mut account, &mut held, Some(&route), v2_lib::autorun::runner::AreaRoute::To(&route))
        .await
        .unwrap();
    assert!(!outcomes[0].ok, "{outcomes:?}");
    // The runner's sentence first, the sign-in after it: read by position.
    assert!(outcomes[0].detail.starts_with(&format!("{UNREACHED_PREFIX}Leave\": click 2, ")), "{}", outcomes[0].detail);
    assert!(outcomes[0].detail.ends_with(&format!("{AFTER_SIGN_IN}signed in as Administrator)")), "{}", outcomes[0].detail);
    assert!(outcomes[0].detail.contains(&format!("{TRIED_TWICE}{AFTER_SIGN_IN}")), "{}", outcomes[0].detail);
    assert_eq!(app.log.lock().unwrap().iter().filter(|l| *l == "click Leave").count(), 2, "one more go, then no more");
    assert_eq!(outcomes[1].detail, "not run: the module screen was not reached after the sign-in");
    assert!(!app.log.lock().unwrap().iter().any(|l| l.starts_with("check")));
    let steps = vec![StepRecord { step_number: 1, outcomes, screenshot: None, downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None }];
    let p = propose(&signs_in, &steps, Some(true), false);
    assert_eq!(p.verdict, "Blocked", "{}", p.reason);
    assert!(p.reason.starts_with(UNREACHED_PREFIX), "{}", p.reason);
}

fn lee() -> v2_lib::autorun::accounts::Account {
    v2_lib::autorun::accounts::Account {
        key: "lee".into(),
        label: "Lee".into(),
        username: "lee".into(),
        password: common::PASSWORD.into(),
    }
}

#[tokio::test]
async fn the_runs_account_signs_in_every_case_over_the_account_a_script_names() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    save_accounts(root, &[common::account(), lee()]).unwrap();
    store::save_script(root, &script(1, Some("admin"), serde_json::json!([]))).unwrap();
    store::save_script(root, &script(2, None, serde_json::json!([]))).unwrap();
    let (d1, _) = common::stateful_app(false, None);
    let (d2, _) = common::stateful_app(false, None);
    let mut browsers = browsers_of(vec![d1, d2]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None), to_run(2, None)], Some("lee"), false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    for case in &run.cases {
        assert_eq!(case.account.as_deref(), Some("lee"), "case {}", case.case_id);
        assert_eq!(case.steps[0].step_number, SIGN_IN_STEP);
        assert!(case.steps[0].outcomes.iter().all(|o| o.ok), "{:?}", case.steps[0].outcomes);
    }
}

#[tokio::test]
async fn a_script_saved_before_addresses_were_switched_off_is_blocked_at_its_navigate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_nav(root, "Acme", "Web", &NavFile { direct_urls: false, modules: vec![], save_words: vec![] }).unwrap();
    // `save_script` does not apply the rule: this is a file from before.
    store::save_script(
        root,
        &script(1, None, serde_json::json!([{ "step_number": 1, "actions": [
            { "kind": "navigate", "url": "https://app.example/leave" },
            { "kind": "check_text", "value": "yes" }
        ] }])),
    )
    .unwrap();
    let mut browsers = browsers_of(vec![common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(1, "case 1".to_string())], &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].reason, no_address(1));
}

/// After an unattended run, the project's quirks count what THIS call ran
/// - a case record already in the run (a resumed run) is not counted again.
#[tokio::test]
async fn a_run_counts_quirk_evidence_for_the_cases_it_ran_and_no_others() {
    use v2_lib::autorun::quirks::{load_quirks, save_quirks, Quirk, QuirkSource};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    let mut about_1 = Quirk::new("case one's page settles late", "assistant", "autorun", 1);
    about_1.sources = vec![QuirkSource { case_id: 1, steps: vec![1], class: None }];
    let mut about_2 = Quirk::new("case two's grid paginates", "assistant", "autorun", 2);
    about_2.sources = vec![QuirkSource { case_id: 2, steps: vec![1], class: None }];
    save_quirks(root, "Acme", "Web", &[about_1, about_2]).unwrap();

    // The run already holds case 2 from an earlier call, step 1 passed.
    let mut run = new_run("run-q");
    let mut earlier: v2_lib::autorun::CaseRecord = serde_json::from_value(serde_json::json!({
        "case_id": 2, "title": "case 2", "verdict": "", "note": "", "steps": []
    }))
    .unwrap();
    earlier.steps.push(StepRecord { step_number: 1, outcomes: vec![ActionOutcome::passed("ok")], screenshot: None, downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None });
    run.cases.push(earlier);

    let mut browsers = FakeBrowsers {
        queue: [Some(common::FakePage::default().driver())].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let cancel = AtomicBool::new(false);
    let cases = vec![(1, "case 1".to_string())];
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(run.cases.last().unwrap().proposed, "Passed");

    let quirks = load_quirks(root, "Acme", "Web").unwrap();
    let one = quirks.iter().find(|q| q.text.starts_with("case one")).unwrap();
    let two = quirks.iter().find(|q| q.text.starts_with("case two")).unwrap();
    assert_eq!((one.confirmed, one.doubted), (1, 0), "the case this call ran confirms its note");
    assert!(one.last_confirmed.is_some());
    assert_eq!((two.confirmed, two.doubted), (0, 0), "the earlier record is not counted again");
}

// ------------------------------------------------------------ preconditions

/// A script with one precondition: the cycle-flow's Publish stage for 274.
fn needs_publish(case_id: i32, account: Option<&str>) -> CaseScript {
    let mut sc = script(case_id, account, serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ]));
    sc.preconditions = vec![v2_lib::autorun::Precondition {
        flow: "pms-performance-cycle".into(),
        stage: "publish".into(),
        value: serde_json::json!(274),
        why: Some("the case opens a published cycle".into()),
    }];
    sc
}

fn save_cycle_flow(root: &Path) {
    let flow: v2_lib::api_templates::flow::Flow = serde_json::from_value(common::cycle_flow_json()).unwrap();
    v2_lib::api_templates::flow_store::save(root, "Acme", "Web", &flow).unwrap();
}

/// A precondition not met Blocks its case before a browser opens - so it
/// never signs in - with the sentence as its reason, and the next case
/// runs.
#[tokio::test]
async fn a_precondition_not_met_blocks_the_case_before_sign_in_and_the_run_goes_on() {
    let _g = crate::serial::activity_log();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_cycle_flow(root);
    store::save_script(root, &needs_publish(1, Some("lead"))).unwrap();
    store::save_script(root, &passing_script(2)).unwrap();
    let db = PreconditionDb::Ready(common::FakeStageDb::new().answer("/*publish*/", Ok(false)));
    let mut browsers = browsers_of(vec![common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases_checked(
        &mut browsers,
        root,
        "Acme",
        "Web",
        &mut run,
        &[to_run(1, None), to_run(2, None)],
        None,
        false,
        &quick(),
        &cancel,
        &db,
        &mut |_| {},
    )
    .await
    .unwrap();
    assert_eq!(browsers.opened, 1, "only the second case opened a browser");
    let blocked = &run.cases[0];
    assert_eq!(blocked.proposed, "Blocked");
    assert_eq!(
        blocked.reason,
        "precondition not met: Publish for 274 (Performance cycle wizard) - the case opens a published cycle"
    );
    let PreconditionDb::Ready(fake) = &db else { unreachable!() };
    let asked = fake.calls();
    assert_eq!(asked.len(), 1, "only the case with a precondition asks the database: {asked:?}");
    assert!(asked[0].contains("cycle_id = 274"), "{asked:?}");
    assert_eq!(run.cases[1].proposed, "Passed", "{:?}", run.cases[1]);
}

#[tokio::test]
async fn every_precondition_done_lets_the_case_run() {
    let _g = crate::serial::activity_log();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_cycle_flow(root);
    store::save_script(root, &needs_publish(1, None)).unwrap();
    let db = PreconditionDb::Ready(common::FakeStageDb::new().answer("/*publish*/", Ok(true)));
    let mut browsers = browsers_of(vec![common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases_checked(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, false, &quick(), &cancel, &db, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(browsers.opened, 1);
    assert_eq!(run.cases[0].proposed, "Passed", "{:?}", run.cases[0]);
    assert_eq!(run.cases[0].notice, None, "checked, so nothing to say");
}

/// While Database Read Access is off no precondition is asked: the case
/// with preconditions runs, carrying the notice, and the one without
/// carries none.
#[tokio::test]
async fn with_reading_off_a_case_runs_unchecked_and_its_record_says_so() {
    let _g = crate::serial::activity_log();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_cycle_flow(root);
    store::save_script(root, &needs_publish(1, None)).unwrap();
    store::save_script(root, &passing_script(2)).unwrap();
    let db: PreconditionDb<common::FakeStageDb> = PreconditionDb::ReadingOff;
    let mut browsers = browsers_of(vec![common::FakePage::default().driver(), common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases_checked(
        &mut browsers,
        root,
        "Acme",
        "Web",
        &mut run,
        &[to_run(1, None), to_run(2, None)],
        None,
        false,
        &quick(),
        &cancel,
        &db,
        &mut |_| {},
    )
    .await
    .unwrap();
    assert_eq!(browsers.opened, 2, "neither case was Blocked");
    assert_ne!(run.cases[0].proposed, "Blocked", "{:?}", run.cases[0]);
    assert_eq!(
        run.cases[0].notice.as_deref(),
        Some("preconditions were not checked: Database Read Access is off on the AI Bridge tab")
    );
    assert_eq!(run.cases[0].notice.as_deref(), Some(NOT_CHECKED));
    assert_eq!(run.cases[1].notice, None, "a script without preconditions is untouched");
    assert_eq!(run.cases[1].proposed, "Passed");
    // And it is on the saved run.
    let saved = store::load_run(root, "run-x").unwrap().unwrap();
    assert_eq!(saved.cases[0].notice.as_deref(), Some(NOT_CHECKED));
}

/// With no database to ask, a case with preconditions is Blocked and one
/// without runs as it always has.
#[tokio::test]
async fn without_a_database_only_a_case_with_preconditions_is_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_cycle_flow(root);
    store::save_script(root, &needs_publish(1, None)).unwrap();
    store::save_script(root, &passing_script(2)).unwrap();
    let mut browsers = browsers_of(vec![common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None), to_run(2, None)], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(browsers.opened, 1);
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].reason, "preconditions need a database chosen on the AI Bridge tab");
    assert_eq!(run.cases[1].proposed, "Passed");
}

/// Every case's browser saves its downloads in the run's own folder, beside
/// its screenshots.
#[tokio::test]
async fn every_cases_browser_saves_its_downloads_in_the_runs_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    store::save_script(root, &passing_script(2)).unwrap();
    let mut browsers = FakeBrowsers {
        queue: [Some(common::FakePage::default().driver()), Some(common::FakePage::default().driver())].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-77");
    let cases = vec![(1, "case 1".to_string()), (2, "case 2".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let folder = store::downloads_dir(root, "run-77");
    assert_eq!(browsers.returned.len(), 2);
    for d in &browsers.returned {
        assert_eq!(d.download_dirs, [folder.clone()]);
    }
}

/// A download still on its way when a case ends.
fn arriving() -> v2_lib::browser::downloads::DownloadEntry {
    v2_lib::browser::downloads::DownloadEntry {
        guid: "g-1".into(),
        name: "Template.xlsx".into(),
        path: "g-1".into(),
        started_at: std::time::Instant::now(),
        state: v2_lib::browser::downloads::DownloadState::InProgress,
        bytes: 0,
    }
}

/// A Stop pressed while a case's browser waits for a download ends the
/// wait at the next look, not after the whole settle.
#[tokio::test]
async fn a_stop_ends_the_wait_for_a_download_at_once() {
    let mut d = common::FakePage::default().driver();
    d.downloads.push(arriving());
    let cancel = AtomicBool::new(false);
    let began = std::time::Instant::now();
    let stop = async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        cancel.store(true, Ordering::SeqCst);
    };
    tokio::join!(settle_downloads(&mut d, &cancel), stop);
    assert!(began.elapsed() < std::time::Duration::from_millis(1000), "took {:?}", began.elapsed());
}

/// Stopped before the settle, the browser closes without waiting at all.
#[tokio::test]
async fn a_stopped_case_does_not_wait_for_its_downloads() {
    let mut d = common::FakePage::default().driver();
    d.downloads.push(arriving());
    let cancel = AtomicBool::new(true);
    let began = std::time::Instant::now();
    settle_downloads(&mut d, &cancel).await;
    assert!(began.elapsed() < std::time::Duration::from_millis(50), "took {:?}", began.elapsed());
}

/// Review follow-up 4: a download stuck in progress costs a case one
/// settle, not one in the case and another before its browser closes.
#[tokio::test]
async fn a_stuck_download_costs_a_case_one_settle() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    let mut d = common::FakePage::default().driver();
    d.downloads.push(arriving());
    let mut browsers = FakeBrowsers { queue: [Some(d)].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-78");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    let started = std::time::Instant::now();
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let took = started.elapsed();
    assert!(took >= std::time::Duration::from_secs(5), "the case did wait for the download: {took:?}");
    assert!(took < std::time::Duration::from_secs(9), "two settles: {took:?}");
    assert_eq!(browsers.closed, 1);
}

// ---- Tabs ------------------------------------------------------------------

/// `checking_driver` with one tab the page opened, waiting to be named.
fn tabbed(mut d: common::ScriptedDriver) -> common::ScriptedDriver {
    d.tabs.unnamed = 1;
    d
}

/// A one-step script that follows a new tab and checks `value` in it.
fn in_a_tab(case_id: i32, value: &str) -> CaseScript {
    script(case_id, None, serde_json::json!([
        { "step_number": 1, "actions": [
            { "kind": "expect_tab", "name": "report" },
            { "kind": "switch_tab", "name": "report" },
            { "kind": "check_text", "value": value }
        ] },
        { "step_number": 2, "actions": [{ "kind": "check_text", "value": value }] }
    ]))
}

/// Review Focus 4: whichever way a case ends, every tab but `main` is
/// closed before its browser is given back.
#[tokio::test]
async fn every_case_closes_its_tabs_whether_it_passed_or_failed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &in_a_tab(1, "yes")).unwrap();
    store::save_script(root, &in_a_tab(2, "no")).unwrap();
    let mut browsers = FakeBrowsers {
        queue: [Some(tabbed(checking_driver())), Some(tabbed(checking_driver()))].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-tabs");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None), to_run(2, None)], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(run.cases[0].proposed, "Passed", "{:?}", run.cases[0]);
    assert_eq!(run.cases[1].proposed, "Failed", "{:?}", run.cases[1]);
    for d in &browsers.returned {
        assert_eq!(d.tabs.closed_others, 1, "a case gave its browser back with its tabs open");
        assert!(d.tabs.open.is_empty());
    }
}

/// A case stopped before it began, and one whose guard could not be
/// switched on, still close every tab but `main` on the way out.
#[tokio::test]
async fn a_stopped_case_and_a_case_that_never_started_close_their_tabs_too() {
    use v2_lib::autorun::replay::run_case;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut d = tabbed(checking_driver());
    d.tabs.open.push("left".into());
    let stopped = AtomicBool::new(true);
    let rec = run_case(&mut d, root, "Acme", "Web", &in_a_tab(1, "yes"), &quick(), &stopped, &mut |_| {}).await;
    assert_eq!(rec.reason, "stopped before it finished");
    assert_eq!(d.tabs.closed_others, 1);
    assert!(d.tabs.open.is_empty());

    let mut refusing = common::ScriptedDriver::new(|method, _| match method {
        "Fetch.enable" => Err(CdpError::Protocol { method: "Fetch.enable".into(), message: "no".into() }),
        _ => Ok(serde_json::json!({})),
    });
    refusing.tabs.open.push("left".into());
    let mut no_save = in_a_tab(2, "yes");
    no_save.no_save = true;
    let go = AtomicBool::new(false);
    let rec = run_case(&mut refusing, root, "Acme", "Web", &no_save, &quick(), &go, &mut |_| {}).await;
    assert_eq!(rec.proposed, "Blocked", "{rec:?}");
    assert_eq!(refusing.tabs.closed_others, 1);
}

/// A case retried after a failure that looked transient: each go closes
/// its own tabs, and the second go follows its tab afresh.
#[tokio::test]
async fn a_retried_case_closes_the_tabs_of_each_go() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &in_a_tab(1, "yes")).unwrap();
    let mut browsers = FakeBrowsers {
        queue: [Some(tabbed(harness_driver())), Some(tabbed(checking_driver()))].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-retry-tabs");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, true, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(browsers.returned.len(), 2, "no retry");
    assert_eq!(run.cases[0].proposed, "Passed", "{:?}", run.cases[0]);
    for d in &browsers.returned {
        assert_eq!(d.tabs.closed_others, 1);
        assert!(d.tabs.open.is_empty());
    }
}

/// A run that pauses at a reset point: the case before it has closed its
/// tabs, and the case after starts with none.
#[tokio::test]
async fn a_reset_pause_comes_after_the_case_before_closed_its_tabs() {
    use v2_lib::autorun::plan::Reset;
    use v2_lib::autorun::replay::{run_cases_planned, ResetGate};
    struct Carry;
    impl ResetGate for Carry {
        async fn wait(&mut self, _reset: &Reset, _remaining: &[i32], _cancel: &AtomicBool) -> bool {
            true
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &in_a_tab(1, "yes")).unwrap();
    store::save_script(root, &in_a_tab(2, "yes")).unwrap();
    let mut browsers = FakeBrowsers {
        queue: [Some(tabbed(checking_driver())), Some(tabbed(checking_driver()))].into(),
        opened: 0,
        closed: 0,
        returned: vec![],
    };
    let mut run = new_run("run-reset-tabs");
    let cancel = AtomicBool::new(false);
    let reset = Reset { before_case_id: 2, names: vec!["x".into()], changed_by: vec![] };
    run_cases_planned(
        &mut browsers,
        root,
        "Acme",
        "Web",
        &mut run,
        &[to_run(1, None), to_run(2, None)],
        None,
        false,
        &quick(),
        &cancel,
        &PreconditionDb::<v2_lib::autorun::preconditions::NoDb>::ReadingOff,
        &[reset],
        &mut Carry,
        &mut |_| {},
    )
    .await
    .unwrap();
    assert_eq!(run.resets.len(), 1);
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", run.cases);
    for d in &browsers.returned {
        assert_eq!(d.tabs.closed_others, 1);
    }
}

/// The run file says which tab a step ran in, when it was not `main`, and
/// says nothing for a step in `main`.
#[tokio::test]
async fn a_step_that_ran_in_another_tab_says_so_on_its_record() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut s = in_a_tab(1, "yes");
    s.steps[1].actions.insert(0, serde_json::from_value(serde_json::json!({ "kind": "switch_tab", "name": "main" })).unwrap());
    store::save_script(root, &s).unwrap();
    let mut browsers = FakeBrowsers { queue: [Some(tabbed(checking_driver()))].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-tab-record");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    let steps = &run.cases[0].steps;
    assert_eq!(steps[0].tab.as_deref(), Some("report"));
    assert_eq!(steps[1].tab, None);
    let saved = serde_json::to_value(&steps[0]).unwrap();
    assert_eq!(saved["tab"], "report");
    let in_main = serde_json::to_value(&steps[1]).unwrap();
    assert!(in_main.get("tab").is_none(), "{in_main}");
}

/// Review Focus 5: a file a step saved in another tab is on that step's
/// record.
#[tokio::test]
async fn a_file_saved_in_another_tab_is_on_the_step_that_ran_there() {
    use v2_lib::browser::downloads::{DownloadEntry, DownloadState};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &in_a_tab(1, "yes")).unwrap();
    let mut d = tabbed(checking_driver());
    d.downloads_on_call.push((
        "Runtime.callFunctionOn".into(),
        DownloadEntry {
            guid: "g".into(),
            name: "report.csv".into(),
            path: root.join("report.csv"),
            started_at: std::time::Instant::now(),
            state: DownloadState::Completed,
            bytes: 3,
        },
    ));
    let mut browsers = FakeBrowsers { queue: [Some(d)].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-tab-download");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, false, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    let steps = &run.cases[0].steps;
    assert_eq!(steps[0].tab.as_deref(), Some("report"));
    assert_eq!(steps[0].downloads, ["report.csv"]);
    assert!(steps[1].downloads.is_empty());
}

/// A run file and a script saved before tabs read and write back exactly
/// as they were.
#[test]
fn a_run_file_and_a_script_from_before_tabs_round_trip_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let old = script(5, Some("hr.admin"), serde_json::json!([
        { "step_number": 1, "actions": [
            { "kind": "navigate", "url": "/hr/home" },
            { "kind": "click", "selector": { "role": "button", "name": "Save" } },
            { "kind": "expect_visible", "selector": { "css": "#done" }, "timeout_ms": 5000 }
        ] }
    ]));
    fn find(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(f) = find(&p, name) {
                    return Some(f);
                }
            } else if p.file_name().is_some_and(|n| n == name) {
                return Some(p);
            }
        }
        None
    }
    store::save_script(root, &old).unwrap();
    let file = find(root, "case-5.json").expect("the script was not saved");
    let first = std::fs::read(&file).unwrap();
    let loaded = store::load_script(root, 5).unwrap().unwrap();
    store::save_script(root, &loaded).unwrap();
    let second = std::fs::read(&file).unwrap();
    assert_eq!(first, second, "an old script changed on its way through");

    let record = r#"{"step_number":1,"outcomes":[{"ok":true,"detail":"clicked"}],"screenshot":"a.jpg","downloads":["x.csv"]}"#;
    let step: StepRecord = serde_json::from_str(record).unwrap();
    assert_eq!(step.tab, None);
    assert_eq!(serde_json::to_string(&step).unwrap(), record);
}

/// An unattended run keeps, on each step's record, the components it used
/// and their versions, and each expanded action's outcome names its
/// component.
#[tokio::test]
async fn an_unattended_run_records_the_components_each_step_used() {
    use v2_lib::autorun::components::{put, ComponentUse};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let c = serde_json::from_value(serde_json::json!({
        "name": "Say yes", "description": "d", "version": 3,
        "inputs": [{ "name": "word", "kind": "text", "description": "" }],
        "actions": [{ "kind": "check_text", "value": "{{word}}" }, { "kind": "check_text", "value": "yes" }]
    }))
    .unwrap();
    put(root, "Acme", "Web", c).unwrap();
    let case = script(
        1,
        None,
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "use_component", "component": "say yes", "inputs": { "word": "yes" } }] },
            { "step_number": 2, "actions": [{ "kind": "check_text", "value": "yes" }] },
        ]),
    );
    store::save_script(root, &case).unwrap();
    let mut browsers = FakeBrowsers { queue: [Some(checking_driver())].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();

    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Passed", "{:?}", rec.steps);
    let one = rec.steps.iter().find(|s| s.step_number == 1).unwrap();
    assert_eq!(one.components, vec![ComponentUse { name: "Say yes".into(), version: 3 }]);
    assert_eq!(one.outcomes.len(), 2);
    assert!(one.outcomes.iter().all(|o| o.ok && o.component.as_deref() == Some("Say yes")), "{:?}", one.outcomes);
    let two = rec.steps.iter().find(|s| s.step_number == 2).unwrap();
    assert!(two.components.is_empty() && two.outcomes[0].component.is_none());
    // And it is on disk the same way.
    let saved = store::list_runs(root).into_iter().find(|r| r.id == "run-x").unwrap();
    assert_eq!(saved.cases[0].steps.iter().find(|s| s.step_number == 1).unwrap().components, one.components);
}

// ---- components ----

/// A component's `navigate` refused because this project does not allow
/// addresses is the run's refusal, as it is in a script's own step.
#[test]
fn a_refusal_inside_a_component_is_blocked_not_failed() {
    use v2_lib::autorun::components::ComponentFile;
    use v2_lib::autorun::replay::propose_with;
    let file: ComponentFile = serde_json::from_value(serde_json::json!({ "components": [{
        "name": "Open help", "description": "d", "inputs": [], "version": 1,
        "actions": [{ "kind": "navigate", "url": "https://app.example/help" }]
    }] }))
    .unwrap();
    let sc = script(1, None, serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "check_text", "value": "yes" },
        { "kind": "use_component", "component": "Open help", "inputs": {} }
    ] }]));
    let mut refused = ActionOutcome::failed(no_address(1));
    refused.component = Some("Open help".into());
    let steps = vec![StepRecord {
        step_number: 1,
        outcomes: vec![ActionOutcome::passed("page contains yes"), refused],
        screenshot: None,
        downloads: vec![],
        tab: None,
        dialog: None,
        components: Vec::new(), duration_ms: None,
    }];
    let p = propose_with(&sc, &steps, None, false, &file);
    assert_eq!((p.verdict, p.reason.as_str()), ("Blocked", no_address(1).as_str()));
}

#[tokio::test]
async fn a_case_record_carries_its_phase_timings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    let mut browsers = FakeBrowsers { queue: [Some(common::FakePage::default().driver())].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();

    let p = run.cases[0].phases.as_ref().expect("a case that ran carries its phase timings");
    // A case with no account and no route has no sign-in and no area trip.
    assert_eq!((p.sign_in_ms, p.area_ms), (0, 0));
    // Total spans open to close, so it holds every part.
    assert!(p.total_ms >= p.open_ms + p.sign_in_ms + p.area_ms + p.steps_ms + p.close_ms, "{p:?}");
}

#[tokio::test]
async fn a_signed_in_case_times_its_sign_in_inside_its_total() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    save_accounts(root, &[common::account()]).unwrap();
    store::save_script(root, &script(1, Some("admin"), serde_json::json!([]))).unwrap();
    let (d, _state) = common::stateful_app(false, None);
    let mut browsers = FakeBrowsers { queue: [Some(d)].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(1, "case 1".to_string())], &quick(), &cancel, &mut |_| {}).await.unwrap();

    let rec = &run.cases[0];
    let p = rec.phases.as_ref().unwrap();
    assert!(rec.steps[0].duration_ms.is_some(), "the sign-in step is timed");
    assert!(p.total_ms >= p.sign_in_ms, "{p:?}");
}

#[tokio::test]
async fn steps_carry_their_duration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let two = script(1, None, serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "no" }] },
        { "step_number": 2, "actions": [{ "kind": "check_text", "value": "yes" }] }
    ]));
    store::save_script(root, &two).unwrap();
    let mut browsers = FakeBrowsers { queue: [Some(checking_driver())].into(), opened: 0, closed: 0, returned: vec![] };
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(1, "case 1".to_string())], &quick(), &cancel, &mut |_| {}).await.unwrap();

    let steps = &run.cases[0].steps;
    assert!(steps[0].duration_ms.is_some(), "a step that ran is timed");
    assert_eq!(steps[1].duration_ms, None, "a step that did not run has no duration");
    let sum: u64 = steps.iter().filter_map(|s| s.duration_ms).sum();
    assert!(run.cases[0].phases.as_ref().unwrap().steps_ms >= sum);
}

// ---- Screenshots are pruned once per run, not on every save ----

fn shot_files(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(root.join("shots"))
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// `n` old pictures, older than anything a run takes now.
fn seed_old_shots(root: &Path, n: usize) {
    std::fs::create_dir_all(root.join("shots")).unwrap();
    for i in 0..n {
        std::fs::write(root.join("shots").join(format!("shot-1-{i:06}.jpg")), b"old").unwrap();
    }
}

/// Saving a picture reads no run and drops nothing: the folder holds every
/// one past the budget until the run ends.
#[test]
fn saving_a_case_does_not_read_other_runs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for _ in 0..1003 {
        store::save_shot(root, b"x").unwrap();
    }
    assert_eq!(shot_files(root).len(), 1003, "no save pruned anything");
}

/// The run drops the oldest beyond the budget once, after its last case:
/// while a case is still being reported the folder is untouched, and when
/// the run returns it is down to the budget with the run's own picture kept.
#[tokio::test]
async fn a_run_prunes_once_at_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_old_shots(root, 1005);
    store::save_script(root, &one_check(None)).unwrap();
    let mut browsers = browsers_of(vec![checking_driver()]);
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    let mut at_done = 0usize;
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |p| {
        if p.phase == "done" {
            at_done = shot_files(root).len();
        }
    })
    .await
    .unwrap();
    assert!(at_done > 1005, "nothing was pruned while the case was saved: {at_done}");
    let left = shot_files(root);
    // The budget, plus this run's own pictures: the run is unpublished, so
    // its pictures are protected and sit outside the budget.
    let own_count = run.cases[0].steps.iter().filter(|s| s.screenshot.is_some()).count();
    assert_eq!(left.len(), 1000 + own_count, "pruned to the budget at the end");
    assert!(!left.contains(&"shot-1-000000.jpg".to_string()), "the oldest went");
    let own = run.cases[0].steps[0].screenshot.as_ref().expect("the case took a picture");
    assert!(left.contains(own), "the run's own newest picture stays");
}

/// A stopped run prunes too, and a picture an unpublished run still
/// references survives however old it is.
#[tokio::test]
async fn protected_shots_survive_the_end_of_run_prune() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_old_shots(root, 1005);
    let mut earlier = new_run("run-earlier");
    earlier.cases.push(v2_lib::autorun::CaseRecord {
        case_id: 9,
        title: "t".into(),
        verdict: "".into(),
        note: "".into(),
        steps: vec![StepRecord { step_number: 1, outcomes: vec![], screenshot: Some("shot-1-000000.jpg".into()), downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None }],
        proposed: "".into(),
        reason: "".into(),
        duration_ms: None,
        account: None,
        retried: None,
        notice: None,
        page_errors_seen: 0,
        phases: None,
    });
    store::save_run(root, &earlier).unwrap();
    store::save_script(root, &one_check(None)).unwrap();
    let mut browsers = browsers_of(vec![checking_driver()]);
    let mut run = new_run("run-x");
    let cases = vec![(1, "case 1".to_string())];
    let cancel = AtomicBool::new(false);
    cancel.store(true, Ordering::SeqCst); // Stop before the first case
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &cases, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let left = shot_files(root);
    assert!(left.contains(&"shot-1-000000.jpg".to_string()), "protected by the unpublished run");
    assert_eq!(left.len(), 1001, "the budget plus the one protected picture");
    assert!(!left.contains(&"shot-1-000001.jpg".to_string()), "an unprotected old one went, even on Stop");
}
