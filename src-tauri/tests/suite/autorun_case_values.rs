//! A script's check values come from its case: a value the case does not
//! hold needs a `because`, and a count the case states differently is
//! refused (`case_values`, `case_text`, and the save route's value gate).

use crate::common;
use serde_json::{json, Value};
use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::autorun::case_text::{numbers, range_members, stated_counts, CaseText};
use v2_lib::autorun::case_values::{check_case_values, check_count, values_of_step, ValueKind};
use v2_lib::autorun::store::set_root;
use v2_lib::autorun::CaseScript;
use v2_lib::steps_xml::{build_steps_xml, Step};
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-autorun-case-values-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ctx() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        preconditions_ref: Some("Custom.Pre".into()),
        ..BridgeContext::default()
    }
}

fn case_of(id: i32, title: &str, steps: &[(&str, &str)], preconditions: &str) -> v2_lib::ado::TestCaseFull {
    let steps: Vec<Step> = steps
        .iter()
        .map(|(a, e)| Step { action: (*a).into(), expected: (*e).into(), shared: None })
        .collect();
    v2_lib::ado::TestCaseFull {
        id,
        title: title.into(),
        tags: String::new(),
        automation_status: "Not Automated".into(),
        steps_xml: build_steps_xml(&steps),
        step_ids: vec![],
        steps_legacy: None,
        steps,
        module_value: String::new(),
        preconditions: preconditions.into(),
        preconditions_legacy: None,
        preconditions_stored: None,
        priority: None,
    }
}

/// Case 147556: the DEF table holds exactly 4 levels; step 2 expects a
/// dropdown of Level 1 to Level 4.
fn case_147556() -> v2_lib::ado::TestCaseFull {
    case_of(
        147556,
        "DEF levels",
        &[("Open the DEF page", ""), ("Open the level dropdown", "The dropdown lists Level 1 to Level 4")],
        "The DEF table holds exactly 4 levels",
    )
}

fn text_of(case: &v2_lib::ado::TestCaseFull) -> CaseText {
    CaseText::from_case(case)
}

fn script(steps: Value) -> CaseScript {
    serde_json::from_value(json!({ "case_id": 147556, "title": "DEF levels", "steps": steps })).unwrap()
}

fn level_selector() -> Value {
    json!([{ "role": "combobox", "name": "Level" }, { "role": "option" }])
}

fn two_steps(action: Value, because: Value) -> CaseScript {
    script(json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/def" }] },
        { "step_number": 2, "actions": [action], "because": because },
    ]))
}

fn check(case: &v2_lib::ado::TestCaseFull, s: &CaseScript, only: Option<&[i32]>) -> v2_lib::autorun::case_values::ValueReport {
    check_case_values(s, &text_of(case), only, "AUTOTEST")
}

// ------------------------------------------------------------- pure rules

#[test]
fn the_147556_script_is_refused_naming_both_counts() {
    let case = case_147556();
    let action = json!({ "kind": "expect_count", "selector": level_selector(), "equals": 6 });
    let r = check(&case, &two_steps(action.clone(), json!([])), None);
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
    assert!(
        r.refusals[0].starts_with("step 2: the check counts 6 but the case says 4 (\"exactly 4 levels\") - "),
        "{:?}",
        r.refusals
    );
    // A because never lifts it.
    let r = check(&case, &two_steps(action, json!([{ "value": "6", "why": "one row per DEF level the precondition lists" }])), None);
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
    assert!(r.refusals[0].contains("the check counts 6 but the case says 4"), "{:?}", r.refusals);
    assert!(r.listed.is_empty());
}

