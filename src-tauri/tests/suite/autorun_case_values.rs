//! A script's check values come from its case: a value the case does not
//! hold needs a `because`, and a count the case states differently is
//! refused (`case_values`, `case_text`, and the save route's value gate).

use crate::common;
use serde_json::{json, Value};
use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::autorun::case_text::{numbers, range_members, stated_counts, CaseText};
use v2_lib::autorun::case_values::{check_case_values, check_count, check_repair_values, values_of_step, ValueKind};
use v2_lib::autorun::edits::GiveUp;
use v2_lib::autorun::store::set_root;
use v2_lib::autorun::case_values::SourcedValue;
use v2_lib::autorun::{CaseScript, Provenance};
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
    // The case says 4 levels, but neither the check nor its step speaks of levels.
    let case = case_of(
        8,
        "Buttons",
        &[("Open the page", ""), ("Look at the toolbar", "The toolbar is shown")],
        "The DEF table holds exactly 4 levels",
    );
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
        out.lines().any(|l| l == "case 7: assumed: step 2 \"Record saved successfully\" - the confirmation step 3 expects"),
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

// ------------------------------------------------------- review fixes

fn pre_case(pre: &str) -> v2_lib::ado::TestCaseFull {
    case_of(11, "Plain", &[("Open the page", ""), ("Look", "A list is shown")], pre)
}

fn rows_count(n: u32) -> Value {
    json!({ "kind": "expect_row_count", "table": { "role": "table" }, "equals": n })
}

#[test]
fn a_stated_noun_matches_check_values_as_a_word_never_keys_and_a_year_is_no_count() {
    for pre in ["The user has 2 roles assigned", "The employee profile has 5 tabs", "The 2026 appraisal cycle is active"] {
        let case = pre_case(pre);
        let covered = two_steps(rows_count(7), json!([{ "value": "7", "why": "the 7 rows step 1 adds" }]));
        let r = check(&case, &covered, None);
        assert!(r.refusals.is_empty(), "{pre}: {:?}", r.refusals);
        assert_eq!(r.listed.len(), 1, "{pre}");
    }
    // The noun in a locator VALUE still contradicts.
    let case = pre_case("The user has 2 roles assigned");
    let action = json!({ "kind": "expect_count", "selector": { "role": "row", "name": "Roles" }, "equals": 7 });
    let r = check(&case, &two_steps(action, json!([])), None);
    assert!(r.refusals[0].contains("the check counts 7 but the case says 2"), "{:?}", r.refusals);
}

#[test]
fn an_order_is_hard_refused_only_when_longer_than_the_stated_count() {
    let case = case_147556();
    let order = |n: usize| {
        let texts: Vec<String> = (1..=n).map(|i| format!("Level {i}")).collect();
        json!({ "kind": "expect_order", "selector": level_selector(), "texts": texts })
    };
    let r = check(&case, &two_steps(order(2), json!([])), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    let r = check(&case, &two_steps(order(6), json!([])), None);
    assert!(r.refusals.iter().any(|l| l.contains("the check counts 6 but the case says 4")), "{:?}", r.refusals);
    // A selector name holding "texts" is no order.
    let action = json!({ "kind": "expect_count", "selector": { "role": "row", "name": "texts" }, "equals": 3 });
    let r = check(&pre_case("The DEF table holds exactly 4 levels"), &two_steps(action, json!([{ "value": "3", "why": "step 1 adds three" }])), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
}

#[test]
fn bounds_above_a_stated_count_and_a_noun_in_the_step_text_are_hard_refused() {
    let case = case_147556();
    let least = json!({ "kind": "expect_count", "selector": level_selector(), "at_least": 6 });
    let r = check(&case, &two_steps(least, json!([{ "value": "6", "why": "one row per DEF level" }])), None);
    assert!(r.refusals[0].contains("the check counts 6 but the case says 4"), "{:?}", r.refusals);
    let most = json!({ "kind": "expect_count", "selector": { "css": "#ddl option" }, "at_most": 3 });
    let r = check(&case, &two_steps(most, json!([{ "value": "3", "why": "x" }])), None);
    assert!(r.refusals[0].contains("the check counts 3 but the case says 4"), "{:?}", r.refusals);
    let each = json!({ "kind": "expect_each", "selector": level_selector(), "min": 6, "not_empty": true });
    let r = check(&case, &two_steps(each, json!([])), None);
    assert!(r.refusals[0].contains("the check counts 6 but the case says 4"), "{:?}", r.refusals);
    // The selector never spells the noun; step 2's own text does ("the level dropdown").
    let css = json!({ "kind": "expect_count", "selector": { "css": "#ddlDefLvl option" }, "equals": 6 });
    let r = check(&case, &two_steps(css, json!([{ "value": "6", "why": "one row per DEF level" }])), None);
    assert!(r.refusals[0].contains("the check counts 6 but the case says 4"), "{:?}", r.refusals);
}

#[test]
fn expect_each_with_an_attribute_is_not_compared() {
    let case = case_147556();
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_each", "selector": "#rows", "attribute": "aria-checked", "equals": "true" },
        { "kind": "expect_each", "selector": "#rows", "attribute": "class", "contains": "selected row" },
    ] }]));
    assert!(values_of_step(&s.steps[0]).is_empty());
    assert!(check(&case, &s, None).refusals.is_empty());
}

