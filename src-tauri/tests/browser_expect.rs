//! Expectations look again until they hold or time runs out, and a failure
//! says what was actually seen.

mod common;

use common::FakePage;
use serde_json::json;
use v2_lib::browser::actions::{execute_with, Action};
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 250, nav_ms: 300, poll_ms: 10, highlight_ms: 0 }
}

fn action(v: serde_json::Value) -> Action {
    serde_json::from_value(v).unwrap()
}

#[tokio::test]
async fn visible_holds_once_the_element_shows_up() {
    let mut d = FakePage { appears_on_look: 3, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_visible", "selector": { "css": "#toast" } })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "#toast is visible");
}

#[tokio::test]
async fn visible_fails_with_what_it_saw() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_visible", "selector": { "css": "#toast" } })), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("250ms") && out.detail.contains("#toast") && out.detail.contains("is not on the page"), "{}", out.detail);

    // A legacy selector has no visibility filter of its own, so the
    // expectation checks: there, but not showing, is not visible.
    let mut d = FakePage { visible: false, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_visible", "selector": "#toast" })), &quick()).await;
    assert!(!out.ok && out.detail.contains("is there but cannot be seen"), "{}", out.detail);
}

#[tokio::test]
async fn hidden_holds_when_nothing_can_be_seen() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_hidden", "selector": { "role": "dialog" } })), &quick()).await;
    assert!(out.ok, "{}", out.detail);

    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_hidden", "selector": { "css": "#spinner" } })), &quick()).await;
    assert!(!out.ok && out.detail.contains("is still visible"), "{}", out.detail);
}

/// The text settles after a moment, the way a saved record's name does.
#[tokio::test]
async fn text_is_compared_after_collapsing_whitespace_and_retried() {
    let mut d = FakePage { texts: vec!["Saving...", "  Custom   4-Point\n"], ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_text", "selector": { "css": "h2" }, "equals": "Custom 4-Point" })),
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
}

#[tokio::test]
async fn a_text_mismatch_shows_both_sides() {
    let mut d = FakePage { texts: vec!["Custom 5-Point"], ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_text", "selector": { "css": "h2" }, "equals": "Custom 4-Point" })),
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.contains("\"Custom 4-Point\"") && out.detail.contains("\"Custom 5-Point\""), "{}", out.detail);
}

#[tokio::test]
async fn contains_text_is_a_substring_and_case_matters() {
    let mut d = FakePage { texts: vec!["Rating method saved successfully"], ..FakePage::default() }.driver();
    let yes = action(json!({ "kind": "expect_contains_text", "selector": { "css": ".toast" }, "value": "saved successfully" }));
    assert!(execute_with(&mut d, &yes, &quick()).await.ok);
    let no = action(json!({ "kind": "expect_contains_text", "selector": { "css": ".toast" }, "value": "Saved Successfully" }));
    assert!(!execute_with(&mut d, &no, &quick()).await.ok);
}

/// Reading "the" text of three elements would be a guess.
#[tokio::test]
async fn a_text_expectation_needs_exactly_one_element() {
    let mut d = FakePage { found: 3, ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_text", "selector": { "css": "li" }, "equals": "one" })),
        &quick(),
    )
    .await;
    assert!(!out.ok && out.detail.contains("matched 3 elements"), "{}", out.detail);
}

#[tokio::test]
async fn count_says_how_many_it_found() {
    let mut d = FakePage { found: 3, ..FakePage::default() }.driver();
    let three = action(json!({ "kind": "expect_count", "selector": { "css": "tbody tr" }, "equals": 3 }));
    assert!(execute_with(&mut d, &three, &quick()).await.ok);
    let two = action(json!({ "kind": "expect_count", "selector": { "css": "tbody tr" }, "equals": 2 }));
    let out = execute_with(&mut d, &two, &quick()).await;
    assert!(!out.ok && out.detail.contains("expected 2") && out.detail.contains("counted 3"), "{}", out.detail);
    // Zero is a real expectation: "the row is gone".
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let none = action(json!({ "kind": "expect_count", "selector": { "css": "tbody tr" }, "equals": 0 }));
    assert!(execute_with(&mut d, &none, &quick()).await.ok);
}

#[tokio::test]
async fn attribute_compares_and_reports_a_missing_one() {
    let check = action(json!({ "kind": "expect_attribute", "selector": { "css": "#agree" }, "name": "aria-checked", "equals": "true" }));
    let mut d = FakePage { attribute: Some("true"), ..FakePage::default() }.driver();
    assert!(execute_with(&mut d, &check, &quick()).await.ok);
    let mut d = FakePage { attribute: None, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &quick()).await;
    assert!(!out.ok && out.detail.contains("has no aria-checked"), "{}", out.detail);
}

#[tokio::test]
async fn its_own_timeout_wins_over_the_default() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_visible", "selector": { "css": "#x" }, "timeout_ms": 40 })),
        &quick(),
    )
    .await;
    assert!(out.detail.contains("40ms"), "{}", out.detail);
}

#[test]
fn an_expectation_is_validated_like_any_other_action() {
    assert!(action(json!({ "kind": "expect_visible", "selector": {} })).validate().is_err());
    assert!(action(json!({ "kind": "expect_attribute", "selector": "#a", "name": " ", "equals": "x" })).validate().is_err());
    // An empty expected text is legitimate: "the field is now empty".
    assert!(action(json!({ "kind": "expect_text", "selector": "#a", "equals": "" })).validate().is_ok());
    assert!(action(json!({ "kind": "expect_contains_text", "selector": "#a", "value": "" })).validate().is_err());
}
