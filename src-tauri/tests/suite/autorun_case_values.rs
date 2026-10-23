//! A script's check values come from its case: a value the case does not
//! hold needs a `because`, and a count the case states differently is
//! refused (`case_values`, `case_text`, and the save route's value gate).

use crate::common;
use serde_json::{json, Value};
use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::autorun::case_text::{numbers, range_members, stated_counts, CaseText};
use v2_lib::autorun::case_values::{check_case_values, check_case_values_against, check_count, check_repair_values, values_of_step, ValueKind};
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
            { "kind": "expect_count", "selector": "#rows", "at_least": 1 },
            { "kind": "expect_count", "selector": "#rows", "at_least": 0 },
            { "kind": "expect_each", "selector": "#rows", "min": 1, "not_empty": true },
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

/// REPLACES `patterns_attributes_and_styles_are_not_compared` (item 39 reverses it): every check kind
/// gives its values, a pattern, an attribute, a style, a state and a visible name included; an
/// address is still not one.
#[test]
fn every_check_kind_gives_its_values() {
    let case = case_147556();
    let s = script(json!([
        { "step_number": 1, "actions": [
            { "kind": "expect_text", "selector": "#a", "matches": "Level (5|6)" },
            { "kind": "expect_attribute", "selector": "#a", "name": "title", "equals": "Six levels" },
            { "kind": "expect_style", "selector": "#a", "property": "color", "equals": "rgb(1, 2, 3)" },
            { "kind": "check_url", "contains": "/def/levels6" },
            { "kind": "expect_each", "selector": "#rows", "matches": "Level 6" },
            { "kind": "expect_checked", "selector": "#a" },
            { "kind": "expect_visible", "selector": { "role": "button", "name": "Save" } },
        ] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#go" }] },
    ]));
    let values: Vec<(ValueKind, String, bool)> =
        values_of_step(&s.steps[0]).into_iter().map(|v| (v.kind, v.value, v.strict)).collect();
    let want = vec![
        (ValueKind::Pattern, "Level (5|6)".to_string(), false),
        (ValueKind::Text, "Six levels".to_string(), false),
        (ValueKind::Text, "rgb(1, 2, 3)".to_string(), false),
        (ValueKind::Pattern, "Level 6".to_string(), false),
        (ValueKind::State, "true".to_string(), false),
        (ValueKind::Text, "Save".to_string(), false),
    ];
    assert_eq!(values, want);
    // The two patterns write only "level", which the case holds. Step 1 ("Open the DEF page") says no
    // state, so `true` is refused too; the attribute, the style and the visible name are not the case's.
    // Each refusal names its kind. The address is not among them.
    let r = check(&case, &s, None);
    assert_eq!(r.refusals.len(), 4, "{:?}", r.refusals);
    assert!(r.refusals.iter().all(|l| l.contains("is not in the case - take it from the case's steps")), "{:?}", r.refusals);
    for kind in ["the attribute value \"Six levels\"", "the style value \"rgb(1, 2, 3)\"", "the state \"true\"", "the text \"Save\""] {
        assert!(r.refusals.iter().any(|l| l.starts_with(&format!("step 1: {kind} is not in the case"))), "{kind}: {:?}", r.refusals);
    }
    assert!(!r.refusals.iter().any(|l| l.contains("levels6")), "{:?}", r.refusals);
    let from_case: Vec<&str> =
        r.sourced.iter().filter(|v| v.provenance == Some(Provenance::FromCase)).map(|v| v.value.as_str()).collect();
    assert_eq!(from_case, vec!["Level (5|6)", "Level 6"]);
}

/// The last link of an `expect_visible` locator gives its `text` and its `name`; a chain's earlier links
/// and a legacy string give none.
#[test]
fn expect_visible_gives_the_name_and_text_of_the_last_link_only() {
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_visible", "selector": [{ "role": "dialog", "name": "Rate" }, { "role": "button", "name": "Save", "exact": true }] },
        { "kind": "expect_visible", "selector": { "text": "Saved with a receipt" } },
        { "kind": "expect_visible", "selector": "#toast" },
    ] }]));
    let v: Vec<(String, bool)> = values_of_step(&s.steps[0]).into_iter().map(|v| (v.value, v.exact)).collect();
    assert_eq!(v, vec![("Save".to_string(), true), ("Saved with a receipt".to_string(), false)]);
}

/// A pattern taken from the page is refused like a text is; the same with a because saves, listed;
/// an old step that already had it is not held again.
#[test]
fn a_matches_pattern_taken_from_the_page_with_no_because_is_refused() {
    let case = case_of(9, "Rate", &[("Open the page", ""), ("Look at the heading", "A heading appears")], "");
    let action = json!({ "kind": "expect_text", "selector": "h1", "matches": "^Customer Focus$" });
    let bare = script(json!([{ "step_number": 1, "actions": [action.clone()] }]));
    let r = check_case_values(&bare, &text_of(&case), None, "AUTOTEST");
    assert_eq!(
        r.refusals,
        vec!["step 1: the pattern \"^Customer Focus$\" is not in the case - take it from the case's steps, expected results or preconditions as written, or give it a \"because\" on the step ({ \"value\": \"^Customer Focus$\", \"why\": \"where the case defines it\" })".to_string()]
    );
    let covered = script(json!([{ "step_number": 1, "actions": [action],
        "because": [{ "value": "^Customer Focus$", "why": "the heading step 2 expects" }] }]));
    let r = check_case_values(&covered, &text_of(&case), None, "AUTOTEST");
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced, vec![sv(1, "^Customer Focus$", Some(Provenance::Assumed), "the heading step 2 expects")]);
    assert_eq!(r.listed, vec!["step 1 \"^Customer Focus$\" - the heading step 2 expects".to_string()]);
    // An old step that already had it is not held again, in the step the repair declares.
    let r = check_case_values_against(&bare, &text_of(&case), Some(&[1]), "AUTOTEST", Some(&bare));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    // Nor is a step the save does not name.
    let r = check_case_values_against(&bare, &text_of(&case), Some(&[2]), "AUTOTEST", None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
}

#[test]
fn a_pattern_whose_words_are_the_cases_is_from_the_case() {
    let case = case_of(
        9,
        "Evaluations",
        &[("Open the cycle", ""), ("Read the subtitle", "It shows the evaluation and the participants")],
        "",
    );
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_text", "selector": "p", "matches": "^\\d+° Evaluation · \\d+ participants$" },
    ] }]));
    let r = check_case_values(&s, &text_of(&case), None, "AUTOTEST");
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced.len(), 1);
    assert_eq!(r.sourced[0].provenance, Some(Provenance::FromCase));
    // One word the case does not hold and it is refused.
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_text", "selector": "p", "matches": "^\\d+° Evaluation · \\d+ managers$" },
    ] }]));
    assert_eq!(check_case_values(&s, &text_of(&case), None, "AUTOTEST").refusals.len(), 1);
}