#[test]
fn more_range_phrasings_are_read() {
    assert_eq!(range_members("level 1 to 4"), vec!["level 1", "level 2", "level 3", "level 4"]);
    assert_eq!(range_members("levels 1-4"), vec!["level 1", "level 2", "level 3", "level 4"]);
    assert_eq!(range_members("level 1 - level 3"), vec!["level 1", "level 2", "level 3"]);
    assert_eq!(range_members("levels 1 through 2"), vec!["level 1", "level 2"]);
}

#[test]
fn number_words_are_case_numbers() {
    let case = case_of(12, "Grid", &[("Open", ""), ("Look", "The grid shows four columns")], "");
    let n = text_of(&case).case_numbers();
    assert!(n.contains(&4) && !n.contains(&5));
    let action = json!({ "kind": "expect_count", "selector": { "role": "columnheader" }, "equals": 4 });
    assert!(check(&case, &two_steps(action, json!([])), None).refusals.is_empty());
}

#[test]
fn a_because_names_the_app_only_by_its_opening_or_a_whole_phrase() {
    let case = case_147556();
    let action = json!({ "kind": "expect_contains_text", "selector": "#x", "value": "Six levels here" });
    for why in ["the message that is seen in step 3", "the record currently selected in step 2", "the observed total step 4 states"] {
        let r = check(&case, &two_steps(action.clone(), json!([{ "value": "Six levels here", "why": why }])), None);
        assert!(r.refusals.is_empty(), "{why}: {:?}", r.refusals);
    }
    for why in ["The app shows six", "the page has six", "what is on the page today", "because the page shows 6"] {
        let r = check(&case, &two_steps(action.clone(), json!([{ "value": "Six levels here", "why": why }])), None);
        assert_eq!(r.refusals.len(), 1, "{why}: {:?}", r.refusals);
    }
}

#[test]
fn the_test_prefix_matches_as_a_whole_token() {
    let case = pre_case("");
    let action = |v: &str| json!({ "kind": "expect_contains_text", "selector": "#x", "value": v });
    let r = check_case_values(&two_steps(action("Attendance marked"), json!([])), &text_of(&case), None, "AT");
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
    for ok in ["AT Cycle 9", "AT-9", "AT_9", "AT"] {
        let r = check_case_values(&two_steps(action(ok), json!([])), &text_of(&case), None, "AT");
        assert!(r.refusals.is_empty(), "{ok}: {:?}", r.refusals);
    }
}

#[test]
fn a_step_without_because_keeps_its_signature_and_an_old_script_loads() {
    let s = two_steps(json!({ "kind": "check_text", "value": "x" }), json!([]));
    let step = &s.steps[1];
    assert!(step.because.is_empty());
    assert_eq!(
        v2_lib::autorun::edits::step_signature(step),
        format!("{}|", serde_json::to_string(&step.actions).unwrap())
    );
    let with = two_steps(json!({ "kind": "check_text", "value": "x" }), json!([{ "value": "x", "why": "y" }]));
    assert!(v2_lib::autorun::edits::step_signature(&with.steps[1]).contains("|because:"));
    let old: CaseScript = serde_json::from_value(json!({
        "case_id": 1, "title": "t", "steps": [{ "step_number": 1, "actions": [] }]
    }))
    .unwrap();
    assert!(old.steps[0].because.is_empty());
    assert!(!serde_json::to_string(&old).unwrap().contains("because"));
}

#[test]
fn a_patch_steps_entry_carries_its_because() {
    use v2_lib::ai_bridge::apply_patch_steps;
    let stored = two_steps(json!({ "kind": "check_text", "value": "x" }), json!([]));
    let load = move |_: i32| Ok(Some(stored.clone()));
    let mut items = vec![json!({
        "case_id": 147556,
        "patch_steps": [{ "step": 2, "actions": [{ "kind": "check_text", "value": "x" }], "because": [{ "value": "x", "why": "step 3 names it" }] }]
    })];
    apply_patch_steps(&mut items, &load).unwrap();
    let s: CaseScript = serde_json::from_value(items[0].clone()).unwrap();
    assert_eq!(s.steps[1].because.len(), 1);
}

fn three_step_case() -> v2_lib::ado::TestCaseFull {
    case_of(7, "Three", &[("One", ""), ("Two", "Alpha appears"), ("Three", "Beta appears")], "")
}

fn three_step_body(second: &str, third: &str, because: Value) -> String {
    json!([{
        "case_id": 7, "title": "Three",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] },
            { "step_number": 2, "actions": [{ "kind": "expect_contains_text", "selector": "#t", "value": second }], "because": because },
            { "step_number": 3, "actions": [{ "kind": "expect_contains_text", "selector": "#t", "value": third }] },
        ]
    }])
    .to_string()
}

