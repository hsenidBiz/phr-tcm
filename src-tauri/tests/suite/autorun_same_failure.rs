//! The same failure after a repair is a `STOP:` line (`failures::same_failure_stop`), and is
//! not a stop reason, so the step can still be marked as a suspected defect.

use serde_json::json;
use std::path::Path;
use v2_lib::autorun::failures::{describe_failures_in, signature, stop_reason, SAME_FAILURE_STOP};
use v2_lib::autorun::replay_record::{latest_for_case, load, note};
use v2_lib::autorun::replay_to::{ReplayEnd, ReplayPhase};
use v2_lib::autorun::store::{self, save_run};
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, StepRecord};
use v2_lib::browser::actions::ActionOutcome;

const ID: i32 = 41;
const T1: u64 = 1_800_000_000_000;

fn case_failing(step: i32, action: usize, words: &str) -> CaseRecord {
    let mut outcomes: Vec<ActionOutcome> = (0..action).map(|_| ActionOutcome::passed("done")).collect();
    outcomes.push(ActionOutcome::failed(words));
    CaseRecord {
        case_id: ID,
        title: "Save a request".into(),
        verdict: "Failed".into(),
        note: String::new(),
        steps: vec![StepRecord {
            step_number: step,
            outcomes,
            screenshot: None,
            downloads: vec![],
            tab: None,
            dialog: None,
            components: Vec::new(),
            duration_ms: None,
        }],
        proposed: String::new(),
        reason: String::new(),
        duration_ms: None,
        account: None,
        retried: None,
        notice: None,
        product_bugs: Vec::new(),
        page_errors_seen: 0,
        phases: None,
        cleanup: None,
    }
}

fn run_at(at: u64, case: CaseRecord) -> LocalRun {
    LocalRun {
        id: format!("run-{at}"),
        pbi_id: 1,
        started_at: at.to_string(),
        cases: vec![case],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    }
}

fn iso(ms: u64) -> String {
    v2_lib::applog::iso_of(std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms))
}

fn script(repairs: u32, saved_at_ms: u64) -> CaseScript {
    serde_json::from_value(json!({
        "case_id": ID,
        "title": "Save a request",
        "repairs": repairs,
        "saved_at": iso(saved_at_ms),
        "steps": [
            { "step_number": 3, "actions": [ { "kind": "click", "selector": { "css": "#save" } }, { "kind": "click", "selector": { "css": "#ok" } } ] }
        ]
    }))
    .unwrap()
}

fn describe(root: &Path, run: &LocalRun) -> String {
    let scripts: Vec<CaseScript> = store::load_script(root, ID).unwrap().into_iter().collect();
    describe_failures_in(Some(root), run, &scripts, &Default::default())
}

fn stop_line() -> String {
    format!("STOP: {SAME_FAILURE_STOP}")
}

#[test]
fn the_signature_masks_digits_and_keeps_step_action_and_words() {
    let a = signature(&case_failing(3, 1, "Timed out 5000ms waiting for Save")).unwrap();
    let b = signature(&case_failing(3, 1, "Timed out 7000ms  waiting for   Save\nsecond line")).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.words, "timed out #ms waiting for save");
    assert_ne!(a, signature(&case_failing(4, 1, "Timed out 5000ms waiting for Save")).unwrap());
    assert_ne!(a, signature(&case_failing(3, 0, "Timed out 5000ms waiting for Save")).unwrap());
    assert_ne!(a, signature(&case_failing(3, 1, "Timed out 5000ms waiting for Cancel")).unwrap());
    let mut passed = case_failing(3, 1, "x");
    passed.steps[0].outcomes.iter_mut().for_each(|o| o.ok = true);
    assert!(signature(&passed).is_none());
}

#[test]
fn the_same_failure_after_a_repair_is_a_stop() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::save_script(dir.path(), &script(1, T1 + 60_000)).unwrap();
    save_run(dir.path(), &run_at(T1, case_failing(3, 1, "Timed out 5000ms waiting for Save"))).unwrap();
    let r2 = run_at(T1 + 120_000, case_failing(3, 1, "Timed out 7000ms waiting for Save"));
    save_run(dir.path(), &r2).unwrap();
    let out = describe(dir.path(), &r2);
    assert!(out.contains(&stop_line()), "{out}");
}

#[test]
fn a_different_failure_after_a_repair_is_no_stop() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    store::save_script(dir.path(), &script(1, T1 + 60_000)).unwrap();
    save_run(dir.path(), &run_at(T1, case_failing(3, 1, "Timed out 5000ms waiting for Save"))).unwrap();
    let r2 = run_at(T1 + 120_000, case_failing(3, 1, "nothing on the page answers to #ok"));
    save_run(dir.path(), &r2).unwrap();
    assert!(!describe(dir.path(), &r2).contains("STOP:"));
}

#[test]
fn the_same_failure_without_a_repair_between_is_no_stop() {
    let _a = crate::serial::autorun();
    let first = case_failing(3, 1, "Timed out 5000ms waiting for Save");
    let r2 = run_at(T1 + 120_000, case_failing(3, 1, "Timed out 5000ms waiting for Save"));
    for saved in [script(1, T1 - 60_000), script(0, T1 + 60_000)] {
        let dir = tempfile::tempdir().unwrap();
        store::save_script(dir.path(), &saved).unwrap();
        save_run(dir.path(), &run_at(T1, first.clone())).unwrap();
        save_run(dir.path(), &r2).unwrap();
        assert!(!describe(dir.path(), &r2).contains("STOP:"));
    }
}

#[test]
fn two_replays_keep_the_first_failure_to_compare() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    let stopped = || ReplayEnd::StoppedAt {
        phase: ReplayPhase::Step,
        step: 3,
        why: "Timed out 5000ms waiting for Save".into(),
        outcomes: vec![ActionOutcome::failed("Timed out 5000ms waiting for Save")],
    };
    note(dir.path(), ID, "Save a request", 3, &stopped(), None, T1);
    assert!(load(dir.path(), ID).unwrap().previous.is_none());
    store::save_script(dir.path(), &script(1, T1 + 60_000)).unwrap();
    note(dir.path(), ID, "Save a request", 3, &stopped(), None, T1 + 120_000);
    let kept = load(dir.path(), ID).unwrap();
    assert_eq!(kept.at, T1 + 120_000);
    assert_eq!(kept.previous.as_ref().map(|p| p.at), Some(T1));
    let run = latest_for_case(dir.path(), ID).unwrap();
    let out = describe(dir.path(), &run);
    assert!(out.contains(&stop_line()), "{out}");
}

#[test]
fn the_same_failure_is_not_a_stop_reason() {
    let case = case_failing(3, 1, "Timed out 5000ms waiting for Save");
    assert!(stop_reason(&case).is_none());
}
