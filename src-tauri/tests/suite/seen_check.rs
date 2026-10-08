//! The save check: a script names only what the app has seen on the live
//! page, unless the script typed it itself or the test case says it.

use v2_lib::autorun::discovery_map::{AreaMap, DiscoveryMap, PageMap, SeenElement};
use v2_lib::autorun::edits::Edit;
use v2_lib::autorun::seen_check::{check_seen, steps_to_check};
use v2_lib::autorun::CaseScript;
use v2_lib::browser::locator::{LocatorStep, Target};

fn role(r: &str, n: &str) -> LocatorStep {
    LocatorStep { role: Some(r.to_string()), name: Some(n.to_string()), ..LocatorStep::default() }
}

fn text(t: &str) -> LocatorStep {
    LocatorStep { text: Some(t.to_string()), ..LocatorStep::default() }
}

/// A map holding these links, each its own element, on `path` in `area`.
fn map_with(area: &str, path: &str, links: &[LocatorStep]) -> DiscoveryMap {
    let elements = links
        .iter()
        .map(|l| SeenElement {
            key: l.seen_key().unwrap(),
            locator: Target::One(l.clone()),
            role: l.role.clone().unwrap_or_default(),
            name: l.name.clone().unwrap_or_default(),
            kind: "other".to_string(),
            required: false,
            seen_at: 0,
        })
        .collect();
    DiscoveryMap {
        areas: vec![AreaMap {
            area: area.to_string(),
            pages: vec![PageMap { path: path.to_string(), title: String::new(), elements }],
            ..AreaMap::default()
        }],
    }
}

fn script(area: Option<&str>, steps: serde_json::Value) -> CaseScript {
    let mut v = serde_json::json!({ "case_id": 7, "title": "T", "steps": steps });
    if let Some(a) = area {
        v["area"] = serde_json::json!(a);
    }
    serde_json::from_value(v).unwrap()
}

fn refusal(step: i32, describe: &str) -> String {
    format!(
        "Step {step}: {describe} was never seen on the live app. Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again."
    )
}

#[test]
fn a_seen_locator_passes_and_an_unseen_one_names_its_step() {
    let map = map_with("Ratings", "/ratings", &[role("button", "Save")]);
    let ok = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [
            { "kind": "click", "selector": { "role": "button", "name": "  SAVE " } }
        ]}]),
    );
    assert_eq!(check_seen(&map, &ok, &[], None), Ok(()));

    let bad = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Save" } }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Publish" } }] }
        ]),
    );
    assert_eq!(check_seen(&map, &bad, &[], None), Err(refusal(2, "button \"Publish\"")));
}

#[test]
fn every_link_of_a_chain_must_be_seen() {
    let map = map_with("Ratings", "/ratings", &[role("button", "Add Method")]);
    let s = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": [
            { "role": "dialog", "name": "Add Rating Method" },
            { "role": "button", "name": "Add Method" }
        ]}]}]),
    );
    assert_eq!(
        check_seen(&map, &s, &[], None),
        Err(refusal(1, "button \"Add Method\" in dialog \"Add Rating Method\""))
    );
    let both = map_with("Ratings", "/ratings", &[role("button", "Add Method"), role("dialog", "Add Rating Method")]);
    assert_eq!(check_seen(&both, &s, &[], None), Ok(()));
}

#[test]
fn a_locator_holding_text_typed_earlier_is_exempt_but_not_typed_later() {
    let map = map_with("Ratings", "/ratings", &[role("textbox", "Name"), role("button", "Search")]);
    let earlier = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "Quarterly Plan" }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "role": "cell", "name": "Quarterly plan 2026" } }] }
        ]),
    );
    assert_eq!(check_seen(&map, &earlier, &[], None), Ok(()));

    // Typed in the same step, or after: no exemption.
    let later = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "cell", "name": "Quarterly plan 2026" } }] },
            { "step_number": 2, "actions": [{ "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "Quarterly Plan" }] }
        ]),
    );
    assert_eq!(check_seen(&map, &later, &[], None), Err(refusal(1, "cell \"Quarterly plan 2026\"")));
}