#[tokio::test]
async fn a_repair_is_value_checked_only_on_its_declared_steps() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with(&three_step_case()).await;
    // Saved directly: step 2's value is not in the case.
    let first = three_step_body("Gamma", "Beta", json!([]));
    let mut saved: CaseScript = serde_json::from_value(serde_json::from_str::<Value>(&first).unwrap()[0].clone()).unwrap();
    saved.organization = Some("acme".into());
    saved.project = Some("Web".into());
    v2_lib::autorun::store::save_script(dir.path(), &saved).unwrap();
    // Repair step 3 only.
    let repaired = three_step_body("Gamma", "Beta appears", json!([]));
    let body = json!({
        "scripts": serde_json::from_str::<Value>(&repaired).unwrap(),
        "edits": [{ "case_id": 7, "steps": [3], "why": "the toast moved" }],
    })
    .to_string();
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    // Repair step 2 and it is judged.
    let body = json!({
        "scripts": serde_json::from_str::<Value>(&three_step_body("Delta", "Beta appears", json!([]))).unwrap(),
        "edits": [{ "case_id": 7, "steps": [2], "why": "the toast moved" }],
    })
    .to_string();
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.starts_with("case 7: step 2: the text \"Delta\" is not in the case"), "{out}");
}

#[tokio::test]
async fn a_dry_run_lists_the_values_not_in_the_case() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with(&three_step_case()).await;
    let because = json!([{ "value": "Gamma", "why": "the toast step 3 expects" }]);
    let body = json!({
        "scripts": serde_json::from_str::<Value>(&three_step_body("Gamma", "Beta", because)).unwrap(),
        "dry_run": true,
    })
    .to_string();
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.lines().any(|l| l == "case 7: assumed: step 2 \"Gamma\" - the toast step 3 expects"), "{out}");
    assert!(v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().is_none());
}

#[tokio::test]
async fn value_notes_count_toward_the_notes_cap() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with(&three_step_case()).await;
    let because: Vec<Value> = (0..25).map(|i| json!({ "value": format!("Unused {i}"), "why": "step 3 names it" })).collect();
    let body = three_step_body("Alpha", "Beta", Value::Array(because));
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let notes = out.lines().filter(|l| l.contains(": note: ")).count();
    assert_eq!(notes, 20, "{out}");
    assert!(out.lines().any(|l| l == "and 5 more notes"), "{out}");
}

#[test]
fn the_step_text_names_the_noun_only_when_the_check_names_none_of_its_own() {
    let case = case_147556();
    let css = json!({ "kind": "expect_count", "selector": { "css": "#ddlDefLvl option" }, "equals": 6 });
    let r = check(&case, &two_steps(css, json!([{ "value": "6", "why": "one row per DEF level" }])), None);
    assert!(r.refusals[0].contains("the check counts 6 but the case says 4"), "{:?}", r.refusals);
    let button = json!({ "kind": "expect_count", "selector": { "role": "button" }, "equals": 3 });
    let r = check(&case, &two_steps(button, json!([{ "value": "3", "why": "the three buttons step 1 names" }])), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.listed.len(), 1);
}

// ------------------------------------------------------- the check floor

fn give(step: i32, kind: &str, why: &str) -> GiveUp {
    GiveUp { step, kind: kind.into(), why: why.into() }
}

fn two_step_script(first: Value, second: Value) -> CaseScript {
    script(json!([
        { "step_number": 1, "actions": first },
        { "step_number": 2, "actions": second },
    ]))
}

fn act(v: Value) -> v2_lib::browser::actions::Action {
    serde_json::from_value(v).unwrap()
}

/// Three checks: one in step 1, an `expect_visible` and an `expect_count`
/// of 4 (the case's count) in step 2.
fn three_checks() -> CaseScript {
    two_step_script(
        json!([{ "kind": "click", "selector": "#a" }, { "kind": "expect_visible", "selector": "#a" }]),
        json!([
            { "kind": "expect_visible", "selector": "#b" },
            { "kind": "expect_count", "selector": level_selector(), "equals": 4 },
        ]),
    )
}

fn repair(old: &CaseScript, new: &CaseScript, gives_up: &[GiveUp]) -> Vec<String> {
    check_repair_values(old, new, &text_of(&case_147556()), &[2], gives_up, "AUTOTEST")
}

fn without_the_visible_check() -> CaseScript {
    two_step_script(
        json!([{ "kind": "click", "selector": "#a" }, { "kind": "expect_visible", "selector": "#a" }]),
        json!([{ "kind": "expect_count", "selector": level_selector(), "equals": 4 }]),
    )
}

#[test]
fn a_repair_that_changes_an_expected_count_to_a_value_not_in_the_case_is_refused() {
    let old = two_step_script(json!([{ "kind": "click", "selector": "#a" }]), json!([{ "kind": "expect_count", "selector": level_selector(), "equals": 4 }]));
    let new = two_step_script(json!([{ "kind": "click", "selector": "#a" }]), json!([{ "kind": "expect_count", "selector": level_selector(), "equals": 6 }]));
    let r = repair(&old, &new, &[]);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].starts_with("step 2: the repair drops \"4\", which the case gives - a repair keeps the case's values;"), "{r:?}");
    // And the value check of the new script says the count contradicts the case.
    let found = check_case_values(&new, &text_of(&case_147556()), Some(&[2]), "AUTOTEST");
    assert!(found.refusals.iter().any(|r| r.contains("the check counts 6 but the case says 4")), "{:?}", found.refusals);
}

