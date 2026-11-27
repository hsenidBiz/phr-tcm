//! Saving an `expect_database` check, and the assistant's help for it: a
//! `{{noted.name}}` must follow a `remember_text` of the case, a write is
//! refused, its literal is held against the case, and the guide and the
//! examples teach it.

use serde_json::{json, Value};
use v2_lib::autorun::action_help::{example_for, kinds};
use v2_lib::autorun::case_text::CaseText;
use v2_lib::autorun::case_values::check_case_values;
use v2_lib::autorun::guide::autorun_guide;
use v2_lib::autorun::store::check_scripts;
use v2_lib::autorun::CaseScript;

fn script(steps: Value) -> CaseScript {
    serde_json::from_value(json!({ "case_id": 147556, "title": "Leave", "steps": steps })).unwrap()
}

fn check(sql: &str) -> Value {
    json!({ "kind": "expect_database", "sql": sql, "column": "status", "equals": "Approved" })
}

const NOTED: &str = "SELECT TOP 1 status FROM LeaveRequest WHERE request_no = {{noted.req}} ORDER BY created_at DESC";

#[test]
fn save_refuses_unknown_noted_name() {
    let s = script(json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#go" }] },
        { "step_number": 2, "actions": [check(NOTED)] },
    ]));
    let err = check_scripts(&[s]).unwrap_err();
    assert!(err.contains("req"), "{err}");
    assert!(err.contains("step 2"), "{err}");
}

#[test]
fn save_accepts_noted_after_remember() {
    let s = script(json!([
        { "step_number": 1, "actions": [{ "kind": "remember_text", "selector": "#no", "as": "req" }] },
        { "step_number": 2, "actions": [check(NOTED)] },
    ]));
    assert_eq!(check_scripts(&[s]), Ok(()));
}

#[test]
fn save_refuses_write_sql() {
    let s = script(json!([
        { "step_number": 1, "actions": [json!({ "kind": "expect_database", "sql": "DELETE FROM t", "exists": true })] },
    ]));
    assert!(check_scripts(&[s]).is_err());
}

#[test]
fn case_values_checks_db_equals_literal() {
    let s = script(json!([
        { "step_number": 1, "actions": [check("SELECT TOP 1 status FROM LeaveRequest ORDER BY created_at DESC")] },
    ]));
    let case = CaseText {
        title: "Leave".into(),
        steps: vec![("Submit the request".into(), "The request is saved".into())],
        preconditions: String::new(),
    };
    let r = check_case_values(&s, &case, None, "AUTOTEST");
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
    assert!(r.refusals[0].contains("Approved"), "{:?}", r.refusals);

    let held = CaseText { steps: vec![("Submit the request".into(), "The status is Approved".into())], ..case };
    let r = check_case_values(&s, &held, None, "AUTOTEST");
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
}

#[test]
fn examples_include_expect_database() {
    assert!(kinds().contains(&"expect_database"));
    let ex = example_for("expect_database").unwrap();
    assert_eq!(
        ex,
        r#"{"kind":"expect_database","sql":"SELECT TOP 1 status FROM LeaveRequest WHERE request_no = {{noted.request}} ORDER BY created_at DESC","column":"status","equals":"Approved"}"#
    );
    let a: v2_lib::browser::actions::Action = serde_json::from_str(ex).unwrap();
    assert!(matches!(a, v2_lib::browser::actions::Action::ExpectDatabase { .. }));
}

#[test]
fn guide_has_database_section() {
    let g = autorun_guide();
    assert!(g.contains("## Checking the database"));
    assert!(g.contains("{{noted."));
}

fn component(sql: &str) -> v2_lib::autorun::components::Component {
    serde_json::from_value(json!({
        "name": "who_approved", "description": "Checks who approved",
        "inputs": [{ "name": "who", "kind": "text", "description": "the name" }],
        "actions": [{ "kind": "expect_database", "sql": sql, "exists": true }]
    }))
    .unwrap()
}

#[test]
fn a_component_input_reaches_the_sql_as_a_quoted_literal() {
    use v2_lib::autorun::components::{check_component, expand};
    let c = component("SELECT 1 FROM leave WHERE emp = {{who}}");
    assert_eq!(check_component(&c), Ok(()));
    let inputs = json!({ "who": "O'Brien" }).as_object().unwrap().clone();
    let out = expand(&c, &inputs).unwrap();
    let v2_lib::browser::actions::Action::ExpectDatabase { sql, .. } = &out[0] else { panic!("kind") };
    assert_eq!(sql, "SELECT 1 FROM leave WHERE emp = N'O''Brien'");
    // A data placeholder stays one for the run's own binding.
    let inputs = json!({ "who": "{{setup.emp}}" }).as_object().unwrap().clone();
    let out = expand(&c, &inputs).unwrap();
    let v2_lib::browser::actions::Action::ExpectDatabase { sql, .. } = &out[0] else { panic!("kind") };
    assert_eq!(sql, "SELECT 1 FROM leave WHERE emp = {{setup.emp}}");
}

#[test]
fn a_component_refuses_its_input_inside_quotes_in_a_check() {
    use v2_lib::autorun::components::check_component;
    for sql in [
        "SELECT 1 FROM leave WHERE emp = '{{who}}'",
        "SELECT 1 FROM leave /* {{who}} */",
        "SELECT [{{who}}] FROM leave",
    ] {
        assert!(check_component(&component(sql)).is_err(), "{sql}");
    }
}