#[test]
fn a_check_whose_text_comes_from_the_expected_result_is_exempt_but_a_click_is_not() {
    let map = map_with("Ratings", "/ratings", &[]);
    let case_text = vec!["Open the ratings".to_string(), "A toast says Rating   SAVED successfully".to_string()];
    let check = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [
            { "kind": "expect_visible", "selector": { "text": "rating saved" } },
            { "kind": "wait_for", "selector": { "role": "alert", "name": "Rating saved" }, "timeout_ms": 1000 },
            { "kind": "expect_row", "table": { "role": "table", "name": "Open the ratings" }, "cells": { "A": "b" } }
        ]}]),
    );
    assert_eq!(check_seen(&map, &check, &case_text, None), Ok(()));

    let click = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": { "text": "rating saved" } }] }]),
    );
    assert_eq!(check_seen(&map, &click, &case_text, None), Err(refusal(1, "text \"rating saved\"")));
}

#[test]
fn only_declared_steps_are_checked_on_a_repair() {
    let map = map_with("Ratings", "/ratings", &[role("button", "Save")]);
    let s = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Old" } }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Save" } }] },
            { "step_number": 3, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "New" } }] }
        ]),
    );
    assert_eq!(check_seen(&map, &s, &[], Some(&[2])), Ok(()));
    assert_eq!(check_seen(&map, &s, &[], Some(&[2, 3])), Err(refusal(3, "button \"New\"")));
    assert_eq!(check_seen(&map, &s, &[], None), Err(refusal(1, "button \"Old\"")));
}

#[test]
fn navigate_needs_a_seen_path() {
    let map = map_with("Ratings", "/ratings", &[]);
    let seen = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/ratings?x=1#top" }] }]),
    );
    assert_eq!(check_seen(&map, &seen, &[], None), Ok(()));
    let unseen = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/payroll?id=9" }] }]),
    );
    assert_eq!(check_seen(&map, &unseen, &[], None), Err(refusal(1, "/payroll")));
}

#[test]
fn areas_the_script_visits_count() {
    let map = map_with("Payroll", "/payroll", &[role("button", "Run")]);
    let steps = serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Run" } }] },
        { "step_number": 2, "actions": [{ "kind": "return_to_area", "area": "Payroll" }] }
    ]);
    assert_eq!(check_seen(&map, &script(Some("Ratings"), steps), &[], None), Ok(()));

    let not_visited = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Run" } }] }]),
    );
    assert_eq!(check_seen(&map, &not_visited, &[], None), Err(refusal(1, "button \"Run\"")));
    // The script's own area counts too.
    assert_eq!(check_seen(&map, &script(Some("Payroll"), serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Run" } }] }
    ])), &[], None), Ok(()));
}

#[test]
fn unattributed_locators_count_for_any_area() {
    let map = map_with("", "/", &[text("Welcome")]);
    let s = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": { "text": "Welcome" } }] }]),
    );
    assert_eq!(check_seen(&map, &s, &[], None), Ok(()));
    assert_eq!(check_seen(&map, &script(None, serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": { "text": "Welcome" } }] }
    ])), &[], None), Ok(()));
}

#[test]
fn two_character_typed_values_do_not_exempt() {
    let map = map_with("Ratings", "/ratings", &[role("textbox", "Code")]);
    let s = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "fill", "selector": { "role": "textbox", "name": "Code" }, "value": " ab " }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "role": "option", "name": "Abacus" } }] }
        ]),
    );
    assert_eq!(check_seen(&map, &s, &[], None), Err(refusal(2, "option \"Abacus\"")));
}

#[test]
fn open_tab_needs_a_seen_path() {
    let map = map_with("Ratings", "/ratings", &[]);
    let seen = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "open_tab", "name": "two", "url": "https://app.example/ratings?x=1" }] }]),
    );
    assert_eq!(check_seen(&map, &seen, &[], None), Ok(()));
    let unseen = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "open_tab", "name": "two", "url": "https://app.example/payroll#x" }] }]),
    );
    assert_eq!(check_seen(&map, &unseen, &[], None), Err(refusal(1, "/payroll")));
}