#[test]
fn a_pattern_with_no_words_is_refused_unless_a_because_names_it() {
    let case = case_of(9, "Weights", &[("Open", ""), ("Look", "A weight is shown")], "");
    let action = json!({ "kind": "expect_text", "selector": "td", "matches": "^\\d+$" });
    let bare = script(json!([{ "step_number": 1, "actions": [action.clone()] }]));
    assert_eq!(check_case_values(&bare, &text_of(&case), None, "AUTOTEST").refusals.len(), 1);
    let covered = script(json!([{ "step_number": 1, "actions": [action],
        "because": [{ "value": "^\\d+$", "why": "a weight is a whole number", "provenance": "invented" }] }]));
    let r = check_case_values(&covered, &text_of(&case), None, "AUTOTEST");
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced, vec![sv(1, "^\\d+$", Some(Provenance::Invented), "a weight is a whole number")]);
}

#[test]
fn literal_words_are_the_patterns_own_words() {
    use v2_lib::browser::pattern::literal_words;
    assert_eq!(literal_words("^\\d+° Evaluation · \\d+ participants$"), vec!["evaluation", "participants"]);
    assert_eq!(literal_words("Level (5|6)"), vec!["level"]);
    assert!(literal_words("^\\d+$").is_empty());
    assert!(literal_words("[A-Z]{3}-\\d+").is_empty());
    assert_eq!(literal_words("^Open .+$"), vec!["open"]);
    assert!(literal_words("a\\.b").is_empty());
    // A letter a quantifier follows is not required, and a word is listed once.
    assert_eq!(literal_words("^Level fa?r Level$"), vec!["level"]);
    assert_eq!(literal_words("Save|Cancel|save"), vec!["save", "cancel"]);
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

#[tokio::test]
async fn a_save_of_a_case_waiting_for_a_ruling_is_not_checked() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let case = case_of(7, "Save a rating", &[("Open", ""), ("Save", "A confirmation appears")], "");
    let (_server, client) = client_with(&case).await;
    v2_lib::autorun::divergences::update(dir.path(), "acme", "Web", |list| {
        v2_lib::autorun::divergences::report_in(
            list,
            7,
            2,
            "A confirmation appears",
            "nothing appeared",
            v2_lib::autorun::divergences::DivergenceKind::Data,
            1_786_000_000_000,
        )
    })
    .unwrap();
    let action = json!({ "kind": "expect_contains_text", "selector": "#toast", "value": "confirmation" });
    let body = bundle(7, "Save a rating", action, json!([]));
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(
        out.lines().any(|l| l == "case 7: not marked checked: a divergence of the case waits for a person's ruling, and the script counts as checked once a run passes after the ruling"),
        "{out}"
    );
    assert!(!v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap().checked);
}

#[tokio::test]
async fn a_new_script_with_no_divergence_is_checked() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let case = case_of(7, "Save a rating", &[("Open", ""), ("Save", "A confirmation appears")], "");
    let (_server, client) = client_with(&case).await;
    let action = json!({ "kind": "expect_contains_text", "selector": "#toast", "value": "confirmation" });
    let body = bundle(7, "Save a rating", action, json!([]));
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(!out.contains("not marked checked"), "{out}");
    assert!(v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap().checked);
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

/// REPLACES `expect_each_with_an_attribute_is_not_compared` (item 39 reverses it): an attribute's value
/// is read, as a state when it is true or false and as a text otherwise, and refused like one unless
/// the case holds it or a `because` covers it.
#[test]
fn expect_each_with_an_attribute_is_refused_unless_held_or_covered() {
    let case = case_of(9, "Levels", &[("Go to the page", ""), ("Look at the list", "Rows are shown")], "");
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_each", "selector": "#rows", "attribute": "aria-checked", "equals": "true" },
        { "kind": "expect_each", "selector": "#rows", "attribute": "class", "contains": "selected row" },
    ] }]));
    let kinds: Vec<(ValueKind, String)> = values_of_step(&s.steps[0]).into_iter().map(|v| (v.kind, v.value)).collect();
    assert_eq!(kinds, vec![(ValueKind::State, "true".to_string()), (ValueKind::Text, "selected row".to_string())]);
    assert_eq!(check(&case, &s, None).refusals.len(), 2);
    let covered = script(json!([{ "step_number": 1, "actions": s.steps[0].actions,
        "because": [
            { "value": "true", "why": "every level row starts ticked" },
            { "value": "selected row", "why": "the class the level list gives its picked row" },
        ] }]));
    let r = check(&case, &covered, None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.listed.len(), 2);
}

/// A state is from the case when the step's own words say it; a value the words do not say is refused.
#[test]
fn a_state_is_from_the_case_when_the_step_says_it() {
    let case = case_of(
        9,
        "Rows",
        &[("Open the list", ""), ("Pick a row", "The row stays selected"), ("Clear it", "The row is not selected"), ("Look", "A row shows")],
        "",
    );
    let s = |step: i32, checked: bool| {
        script(json!([{ "step_number": step, "actions": [{ "kind": "expect_checked", "selector": "#row", "checked": checked }] }]))
    };
    // Step 2: "stays selected" says true.
    let r = check(&case, &s(2, true), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced[0].provenance, Some(Provenance::FromCase));
    // Step 3: "is not selected" says false.
    let r = check(&case, &s(3, false), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced[0].provenance, Some(Provenance::FromCase));
    // Step 4 says neither; step 2 does not say false.
    assert_eq!(check(&case, &s(4, true), None).refusals.len(), 1);
    assert_eq!(check(&case, &s(2, false), None).refusals.len(), 1);
}

#[test]
fn a_count_of_0_or_1_is_from_the_case_when_the_step_says_none_or_one() {
    let case = case_of(
        9,
        "Rows",
        &[("Open the list", ""), ("Search for a name nobody has", "No rows are shown"), ("Search again", "Only one row is shown"), ("Look", "A row shows")],
        "",
    );
    let count = |step: i32, n: u32| {
        script(json!([{ "step_number": step, "actions": [{ "kind": "expect_count", "selector": "#rows", "equals": n }] }]))
    };
    let r = check(&case, &count(2, 0), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced[0].provenance, Some(Provenance::FromCase));
    let r = check(&case, &count(3, 1), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced[0].provenance, Some(Provenance::FromCase));
    // A step that says neither is refused with the count sentence, and a because lifts it.
    let r = check(&case, &count(4, 1), None);
    assert!(r.refusals[0].starts_with("step 4: the count \"1\" is not in the case"), "{:?}", r.refusals);
    assert_eq!(check(&case, &count(4, 0), None).refusals.len(), 1);
    let upper = script(json!([{ "step_number": 4, "actions": [{ "kind": "expect_count", "selector": "#rows", "at_most": 1 }],
        "because": [{ "value": "1", "why": "the row the search leaves" }] }]));
    assert!(check(&case, &upper, None).refusals.is_empty());
}

/// A one-step case reading `action` / `expected`, and whether a script checking `check_action` in it is
/// saved with no because.
fn saved_against(action: &str, expected: &str, check_action: Value) -> bool {
    let case = case_of(9, "Case", &[(action, expected)], "");
    let s = script(json!([{ "step_number": 1, "actions": [check_action] }]));
    check(&case, &s, None).refusals.is_empty()
}

fn checked(state: bool) -> Value {
    json!({ "kind": "expect_checked", "selector": "#box", "checked": state })
}