#[test]
fn an_order_of_six_levels_is_refused_and_an_order_of_four_is_saved() {
    let case = case_147556();
    let order = |n: usize| {
        let texts: Vec<String> = (1..=n).map(|i| format!("Level {i}")).collect();
        json!({ "kind": "expect_order", "selector": level_selector(), "texts": texts })
    };
    let r = check(&case, &two_steps(order(6), json!([])), None);
    assert!(r.refusals.iter().any(|l| l.starts_with("step 2: the check counts 6 but the case says 4")), "{:?}", r.refusals);
    assert!(r.refusals.iter().any(|l| l.starts_with("step 2: the text \"Level 5\" is not in the case")), "{:?}", r.refusals);
    assert!(r.refusals.iter().any(|l| l.starts_with("step 2: the text \"Level 6\" is not in the case")), "{:?}", r.refusals);
    let r = check(&case, &two_steps(order(4), json!([])), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
}

#[test]
fn a_value_with_a_because_is_saved_and_listed() {
    let case = case_of(
        9,
        "Save",
        &[("Open the page", ""), ("Save the form", "A confirmation appears"), ("Check it", "Record saved successfully")],
        "",
    );
    // The case holds the words in step 3, so give the check words that are not there anywhere.
    let action = json!({ "kind": "expect_contains_text", "selector": "#toast", "value": "Saved with a receipt" });
    let bare = two_steps(action.clone(), json!([]));
    let r = check(&case, &bare, None);
    assert!(r.refusals.iter().any(|l| l.starts_with("step 2: the text \"Saved with a receipt\" is not in the case")), "{:?}", r.refusals);
    let covered = two_steps(action, json!([{ "value": "Saved with a receipt", "why": "the confirmation step 3 expects" }]));
    let r = check(&case, &covered, None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.listed, vec!["step 2 \"Saved with a receipt\" - the confirmation step 3 expects".to_string()]);
}

#[test]
fn a_because_naming_the_app_is_refused() {
    let case = case_147556();
    let action = json!({ "kind": "expect_contains_text", "selector": "#x", "value": "Six levels here" });
    let s = two_steps(action, json!([{ "value": "Six levels here", "why": "the app shows 6 levels" }]));
    let r = check(&case, &s, None);
    assert!(
        r.refusals.iter().any(|l| l
            == "step 2: the \"because\" for \"Six levels here\" names the app as its source - a check's value comes from the case; when the app differs, call report_case_divergence"),
        "{:?}",
        r.refusals
    );
}

#[test]
fn a_malformed_because_is_refused_once() {
    let case = case_147556();
    let action = json!({ "kind": "expect_contains_text", "selector": "#x", "value": "Level 1" });
    let s = two_steps(action, json!([{ "value": "", "why": "x" }, { "value": "a", "why": "two\nlines" }]));
    let r = check(&case, &s, None);
    let want = "step 2: each \"because\" is { \"value\", \"why\" }, the why one line of at most 200 characters";
    assert_eq!(r.refusals.iter().filter(|l| l.as_str() == want).count(), 1, "{:?}", r.refusals);
}

#[test]
fn exempt_values_need_no_because() {
    let case = case_of(5, "Plain", &[("Open", ""), ("Look", "A row shows")], "");
    let mut s = script(json!([
        { "step_number": 1, "actions": [
            { "kind": "fill", "selector": "#name", "value": "Zebra Stripes" },
            { "kind": "upload", "selector": "#file", "file": "payslip.pdf" },
        ] },
        { "step_number": 2, "actions": [
            { "kind": "expect_count", "selector": "#rows", "equals": 1 },
            { "kind": "expect_count", "selector": "#rows", "equals": 0 },
            { "kind": "expect_count", "selector": "#rows", "equals": 274 },
            { "kind": "expect_contains_text", "selector": "#a", "value": "{{setup.cycle_name}} Draft" },
            { "kind": "expect_contains_text", "selector": "#a", "value": "AUTOTEST Cycle 9" },
            { "kind": "expect_contains_text", "selector": "#a", "value": "Zebra Stripes" },
            { "kind": "expect_contains_text", "selector": "#a", "value": "payslip.pdf" },
            { "kind": "expect_contains_text", "selector": "#a", "value": "payslip" },
            { "kind": "expect_contains_text", "selector": "#a", "value": "OK" },
        ] },
    ]));
    s.preconditions = serde_json::from_value(json!([{ "flow": "f", "stage": "s", "value": 274 }])).unwrap();
    let r = check(&case, &s, None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
}

#[test]
fn patterns_attributes_and_styles_are_not_compared() {
    let case = case_147556();
    let s = script(json!([
        { "step_number": 1, "actions": [
            { "kind": "expect_text", "selector": "#a", "matches": "Level (5|6)" },
            { "kind": "expect_attribute", "selector": "#a", "name": "title", "equals": "Six levels" },
            { "kind": "expect_style", "selector": "#a", "property": "color", "equals": "rgb(1, 2, 3)" },
            { "kind": "check_url", "contains": "/def/levels6" },
            { "kind": "expect_each", "selector": "#rows", "matches": "Level 6" },
        ] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#go" }] },
    ]));
    let r = check(&case, &s, None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    let values = values_of_step(&s.steps[0]);
    assert!(values.is_empty(), "{values:?}");
}

#[test]
fn a_count_whose_noun_is_not_in_the_check_needs_only_a_because() {
    let case = case_147556();
    let action = json!({ "kind": "expect_count", "selector": { "role": "button" }, "equals": 3 });
    let r = check(&case, &two_steps(action.clone(), json!([])), None);
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
    assert!(r.refusals[0].starts_with("step 2: the count \"3\" is not in the case - take it from the case's steps"), "{:?}", r.refusals);
    let r = check(&case, &two_steps(action, json!([{ "value": "3", "why": "the three buttons step 1 names" }])), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.listed.len(), 1);
}

#[test]
fn a_repair_checks_only_its_declared_steps() {
    let case = case_147556();
    let s = script(json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/def" }] },
        { "step_number": 2, "actions": [{ "kind": "expect_contains_text", "selector": "#a", "value": "Level 9" }] },
        { "step_number": 3, "actions": [{ "kind": "expect_contains_text", "selector": "#a", "value": "Level 2" }] },
    ]));
    assert!(check(&case, &s, Some(&[3])).refusals.is_empty());
    assert_eq!(check(&case, &s, Some(&[2])).refusals.len(), 1);
    assert_eq!(check(&case, &s, None).refusals.len(), 1);
}

#[test]
fn a_because_no_check_uses_is_noted() {
    let case = case_147556();
    let action = json!({ "kind": "expect_contains_text", "selector": "#a", "value": "Level 2" });
    let r = check(&case, &two_steps(action, json!([{ "value": "Level 9", "why": "step 3 names it" }])), None);
    assert_eq!(r.notes, vec!["step 2: note: \"because\" names \"Level 9\", which no check of the step uses".to_string()]);
}

#[test]
fn the_cleanup_step_is_never_compared() {
    let case = case_147556();
    let s = script(json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#a" }] },
        { "step_number": 3, "cleanup": true, "actions": [{ "kind": "check_text", "value": "Something else entirely" }] },
    ]));
    assert!(check(&case, &s, None).refusals.is_empty());
    assert_eq!(check_count(&s), 0);
}