#[test]
fn a_repair_that_removes_a_check_is_refused() {
    let r = repair(&three_checks(), &without_the_visible_check(), &[]);
    assert_eq!(r.len(), 2, "{r:?}");
    assert!(r[0].starts_with("the repair lowers the script's checks from 3 to 2 - a repair keeps every check unless"), "{r:?}");
    assert!(
        r[1].starts_with("step 2: the repair removes 1 of its checks and gives_up names 0 - add { \"step\": 2, \"kind\": \"<the check's kind>\", \"why\": \"...\" } for each one, or keep it"),
        "{r:?}"
    );
}

#[test]
fn a_repair_that_names_the_check_it_gives_up_passes() {
    let r = repair(&three_checks(), &without_the_visible_check(), &[give(2, " Expect_Visible ", "step 4 already counts the same rows")]);
    assert!(r.is_empty(), "{r:?}");
}

#[test]
fn a_given_up_check_takes_its_value_with_it() {
    let old = without_the_visible_check();
    let new = two_step_script(json!([{ "kind": "click", "selector": "#a" }, { "kind": "expect_visible", "selector": "#a" }]), json!([]));
    let r = repair(&old, &new, &[]);
    assert!(r.iter().any(|l| l.contains("the repair drops \"4\"")), "{r:?}");
    let r = repair(&old, &new, &[give(2, "expect_count", "step 4 already counts the same rows")]);
    assert!(r.is_empty(), "{r:?}");
}

#[test]
fn giving_up_a_check_because_of_the_app_is_refused() {
    let r = repair(&three_checks(), &without_the_visible_check(), &[give(2, "expect_visible", "the app shows no icon")]);
    assert!(
        r.contains(&"step 2: a check is never given up because of what the app shows - keep it as the case says and call report_case_divergence".to_string()),
        "{r:?}"
    );
    // The refused entry does not count: the floor still fails.
    assert!(r.iter().any(|l| l.starts_with("the repair lowers the script's checks from 3 to 2")), "{r:?}");
}

#[test]
fn gives_up_naming_more_than_the_repair_removes_is_refused() {
    // The text check is swapped for a visible one: one kind goes, the count stays.
    let mut old = three_checks();
    old.steps[1].actions[0] = act(json!({ "kind": "expect_text", "selector": "#t", "equals": "Saved" }));
    let r = repair(&old, &three_checks(), &[give(2, "expect_text", "step 4 already proves it")]);
    assert_eq!(
        r,
        vec!["step 2: gives_up names 1 checks but the repair removes 0 there - name only the checks it removes".to_string()]
    );
    // Naming a check the repair keeps matches nothing it removes.
    let r = repair(&three_checks(), &three_checks(), &[give(2, "expect_visible", "step 4 already proves it")]);
    assert!(r.iter().any(|l| l.starts_with("step 2: gives_up names a expect_visible check, and the old step 2 has none")), "{r:?}");
}

#[test]
fn a_kind_the_old_step_never_had_is_refused() {
    let r = repair(
        &three_checks(),
        &three_checks(),
        &[give(2, "expect_text", "step 4 already proves it"), give(9, "expect_visible", "step 4 already proves it")],
    );
    assert!(r.contains(&"step 2: gives_up names a expect_text check, and the old step 2 has none - name the kind of a check the repair removes".to_string()), "{r:?}");
    assert!(r.iter().any(|l| l.starts_with("step 9: gives_up names a expect_visible check")), "{r:?}");
}

#[test]
fn a_malformed_gives_up_entry_is_refused_once() {
    let r = repair(&three_checks(), &three_checks(), &[give(2, "", "x"), give(2, "expect_visible", ""), give(2, "expect_visible", "a\nb")]);
    let shape = "the edit's gives_up entries are { \"step\", \"kind\", \"why\" }, the why one line of at most 200 characters";
    assert_eq!(r.iter().filter(|l| *l == shape).count(), 1, "{r:?}");
}

#[test]
fn a_repair_that_turns_a_checked_step_unchecked_is_refused() {
    let mut new = two_step_script(
        json!([{ "kind": "click", "selector": "#a" }, { "kind": "expect_visible", "selector": "#a" }]),
        json!([]),
    );
    new.steps[1].unchecked = Some("a person checks the levels".into());
    let r = repair(&three_checks(), &new, &[]);
    assert!(r.iter().any(|l| l.starts_with("the repair lowers the script's checks from 3 to 1 - ")), "{r:?}");
    assert!(r.iter().any(|l| l.starts_with("step 2: the repair removes 2 of its checks and gives_up names 0 - ")), "{r:?}");
}