fn pressed(state: &str) -> Value {
    json!({ "kind": "expect_attribute", "selector": "#card", "name": "aria-pressed", "equals": state })
}

fn count_is(n: u32) -> Value {
    json!({ "kind": "expect_count", "selector": "#rows", "equals": n })
}

/// "Open", "on", "current", "no", "not" and "only" are in almost every step: none of them holds a state
/// or a 0 or a 1 on its own (final review I1 and M4).
#[test]
fn everyday_words_hold_no_state_and_no_count() {
    // "Open the page" holds no true; neither does "Click on".
    assert!(!saved_against("Open the page", "", checked(true)));
    assert!(!saved_against("Click on the Customer Focus card", "", pressed("true")));
    assert!(!saved_against("Open the current page", "The page loads", checked(true)));
    assert!(!saved_against("Open the page", "", checked(false)));
    // The review's scenario: nothing in "Click on ... not selected ... no error" lets the card be pressed.
    let action = "Click on the Customer Focus card";
    let expected = "The card is not selected and no error shows";
    assert!(!saved_against(action, expected, pressed("true")));
    assert!(!saved_against(action, expected, checked(true)));
    // "only" holds neither 0 nor 1; "not", "never", "without" and "hidden" hold no 0 either.
    assert!(!saved_against("Open the list", "Only admins see it", count_is(0)));
    assert!(!saved_against("Open the list", "Only admins see it", count_is(1)));
    assert!(!saved_against("Open the list", "The list is not empty", count_is(0)));
    assert!(!saved_against("Open the list", "It is never cut off and shows without a scrollbar", count_is(0)));
    assert!(!saved_against("Open the list", "Rows are hidden when filtered", count_is(0)));
}

/// "is not selected" holds false and not true.
#[test]
fn a_negation_flips_the_state_word_after_it() {
    let (action, expected) = ("Pick a row", "The row is not selected");
    assert!(saved_against(action, expected, checked(false)));
    assert!(!saved_against(action, expected, checked(true)));
    // The same through the other negations, and with a word between.
    for said in [
        "The row isn't selected",
        "The row isn\u{2019}t selected",
        "The row is not yet selected",
        "The row is no longer selected",
        "The row is not checked",
    ] {
        assert!(saved_against("Pick a row", said, checked(false)), "{said}");
        assert!(!saved_against("Pick a row", said, checked(true)), "{said}");
    }
    // A negated false word is true again.
    assert!(saved_against("Pick a row", "The row is not disabled", checked(true)));
    assert!(!saved_against("Pick a row", "The row is not disabled", checked(false)));
    // A negation more than 2 words away flips nothing.
    assert!(saved_against("Do not click the row", "The row is selected", checked(true)));
    assert!(!saved_against("Do not click the row", "The row is selected", checked(false)));
}

#[test]
fn the_state_words_hold_true_and_false_as_ruled() {
    for word in ["checked", "ticked", "selected", "enabled", "pressed", "expanded", "shown", "visible", "active"] {
        let said = format!("The box is {word}");
        assert!(saved_against("Look", &said, checked(true)), "{word}");
        assert!(!saved_against("Look", &said, checked(false)), "{word}");
    }
    for word in ["unchecked", "unticked", "disabled", "collapsed", "hidden", "inactive"] {
        let said = format!("The box is {word}");
        assert!(saved_against("Look", &said, checked(false)), "{word}");
        assert!(!saved_against("Look", &said, checked(true)), "{word}");
    }
    // The old loose words no longer hold anything.
    for word in ["open", "opened", "current", "chosen", "highlighted", "yes", "true", "closed", "cleared", "false", "unselected"] {
        let said = format!("The box is {word}");
        assert!(!saved_against("Look", &said, checked(true)) && !saved_against("Look", &said, checked(false)), "{word}");
    }
    // `on` and `off` hold a state only after is, turned or switched.
    for said in ["The switch is on", "The switch is turned on", "Switched on, the switch lights", "It is not off"] {
        assert!(saved_against("Look", said, checked(true)), "{said}");
    }
    for said in ["The switch is off", "The switch is turned off", "Switched off, the switch dims", "It is not on"] {
        assert!(saved_against("Look", said, checked(false)), "{said}");
    }
    for said in ["Click on the switch", "The switch is on the page", "Turn on the lights", "Log off the site"] {
        assert!(!saved_against("Look", said, checked(true)) && !saved_against("Look", said, checked(false)), "{said}");
    }
}

#[test]
fn zero_and_one_are_held_by_unambiguous_wording() {
    for said in ["0 rows are listed", "Zero rows are listed", "None are listed", "No rows are listed", "Nothing is listed", "No error shows"] {
        assert!(saved_against("Look", said, count_is(0)), "{said}");
        assert!(!saved_against("Look", said, count_is(1)), "{said}");
    }
    // "no longer" is not "no <noun>".
    assert!(!saved_against("Look", "The row is no longer listed", count_is(0)));
    for said in ["1 row is listed", "One row is listed", "A single row is listed", "Exactly one row is listed"] {
        assert!(saved_against("Look", said, count_is(1)), "{said}");
        assert!(!saved_against("Look", said, count_is(0)), "{said}");
    }
    assert!(!saved_against("Look", "A row is listed", count_is(1)));
}

/// A healer that flips true to false with no source is refused: the case gives the old state, and the new
/// one is held like a new value (final review I2).
#[test]
fn a_repair_flipping_a_state_is_refused_unless_the_case_holds_it_or_a_because_does() {
    let case = case_of(9, "Notify", &[("Open the settings", ""), ("Look at the Notify box", "The Notify box is checked")], "");
    let step = |c: Value, because: Value| {
        script(json!([
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/s" }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": "#a" }, c], "because": because },
        ]))
    };
    let old = step(checked(true), json!([]));
    let flipped = step(checked(false), json!([]));
    let r = check_repair_values(&old, &flipped, &text_of(&case), &[2], &[], "AUTOTEST");
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].contains("the repair drops \"true\", which the case gives"), "{r:?}");
    let held = check_case_values_against(&flipped, &text_of(&case), Some(&[2]), "AUTOTEST", Some(&old));
    assert_eq!(held.refusals.len(), 1, "{:?}", held.refusals);
    assert!(held.refusals[0].starts_with("step 2: the state \"false\" is not in the case"), "{:?}", held.refusals);
    // The same state, moved to another locator, is kept and not held again.
    let same = step(json!({ "kind": "expect_checked", "selector": "#notify", "checked": true }), json!([]));
    assert!(check_repair_values(&old, &same, &text_of(&case), &[2], &[], "AUTOTEST").is_empty());
    assert!(check_case_values_against(&same, &text_of(&case), Some(&[2]), "AUTOTEST", Some(&old)).refusals.is_empty());
    // A state the case did not give, listed under a because, a repair may change; the new one needs its own.
    let stale = step(checked(false), json!([{ "value": "false", "why": "the box starts unticked" }]));
    let to_true = step(checked(true), json!([]));
    assert!(check_repair_values(&stale, &to_true, &text_of(&case), &[2], &[], "AUTOTEST").is_empty());
    let case_silent = case_of(9, "Notify", &[("Open the settings", ""), ("Look at the Notify box", "The Notify box shows")], "");
    let held = check_case_values_against(&to_true, &text_of(&case_silent), Some(&[2]), "AUTOTEST", Some(&stale));
    assert_eq!(held.refusals.len(), 1, "{:?}", held.refusals);
    let with_because = step(checked(true), json!([{ "value": "true", "why": "the box starts ticked" }]));
    let held = check_case_values_against(&with_because, &text_of(&case_silent), Some(&[2]), "AUTOTEST", Some(&stale));
    assert!(held.refusals.is_empty(), "{:?}", held.refusals);
}