#[test]
fn a_changed_script_with_no_declared_steps_is_checked_in_full() {
    // No declaration: every step is checked.
    assert_eq!(steps_to_check(None), None);
    let edit: Edit = serde_json::from_value(serde_json::json!({ "case_id": 7, "steps": [2], "why": "w" })).unwrap();
    assert_eq!(steps_to_check(Some(&edit)), Some(vec![2]));

    let map = map_with("Ratings", "/ratings", &[role("button", "Save")]);
    let s = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Old" } }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Save" } }] }
        ]),
    );
    assert_eq!(check_seen(&map, &s, &[], steps_to_check(None).as_deref()), Err(refusal(1, "button \"Old\"")));
}

#[test]
fn a_short_or_partial_word_from_the_case_does_not_exempt_a_check() {
    let map = map_with("Ratings", "/ratings", &[]);
    let case_text = vec!["The token is shown".to_string(), "Ratings are listed".to_string()];
    let short = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": { "role": "button", "name": "OK" } }] }]),
    );
    assert_eq!(check_seen(&map, &short, &case_text, None), Err(refusal(1, "button \"OK\"")));
    // Long enough, but only part of a word.
    let partial = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": { "text": "oken" } }] }]),
    );
    assert_eq!(check_seen(&map, &partial, &case_text, None), Err(refusal(1, "text \"oken\"")));
    let inside = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": { "text": "Rating" } }] }]),
    );
    assert_eq!(check_seen(&map, &inside, &case_text, None), Err(refusal(1, "text \"Rating\"")));
}

#[test]
fn a_whole_phrase_from_the_case_exempts_a_check() {
    let map = map_with("Ratings", "/ratings", &[]);
    let case_text = vec!["A toast says \"Rating saved.\"".to_string(), "Token shown".to_string()];
    for name in ["Rating saved", "token", "token shown", "a toast says"] {
        let s = script(
            Some("Ratings"),
            serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": { "text": name } }] }]),
        );
        assert_eq!(check_seen(&map, &s, &case_text, None), Ok(()), "{name}");
    }
}

/// The typed-value exception: a value the script typed earlier, of at
/// least 3 characters, exempts a locator whose text or name holds that
/// value as whole words. Never the other way round: a short locator name
/// that merely sits inside a longer typed value is not exempt.
#[test]
fn a_typed_value_exempts_only_a_locator_that_holds_it_as_whole_words() {
    let map = map_with("Ratings", "/ratings", &[role("searchbox", "Search")]);
    let typed = |value: &str, name: &str| {
        script(
            Some("Ratings"),
            serde_json::json!([
                { "step_number": 1, "actions": [{ "kind": "fill", "selector": { "role": "searchbox", "name": "Search" }, "value": value }] },
                { "step_number": 2, "actions": [{ "kind": "click", "selector": { "role": "button", "name": name } }] }
            ]),
        )
    };
    // "test" is a whole word inside "Test connection".
    assert_eq!(check_seen(&map, &typed("Test", "Test connection"), &[], None), Ok(()));
    // The locator's name inside the typed value does not count.
    assert_eq!(
        check_seen(&map, &typed("AutoTest Leave 7", "Leave"), &[], None),
        Err(refusal(2, "button \"Leave\""))
    );
    // Part of a word is not a word.
    assert_eq!(
        check_seen(&map, &typed("Test", "Contest"), &[], None),
        Err(refusal(2, "button \"Contest\""))
    );
    assert_eq!(
        check_seen(&map, &typed("Test", "Testing"), &[], None),
        Err(refusal(2, "button \"Testing\""))
    );
    // Two characters exempt nothing, even as a whole word.
    assert_eq!(
        check_seen(&map, &typed("QA", "QA report"), &[], None),
        Err(refusal(2, "button \"QA report\""))
    );
}

#[test]
fn a_row_holding_the_typed_record_is_exempt() {
    let map = map_with("Leave", "/leave", &[role("textbox", "Title")]);
    let s = script(
        Some("Leave"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "fill", "selector": { "role": "textbox", "name": "Title" }, "value": "AutoTest Leave 7" }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "role": "row", "name": "AutoTest Leave 7 Pending" } }] }
        ]),
    );
    assert_eq!(check_seen(&map, &s, &[], None), Ok(()));
}