#[test]
fn the_floor_is_the_stored_count() {
    let mut old = three_checks();
    old.checks = 5;
    let r = repair(&old, &three_checks(), &[]);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].starts_with("the repair lowers the script's checks from 5 to 3 - "), "{r:?}");
}

#[test]
fn a_locator_repair_keeps_values_and_passes() {
    let mut new = three_checks();
    new.steps[1].actions = vec![
        act(json!({ "kind": "expect_visible", "selector": "#b2" })),
        act(json!({ "kind": "expect_count", "selector": { "role": "option", "name": "Level" }, "equals": 4 })),
    ];
    assert!(repair(&three_checks(), &new, &[]).is_empty());
}

#[test]
fn a_repair_may_widen_a_check_that_keeps_the_case_words() {
    let case = case_of(7, "Save", &[("Open", ""), ("Save", "Saved appears")], "");
    let old = two_step_script(json!([{ "kind": "click", "selector": "#a" }]), json!([{ "kind": "expect_text", "selector": "#t", "equals": "Saved" }]));
    let new = two_step_script(json!([{ "kind": "click", "selector": "#a" }]), json!([{ "kind": "expect_contains_text", "selector": "#t", "value": "Saved successfully" }]));
    let r = check_repair_values(&old, &new, &text_of(&case), &[2], &[], "AUTOTEST");
    assert!(r.is_empty(), "{r:?}");
    // Another word does not keep it.
    let other = two_step_script(json!([{ "kind": "click", "selector": "#a" }]), json!([{ "kind": "expect_text", "selector": "#t", "equals": "Done" }]));
    let r = check_repair_values(&old, &other, &text_of(&case), &[2], &[], "AUTOTEST");
    assert!(r.iter().any(|l| l.contains("the repair drops \"Saved\"")), "{r:?}");
}

#[test]
fn a_value_the_rewritten_case_no_longer_holds_may_change() {
    let case = case_of(
        1,
        "DEF levels",
        &[("Open", ""), ("Open the level dropdown", "The dropdown lists exactly 6 levels")],
        "The DEF table holds exactly 6 levels",
    );
    let old = two_step_script(json!([{ "kind": "click", "selector": "#a" }]), json!([{ "kind": "expect_count", "selector": level_selector(), "equals": 4 }]));
    let new = two_step_script(json!([{ "kind": "click", "selector": "#a" }]), json!([{ "kind": "expect_count", "selector": level_selector(), "equals": 6 }]));
    assert!(check_repair_values(&old, &new, &text_of(&case), &[2], &[], "AUTOTEST").is_empty());
}

fn repair_case() -> v2_lib::ado::TestCaseFull {
    case_of(7, "Save a rating", &[("Open", ""), ("Save", "A confirmation appears")], "")
}

fn two_check_body(second: Value) -> String {
    json!([{
        "case_id": 7, "title": "Save a rating",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "click", "selector": "#a" }] },
            { "step_number": 2, "actions": second },
        ]
    }])
    .to_string()
}

/// Step 2 holds a visible check and a count of one row.
fn old_two_checks() -> Value {
    json!([
        { "kind": "expect_visible", "selector": "#t" },
        { "kind": "expect_count", "selector": "#rows", "equals": 1 },
    ])
}

fn store_old(root: &std::path::Path, second: Value) {
    let mut saved: CaseScript = serde_json::from_value(serde_json::from_str::<Value>(&two_check_body(second)).unwrap()[0].clone()).unwrap();
    saved.organization = Some("acme".into());
    saved.project = Some("Web".into());
    v2_lib::autorun::store::save_script(root, &saved).unwrap();
}

#[tokio::test]
async fn a_repair_lowering_the_checks_is_refused_through_the_save() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with(&repair_case()).await;
    store_old(dir.path(), old_two_checks());
    let body = json!({
        "scripts": serde_json::from_str::<Value>(&two_check_body(json!([{ "kind": "expect_visible", "selector": "#t" }]))).unwrap(),
        "edits": [{ "case_id": 7, "steps": [2], "why": "the rows moved" }],
    })
    .to_string();
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.starts_with("case 7: the repair lowers the script's checks from 2 to 1 - "), "{out}");
    assert_eq!(check_count(&v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap()), 2, "nothing is written");
}

#[tokio::test]
async fn a_named_check_given_up_is_saved_and_said() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with(&repair_case()).await;
    store_old(dir.path(), old_two_checks());
    let body = json!({
        "scripts": serde_json::from_str::<Value>(&two_check_body(json!([{ "kind": "expect_visible", "selector": "#t" }]))).unwrap(),
        "edits": [{ "case_id": 7, "steps": [2], "why": "the rows moved", "gives_up": [
            { "step": 2, "kind": "expect_count", "why": "step 4 already counts the same rows" }
        ] }],
    })
    .to_string();
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[1],
        "case 7: check given up: step 2 expect_count - step 4 already counts the same rows. The person sees it on the case; name it in your report."
    );
    let saved = v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(saved.checks, 1);
    assert_eq!(saved.given_up.len(), 1);
    assert_eq!((saved.given_up[0].step, saved.given_up[0].kind.as_str()), (2, "expect_count"));
    assert!(!saved.given_up[0].at.is_empty());
}