/// Every kind of value is compared old against new in a repair, not only the long ones.
#[test]
fn a_repair_keeps_each_kind_of_value_the_case_gives() {
    let case = case_of(
        9,
        "Receipt",
        &[("Open the receipt", ""), ("Look", "The Save button is shown, the total reads Net pay, one row is listed and the card is pressed")],
        "",
    );
    let make = |c: Value| {
        script(json!([
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/r" }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": "#a" }, c] },
        ]))
    };
    let kept = |old: Value, new: Value| check_repair_values(&make(old), &make(new), &text_of(&case), &[2], &[], "AUTOTEST");
    // A visible name.
    let name = |n: &str| json!({ "kind": "expect_visible", "selector": { "role": "button", "name": n } });
    assert!(kept(name("Save"), name("Save")).is_empty());
    assert_eq!(kept(name("Save"), name("Submit")).len(), 1);
    // A count of 1.
    let one = |n: u32| json!({ "kind": "expect_count", "selector": "#rows", "equals": n });
    assert!(kept(one(1), one(1)).is_empty());
    assert_eq!(kept(one(1), one(3)).len(), 1);
    // An attribute's value, a state and a pattern.
    let attr = |v: &str| json!({ "kind": "expect_attribute", "selector": "#card", "name": "title", "equals": v });
    assert_eq!(kept(attr("Net pay"), attr("Gross pay")).len(), 1);
    assert!(kept(attr("Net pay"), attr("Net pay")).is_empty());
    assert_eq!(kept(pressed("true"), pressed("false")).len(), 1);
    let pat = |p: &str| json!({ "kind": "expect_text", "selector": "#t", "matches": p });
    assert_eq!(kept(pat("^Net pay$"), pat("^Gross$")).len(), 1);
    assert!(kept(pat("^Net pay$"), pat("^Net pay \\d+$")).is_empty());
}

/// An optional letter or group in a pattern is both forms; either one in the case holds it (final review I3).
#[test]
fn a_pattern_with_an_optional_letter_is_from_the_case_in_either_form() {
    use v2_lib::browser::pattern::literal_word_forms;
    let forms = |p: &str| literal_word_forms(p);
    assert_eq!(forms("^\\d+ participants?$"), vec![vec!["participant".to_string(), "participants".to_string()]]);
    assert_eq!(
        forms("^Records? saved$"),
        vec![vec!["record".to_string(), "records".to_string()], vec!["saved".to_string()]]
    );
    assert_eq!(forms("Record(s)? saved")[0], vec!["record".to_string(), "records".to_string()]);
    assert_eq!(forms("Record(s) saved")[0], vec!["records".to_string()]);
    assert_eq!(forms("colou?r")[0], vec!["color".to_string(), "colour".to_string()]);
    assert_eq!(forms("(Total )?records")[0], vec!["records".to_string()]);
    assert_eq!(forms("participants*")[0], vec!["participant".to_string(), "participants".to_string()]);
    assert_eq!(forms("Level+")[0], vec!["level".to_string()]);
    assert_eq!(forms("^Total.+?Net pay"), vec![vec!["total".to_string()], vec!["net".to_string()], vec!["pay".to_string()]]);
    assert_eq!(forms("a{2}bcd")[0], vec!["bcd".to_string()]);
    // An optional letter that leaves under 3 required letters is no word.
    assert!(forms("fa?r").is_empty());
    let pattern = |p: &str| json!({ "kind": "expect_text", "selector": "p", "matches": p });
    for said in ["2 participants are listed", "1 participant is listed"] {
        assert!(saved_against("Look", said, pattern("^\\d+ participants?$")), "{said}");
        assert!(!saved_against("Look", said, pattern("^\\d+ managers?$")), "{said}");
    }
    for said in ["Record saved", "Records saved"] {
        assert!(saved_against("Look", said, pattern("^Records? saved$")), "{said}");
        assert!(saved_against("Look", said, pattern("^Record(s)? saved$")), "{said}");
    }
    assert!(!saved_against("Look", "Records saved", pattern("^Records? deleted$")));
}

/// A pattern gets the exemptions text has: words the script types, the test name prefix (final review M6).
#[test]
fn a_pattern_of_typed_or_prefixed_words_needs_no_because() {
    let case = case_of(9, "Rate", &[("Open", ""), ("Look", "A heading appears")], "");
    let typed = script(json!([{ "step_number": 1, "actions": [
        { "kind": "fill", "selector": "#name", "value": "Reviewer Nora" },
        { "kind": "expect_text", "selector": "h1", "matches": "^Reviewer Nora$" },
    ] }]));
    let r = check(&case, &typed, None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced[0].provenance, Some(Provenance::Invented));
    assert_eq!(r.sourced[0].why, "typed by the script");
    // Not typed: refused.
    let other = script(json!([{ "step_number": 1, "actions": [
        { "kind": "fill", "selector": "#name", "value": "Reviewer Nora" },
        { "kind": "expect_text", "selector": "h1", "matches": "^Manager Omar$" },
    ] }]));
    assert_eq!(check(&case, &other, None).refusals.len(), 1);
    let prefixed = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_text", "selector": "h1", "matches": "^AUTOTEST \\d+ saved$" },
    ] }]));
    let r = check(&case, &prefixed, None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced[0].provenance, Some(Provenance::Invented));
    assert_eq!(r.sourced[0].why, "a record named with the test name prefix");
}

/// The refusal names the kind of value, not always "the text" (final review M7).
#[test]
fn the_refusal_names_the_kind_of_value() {
    let case = case_of(9, "Case", &[("Look", "A card appears")], "");
    let sentence = |c: Value| {
        let s = script(json!([{ "step_number": 1, "actions": [c] }]));
        check(&case, &s, None).refusals.into_iter().next().unwrap_or_default()
    };
    let starts = |c: Value, want: &str| {
        let r = sentence(c);
        assert!(r.starts_with(want), "{r}");
        r
    };
    starts(json!({ "kind": "expect_text", "selector": "p", "equals": "Shipped order" }), "step 1: the text \"Shipped order\" is not in the case");
    starts(json!({ "kind": "expect_text", "selector": "p", "matches": "^Shipped$" }), "step 1: the pattern \"^Shipped$\" is not in the case");
    starts(json!({ "kind": "expect_count", "selector": "p", "equals": 7 }), "step 1: the count \"7\" is not in the case");
    starts(
        json!({ "kind": "expect_attribute", "selector": "p", "name": "title", "equals": "Shipped" }),
        "step 1: the attribute value \"Shipped\" is not in the case",
    );
    starts(
        json!({ "kind": "expect_each", "selector": "p", "attribute": "title", "contains": "Shipped" }),
        "step 1: the attribute value \"Shipped\" is not in the case",
    );
    starts(
        json!({ "kind": "expect_style", "selector": "p", "property": "color", "equals": "rgb(1, 2, 3)" }),
        "step 1: the style value \"rgb(1, 2, 3)\" is not in the case",
    );
    let state = starts(checked(true), "step 1: the state \"true\" is not in the case");
    assert!(state.contains("a step holds true with: checked, ticked, selected"), "{state}");
    let state = starts(checked(false), "step 1: the state \"false\" is not in the case");
    assert!(state.contains("a step holds false with: unchecked, unticked, disabled"), "{state}");
}

