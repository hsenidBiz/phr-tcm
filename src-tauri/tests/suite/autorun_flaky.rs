//! Flaky cases: cases that both passed and failed in their recent runs on one environment, and the
//! person's "Flaky" mark on a case's script.

use v2_lib::autorun::flaky::{flaky_cases, flaky_cases_with_marks, FLAKY_WINDOW};
use v2_lib::autorun::store::{load_script, save_script, save_scripts_atomically, set_flaky};
use v2_lib::autorun::{CaseRecord, CaseScript, FlakyMark, LocalRun, StepRecord, StepScript};
use v2_lib::browser::actions::{Action, ActionOutcome};

fn record(case_id: i32, outcome: &str, detail: &str) -> CaseRecord {
    let outcomes = if outcome == "Failed" && !detail.is_empty() {
        vec![ActionOutcome::passed("ok"), ActionOutcome::failed(detail)]
    } else {
        vec![ActionOutcome::passed("ok")]
    };
    let mut step = step_defaults();
    step.outcomes = outcomes;
    CaseRecord {
        case_id,
        title: format!("Case {case_id}"),
        verdict: String::new(),
        note: String::new(),
        steps: vec![step],
        proposed: outcome.to_string(),
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

fn step_defaults() -> StepRecord {
    serde_json::from_value(serde_json::json!({ "step_number": 1, "outcomes": [] })).unwrap()
}

fn run(n: u64, env: Option<&str>, cases: Vec<CaseRecord>) -> LocalRun {
    LocalRun {
        id: format!("run-{n}"),
        pbi_id: 1,
        started_at: n.to_string(),
        cases,
        mode: "unattended".into(),
        published: None,
        environment: env.map(str::to_string),
        resets: Vec::new(),
        network_pauses: Vec::new(),
    }
}

fn one(outcome: &str, n: u64, env: &str) -> LocalRun {
    run(n, Some(env), vec![record(7, outcome, if outcome == "Failed" { "Save did not answer" } else { "" })])
}

#[test]
fn a_pass_and_a_fail_on_one_environment_is_flaky() {
    let runs = vec![one("Passed", 1, "QA"), one("Failed", 2, "QA"), one("Passed", 3, "QA")];
    let got = flaky_cases(&runs, FLAKY_WINDOW);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].case_id, 7);
    assert_eq!(got[0].environment.as_deref(), Some("QA"));
    assert_eq!(got[0].pattern, vec![true, false, true]);
    assert_eq!(got[0].last_failed_at, "2");
}

#[test]
fn the_order_the_runs_come_in_does_not_matter() {
    let runs = vec![one("Passed", 3, "QA"), one("Passed", 1, "QA"), one("Failed", 2, "QA")];
    assert_eq!(flaky_cases(&runs, FLAKY_WINDOW)[0].pattern, vec![true, false, true]);
}

#[test]
fn all_passing_or_all_failing_is_not_flaky() {
    let passing = vec![one("Passed", 1, "QA"), one("Passed", 2, "QA")];
    let failing = vec![one("Failed", 1, "QA"), one("Failed", 2, "QA")];
    assert!(flaky_cases(&passing, FLAKY_WINDOW).is_empty());
    assert!(flaky_cases(&failing, FLAKY_WINDOW).is_empty());
}

#[test]
fn a_case_is_judged_on_each_environment_alone() {
    let runs = vec![
        one("Passed", 1, "QA"),
        one("Failed", 2, "QA"),
        one("Failed", 3, "UAT"),
        one("Failed", 4, "UAT"),
    ];
    let got = flaky_cases(&runs, FLAKY_WINDOW);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].environment.as_deref(), Some("QA"));
}

#[test]
fn only_the_newest_window_of_runs_counts() {
    // The one failure is the oldest of eleven runs: outside a window of ten.
    let mut runs = vec![one("Failed", 1, "QA")];
    for n in 2..=11 {
        runs.push(one("Passed", n, "QA"));
    }
    assert!(flaky_cases(&runs, 10).is_empty());
    // Inside a wider window it is flaky, and the pattern is oldest first.
    let wide = flaky_cases(&runs, 11);
    assert_eq!(wide.len(), 1);
    assert_eq!(wide[0].pattern.len(), 11);
    assert!(!wide[0].pattern[0]);
    // A window of ten keeps ten.
    runs.push(one("Failed", 12, "QA"));
    let got = flaky_cases(&runs, 10);
    assert_eq!(got[0].pattern.len(), 10);
    assert!(!*got[0].pattern.last().unwrap());
}

#[test]
fn blocked_and_unjudged_cases_are_neither_a_pass_nor_a_fail() {
    let runs = vec![one("Passed", 1, "QA"), one("Blocked", 2, "QA"), one("", 3, "QA")];
    assert!(flaky_cases(&runs, FLAKY_WINDOW).is_empty());
    let runs = vec![one("Passed", 1, "QA"), one("Blocked", 2, "QA"), one("Failed", 3, "QA")];
    assert_eq!(flaky_cases(&runs, FLAKY_WINDOW)[0].pattern, vec![true, false]);
}