#[tokio::test]
async fn every_save_stamps_the_number_of_checks() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with(&repair_case()).await;
    let body = two_check_body(old_two_checks());
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let saved = v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(saved.checks as usize, check_count(&saved));
    assert_eq!(saved.checks, 2);
    assert!(saved.given_up.is_empty());
}

#[tokio::test]
async fn an_unchanged_resave_keeps_its_given_up_and_stays_unchanged() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with(&repair_case()).await;
    let body = two_check_body(old_two_checks());
    see(dir.path(), &body);
    let mut saved: CaseScript = serde_json::from_value(serde_json::from_str::<Value>(&body).unwrap()[0].clone()).unwrap();
    saved.organization = Some("acme".into());
    saved.project = Some("Web".into());
    saved.given_up = vec![v2_lib::autorun::GivenUp {
        step: 2,
        kind: "expect_text".into(),
        why: "step 4 proves it".into(),
        at: "2026-10-21T10:00:00Z".into(),
    }];
    v2_lib::autorun::store::save_script(dir.path(), &saved).unwrap();
    assert_eq!(v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap().checks, 0, "an old script has no stored floor");
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("(unchanged)"), "{out}");
    let after = v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(after.given_up, saved.given_up);
    assert_eq!(after.checks, 2, "stamped on the next save");
}

#[test]
fn the_persons_save_stamps_the_checks_and_clears_what_was_given_up() {
    let dir = TempDir::new();
    let mut s = three_checks();
    s.case_id = 7;
    s.checks = 9;
    s.given_up = vec![v2_lib::autorun::GivenUp {
        step: 2,
        kind: "expect_text".into(),
        why: "step 4 proves it".into(),
        at: "2026-10-21T10:00:00Z".into(),
    }];
    v2_lib::commands::autorun::save_script_from_editor(dir.path(), "acme", "Web", s).unwrap();
    let after = v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(after.checks, 3);
    assert!(after.given_up.is_empty());
}

// ------------------------------------------------------ value sources

fn sv(step: i32, value: &str, provenance: Option<Provenance>, why: &str) -> SourcedValue {
    SourcedValue { step, value: value.into(), provenance, why: why.into() }
}

fn five_step_script() -> CaseScript {
    let texts: Vec<String> = (1..=4).map(|i| format!("Level {i}")).collect();
    script(json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/def" }] },
        { "step_number": 2, "actions": [{ "kind": "expect_order", "selector": level_selector(), "texts": texts }] },
        { "step_number": 3, "actions": [
            { "kind": "fill", "selector": "#goal", "value": "AUTOTEST Goal 1" },
            { "kind": "expect_text", "selector": "#goal", "equals": "AUTOTEST Goal 1" },
        ] },
        { "step_number": 4, "actions": [{ "kind": "expect_contains_text", "selector": "#toast", "value": "Record saved successfully" }],
          "because": [{ "value": "Record saved successfully", "why": "the confirmation step 3 expects" }] },
        { "step_number": 5, "actions": [{ "kind": "expect_count", "selector": "#rows", "equals": 1 }] },
    ]))
}