/// A download's contents are compared; the file's name, its sheet, a tab's address, a sorted column's name
/// and a response are not values the case gives (final review M5).
#[test]
fn a_downloads_contents_are_values_and_where_to_look_is_not() {
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_download", "name": "payslips*.xlsx", "sheet": "Summary",
          "contains_text": ["Quarterly total"],
          "headers": { "exact": ["Name", "Amount"] },
          "cells": [{ "ref": "B2", "text": "Nora" }, { "ref": "C2", "text": "Omar", "match": "contains" }],
          "pdf": { "contains": ["Payslip", "Net pay"], "pages": { "at_least": 2 }, "on_page": [{ "page": 1, "contains": "Header" }] } },
        { "kind": "expect_tab", "name": "help", "url_contains": "/help/payslips" },
        { "kind": "expect_sorted", "table": "#t", "column": "Pay period", "order": "ascending" },
    ] }]));
    let v = values_of_step(&s.steps[0]);
    let texts: Vec<&str> = v.iter().filter(|c| c.kind == ValueKind::Text).map(|c| c.value.as_str()).collect();
    assert_eq!(texts, vec!["Quarterly total", "Name", "Amount", "Nora", "Omar", "Payslip", "Net pay", "Header"]);
    let counts: Vec<&str> = v.iter().filter(|c| c.kind == ValueKind::Count).map(|c| c.value.as_str()).collect();
    assert_eq!(counts, vec!["2"]);
    let case = case_of(9, "Payslips", &[("Download the payslips", "The file lists Name and Amount")], "");
    let r = check(&case, &s, None);
    assert!(r.refusals.iter().any(|l| l.contains("\"Quarterly total\"")), "{:?}", r.refusals);
    assert!(r.refusals.iter().any(|l| l.contains("\"Net pay\"")), "{:?}", r.refusals);
    assert!(!r.refusals.iter().any(|l| l.contains("\"Name\"") || l.contains("\"Amount\"")), "{:?}", r.refusals);
    assert!(!r.refusals.iter().any(|l| l.contains("help/payslips") || l.contains("Pay period") || l.contains("Summary")), "{:?}", r.refusals);
}

#[test]
fn at_least_1_and_min_1_stay_exempt_as_presence() {
    let case = case_of(9, "Rows", &[("Open", ""), ("Look", "A row shows")], "");
    let s = script(json!([{ "step_number": 2, "actions": [
        { "kind": "expect_count", "selector": "#rows", "at_least": 1 },
        { "kind": "expect_count", "selector": "#rows", "at_least": 0 },
        { "kind": "expect_each", "selector": "#rows", "min": 1, "not_empty": true },
    ] }]));
    let r = check(&case, &s, None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert!(r.sourced.iter().all(|v| v.provenance.is_none()), "{:?}", r.sourced);
}

#[test]
fn a_number_written_as_text_is_sourced_however_short() {
    let case = case_of(9, "Cards", &[("Open", ""), ("Look at the card", "It lists one participant"), ("Look again", "A card shows")], "");
    let s = |step: i32| script(json!([{ "step_number": step, "actions": [{ "kind": "expect_text", "selector": "#n", "equals": "1" }] }]));
    let r = check(&case, &s(2), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced[0].provenance, Some(Provenance::FromCase));
    let r = check(&case, &s(3), None);
    assert!(r.refusals[0].starts_with("step 3: the text \"1\" is not in the case"), "{:?}", r.refusals);
    // A short word is still exempt.
    let word = script(json!([{ "step_number": 3, "actions": [{ "kind": "expect_text", "selector": "#n", "equals": "OK" }] }]));
    assert!(check(&case, &word, None).refusals.is_empty());
    // "12" is held by the case's own number; "7%" is a number too and is not in the case.
    let twelve = case_of(9, "Cards", &[("Open", ""), ("Look", "12 cards are listed")], "");
    let t = script(json!([{ "step_number": 2, "actions": [{ "kind": "expect_text", "selector": "#n", "equals": "12" }] }]));
    assert!(check(&twelve, &t, None).refusals.is_empty());
    let pct = script(json!([{ "step_number": 2, "actions": [{ "kind": "expect_text", "selector": "#n", "equals": "7%" }] }]));
    assert_eq!(check(&twelve, &pct, None).refusals.len(), 1);
}

/// A count of 0 is held by the step's words, not against a count the case states for something else:
/// refused with the plain sentence, never as a contradiction, and a because lifts it.
#[test]
fn a_count_of_0_is_never_held_against_a_stated_count() {
    let case = case_of(9, "Buttons", &[("Open the toolbar", ""), ("Look", "The toolbar shows 2 buttons")], "");
    let action = json!({ "kind": "expect_count", "selector": { "role": "button", "name": "Save" }, "equals": 0 });
    let r = check(&case, &two_steps(action.clone(), json!([])), None);
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
    assert!(r.refusals[0].starts_with("step 2: the count \"0\" is not in the case"), "{:?}", r.refusals);
    assert!(!r.refusals[0].contains("but the case says"), "{:?}", r.refusals);
    let r = check(&case, &two_steps(action, json!([{ "value": "0", "why": "Save is hidden once the form is locked" }])), None);
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
}

/// A repair keeps every value the case gives, of every kind. A value the case does not give (one a
/// `because` covered) it may change, and the save then holds what it wrote like any new value.
#[test]
fn a_listed_value_is_never_held_by_a_repair() {
    // The case does not say "level": the old pattern was listed under a because.
    let case = case_of(9, "Ratings", &[("Open", ""), ("Look", "The heading shows")], "");
    let old = two_step_script(
        json!([{ "kind": "click", "selector": "#a" }]),
        json!([{ "kind": "expect_text", "selector": "h1", "matches": "^Level$" }]),
    );
    let new = two_step_script(
        json!([{ "kind": "click", "selector": "#a" }]),
        json!([{ "kind": "expect_text", "selector": "h1", "matches": "^Rating$" }]),
    );
    let r = check_repair_values(&old, &new, &text_of(&case), &[2], &[], "AUTOTEST");
    assert!(r.is_empty(), "{r:?}");
    // The save still holds what the repair wrote.
    let held = check_case_values_against(&new, &text_of(&case), Some(&[2]), "AUTOTEST", Some(&old));
    assert_eq!(held.refusals.len(), 1, "{:?}", held.refusals);
    let held = check_case_values_against(&old, &text_of(&case), Some(&[2]), "AUTOTEST", Some(&old));
    assert!(held.refusals.is_empty(), "{:?}", held.refusals);
}

/// The case gives the old pattern's word: a repair that drops it is refused, one that widens it is not.
#[test]
fn a_repair_keeps_a_pattern_the_case_gives() {
    let case = case_of(9, "Levels", &[("Open", ""), ("Look", "The level heading shows")], "");
    let old = two_step_script(
        json!([{ "kind": "click", "selector": "#a" }]),
        json!([{ "kind": "expect_text", "selector": "h1", "matches": "^Level$" }]),
    );
    let other = two_step_script(
        json!([{ "kind": "click", "selector": "#a" }]),
        json!([{ "kind": "expect_text", "selector": "h1", "matches": "^Rating$" }]),
    );
    let r = check_repair_values(&old, &other, &text_of(&case), &[2], &[], "AUTOTEST");
    assert!(r.iter().any(|l| l.contains("the repair drops \"^Level$\"")), "{r:?}");
    let wider = two_step_script(
        json!([{ "kind": "click", "selector": "#a" }]),
        json!([{ "kind": "expect_text", "selector": "h1", "matches": "^Level \\d+$" }]),
    );
    let r = check_repair_values(&old, &wider, &text_of(&case), &[2], &[], "AUTOTEST");
    assert!(r.is_empty(), "{r:?}");
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
    case_of(7, "Save a rating", &[("Open", ""), ("Save", "A confirmation appears and one row is listed")], "")
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
        { "step_number": 5, "actions": [{ "kind": "expect_count", "selector": "#rows", "at_least": 1 }] },
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
        { "kind": "expect_count", "selector": "#rows", "at_least": 1 },
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
    let step_4 = json!({ "step_number": 4, "actions": [{ "kind": "expect_count", "selector": "#rows", "at_least": 1 }] });
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
    let step_3 = json!({ "step_number": 3, "actions": [{ "kind": "expect_count", "selector": "#rows", "at_least": 1 }] });
    let step_4 = json!({ "step_number": 4, "actions": [{ "kind": "expect_count", "selector": "#rows", "at_least": 1 }] });
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

#[test]
fn a_repair_holds_only_the_values_new_or_changed_in_its_steps() {
    let case = case_147556();
    // A script saved before the rule: step 2 checks a text the case never held.
    let stale = json!({ "kind": "expect_text", "selector": level_selector(), "equals": "Legacy label" });
    let old = two_steps(stale.clone(), json!([]));
    // The healer fixes only the locator of the same check.
    let locator_only = two_steps(
        json!({ "kind": "expect_text", "selector": [{ "role": "combobox", "name": "Levels" }, { "role": "option" }], "equals": "Legacy label" }),
        json!([]),
    );
    let text = text_of(&case);
    // Held whole, the old value is refused ...
    let whole = check_case_values(&locator_only, &text, Some(&[2]), "AUTOTEST");
    assert_eq!(whole.refusals.len(), 1, "{:?}", whole.refusals);
    // ... a repair of the step does not hold what the old step already had ...
    let repaired = check_case_values_against(&locator_only, &text, Some(&[2]), "AUTOTEST", Some(&old));
    assert!(repaired.refusals.is_empty(), "{:?}", repaired.refusals);
    // ... but a value that is new or changed is held, with or without the old step.
    let changed = two_steps(
        json!({ "kind": "expect_text", "selector": level_selector(), "equals": "Brand new label" }),
        json!([]),
    );
    let held = check_case_values_against(&changed, &text, Some(&[2]), "AUTOTEST", Some(&old));
    assert_eq!(held.refusals.len(), 1, "{:?}", held.refusals);
    assert!(held.refusals[0].contains("Brand new label"), "{:?}", held.refusals);
    // The same value on a different step number is new there.
    let moved = script(json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/def" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#a" }] },
        { "step_number": 3, "actions": [stale] },
    ]));
    let held = check_case_values_against(&moved, &text, Some(&[3]), "AUTOTEST", Some(&old));
    assert_eq!(held.refusals.len(), 1, "{:?}", held.refusals);
}