#[test]
fn check_count_counts_nested_checks_but_not_the_cleanup() {
    let s = script(json!([
        { "step_number": 1, "actions": [
            { "kind": "check_text", "value": "a" },
            { "kind": "when_visible", "selector": "#b", "then": [{ "kind": "check_text", "value": "b" }] },
        ] },
        { "step_number": 2, "cleanup": true, "actions": [{ "kind": "check_text", "value": "c" }] },
    ]));
    assert_eq!(check_count(&s), 2);
}

#[test]
fn values_are_read_by_kind() {
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_count", "selector": "#a", "at_least": 3 },
        { "kind": "expect_row_count", "table": "#t", "equals": 5 },
        { "kind": "expect_each", "selector": "#a", "min": 2, "contains": "Foo" },
        { "kind": "expect_row", "table": "#t", "cells": { "Name": "Alice" } },
    ] }]));
    let v = values_of_step(&s.steps[0]);
    let counts: Vec<(&str, bool)> =
        v.iter().filter(|c| c.kind == ValueKind::Count).map(|c| (c.value.as_str(), c.exact)).collect();
    assert_eq!(counts, vec![("3", false), ("5", true), ("2", false)]);
    let texts: Vec<&str> = v.iter().filter(|c| c.kind == ValueKind::Text).map(|c| c.value.as_str()).collect();
    assert_eq!(texts, vec!["Foo", "Alice"]);
}

// -------------------------------------------------------------- case text

#[test]
fn stated_counts_skip_labels_ranges_and_time() {
    assert!(stated_counts("Level 1 to Level 4").is_empty());
    assert!(stated_counts("wait 5 seconds").is_empty());
    assert!(stated_counts("step 3").is_empty());
    let c = stated_counts("The DEF table holds exactly 4 levels");
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].n, c[0].phrase.as_str(), c[0].stem.as_str()), (4, "exactly 4 levels", "level"));
    let c = stated_counts("It lists 2 records.");
    assert_eq!((c[0].n, c[0].phrase.as_str(), c[0].stem.as_str()), (2, "2 records", "record"));
}