#[test]
fn every_value_in_scope_has_a_source() {
    let case = case_of(
        147556,
        "DEF levels",
        &[("Open", ""), ("Open the level dropdown", "The dropdown lists Level 1 to Level 4"), ("Add", ""), ("Save", ""), ("Look", "")],
        "",
    );
    let r = check(&case, &five_step_script(), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    use Provenance::*;
    assert_eq!(
        r.sourced,
        vec![
            sv(2, "4", Some(FromCase), ""),
            sv(2, "Level 1", Some(FromCase), ""),
            sv(2, "Level 2", Some(FromCase), ""),
            sv(2, "Level 3", Some(FromCase), ""),
            sv(2, "Level 4", Some(FromCase), ""),
            sv(3, "AUTOTEST Goal 1", Some(Invented), "typed by the script"),
            sv(4, "Record saved successfully", Some(Assumed), "the confirmation step 3 expects"),
            sv(5, "1", None, ""),
        ]
    );
}

#[test]
fn a_because_marked_invented_is_sourced_as_invented() {
    let case = case_of(9, "Pay", &[("Open", ""), ("Pay", "A receipt appears")], "");
    let action = json!({ "kind": "expect_contains_text", "selector": "#total", "value": "USD" });
    let s = two_steps(action, json!([{ "value": "USD", "why": "one currency the case leaves free", "provenance": "invented" }]));
    let r = check(&case, &s, None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced, vec![sv(2, "USD", Some(Provenance::Invented), "one currency the case leaves free")]);
}

#[test]
fn a_because_marked_from_case_is_refused() {
    let case = case_of(9, "Pay", &[("Open", ""), ("Pay", "A receipt appears")], "");
    let action = json!({ "kind": "expect_contains_text", "selector": "#total", "value": "USD" });
    let s = two_steps(action, json!([{ "value": "USD", "why": "the case says so", "provenance": "from_case" }]));
    let r = check(&case, &s, None);
    assert!(
        r.refusals.contains(&"step 2: a \"because\" is \"assumed\" or \"invented\" - from_case is what the save works out for a value the case holds, which needs no because".to_string()),
        "{:?}",
        r.refusals
    );
}

#[test]
fn an_unknown_provenance_fails_the_parse_naming_the_three() {
    let e = serde_json::from_value::<v2_lib::autorun::Because>(json!({ "value": "x", "why": "y", "provenance": "guessed" }))
        .unwrap_err()
        .to_string();
    assert!(e.contains("unknown variant") && e.contains("from_case") && e.contains("assumed") && e.contains("invented"), "{e}");
}

#[test]
fn an_old_because_without_provenance_reads_as_assumed() {
    let case = case_of(9, "Rows", &[("Open", ""), ("Look", "Rows are shown")], "");
    let entry = json!({ "value": "6", "why": "one row per DEF level the precondition lists" });
    let b: v2_lib::autorun::Because = serde_json::from_value(entry.clone()).unwrap();
    assert_eq!(b.provenance, None);
    assert_eq!(serde_json::to_value(&b).unwrap(), entry, "serializes back without the key");
    let s = two_steps(rows_count(6), json!([entry]));
    let r = check(&case, &s, None);
    assert_eq!(r.sourced, vec![sv(2, "6", Some(Provenance::Assumed), "one row per DEF level the precondition lists")]);
}

async fn answer_for(case: &v2_lib::ado::TestCaseFull, body: String) -> (u16, String) {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with(case).await;
    see(dir.path(), &body);
    route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await
}

fn case_7() -> v2_lib::ado::TestCaseFull {
    case_of(
        7,
        "Rate it",
        &[
            ("Open", ""),
            ("Open the level dropdown", "The dropdown lists Level 1 to Level 4"),
            ("Save", "A confirmation appears"),
            ("Rate", "A rating is shown"),
        ],
        "",
    )
}

fn bundle_7(step_3: Value, step_4: Value) -> String {
    let texts: Vec<String> = (1..=4).map(|i| format!("Level {i}")).collect();
    json!([{
        "case_id": 7,
        "title": "Rate it",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/def" }] },
            { "step_number": 2, "actions": [{ "kind": "expect_order", "selector": level_selector(), "texts": texts }] },
            step_3,
            step_4,
        ]
    }])
    .to_string()
}

#[tokio::test]
async fn the_save_answer_gives_each_values_source() {
    let step_3 = json!({ "step_number": 3,
        "actions": [{ "kind": "expect_contains_text", "selector": "#toast", "value": "Record saved successfully" }],
        "because": [{ "value": "Record saved successfully", "why": "the confirmation step 3 expects" }] });
    let step_4 = json!({ "step_number": 4, "actions": [
        { "kind": "fill", "selector": "#rating", "value": "AUTOTEST Rating 1" },
        { "kind": "expect_text", "selector": "#rating", "equals": "AUTOTEST Rating 1" },
        { "kind": "expect_count", "selector": "#rows", "equals": 1 },
    ] });
    let (status, out) = answer_for(&case_7(), bundle_7(step_3, step_4)).await;
    assert_eq!(status, 200, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.contains(&"case 7: check values: 5 from the case, 1 assumed, 1 invented, 1 exempt"), "{out}");
    assert!(lines.contains(&"case 7: assumed: step 3 \"Record saved successfully\" - the confirmation step 3 expects"), "{out}");
    assert!(lines.contains(&"case 7: invented: step 4 \"AUTOTEST Rating 1\" - typed by the script"), "{out}");
    assert_eq!(
        lines.iter().filter(|l| **l == "Repeat every assumed and invented value in your report before you say the work is done.").count(),
        1,
        "{out}"
    );
}

#[tokio::test]
async fn the_value_lines_stop_at_forty() {
    let actions: Vec<Value> = (1..=45)
        .map(|i| json!({ "kind": "expect_contains_text", "selector": "#toast", "value": format!("Mango {i} stock") }))
        .collect();
    let because: Vec<Value> =
        (1..=45).map(|i| json!({ "value": format!("Mango {i} stock"), "why": "the stock the case lists" })).collect();
    let step_3 = json!({ "step_number": 3, "actions": actions, "because": because });
    let step_4 = json!({ "step_number": 4, "actions": [{ "kind": "expect_count", "selector": "#rows", "equals": 1 }] });
    let (status, out) = answer_for(&case_7(), bundle_7(step_3, step_4)).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out.lines().filter(|l| l.starts_with("case 7: assumed: step 3")).count(), 40, "{out}");
    assert!(
        out.lines().any(|l| l == "and 5 more assumed or invented values - get_autorun_script shows each step's because"),
        "{out}"
    );
    assert!(out.contains("Repeat every assumed and invented value"), "{out}");
}