// ------------------------------------------- timings and same_document (item 36)

/// Step 3 of the search case states 800 ms and says the page does not reload.
fn search_case() -> v2_lib::ado::TestCaseFull {
    case_of(
        147600,
        "Search",
        &[
            ("Open the app", ""),
            ("Open the Search page", ""),
            ("Type a name in Search", "The list refreshes about 800 ms after the last keystroke, without reloading the page"),
        ],
        "",
    )
}

/// The search script: step 3 types, then checks the request with `response` (merged into an
/// `expect_response` of `/api/search`).
fn search_script(response: Value, because: Value) -> CaseScript {
    let mut check = json!({ "kind": "expect_response", "url_contains": "/api/search" });
    for (k, v) in response.as_object().unwrap() {
        check[k] = v.clone();
    }
    script(json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#search" }] },
        {
            "step_number": 3,
            "actions": [{ "kind": "fill", "selector": "#q", "value": "Ann" }, check],
            "because": because,
        },
    ]))
}

fn found(case: &v2_lib::ado::TestCaseFull, s: &CaseScript) -> v2_lib::autorun::case_values::ValueReport {
    check_case_values(s, &text_of(case), Some(&[3]), "AUTOTEST")
}

fn search_case_saying(expected: &str, preconditions: &str) -> v2_lib::ado::TestCaseFull {
    case_of(
        147601,
        "Search",
        &[("Open the app", ""), ("Open the Search page", ""), ("Type a name in Search", expected)],
        preconditions,
    )
}

#[test]
fn a_timing_the_case_states_is_held() {
    let r = found(&search_case(), &search_script(json!({ "not_before_ms": 800 }), json!([])));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced, vec![sv(3, "800", Some(Provenance::FromCase), "")]);
    // started_within_ms is held the same way.
    let r = found(&search_case(), &search_script(json!({ "started_within_ms": 800 }), json!([])));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    // "0.8 s" states 800 too.
    let seconds = search_case_saying("The list refreshes 0.8 s after the last keystroke", "");
    let r = found(&seconds, &search_script(json!({ "not_before_ms": 800 }), json!([])));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    // A duration stated in the preconditions holds it as well.
    let pre = search_case_saying("", "The debounce is 800ms");
    let r = found(&pre, &search_script(json!({ "not_before_ms": 800 }), json!([])));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
}

