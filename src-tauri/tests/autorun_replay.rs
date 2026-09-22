//! The unattended replay engine: a fresh browser per case, a proposed
//! verdict never a real one, and a run file saved after every case so a
//! crash or a stop loses nothing.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use v2_lib::autorun::accounts::save_accounts;
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::replay::{propose, run_selection, Browsers, SIGN_IN_STEP};
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
