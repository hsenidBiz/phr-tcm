//! The steps Auto Run performs itself: which steps count, held to the shared
//! fixture (the TypeScript mirror reads it too), the field's JSON, and the
//! save check's sentences for a bad mark.

use std::collections::BTreeSet;

use v2_lib::autorun::by_auto_run::done_by_auto_run;
use v2_lib::autorun::floor::{check_floor, Expected};
use v2_lib::autorun::{edits, CaseScript, StepScript};

#[derive(serde::Deserialize)]
struct Row {
    name: String,
    script: CaseScript,
    steps: Vec<i32>,
}

#[test]
fn the_steps_done_by_auto_run_follow_the_shared_fixture() {
    let rows: Vec<Row> = serde_json::from_str(include_str!("../fixtures/by_auto_run.json")).unwrap();
    assert!(rows.len() >= 5);
    for r in rows {
        let want: BTreeSet<i32> = r.steps.iter().copied().collect();
        assert_eq!(done_by_auto_run(&r.script), want, "{}", r.name);
    }
}

#[test]
fn the_mark_is_written_only_when_true_and_changes_the_signature() {
    let plain: StepScript = serde_json::from_value(serde_json::json!({ "step_number": 1, "actions": [] })).unwrap();
    assert!(!plain.by_auto_run);
    assert!(serde_json::to_value(&plain).unwrap().get("by_auto_run").is_none());
    let marked: StepScript = serde_json::from_value(serde_json::json!({ "step_number": 1, "actions": [], "by_auto_run": true })).unwrap();
    assert!(marked.by_auto_run);
    assert_eq!(serde_json::to_value(&marked).unwrap()["by_auto_run"], true);
    assert_ne!(edits::step_signature(&plain), edits::step_signature(&marked));
}

fn case(expected: &[&str]) -> Vec<Expected> {
    expected
        .iter()
        .enumerate()
        .map(|(i, e)| Expected { step_number: i as i32 + 1, expected: e.to_string(), shared: false })
        .collect()
}

fn script(steps: serde_json::Value) -> CaseScript {
    serde_json::from_value(serde_json::json!({ "case_id": 1, "title": "t", "steps": steps })).unwrap()
}

#[test]
fn a_marked_step_needs_no_unchecked_sentence_or_check() {
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [], "by_auto_run": true },
        { "step_number": 2, "actions": [{ "kind": "check_text", "value": "Saved" }] }
    ]));
    assert!(check_floor(&sc, &case(&["The app opens", "Saved shows"])).is_empty());
}

#[test]
fn a_marked_step_with_actions_is_refused() {
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }], "by_auto_run": true }
    ]));
    assert_eq!(
        check_floor(&sc, &case(&["The app opens"])),
        vec!["step 1 is marked as done by Auto Run but has actions - drop one or the other".to_string()]
    );
}

#[test]
fn a_marked_step_after_an_unmarked_one_or_in_the_clean_up_is_refused() {
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "Saved" }] },
        { "step_number": 2, "actions": [], "by_auto_run": true },
        { "step_number": 3, "actions": [], "cleanup": true, "unchecked": "removes it", "by_auto_run": true }
    ]));
    assert_eq!(
        check_floor(&sc, &case(&["Saved shows", "The app opens"])),
        vec![
            "step 2 is marked as done by Auto Run but is not one of the opening steps".to_string(),
            "step 3 is marked as done by Auto Run but is not one of the opening steps".to_string(),
        ]
    );
}