#[test]
fn durations_the_case_states_are_read_as_whole_ms() {
    use std::collections::BTreeSet;
    use v2_lib::autorun::case_text::stated_durations;
    let set = |v: &[u64]| v.iter().copied().collect::<BTreeSet<u64>>();
    assert_eq!(stated_durations("about 800ms, then 1.5 seconds"), set(&[800, 1500]));
    assert_eq!(stated_durations("800 ms and 0.8 s and 800 milliseconds"), set(&[800]));
    assert_eq!(stated_durations("wait 2 sec, then 3,5 secs, or 40 msec"), set(&[2000, 3500, 40]));
    // A number with no unit, or beside a letter, or with another word after it, is not a duration.
    assert_eq!(stated_durations("level 4 of 12 sessions, q4 ms, 5 sessions"), set(&[]));
    let text = text_of(&search_case_saying("Wait 1.5 seconds", "The debounce is 800ms"));
    assert_eq!(text.stated_durations(), set(&[800, 1500]));
    // A thousands separator is not a decimal point.
    assert_eq!(stated_durations("after 1,000 ms"), set(&[1000]));
    assert_eq!(stated_durations("after 12,500 milliseconds or 1,000 s"), set(&[12_500, 1_000_000]));
    assert_eq!(stated_durations("3,5 secs and 2,25 s"), set(&[3500, 2250]));
    // A range holds both ends, in the unit its last number carries.
    assert_eq!(stated_durations("within 600 to 900 ms"), set(&[600, 900]));
    assert_eq!(stated_durations("within 600-900 ms"), set(&[600, 900]));
    assert_eq!(stated_durations("within 600 - 900 ms"), set(&[600, 900]));
    assert_eq!(stated_durations("within 0.5\u{2013}1.5 s"), set(&[500, 1500]));
    assert_eq!(stated_durations("between 1,000 to 2,000 ms"), set(&[1000, 2000]));
    // A range with no unit, or of something else, is not a duration.
    assert_eq!(stated_durations("10 to 12 sessions"), set(&[]));
    assert_eq!(stated_durations("levels 1-4"), set(&[]));
}

#[test]
fn a_margin_needs_a_because() {
    let case = search_case();
    let r = found(&case, &search_script(json!({ "not_before_ms": 600 }), json!([])));
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
    assert!(r.refusals[0].starts_with("step 3: the timing \"600\" is not in the case"), "{:?}", r.refusals);
    assert!(r.refusals[0].contains("a timing is held when the case states that duration"), "{:?}", r.refusals);
    let because = json!([{ "value": "600", "why": "the case says about 800 ms; 600 allows for the browser" }]);
    let r = found(&case, &search_script(json!({ "not_before_ms": 600 }), because));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(
        r.listed,
        vec!["step 3 \"600\" - the case says about 800 ms; 600 allows for the browser".to_string()]
    );
    assert_eq!(
        r.sourced,
        vec![sv(3, "600", Some(Provenance::Assumed), "the case says about 800 ms; 600 allows for the browser")]
    );
}

#[test]
fn a_timing_is_never_exempt_as_a_short_number() {
    // 2 ms is not in the case: not a bare value.
    let r = found(&search_case(), &search_script(json!({ "started_within_ms": 2 }), json!([])));
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
    assert!(r.refusals[0].contains("the timing \"2\" is not in the case"), "{:?}", r.refusals);
}

#[test]
fn same_document_is_held_by_the_steps_words() {
    let case = search_case();
    let r = found(&case, &search_script(json!({ "same_document": true }), json!([])));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.sourced, vec![sv(3, "same_document", Some(Provenance::FromCase), "")]);
    // A step that says nothing about reloading does not hold it.
    let silent = search_case_saying("The list refreshes", "");
    let r = found(&silent, &search_script(json!({ "same_document": true }), json!([])));
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
    assert!(r.refusals[0].starts_with("step 3: the no-reload check \"same_document\" is not in the case"), "{:?}", r.refusals);
    assert!(r.refusals[0].contains("does not reload"), "{:?}", r.refusals);
    // A because covers it.
    let because = json!([{ "value": "same_document", "why": "step 3 is a search as you type, the case means no page load" }]);
    let r = found(&silent, &search_script(json!({ "same_document": true }), because));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert_eq!(r.listed.len(), 1, "{:?}", r.listed);
    // false is not a check at all.
    let r = found(&silent, &search_script(json!({ "same_document": false }), json!([])));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    assert!(r.sourced.is_empty(), "{:?}", r.sourced);
}

#[test]
fn the_words_that_say_the_page_does_not_reload() {
    let held = |expected: &str| {
        let case = search_case_saying(expected, "");
        found(&case, &search_script(json!({ "same_document": true }), json!([]))).refusals.is_empty()
    };
    for yes in [
        "The page does not reload",
        "The page doesn't reload",
        "The list refreshes without reloading",
        "No full page reload happens",
        "It never reloads",
        "The user stays on the page",
        "The user remains on the same page",
        "The list updates without a full page reload",
        "The grid refreshes with no full reload",
        "The page does not refresh",
        "The list updates without refreshing",
        "The list updates with no page reload",
        "The page does not reload when the filter changes",
    ] {
        assert!(held(yes), "{yes}");
    }
    for no in ["The page reloads", "The list refreshes", "Reload the page", "The user is on the page"] {
        assert!(!held(no), "{no}");
    }
    // A reload of something that is not the page does not hold it: "page", or no object at all.
    for no in [
        "The list updates with no data reload",
        "The page does not reload the grid",
        "If there is no data reload the page",
        "The list updates without refreshing the totals",
        "The list updates with no grid refresh",
    ] {
        assert!(!held(no), "{no}");
    }
    // The negation must sit within 4 words before the reload, and a clause ends it.
    assert!(!held("There is no sign that anything at all happens before the page reloads"));
    assert!(!held("There is no error. The page reloads"));
}

#[test]
fn a_changed_timing_on_repair_needs_a_source() {
    let case = search_case();
    let text = text_of(&case);
    let because = json!([{ "value": "600", "why": "the case says about 800 ms; 600 allows for the browser" }]);
    let old = search_script(json!({ "not_before_ms": 600 }), because);
    // Lowered to 300 with no because: refused.
    let lowered = search_script(json!({ "not_before_ms": 300 }), json!([]));
    let r = check_case_values_against(&lowered, &text, Some(&[3]), "AUTOTEST", Some(&old));
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
    assert!(r.refusals[0].starts_with("step 3: the timing \"300\" is not in the case"), "{:?}", r.refusals);
    // Still at 600 with no because (as the old one had it): not refused.
    let kept = search_script(json!({ "not_before_ms": 600 }), json!([]));
    let r = check_case_values_against(&kept, &text, Some(&[3]), "AUTOTEST", Some(&old));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
    // Held whole, with no old step, it is refused.
    let r = check_case_values_against(&kept, &text, Some(&[3]), "AUTOTEST", None);
    assert_eq!(r.refusals.len(), 1, "{:?}", r.refusals);
}

#[test]
fn an_old_step_without_a_source_for_same_document_is_not_held_again() {
    let silent = search_case_saying("The list refreshes", "");
    let old = search_script(json!({ "same_document": true }), json!([]));
    let r = check_case_values_against(&old, &text_of(&silent), Some(&[3]), "AUTOTEST", Some(&old));
    assert!(r.refusals.is_empty(), "{:?}", r.refusals);
}

fn search_repair(old: &CaseScript, new: &CaseScript, gives_up: &[GiveUp]) -> Vec<String> {
    check_repair_values(old, new, &text_of(&search_case()), &[3], gives_up, "AUTOTEST")
}