#[test]
fn numbers_ignore_digits_inside_words() {
    let n = numbers("level 4 and q4 and 12th and a1 and (7) and 2024");
    assert!(n.contains(&4) && n.contains(&7) && n.contains(&2024));
    assert!(!n.contains(&12) && !n.contains(&1));
    assert_eq!(numbers("v 12345678901").len(), 0, "runs over 9 digits are skipped");
}

#[test]
fn ranges_give_their_members_and_their_size() {
    assert_eq!(range_members("level 1 to level 4"), vec!["level 1", "level 2", "level 3", "level 4"]);
    assert_eq!(range_members("levels level 1-3 here"), vec!["level 1", "level 2", "level 3"]);
    assert_eq!(range_members("level 1 - 3"), vec!["level 1", "level 2", "level 3"]);
    assert!(range_members("level 4 to level 1").is_empty());
    assert!(range_members("level 1 to step 4").is_empty());
    assert!(range_members("level 1 to level 90").is_empty(), "a range over 50 members is not read");
    let text = text_of(&case_147556());
    assert!(text.holds("Level 3"));
    assert!(text.case_numbers().contains(&4));
    assert!(!text.case_numbers().contains(&6));
}

// ------------------------------------------------------------------ route

async fn client_with(case: &v2_lib::ado::TestCaseFull) -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [{
            "id": case.id,
            "fields": {
                "System.Title": case.title,
                "Microsoft.VSTS.TCM.Steps": case.steps_xml,
                "Custom.Pre": case.preconditions,
            },
            "multilineFieldsFormat": { "Custom.Pre": "markdown" }
        }] })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

fn bundle(case_id: i32, title: &str, action: Value, because: Value) -> String {
    json!([{
        "case_id": case_id,
        "title": title,
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/def" }] },
            { "step_number": 2, "actions": [action], "because": because },
        ]
    }])
    .to_string()
}

fn see(root: &std::path::Path, body: &str) {
    let c = ctx();
    common::see_scripts(root, &c.org, &c.project, body);
}

#[tokio::test]
async fn a_check_whose_count_differs_from_the_case_is_refused() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let case = case_147556();
    let (_server, client) = client_with(&case).await;
    let body = bundle(
        147556,
        "DEF levels",
        json!({ "kind": "expect_count", "selector": level_selector(), "equals": 6 }),
        json!([]),
    );
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(
        out.starts_with("case 147556: step 2: the check counts 6 but the case says 4 (\"exactly 4 levels\") - "),
        "{out}"
    );
    assert!(v2_lib::autorun::store::load_script(dir.path(), 147556).unwrap().is_none(), "nothing is written");
}

#[tokio::test]
async fn a_saved_value_with_a_because_is_listed_in_the_answer() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let case = case_of(7, "Save a rating", &[("Open", ""), ("Save", "A confirmation appears")], "");
    let (_server, client) = client_with(&case).await;
    let action = json!({ "kind": "expect_contains_text", "selector": "#toast", "value": "Record saved successfully" });
    let because = json!([{ "value": "Record saved successfully", "why": "the confirmation step 3 expects" }]);

    let bare = bundle(7, "Save a rating", action.clone(), json!([]));
    see(dir.path(), &bare);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &bare, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.starts_with("case 7: step 2: the text \"Record saved successfully\" is not in the case"), "{out}");

    let body = bundle(7, "Save a rating", action, because);
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(
        out.lines().any(|l| l == "case 7: value not in the case: step 2 \"Record saved successfully\" - the confirmation step 3 expects"),
        "{out}"
    );
    let saved = v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(saved.steps[1].because.len(), 1);
}

#[tokio::test]
async fn an_unchanged_resave_is_not_value_checked() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let case = case_of(7, "Save a rating", &[("Open", ""), ("Save", "A confirmation appears")], "");
    let (_server, client) = client_with(&case).await;
    let action = json!({ "kind": "expect_contains_text", "selector": "#toast", "value": "Record saved successfully" });
    let body = bundle(7, "Save a rating", action, json!([]));
    see(dir.path(), &body);
    let mut script: CaseScript = serde_json::from_value(serde_json::from_str::<Value>(&body).unwrap()[0].clone()).unwrap();
    script.organization = Some("acme".into());
    script.project = Some("Web".into());
    v2_lib::autorun::store::save_script(dir.path(), &script).unwrap();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("(unchanged)"), "{out}");
}