#[test]
fn a_persons_verdict_outranks_the_machines_proposal() {
    let mut passed_by_person = record(7, "Failed", "Save did not answer");
    passed_by_person.verdict = "Passed".into();
    let runs = vec![run(1, Some("QA"), vec![passed_by_person]), one("Passed", 2, "QA")];
    assert!(flaky_cases(&runs, FLAKY_WINDOW).is_empty());
}

#[test]
fn the_last_failure_is_the_failing_outcomes_detail_else_the_note() {
    let mut noted = record(7, "Failed", "");
    noted.note = "Timed out".into();
    let runs = vec![one("Passed", 1, "QA"), run(2, Some("QA"), vec![noted])];
    assert_eq!(flaky_cases(&runs, FLAKY_WINDOW)[0].last_failure.as_deref(), Some("Timed out"));

    let mut older = record(7, "Failed", "Old message");
    older.note = "n".into();
    let runs = vec![run(1, Some("QA"), vec![older]), one("Failed", 2, "QA"), one("Passed", 3, "QA")];
    let got = flaky_cases(&runs, FLAKY_WINDOW);
    assert_eq!(got[0].last_failure.as_deref(), Some("Save did not answer"));
    assert_eq!(got[0].last_failed_at, "2");
}

#[test]
fn a_run_from_before_environments_is_listed_without_a_name() {
    let runs = vec![
        run(1, None, vec![record(7, "Passed", "")]),
        run(2, None, vec![record(7, "Failed", "x")]),
    ];
    let got = flaky_cases(&runs, FLAKY_WINDOW);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].environment, None);
}

fn script(case_id: i32) -> CaseScript {
    let mut s: CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": case_id,
        "title": "Valid login",
        "steps": [],
        "checked": false,
    }))
    .unwrap();
    s.steps = vec![StepScript {
        step_number: 1,
        actions: vec![Action::CheckText { value: "Dashboard".to_string() }],
        unchecked: None,
        cleanup: false,
        because: Vec::new(),
    }];
    s
}

#[test]
fn the_flaky_mark_round_trips_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_script(root, &script(7)).unwrap();
    assert!(load_script(root, 7).unwrap().unwrap().flaky.is_none());

    set_flaky(root, 7, Some(FlakyMark { marked_at: "1786000000000".into() })).unwrap();
    assert_eq!(
        load_script(root, 7).unwrap().unwrap().flaky,
        Some(FlakyMark { marked_at: "1786000000000".into() })
    );

    // A save of the script, from an editor that never saw the mark, keeps it.
    save_scripts_atomically(root, &[script(7)]).unwrap();
    assert!(load_script(root, 7).unwrap().unwrap().flaky.is_some());

    set_flaky(root, 7, None).unwrap();
    assert!(load_script(root, 7).unwrap().unwrap().flaky.is_none());
    // Unmarking leaves nothing in the file.
    let text = std::fs::read_to_string(root.join("scripts").join("case-7.json")).unwrap();
    assert!(!text.contains("flaky"), "{text}");
}

#[test]
fn marking_a_case_with_no_script_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let err = set_flaky(dir.path(), 99, Some(FlakyMark { marked_at: "1".into() })).unwrap_err();
    assert!(err.contains("no script"), "{err}");
    // Unmarking one is harmless.
    set_flaky(dir.path(), 99, None).unwrap();
}

#[test]
fn a_script_file_from_before_the_mark_still_loads() {
    let old = serde_json::json!({ "case_id": 3, "title": "Old", "steps": [], "checked": false });
    let s: CaseScript = serde_json::from_value(old).unwrap();
    assert_eq!(s.flaky, None);
}

#[test]
fn a_marked_case_that_now_fails_every_run_is_still_listed_so_it_can_be_unmarked() {
    let runs = vec![one("Passed", 1, "QA"), one("Failed", 2, "QA"), one("Failed", 3, "QA")];
    let all_fail = vec![one("Failed", 2, "QA"), one("Failed", 3, "QA")];
    assert!(flaky_cases(&all_fail, FLAKY_WINDOW).is_empty());
    let got = flaky_cases_with_marks(&all_fail, FLAKY_WINDOW, &[7]);
    assert_eq!(got.len(), 1);
    assert!(got[0].marked);
    assert_eq!(got[0].pattern, vec![false, false]);
    assert_eq!(got[0].last_failed_at, "3");
    // An unmarked case that does not alternate stays out; a marked alternating one is marked.
    assert!(flaky_cases_with_marks(&all_fail, FLAKY_WINDOW, &[8]).is_empty());
    assert!(flaky_cases_with_marks(&runs, FLAKY_WINDOW, &[7])[0].marked);
    // Marked and always passing is listed too.
    let passing = vec![one("Passed", 1, "QA"), one("Passed", 2, "QA")];
    let got = flaky_cases_with_marks(&passing, FLAKY_WINDOW, &[7]);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].last_failure, None);
}