#[test]
fn a_repair_that_removes_a_timing_must_name_it_in_gives_up() {
    let old = search_script(
        json!({ "not_before_ms": 800, "started_within_ms": 900 }),
        json!([{ "value": "900", "why": "the case says about 800 ms" }]),
    );
    let plain = search_script(json!({}), json!([]));
    let r = search_repair(&old, &plain, &[]);
    assert!(
        r.iter().any(|l| l.starts_with("step 3: the repair removes not_before_ms from an expect_response and gives_up does not name it")),
        "{r:?}"
    );
    assert!(
        r.iter().any(|l| l.starts_with("step 3: the repair removes started_within_ms from an expect_response and gives_up does not name it")),
        "{r:?}"
    );
    // Naming each one passes.
    let r = search_repair(
        &old,
        &plain,
        &[give(3, "not_before_ms", "step 5 times it"), give(3, " Started_Within_Ms ", "step 5 times it")],
    );
    assert!(r.is_empty(), "{r:?}");
    // Naming one leaves the other.
    let r = search_repair(&old, &plain, &[give(3, "not_before_ms", "step 5 times it")]);
    assert!(r.iter().any(|l| l.contains("removes started_within_ms")), "{r:?}");
    assert!(!r.iter().any(|l| l.contains("removes not_before_ms")), "{r:?}");
    // Keeping both asks for nothing.
    assert!(search_repair(&old, &old, &[]).is_empty());
}

#[test]
fn a_repair_that_removes_same_document_must_name_it_in_gives_up() {
    let old = search_script(json!({ "same_document": true }), json!([]));
    let plain = search_script(json!({}), json!([]));
    let r = search_repair(&old, &plain, &[]);
    assert!(
        r.iter().any(|l| l.starts_with("step 3: the repair removes same_document from an expect_response and gives_up does not name it")),
        "{r:?}"
    );
    let r = search_repair(&old, &plain, &[give(3, "same_document", "step 5 already proves it")]);
    assert!(r.is_empty(), "{r:?}");
    // Giving one up for what the app shows is refused like any check.
    let r = search_repair(&old, &plain, &[give(3, "same_document", "the app shows a reload")]);
    assert!(r.iter().any(|l| l.contains("a check is never given up because of what the app shows")), "{r:?}");
    assert!(r.iter().any(|l| l.contains("removes same_document")), "{r:?}");
}

#[test]
fn gives_up_cannot_name_a_timing_the_repair_keeps_or_the_old_step_never_had() {
    let old = search_script(json!({ "not_before_ms": 800 }), json!([]));
    let r = search_repair(&old, &old, &[give(3, "not_before_ms", "step 5 times it")]);
    assert!(
        r.iter().any(|l| l.starts_with("step 3: gives_up names a not_before_ms check, and the old step 3 has none")),
        "{r:?}"
    );
    let r = search_repair(&old, &old, &[give(3, "same_document", "step 5 proves it")]);
    assert!(
        r.iter().any(|l| l.starts_with("step 3: gives_up names a same_document check, and the old step 3 has none")),
        "{r:?}"
    );
}

#[test]
fn giving_up_a_whole_timed_check_needs_only_the_check() {
    let old = search_script(json!({ "not_before_ms": 800, "same_document": true }), json!([]));
    // The expect_response goes, its fields with it: one entry, the check's kind.
    let gone = script(json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#search" }] },
        { "step_number": 3, "actions": [{ "kind": "fill", "selector": "#q", "value": "Ann" }] },
    ]));
    let r = search_repair(&old, &gone, &[give(3, "expect_response", "step 6 checks the same request")]);
    assert!(r.is_empty(), "{r:?}");
}

/// The search script with a second `expect_response` in step 3 (on `/api/count`), the first with `first`.
fn search_script_with_count(first: Value, with_count: bool) -> CaseScript {
    let mut search = json!({ "kind": "expect_response", "url_contains": "/api/search" });
    for (k, v) in first.as_object().unwrap() {
        search[k] = v.clone();
    }
    let mut actions = vec![json!({ "kind": "fill", "selector": "#q", "value": "Ann" }), search];
    if with_count {
        actions.push(json!({ "kind": "expect_response", "url_contains": "/api/count" }));
    }
    script(json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#search" }] },
        { "step_number": 3, "actions": actions },
    ]))
}

#[test]
fn a_timing_taken_off_a_surviving_check_is_named_even_when_another_check_was_dropped() {
    // Old step 3: the search check is timed, and a count check follows. The repair drops the count check
    // (named) and, in the same edit, takes the timing off the search check.
    let old = search_script_with_count(json!({ "not_before_ms": 800 }), true);
    let new = search_script_with_count(json!({}), false);
    let dropped = [give(3, "expect_response", "step 6 checks the count")];
    let r = search_repair(&old, &new, &dropped);
    assert!(
        r.iter().any(|l| l.starts_with("step 3: the repair removes not_before_ms from an expect_response and gives_up does not name it")),
        "{r:?}"
    );
    // The check given up does not excuse the 800 the case states: it belongs to the check that stayed.
    assert!(r.iter().any(|l| l.contains("the repair drops \"800\"")), "{r:?}");
    // Naming the timing as well passes.
    let both = [give(3, "expect_response", "step 6 checks the count"), give(3, "not_before_ms", "step 5 times it")];
    let r = search_repair(&old, &new, &both);
    assert!(r.is_empty(), "{r:?}");
    // The dropped check's own timing goes with it: the whole check is the one entry.
    let old = search_script_with_count(json!({}), true);
    let mut timed_count = old.clone();
    if let Some(step) = timed_count.steps.iter_mut().find(|s| s.step_number == 3) {
        step.actions.push(serde_json::from_value(json!({ "kind": "expect_response", "url_contains": "/api/other", "not_before_ms": 800 })).unwrap());
    }
    let r = search_repair(&timed_count, &old, &dropped);
    assert!(r.is_empty(), "{r:?}");
}

#[test]
fn a_timing_is_matched_to_its_check_by_path_and_method_not_by_name_alone() {
    // Two timed checks of one name: the repair keeps one timing and drops the other. Each survivor is
    // compared with its own old self.
    let old = script(json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#search" }] },
        {
            "step_number": 3,
            "actions": [
                { "kind": "fill", "selector": "#q", "value": "Ann" },
                { "kind": "expect_response", "url_contains": "/api/search", "not_before_ms": 800 },
                { "kind": "expect_response", "url_contains": "/api/count", "not_before_ms": 800 },
            ],
        },
    ]));
    let new = script(json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#search" }] },
        {
            "step_number": 3,
            "actions": [
                { "kind": "fill", "selector": "#q", "value": "Ann" },
                { "kind": "expect_response", "url_contains": "/api/search" },
                { "kind": "expect_response", "url_contains": "/api/count", "not_before_ms": 800 },
            ],
        },
    ]));
    let r = search_repair(&old, &new, &[]);
    assert!(r.iter().any(|l| l.contains("removes not_before_ms from an expect_response")), "{r:?}");
    assert!(search_repair(&old, &old, &[]).is_empty());
}

#[test]
fn a_timing_moved_to_another_field_loses_the_field() {
    let old = search_script(json!({ "not_before_ms": 800 }), json!([]));
    let moved = search_script(json!({ "started_within_ms": 800 }), json!([]));
    let r = search_repair(&old, &moved, &[]);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].contains("removes not_before_ms"), "{r:?}");
}
