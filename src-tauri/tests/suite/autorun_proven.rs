//! A script is checked only while proven: a failed or Blocked run or replay clears it, a pass of the
//! whole case sets it, and a case with a divergence waiting for a ruling stays unchecked.

use v2_lib::autorun::divergences::{self, DivergenceKind};
use v2_lib::autorun::proven::{may_be_checked, note_outcome, outcome_of_record, outcome_of_replay, Outcome};
use v2_lib::autorun::replay_to::{ReplayEnd, ReplayPhase};
use v2_lib::autorun::store::{load_script, save_script};
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, StepScript};
use v2_lib::browser::actions::Action;

fn script(case_id: i32, checked: bool, stamped: bool) -> CaseScript {
    CaseScript {
        case_id,
        title: "Valid login".to_string(),
        account: None,
        area: None,
        steps: vec![StepScript {
            step_number: 1,
            actions: vec![Action::CheckText { value: "Dashboard".to_string() }],
            unchecked: None,
            cleanup: false,
            because: Vec::new(),
        }],
        repairs: 2,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        precondition_repairs: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        needs_state: Default::default(),
        saved_at: Some("1786000000000".to_string()),
        fail_on_unexpected_dialog: false,
        page_errors: None,
        ignore_page_errors: vec![],
        organization: stamped.then(|| "acme".to_string()),
        project: stamped.then(|| "Web".to_string()),
        checked,
        checks: 1,
        given_up: Vec::new(),
    }
}

fn record(case_id: i32, proposed: &str, verdict: &str) -> CaseRecord {
    CaseRecord {
        case_id,
        title: "t".into(),
        verdict: verdict.into(),
        note: String::new(),
        steps: vec![],
        proposed: proposed.into(),
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

#[test]
fn a_failed_or_blocked_case_clears_checked() {
    let _l = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let before = script(5, true, true);
    save_script(root, &before).unwrap();
    note_outcome(root, 5, Outcome::Red);
    let after = load_script(root, 5).unwrap().unwrap();
    assert!(!after.checked);
    assert_eq!(after.saved_at, before.saved_at);
    assert_eq!(after.repairs, before.repairs);
    assert_eq!(after.suspected_defect, before.suspected_defect);
}

#[test]
fn a_passing_case_sets_checked_again() {
    let _l = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_script(root, &script(5, false, true)).unwrap();
    note_outcome(root, 5, Outcome::Green);
    let after = load_script(root, 5).unwrap().unwrap();
    assert!(after.checked);
    assert_eq!(after.saved_at.as_deref(), Some("1786000000000"));
}

#[test]
fn a_pass_does_not_check_a_case_waiting_for_a_ruling() {
    let _l = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_script(root, &script(5, false, true)).unwrap();
    divergences::update(root, "acme", "Web", |list| {
        divergences::report_in(list, 5, 1, "exactly 4 levels", "6 levels", DivergenceKind::Data, 1_786_000_000_000)
    })
    .unwrap();
    assert!(!may_be_checked(root, "acme", "Web", 5));
    note_outcome(root, 5, Outcome::Green);
    assert!(!load_script(root, 5).unwrap().unwrap().checked);
    assert!(may_be_checked(root, "acme", "Web", 6));
}

#[test]
fn a_pass_does_not_check_an_unstamped_script() {
    let _l = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_script(root, &script(5, false, false)).unwrap();
    note_outcome(root, 5, Outcome::Green);
    assert!(!load_script(root, 5).unwrap().unwrap().checked);
    // No script at all is no error.
    note_outcome(root, 77, Outcome::Red);
    note_outcome(root, 77, Outcome::Green);
}

#[test]
fn outcome_of_record_reads_proposed_then_verdict() {
    assert_eq!(outcome_of_record(&record(1, "Passed", "")), Some(Outcome::Green));
    assert_eq!(outcome_of_record(&record(1, "Failed", "")), Some(Outcome::Red));
    assert_eq!(outcome_of_record(&record(1, "Blocked", "")), Some(Outcome::Red));
    assert_eq!(outcome_of_record(&record(1, "", "Failed")), Some(Outcome::Red));
    assert_eq!(outcome_of_record(&record(1, "Passed", "Failed")), Some(Outcome::Green));
    assert_eq!(outcome_of_record(&record(1, "", "")), None);
}

#[test]
fn outcome_of_replay_reads_the_end() {
    let stopped = ReplayEnd::StoppedAt { phase: ReplayPhase::Step, step: 2, why: "x".into(), outcomes: vec![] };
    assert_eq!(outcome_of_replay(&stopped), Some(Outcome::Red));
    assert_eq!(outcome_of_replay(&ReplayEnd::Blocked("x".into())), Some(Outcome::Red));
    let gone = |why: Option<&str>| ReplayEnd::BrowserGone { step: 2, why: why.map(str::to_string), outcomes: vec![] };
    assert_eq!(outcome_of_replay(&gone(Some("closed"))), Some(Outcome::Red));
    assert_eq!(outcome_of_replay(&gone(None)), None);
    let ready = |step, whole| ReplayEnd::Ready { case_id: 1, step, notice: None, whole, cleanup: None };
    assert_eq!(outcome_of_replay(&ready(4, Some(3))), Some(Outcome::Green));
    assert_eq!(outcome_of_replay(&ready(3, Some(3))), None);
    assert_eq!(outcome_of_replay(&ready(2, None)), None);
    assert_eq!(outcome_of_replay(&ReplayEnd::Stopped { step: 2 }), None);
    assert_eq!(outcome_of_replay(&ReplayEnd::Refused("x".into())), None);
}

#[test]
fn the_first_save_of_a_supervised_run_notes_each_case() {
    let _l = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_script(root, &script(5, true, true)).unwrap();
    let run: LocalRun = serde_json::from_value(serde_json::json!({
        "id": "run-proven-1", "pbi_id": 1, "started_at": "1786000000000",
        "cases": [serde_json::to_value(record(5, "", "Failed")).unwrap()],
    }))
    .unwrap();
    v2_lib::commands::autorun::save_run_at(root, run.clone()).unwrap();
    assert!(!load_script(root, 5).unwrap().unwrap().checked);
    // Set by hand, the same run saved again is not the first save: it stays.
    v2_lib::autorun::store::set_checked(root, 5, true).unwrap();
    v2_lib::commands::autorun::save_run_at(root, run).unwrap();
    assert!(load_script(root, 5).unwrap().unwrap().checked);
}
