//! The unattended replay engine: a fresh browser per case, a proposed
//! verdict never a real one, and a run file saved after every case so a
//! crash or a stop loses nothing.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::recipe::save_recipe;
use std::path::Path;
use v2_lib::autorun::nav::{nav_path, no_address, no_path, save_nav, NavFile, Route, NO_ACCOUNT, NO_MODULE, UNREACHED_PREFIX};
use v2_lib::autorun::replay::{propose, run_cases, run_selection, Browsers, CaseToRun, MODULE_STEP, SIGN_IN_STEP};
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
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 20, highlight_ms: 0 }
}

fn new_run(id: &str) -> LocalRun {
    LocalRun { id: id.into(), pbi_id: 42, started_at: "1700000000000".into(), cases: vec![], mode: "unattended".into(), published: None }
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
    let steps = vec![StepRecord { step_number: 1, outcomes: vec![ordinary_fail, harness_fail], screenshot: None }];
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
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some(" leave "))], None, &quick(), &cancel, &mut |p: ReplayProgress| {
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

#[tokio::test]
async fn a_case_with_no_module_is_blocked_and_no_browser_opens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let mut browsers = browsers_of(vec![]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
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
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Payroll"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(browsers.opened, 0);
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].reason, no_path("Payroll"));
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
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(browsers.opened, 0);
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].reason, NO_ACCOUNT);
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
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
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
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Blocked");
    assert_eq!(
        rec.reason,
        "Could not reach module \"Leave\": click 2, link \"Apply Leave\" - the page ended on /hr/leave/other, not /hr/leave/apply."
    );
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
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
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
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();

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
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
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
    let err = run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap_err();
    assert!(err.contains("module paths file is not readable"), "{err}");
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
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
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

/// Review I1: the same words are Blocked on a `sign_in` (its trip back to
/// the module) and Failed on any other action.
#[test]
fn only_a_sign_in_whose_trip_back_failed_is_blocked_by_those_words() {
    let unreached = "Could not reach module \"Leave\": click 2, link \"Apply Leave\" - no visible match.";
    let steps = vec![StepRecord {
        step_number: 1,
        outcomes: vec![
            ActionOutcome::failed(format!("signed in as admin; then {unreached}")),
            ActionOutcome::failed("not run: the module screen was not reached after the sign-in"),
        ],
        screenshot: None,
    }];
    let signs_in = script(1, Some("admin"), serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "sign_in", "account": "admin" }, { "kind": "check_text", "value": "yes" }
    ] }]));
    let p = propose(&signs_in, &steps, Some(true), false);
    assert_eq!((p.verdict, p.reason.as_str()), ("Blocked", unreached));

    let checks = script(1, Some("admin"), serde_json::json!([{ "step_number": 1, "actions": [
        { "kind": "check_text", "value": "x" }, { "kind": "check_text", "value": "yes" }
    ] }]));
    let p = propose(&checks, &steps, Some(true), false);
    assert_eq!(p.verdict, "Failed", "{}", p.reason);
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
    let outcomes = run_step_routed(&mut d, root, "Acme", "Web", &signs_in.steps[0], &quick(), &mut account, Some(&route))
        .await
        .unwrap();
    assert!(!outcomes[0].ok, "{outcomes:?}");
    assert!(outcomes[0].detail.contains(&format!("; then {UNREACHED_PREFIX}Leave\": click 2, ")), "{}", outcomes[0].detail);
    assert_eq!(outcomes[1].detail, "not run: the module screen was not reached after the sign-in");
    assert!(!app.log.lock().unwrap().iter().any(|l| l.starts_with("check")));
    let steps = vec![StepRecord { step_number: 1, outcomes, screenshot: None }];
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
async fn the_scripts_own_account_wins_and_the_runs_account_fills_in_for_a_script_with_none() {
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
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None), to_run(2, None)], Some("lee"), &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(run.cases[0].account.as_deref(), Some("admin"));
    assert_eq!(run.cases[1].account.as_deref(), Some("lee"));
    assert_eq!(run.cases[1].steps[0].step_number, SIGN_IN_STEP);
    assert!(run.cases[1].steps[0].outcomes.iter().all(|o| o.ok), "{:?}", run.cases[1].steps[0].outcomes);
}

#[tokio::test]
async fn a_script_saved_before_addresses_were_switched_off_is_blocked_at_its_navigate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_nav(root, "Acme", "Web", &NavFile { direct_urls: false, modules: vec![] }).unwrap();
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