#[tokio::test]
async fn a_save_with_only_case_values_lists_no_value_line() {
    let step_3 = json!({ "step_number": 3, "actions": [{ "kind": "expect_count", "selector": "#rows", "equals": 1 }] });
    let step_4 = json!({ "step_number": 4, "actions": [{ "kind": "expect_count", "selector": "#rows", "equals": 1 }] });
    let (status, out) = answer_for(&case_7(), bundle_7(step_3, step_4)).await;
    assert_eq!(status, 200, "{out}");
    assert!(out.lines().any(|l| l == "case 7: check values: 5 from the case, 0 assumed, 0 invented, 2 exempt"), "{out}");
    assert!(!out.contains(": assumed: ") && !out.contains(": invented: "), "{out}");
    assert!(!out.contains("Repeat every assumed"), "{out}");
}

// ------------------------------------------------- the check floor, review round 1

#[test]
fn a_gives_up_entry_must_match_a_check_actually_removed() {
    let old = two_step_script(
        json!([{ "kind": "click", "selector": "#a" }]),
        json!([
            { "kind": "expect_count", "selector": level_selector(), "equals": 4 },
            { "kind": "expect_text", "selector": "#t", "equals": "Saved" },
        ]),
    );
    let new = two_step_script(
        json!([{ "kind": "click", "selector": "#a" }]),
        json!([{ "kind": "expect_count", "selector": level_selector(), "equals": 4 }]),
    );
    // The count is still there: naming it gives up nothing real.
    let r = repair(&old, &new, &[give(2, "expect_count", "step 4 already counts the same rows")]);
    assert!(
        r.contains(&"step 2: gives_up names a expect_count check, and the old step 2 has none - name the kind of a check the repair removes".to_string()),
        "{r:?}"
    );
    assert!(r.iter().any(|l| l.starts_with("step 2: the repair removes 1 of its checks and gives_up names 0")), "{r:?}");
    // The text is what went.
    let r = repair(&old, &new, &[give(2, "expect_text", "step 4 already proves the text")]);
    assert!(r.is_empty(), "{r:?}");
}

#[test]
fn a_wrong_kind_never_excuses_a_dropped_value() {
    let old = two_step_script(
        json!([{ "kind": "click", "selector": "#a" }]),
        json!([
            { "kind": "expect_count", "selector": level_selector(), "equals": 4 },
            { "kind": "expect_visible", "selector": "#v" },
        ]),
    );
    let new = two_step_script(json!([{ "kind": "click", "selector": "#a" }]), json!([{ "kind": "expect_count", "selector": level_selector(), "equals": 6 }]));
    // The visible check went, the count moved to 6: naming the count excuses nothing.
    let r = repair(&old, &new, &[give(2, "expect_count", "step 4 already counts the same rows")]);
    assert!(r.iter().any(|l| l.contains("the repair drops \"4\"")), "{r:?}");
}

#[test]
fn a_cleanup_check_can_be_given_up_and_another_check_does_not_offset_it() {
    let with_cleanup = |first: Value, cleanup: Value| -> CaseScript {
        script(json!([
            { "step_number": 1, "actions": first },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": "#b" }] },
            { "step_number": 3, "cleanup": true, "actions": cleanup },
        ]))
    };
    let old = with_cleanup(json!([{ "kind": "expect_visible", "selector": "#a" }]), json!([{ "kind": "expect_visible", "selector": "#c" }]));
    let new = with_cleanup(
        json!([{ "kind": "expect_visible", "selector": "#a" }, { "kind": "expect_visible", "selector": "#d" }]),
        json!([]),
    );
    let ask = |g: &[GiveUp]| check_repair_values(&old, &new, &text_of(&case_147556()), &[3], g, "AUTOTEST");
    let r = ask(&[]);
    assert!(r.iter().any(|l| l.starts_with("step 3: the repair removes 1 of its checks and gives_up names 0")), "{r:?}");
    let r = ask(&[give(3, "expect_visible", "the session is closed by step 4")]);
    assert!(r.is_empty(), "{r:?}");
}

#[tokio::test]
async fn an_unchanged_resave_never_lowers_a_stored_floor() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with(&repair_case()).await;
    let body = two_check_body(old_two_checks());
    see(dir.path(), &body);
    let mut saved: CaseScript = serde_json::from_value(serde_json::from_str::<Value>(&body).unwrap()[0].clone()).unwrap();
    saved.organization = Some("acme".into());
    saved.project = Some("Web".into());
    saved.checks = 5;
    v2_lib::autorun::store::save_script(dir.path(), &saved).unwrap();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("(unchanged)"), "{out}");
    assert_eq!(v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap().checks, 5);
}

#[tokio::test]
async fn a_stray_key_in_a_gives_up_entry_is_named() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with(&repair_case()).await;
    store_old(dir.path(), old_two_checks());
    let body = json!({
        "scripts": serde_json::from_str::<Value>(&two_check_body(json!([{ "kind": "expect_visible", "selector": "#t" }]))).unwrap(),
        "edits": [{ "case_id": 7, "steps": [2], "why": "the rows moved", "gives_up": [
            { "step": 2, "kind": "expect_count", "reason": "step 4 already counts the same rows" }
        ] }],
    })
    .to_string();
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("unknown field") && out.contains("reason"), "{out}");
}
