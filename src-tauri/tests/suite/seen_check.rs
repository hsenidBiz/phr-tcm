//! The save check: a script names only what the app has seen on the live
//! page, unless the script typed it itself or the test case says it.

use v2_lib::autorun::components::{Component, ComponentFile};
use v2_lib::autorun::discovery_map::{AreaMap, DiscoveryMap, PageMap, SeenElement};
use v2_lib::autorun::edits::Edit;
use v2_lib::autorun::seen_check::{
    check_component_seen, check_resolved_inputs, check_seen, check_seen_all, check_seen_all_hinted, check_seen_with_files,
    component_verdict, has_data_placeholders, has_placeholder_inputs, refusal_list, seen_verdict, steps_to_check,
    unseen_component_targets, unseen_targets, SeenVerdict, Unseen, MAX_LISTED,
};
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

/// No components saved in the project.
fn none() -> ComponentFile {
    ComponentFile::default()
}

/// A project holding these components, each written as its JSON.
fn components(list: serde_json::Value) -> ComponentFile {
    let components: Vec<Component> = serde_json::from_value(list).expect("components");
    ComponentFile { components }
}

/// "Edit a row": clicks the row the script names, then a fixed Edit button
/// the component itself was checked for when it was saved.
fn edit_a_row() -> serde_json::Value {
    serde_json::json!({
        "name": "Edit a row", "description": "d", "version": 1,
        "inputs": [{ "name": "row", "kind": "target", "description": "" }],
        "actions": [
            { "kind": "click", "selector": { "input": "row" } },
            { "kind": "click", "selector": { "role": "button", "name": "Edit" } }
        ]
    })
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
    assert_eq!(check_seen(&map, &none(), &ok, &[], None), Ok(()));

    let bad = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Save" } }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Publish" } }] }
        ]),
    );
    assert_eq!(check_seen(&map, &none(), &bad, &[], None), Err(refusal(2, "button \"Publish\"")));
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
        check_seen(&map, &none(), &s, &[], None),
        Err(refusal(1, "button \"Add Method\" in dialog \"Add Rating Method\""))
    );
    let both = map_with("Ratings", "/ratings", &[role("button", "Add Method"), role("dialog", "Add Rating Method")]);
    assert_eq!(check_seen(&both, &none(), &s, &[], None), Ok(()));
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
    assert_eq!(check_seen(&map, &none(), &earlier, &[], None), Ok(()));

    // Typed in the same step, or after: no exemption.
    let later = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "cell", "name": "Quarterly plan 2026" } }] },
            { "step_number": 2, "actions": [{ "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "Quarterly Plan" }] }
        ]),
    );
    assert_eq!(check_seen(&map, &none(), &later, &[], None), Err(refusal(1, "cell \"Quarterly plan 2026\"")));
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
    assert_eq!(check_seen(&map, &none(), &check, &case_text, None), Ok(()));

    let click = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": { "text": "rating saved" } }] }]),
    );
    assert_eq!(check_seen(&map, &none(), &click, &case_text, None), Err(refusal(1, "text \"rating saved\"")));
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
    assert_eq!(check_seen(&map, &none(), &s, &[], Some(&[2])), Ok(()));
    assert_eq!(check_seen(&map, &none(), &s, &[], Some(&[2, 3])), Err(refusal(3, "button \"New\"")));
    assert_eq!(
        check_seen(&map, &none(), &s, &[], None),
        Err(format!("{}
{}", refusal(1, "button \"Old\""), refusal(3, "button \"New\"")))
    );
}

#[test]
fn navigate_needs_a_seen_path() {
    let map = map_with("Ratings", "/ratings", &[]);
    let seen = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/ratings?x=1#top" }] }]),
    );
    assert_eq!(check_seen(&map, &none(), &seen, &[], None), Ok(()));
    let unseen = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/payroll?id=9" }] }]),
    );
    assert_eq!(check_seen(&map, &none(), &unseen, &[], None), Err(refusal(1, "/payroll")));
}

#[test]
fn areas_the_script_visits_count() {
    let map = map_with("Payroll", "/payroll", &[role("button", "Run")]);
    let steps = serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Run" } }] },
        { "step_number": 2, "actions": [{ "kind": "return_to_area", "area": "Payroll" }] }
    ]);
    assert_eq!(check_seen(&map, &none(), &script(Some("Ratings"), steps), &[], None), Ok(()));

    let not_visited = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Run" } }] }]),
    );
    assert_eq!(check_seen(&map, &none(), &not_visited, &[], None), Err(refusal(1, "button \"Run\"")));
    // The script's own area counts too.
    assert_eq!(check_seen(&map, &none(), &script(Some("Payroll"), serde_json::json!([
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
    assert_eq!(check_seen(&map, &none(), &s, &[], None), Ok(()));
    assert_eq!(check_seen(&map, &none(), &script(None, serde_json::json!([
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
    assert_eq!(check_seen(&map, &none(), &s, &[], None), Err(refusal(2, "option \"Abacus\"")));
}

#[test]
fn open_tab_needs_a_seen_path() {
    let map = map_with("Ratings", "/ratings", &[]);
    let seen = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "open_tab", "name": "two", "url": "https://app.example/ratings?x=1" }] }]),
    );
    assert_eq!(check_seen(&map, &none(), &seen, &[], None), Ok(()));
    let unseen = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "open_tab", "name": "two", "url": "https://app.example/payroll#x" }] }]),
    );
    assert_eq!(check_seen(&map, &none(), &unseen, &[], None), Err(refusal(1, "/payroll")));
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
    assert_eq!(check_seen(&map, &none(), &s, &[], steps_to_check(None).as_deref()), Err(refusal(1, "button \"Old\"")));
}

#[test]
fn a_short_or_partial_word_from_the_case_does_not_exempt_a_check() {
    let map = map_with("Ratings", "/ratings", &[]);
    let case_text = vec!["The token is shown".to_string(), "Ratings are listed".to_string()];
    let short = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": { "role": "button", "name": "OK" } }] }]),
    );
    assert_eq!(check_seen(&map, &none(), &short, &case_text, None), Err(refusal(1, "button \"OK\"")));
    // Long enough, but only part of a word.
    let partial = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": { "text": "oken" } }] }]),
    );
    assert_eq!(check_seen(&map, &none(), &partial, &case_text, None), Err(refusal(1, "text \"oken\"")));
    let inside = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "expect_visible", "selector": { "text": "Rating" } }] }]),
    );
    assert_eq!(check_seen(&map, &none(), &inside, &case_text, None), Err(refusal(1, "text \"Rating\"")));
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
        assert_eq!(check_seen(&map, &none(), &s, &case_text, None), Ok(()), "{name}");
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
    assert_eq!(check_seen(&map, &none(), &typed("Test", "Test connection"), &[], None), Ok(()));
    // The locator's name inside the typed value does not count.
    assert_eq!(
        check_seen(&map, &none(), &typed("AutoTest Leave 7", "Leave"), &[], None),
        Err(refusal(2, "button \"Leave\""))
    );
    // Part of a word is not a word.
    assert_eq!(
        check_seen(&map, &none(), &typed("Test", "Contest"), &[], None),
        Err(refusal(2, "button \"Contest\""))
    );
    assert_eq!(
        check_seen(&map, &none(), &typed("Test", "Testing"), &[], None),
        Err(refusal(2, "button \"Testing\""))
    );
    // Two characters exempt nothing, even as a whole word.
    assert_eq!(
        check_seen(&map, &none(), &typed("QA", "QA report"), &[], None),
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
    assert_eq!(check_seen(&map, &none(), &s, &[], None), Ok(()));
}

/// An import names every unseen locator, step by step. A save refused for a
/// page address as well names that refusal alone.
#[test]
fn check_seen_all_lists_every_unseen_locator_and_check_seen_names_a_page_alone() {
    let map = map_with("Ratings", "/ratings", &[role("button", "Save")]);
    let s = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [
                { "kind": "click", "selector": { "role": "button", "name": "Publish" } },
                { "kind": "click", "selector": { "role": "button", "name": "Save" } }
            ] },
            { "step_number": 2, "actions": [
                { "kind": "navigate", "url": "/elsewhere" },
                { "kind": "click", "selector": { "role": "button", "name": "Archive" } }
            ] }
        ]),
    );
    let unseen = |step: i32, what: &str| Unseen { step, locator: what.to_string(), refused: None };
    assert_eq!(
        check_seen_all(&map, &none(), &s, &[], None),
        vec![unseen(1, "button \"Publish\""), unseen(2, "/elsewhere"), unseen(2, "button \"Archive\"")]
    );
    assert_eq!(check_seen(&map, &none(), &s, &[], None), Err(refusal(2, "/elsewhere")));
    assert_eq!(check_seen_all(&map, &none(), &s, &[], Some(&[2])).len(), 2, "only the declared steps");
    let ok = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Save" } }] }]),
    );
    assert!(check_seen_all(&map, &none(), &ok, &[], None).is_empty());
}

/// A navigate to another record of a page the map has seen passes: ids in
/// a path are compared as `:id`, in the map as an older map holds them and
/// in the address the script opens.
#[test]
fn a_navigate_to_another_record_of_a_seen_page_passes() {
    let nav = |url: &str| script(None, serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "navigate", "url": url }] }]));
    for seen in ["/leave/111/edit", "/leave/:id/edit"] {
        let map = map_with("", seen, &[]);
        assert_eq!(check_seen(&map, &none(), &nav("https://h/leave/222/edit?tab=2"), &[], None), Ok(()), "{seen}");
        assert_eq!(check_seen(&map, &none(), &nav("/leave/222/view"), &[], None), Err(refusal(1, "/leave/222/view")), "{seen}");
    }
}

// ---- scripts that use components ----

fn use_component(step: i32, name: &str, inputs: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "step_number": step, "actions": [
        { "kind": "use_component", "component": name, "inputs": inputs }
    ] })
}

/// A component the project does not have is refused at its step, and an
/// import lists it with the rest.
#[test]
fn an_unknown_component_is_refused() {
    let map = map_with("Leave", "/leave", &[role("row", "Alpha")]);
    let s = script(
        Some("Leave"),
        serde_json::json!([
            use_component(1, "Edit a row", serde_json::json!({ "row": { "role": "row", "name": "Alpha" } })),
            use_component(2, " Ghost ", serde_json::json!({}))
        ]),
    );
    let have = components(serde_json::json!([edit_a_row()]));
    assert_eq!(check_seen(&map, &have, &s, &[], None), Err("Step 2: Ghost is not saved in this project".to_string()));
    assert_eq!(
        check_seen_all(&map, &have, &s, &[], None),
        vec![Unseen { step: 2, locator: " Ghost ".to_string(), refused: Some("Ghost is not saved in this project".to_string()) }]
    );
    assert_eq!(check_seen(&map, &have, &s, &[], Some(&[1])), Ok(()), "step 2 is not checked");
    assert_eq!(
        check_seen(&map, &none(), &s, &[], None),
        Err("Step 1: Edit a row is not saved in this project".to_string())
    );
}

/// Every declared input must be given, and of its kind.
#[test]
fn a_missing_or_wrong_kind_input_is_refused() {
    let map = map_with("Leave", "/leave", &[role("textbox", "Day")]);
    let have = components(serde_json::json!([{
        "name": "Pick a date", "description": "d", "version": 1,
        "inputs": [
            { "name": "field", "kind": "target", "description": "" },
            { "name": "day", "kind": "text", "description": "" }
        ],
        "actions": [{ "kind": "fill", "selector": { "input": "field" }, "value": "{{day}}" }]
    }]));
    let field = serde_json::json!({ "role": "textbox", "name": "Day" });
    let with = |inputs: serde_json::Value| script(Some("Leave"), serde_json::json!([use_component(3, "pick a  DATE", inputs)]));
    for (inputs, why) in [
        (serde_json::json!({ "field": field }), "Step 3: Pick a date needs day"),
        (serde_json::json!({ "day": "5" }), "Step 3: Pick a date needs field"),
        (serde_json::json!({ "field": "#day", "day": "5" }), "Step 3: Pick a date needs field to be a locator"),
        (serde_json::json!({ "field": { "nope": 1 }, "day": "5" }), "Step 3: Pick a date needs field to be a locator"),
        (serde_json::json!({ "field": { "input": "field" }, "day": "5" }), "Step 3: Pick a date needs field to be a locator"),
        (serde_json::json!({ "field": field, "day": { "css": "#x" } }), "Step 3: Pick a date needs day to be text"),
    ] {
        assert_eq!(check_seen(&map, &have, &with(inputs.clone()), &[], None), Err(why.to_string()), "{inputs}");
    }
    assert_eq!(check_seen(&map, &have, &with(serde_json::json!({ "field": field, "day": "5" })), &[], None), Ok(()));
}

/// A target input's links are checked like any locator in the script; the
/// component's own fixed locators are not (its Edit button is not on this
/// map). A use inside a `when_visible` is checked too.
#[test]
fn an_unseen_target_input_is_refused_and_a_seen_one_passes() {
    let have = components(serde_json::json!([edit_a_row()]));
    let s = script(
        Some("Leave"),
        serde_json::json!([use_component(1, "Edit a row", serde_json::json!({ "row": [
            { "role": "grid", "name": "Requests" }, { "role": "row", "name": "Alpha" }
        ] }))]),
    );
    let seen = map_with("Leave", "/leave", &[role("grid", "Requests"), role("row", "Alpha")]);
    assert_eq!(check_seen(&seen, &have, &s, &[], None), Ok(()));
    let unseen = map_with("Leave", "/leave", &[role("grid", "Requests")]);
    assert_eq!(
        check_seen(&unseen, &have, &s, &[], None),
        Err(refusal(1, &Target::Chain(vec![role("grid", "Requests"), role("row", "Alpha")]).describe()))
    );
    let guarded = script(
        Some("Leave"),
        serde_json::json!([{ "step_number": 1, "actions": [{
            "kind": "when_visible", "selector": { "role": "grid", "name": "Requests" },
            "then": [{ "kind": "use_component", "component": "Edit a row", "inputs": { "row": { "role": "row", "name": "Beta" } } }]
        }] }]),
    );
    assert_eq!(check_seen(&seen, &have, &guarded, &[], None), Err(refusal(1, "row \"Beta\"")));
}

/// A target input the component only checks for may be a name the test
/// case says; one it clicks may not.
#[test]
fn a_target_input_a_component_checks_for_may_be_named_by_the_case() {
    let map = map_with("Leave", "/leave", &[]);
    let have = components(serde_json::json!([
        {
            "name": "See a message", "description": "d", "version": 1,
            "inputs": [{ "name": "message", "kind": "target", "description": "" }],
            "actions": [{ "kind": "expect_visible", "selector": { "input": "message" } }]
        },
        {
            "name": "Close a message", "description": "d", "version": 1,
            "inputs": [{ "name": "message", "kind": "target", "description": "" }],
            "actions": [{ "kind": "click", "selector": { "input": "message" } }]
        }
    ]));
    let case_text = vec!["The message Leave approved appears".to_string()];
    let with = |name: &str| {
        script(
            Some("Leave"),
            serde_json::json!([use_component(1, name, serde_json::json!({ "message": { "text": "Leave approved" } }))]),
        )
    };
    assert_eq!(check_seen(&map, &have, &with("See a message"), &case_text, None), Ok(()));
    assert_eq!(
        check_seen(&map, &have, &with("Close a message"), &case_text, None),
        Err(refusal(1, "text \"Leave approved\""))
    );
}

/// A value typed in an earlier step exempts a target input that contains
/// it, whether the script typed it or a component did with a text input.
#[test]
fn a_typed_value_exempts_a_target_input_that_contains_it() {
    let map = map_with("Leave", "/leave", &[role("textbox", "Name")]);
    let have = components(serde_json::json!([
        edit_a_row(),
        {
            "name": "Type a name", "description": "d", "version": 1,
            "inputs": [{ "name": "name", "kind": "text", "description": "" }],
            "actions": [{ "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "{{name}}" }]
        }
    ]));
    let row = |name: &str| serde_json::json!({ "row": { "role": "row", "name": name } });
    let typed_by_script = script(
        Some("Leave"),
        serde_json::json!([
            { "step_number": 1, "actions": [
                { "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "AutoTest Leave 7" }
            ] },
            use_component(2, "Edit a row", row("AutoTest Leave 7 Pending"))
        ]),
    );
    assert_eq!(check_seen(&map, &have, &typed_by_script, &[], None), Ok(()));
    let typed_by_component = script(
        Some("Leave"),
        serde_json::json!([
            use_component(1, "Type a name", serde_json::json!({ "name": "AutoTest Leave 9" })),
            use_component(2, "Edit a row", row("AutoTest Leave 9 Pending"))
        ]),
    );
    assert_eq!(check_seen(&map, &have, &typed_by_component, &[], Some(&[2])), Ok(()));
    let not_typed = script(Some("Leave"), serde_json::json!([use_component(1, "Edit a row", row("AutoTest Leave 9 Pending"))]));
    assert_eq!(
        check_seen(&map, &have, &not_typed, &[], None),
        Err(refusal(1, "row \"AutoTest Leave 9 Pending\""))
    );
}

/// "Open a request": clicks the row its text input names, then a fixed
/// Edit button the component was checked for when it was saved.
fn open_a_request() -> serde_json::Value {
    serde_json::json!({
        "name": "Open a request", "description": "d", "version": 1,
        "inputs": [{ "name": "who", "kind": "text", "description": "" }],
        "actions": [
            { "kind": "click", "selector": { "role": "row", "name": "{{who}}" } },
            { "kind": "click", "selector": { "role": "button", "name": "Edit" } }
        ]
    })
}

/// A text input put into a component's locator is checked like any
/// locator the script names: the case may name it only for a check.
#[test]
fn a_text_input_inside_a_fixed_locator_is_seen_checked() {
    let map = map_with("Leave", "/leave", &[role("row", "Alpha")]);
    let have = components(serde_json::json!([
        open_a_request(),
        {
            "name": "See a request", "description": "d", "version": 1,
            "inputs": [{ "name": "who", "kind": "text", "description": "" }],
            "actions": [{ "kind": "expect_visible", "selector": { "role": "row", "name": "{{who}}" } }]
        }
    ]));
    let with = |name: &str, who: &str| {
        script(Some("Leave"), serde_json::json!([use_component(1, name, serde_json::json!({ "who": who }))]))
    };
    assert_eq!(check_seen(&map, &have, &with("Open a request", "Alpha"), &[], None), Ok(()));
    assert_eq!(
        check_seen(&map, &have, &with("Open a request", "Beta"), &[], None),
        Err(refusal(1, "row \"Beta\""))
    );
    let case_text = vec!["The request from Beta shows".to_string()];
    assert_eq!(check_seen(&map, &have, &with("See a request", "Beta"), &case_text, None), Ok(()));
    assert_eq!(
        check_seen(&map, &have, &with("Open a request", "Beta"), &case_text, None),
        Err(refusal(1, "row \"Beta\""))
    );
}

/// A value typed earlier, by the script or earlier in the same
/// component, exempts a locator a text input was put into.
#[test]
fn a_text_input_inside_a_fixed_locator_typed_earlier_is_exempt() {
    let map = map_with("Leave", "/leave", &[role("textbox", "Name")]);
    let have = components(serde_json::json!([
        open_a_request(),
        {
            "name": "Add and open", "description": "d", "version": 1,
            "inputs": [{ "name": "who", "kind": "text", "description": "" }],
            "actions": [
                { "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "{{who}}" },
                { "kind": "click", "selector": { "role": "row", "name": "{{who}} Pending" } }
            ]
        }
    ]));
    let in_component =
        script(Some("Leave"), serde_json::json!([use_component(1, "Add and open", serde_json::json!({ "who": "AutoTest Leave 3" }))]));
    assert_eq!(check_seen(&map, &have, &in_component, &[], None), Ok(()));
    let by_script = script(
        Some("Leave"),
        serde_json::json!([
            { "step_number": 1, "actions": [
                { "kind": "fill", "selector": { "role": "textbox", "name": "Name" }, "value": "AutoTest Leave 4" }
            ] },
            use_component(2, "Open a request", serde_json::json!({ "who": "AutoTest Leave 4 Pending" }))
        ]),
    );
    assert_eq!(check_seen(&map, &have, &by_script, &[], None), Ok(()));
    let not_typed =
        script(Some("Leave"), serde_json::json!([use_component(1, "Open a request", serde_json::json!({ "who": "AutoTest Leave 4 Pending" }))]));
    assert_eq!(
        check_seen(&map, &have, &not_typed, &[], None),
        Err(refusal(1, "row \"AutoTest Leave 4 Pending\""))
    );
}

// ---- locators built from data ----

fn css(c: &str) -> LocatorStep {
    LocatorStep { css: Some(c.to_string()), ..LocatorStep::default() }
}

/// Two maps' areas as one map.
fn joined(a: DiscoveryMap, b: DiscoveryMap) -> DiscoveryMap {
    DiscoveryMap { areas: a.areas.into_iter().chain(b.areas).collect() }
}

/// A script in `area` whose step 1 does `actions`.
fn one_step(area: &str, actions: serde_json::Value) -> CaseScript {
    script(Some(area), serde_json::json!([{ "step_number": 1, "actions": actions }]))
}

fn click(selector: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "kind": "click", "selector": selector })
}

fn refusal_hinted(step: i32, describe: &str, hint: &str) -> String {
    format!(
        "Step {step}: {describe} was never seen on the live app; {hint} Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again."
    )
}

/// The "Edit" button inside the card of the cycle `id` names.
fn edit_in_card(id: &str) -> serde_json::Value {
    serde_json::json!([
        { "css": format!("div[data-cycle-id=\"{id}\"]") },
        { "css": "button[aria-label^=\"Edit\"]" }
    ])
}

/// A placeholder stands for a seen value: only where the rest of the
/// locator was seen, only a non-empty run with no quote, and only with the
/// text around it as seen.
#[test]
fn a_placeholder_matches_only_a_seen_shape() {
    let map = map_with(
        "Cycles",
        "/cycles",
        &[css("div[data-cycle-id=\"10066\"]"), css("button[aria-label^=\"Edit\"]"), role("button", "Edit Cycle A")],
    );
    for id in ["{{setup.cycle_id}}", "{{fixture.pc-draft-before-evaluators.cycle_id}}", "{{ setup.cycle_id }}"] {
        let s = one_step("Cycles", serde_json::json!([click(edit_in_card(id))]));
        assert_eq!(check_seen(&map, &none(), &s, &[], None), Ok(()), "{id}");
    }
    let refused = |selector: serde_json::Value, map: &DiscoveryMap| {
        let s = one_step("Cycles", serde_json::json!([click(selector.clone())]));
        let t: Target = serde_json::from_value(selector).unwrap();
        assert_eq!(check_seen(map, &none(), &s, &[], None), Err(refusal(1, &t.describe())));
    };
    // The text around the placeholder must be as seen.
    refused(edit_in_card("c-{{setup.cycle_id}}"), &map);
    // So must the element and the attribute.
    refused(serde_json::json!({ "css": "span[data-cycle-id=\"{{setup.cycle_id}}\"]" }), &map);
    refused(serde_json::json!({ "css": "div[data-cycle-key=\"{{setup.cycle_id}}\"]" }), &map);
    // Not a placeholder the run fills in: Auto Run fills only fixture and
    // setup values.
    for id in ["{{cycle_id}}", "{{prefix}}", "{{ prefix }}", "{{now:yyyyMMdd}}"] {
        refused(edit_in_card(id), &map);
    }
    // In css, only inside a quoted attribute value: anywhere else it is
    // literal, so a selector made of data matches nothing.
    for c in ["{{setup.sel}}", "div{{setup.x}}", "#{{setup.x}}", ".{{setup.cls}}", "div[{{setup.attr}}=\"10066\"]"] {
        refused(serde_json::json!({ "css": c }), &map);
    }
    // No such attribute seen at all, or only an empty one, or only with
    // another attribute after it: a placeholder never crosses a quote.
    for seen in ["div[data-cycle-name=\"Annual\"]", "div[data-cycle-id=\"\"]", "div[data-cycle-id=\"1\"][data-x=\"2\"]"] {
        let other = map_with("Cycles", "/cycles", &[css(seen), css("button[aria-label^=\"Edit\"]")]);
        refused(edit_in_card("{{setup.cycle_id}}"), &other);
    }
    // Seen in another area only.
    let elsewhere = joined(
        map_with("Cycles", "/cycles", &[css("button[aria-label^=\"Edit\"]")]),
        map_with("Payroll", "/payroll", &[css("div[data-cycle-id=\"10066\"]")]),
    );
    refused(edit_in_card("{{setup.cycle_id}}"), &elsewhere);
    // In a name: the role must be as seen, and the words around it.
    let named = |r: &str, n: &str| one_step("Cycles", serde_json::json!([click(serde_json::json!({ "role": r, "name": n }))]));
    assert_eq!(check_seen(&map, &none(), &named("button", "Edit {{setup.cycle_name}}"), &[], None), Ok(()));
    assert_eq!(
        check_seen(&map, &none(), &named("link", "Edit {{setup.cycle_name}}"), &[], None),
        Err(refusal(1, "link \"Edit {{setup.cycle_name}}\""))
    );
    assert_eq!(
        check_seen(&map, &none(), &named("button", "Open {{setup.cycle_name}}"), &[], None),
        Err(refusal(1, "button \"Open {{setup.cycle_name}}\""))
    );
}

/// "Edit cycle by id": the Edit button in the card of the cycle its text
/// input names. "Open a card": clicks the card its target input names.
fn edit_cycle_by_id() -> ComponentFile {
    components(serde_json::json!([{
        "name": "Edit cycle by id", "description": "d", "version": 1,
        "inputs": [{ "name": "id", "kind": "text", "description": "" }],
        "actions": [{ "kind": "click", "selector": [
            { "css": "div[data-cycle-id=\"{{id}}\"]" },
            { "css": "button[aria-label^=\"Edit\"]" }
        ] }]
    }, {
        "name": "Open a card", "description": "d", "version": 1,
        "inputs": [{ "name": "card", "kind": "target", "description": "" }],
        "actions": [{ "kind": "click", "selector": { "input": "card" } }]
    }]))
}

/// A component input may carry a placeholder: the save checks the
/// locator it makes by its shape, and the run checks the value it took.
#[test]
fn a_component_input_placeholder_is_checked_at_run_time() {
    let map = map_with("Cycles", "/cycles", &[css("div[data-cycle-id=\"10066\"]"), css("button[aria-label^=\"Edit\"]")]);
    let have = edit_cycle_by_id();
    let with = |inputs: serde_json::Value| {
        one_step("Cycles", serde_json::json!([{ "kind": "use_component", "component": "Edit cycle by id", "inputs": inputs }]))
    };
    let saved = with(serde_json::json!({ "id": "{{setup.cycle_id}}" }));
    assert_eq!(check_seen(&map, &have, &saved, &[], None), Ok(()));
    // Any other brace pair is still the component's own placeholder, and
    // a prefix or a time is not filled in by Auto Run.
    for id in ["{{cycle_id}}", "{{prefix}}", "{{now:yyyyMMdd}}"] {
        assert_eq!(
            check_seen(&map, &have, &with(serde_json::json!({ "id": id })), &[], None),
            Err("Step 1: Edit cycle by id got a placeholder as id".to_string()),
            "{id}"
        );
    }
    assert!(has_placeholder_inputs(&saved.steps));

    let run = |filled: &str| {
        let f = with(serde_json::json!({ "id": filled }));
        check_resolved_inputs(&map, &have, &["Cycles"], &saved.steps, &f.steps)
    };
    // A new draft's id: digits, where digits were seen.
    assert_eq!(run("10071"), Ok(()));
    let gave = |id: &str| {
        format!(
            "Step 1: Edit cycle by id: its input id gave button[aria-label^=\"Edit\"] in div[data-cycle-id=\"{id}\"], which does not fit what was seen on the live app. Explore that screen again with discovery, or check the value the setup or fixture gives."
        )
    };
    assert_eq!(run("draft-7"), Err(gave("draft-7")));
    assert_eq!(run("{{setup.cycle_id}}"), Err(gave("{{setup.cycle_id}}")), "never filled in");
    assert_eq!(run("1\"] , div[x=\"2"), Err(gave("1\"] , div[x=\"2")));
    // Checked against the sightings of the areas given only.
    let f = with(serde_json::json!({ "id": "10071" }));
    assert_eq!(
        check_resolved_inputs(&map_with("Payroll", "/p", &[css("div[data-cycle-id=\"1\"]")]), &have, &["Cycles"], &saved.steps, &f.steps),
        Err(gave("10071"))
    );

    // A target input carrying one is checked the same way.
    let card = |c: &str| {
        one_step("Cycles", serde_json::json!([{ "kind": "use_component", "component": "Open a card", "inputs": { "card": { "css": c } } }]))
    };
    let saved = card("div[data-cycle-id=\"{{fixture.pc-draft.cycle_id}}\"]");
    assert_eq!(check_seen(&map, &have, &saved, &[], None), Ok(()));
    assert_eq!(check_resolved_inputs(&map, &have, &["Cycles"], &saved.steps, &card("div[data-cycle-id=\"9\"]").steps), Ok(()));
    assert_eq!(
        check_resolved_inputs(&map, &have, &["Cycles"], &saved.steps, &card("div[data-cycle-id=\"x9\"]").steps),
        Err("Step 1: Open a card: its input card gave div[data-cycle-id=\"x9\"], which does not fit what was seen on the live app. Explore that screen again with discovery, or check the value the setup or fixture gives.".to_string())
    );
    // A use with no placeholder in its inputs has nothing to check.
    assert!(!has_placeholder_inputs(&with(serde_json::json!({ "id": "10066" })).steps));
}

#[test]
fn a_checked_state_on_a_seen_input_passes() {
    let map = map_with("Rules", "/rules", &[css("#er-goals-checkbox input")]);
    let with = |selector: serde_json::Value| one_step("Rules", serde_json::json!([click(selector)]));
    for state in [":checked", ":disabled", ":enabled", ":focus", ":checked:focus"] {
        let c = format!("#er-goals-checkbox input{state}");
        assert_eq!(check_seen(&map, &none(), &with(serde_json::json!({ "css": c })), &[], None), Ok(()), "{c}");
        // A plain string selector reads the same.
        assert_eq!(check_seen(&map, &none(), &with(serde_json::json!(c)), &[], None), Ok(()), "{c}");
    }
    // A state after a space or a combinator, or starting the selector, is
    // an element of its own: never seen here.
    for c in [
        "#er-other input:checked",
        "#er-goals-checkbox input:focus-visible",
        "#er-goals-checkbox select:checked",
        "#er-goals-checkbox input :checked",
        "#er-goals-checkbox input > :checked",
        ":checked",
        "#er-goals-checkbox input :not(.x)",
        "#er-goals-checkbox input+:has(.x)",
    ] {
        assert_eq!(check_seen(&map, &none(), &with(serde_json::json!({ "css": c })), &[], None), Err(refusal(1, c)));
    }
}

#[test]
fn a_not_filter_needs_its_inner_part_seen() {
    let not_annual = ".phr-mc-card:not([data-cycle-name*=\"Annual Performance Review\"])";
    let with = |c: &str| one_step("Cycles", serde_json::json!([click(serde_json::json!({ "css": c }))]));
    let base_only = map_with("Cycles", "/cycles", &[css(".phr-mc-card")]);
    assert_eq!(check_seen(&base_only, &none(), &with(not_annual), &[], None), Err(refusal(1, not_annual)));
    // The base seen carrying the attribute.
    let carrying = map_with(
        "Cycles",
        "/cycles",
        &[css(".phr-mc-card"), css(".phr-mc-card[data-cycle-name=\"Annual Performance Review 2026\"]")],
    );
    assert_eq!(check_seen(&carrying, &none(), &with(not_annual), &[], None), Ok(()));
    // Another element carrying it does not count, nor the attribute
    // without the base seen on its own.
    let other = map_with("Cycles", "/cycles", &[css(".phr-mc-card"), css(".phr-row[data-cycle-name=\"A\"]")]);
    assert_eq!(check_seen(&other, &none(), &with(not_annual), &[], None), Err(refusal(1, not_annual)));
    let no_base = map_with("Cycles", "/cycles", &[css(".phr-mc-card[data-cycle-name=\"A\"]")]);
    assert_eq!(check_seen(&no_base, &none(), &with(not_annual), &[], None), Err(refusal(1, not_annual)));
    // A :has needs its inner part seen inside the base, in the same areas:
    // seen on its own does not do.
    let has_badge = ".phr-mc-card:has(.badge-draft)";
    assert_eq!(check_seen(&base_only, &none(), &with(has_badge), &[], None), Err(refusal(1, has_badge)));
    let apart = map_with("Cycles", "/cycles", &[css(".phr-mc-card"), css(".badge-draft")]);
    assert_eq!(check_seen(&apart, &none(), &with(has_badge), &[], None), Err(refusal(1, has_badge)));
    let badge = map_with("Cycles", "/cycles", &[css(".phr-mc-card"), css(".phr-mc-card .header > .badge-draft")]);
    assert_eq!(check_seen(&badge, &none(), &with(has_badge), &[], None), Ok(()));
    // Or a seen chain with the badge inside the card.
    let mut chained = map_with("Cycles", "/cycles", &[css(".phr-mc-card")]);
    chained.areas[0].pages[0].elements.push(SeenElement {
        key: css(".badge-draft").seen_key().unwrap(),
        locator: Target::Chain(vec![css(".phr-mc-card"), css(".badge-draft")]),
        role: String::new(),
        name: String::new(),
        kind: "other".to_string(),
        required: false,
        seen_at: 0,
    });
    assert_eq!(check_seen(&chained, &none(), &with(has_badge), &[], None), Ok(()));
    let badge_elsewhere = joined(base_only.clone(), map_with("Payroll", "/payroll", &[css(".phr-mc-card .badge-draft")]));
    assert_eq!(check_seen(&badge_elsewhere, &none(), &with(has_badge), &[], None), Err(refusal(1, has_badge)));
    // An attribute :has: the base carrying the attribute itself is not a
    // descendant carrying it.
    let has_named = ".phr-mc-card:has([data-cycle-name*=\"Annual\"])";
    assert_eq!(check_seen(&carrying, &none(), &with(has_named), &[], None), Err(refusal(1, has_named)));
    let inside = map_with("Cycles", "/cycles", &[css(".phr-mc-card"), css(".phr-mc-card span[data-cycle-name=\"Annual 2026\"]")]);
    assert_eq!(check_seen(&inside, &none(), &with(has_named), &[], None), Ok(()));
    // A filter that never closes matches nothing.
    let open = ".phr-mc-card:not(.x";
    assert_eq!(check_seen(&badge, &none(), &with(open), &[], None), Err(refusal(1, open)));
}

#[test]
fn a_test_file_name_and_size_are_exempt() {
    use v2_lib::test_files::TestFile;
    let map = map_with("Policies", "/policies", &[role("button", "Attach")]);
    let files = vec![
        TestFile { name: "policy.docx".to_string(), size: 245_760, modified: String::new() },
        TestFile { name: "other.pdf".to_string(), size: 2_202_009, modified: String::new() },
    ];
    let upload = serde_json::json!({ "kind": "upload", "selector": { "role": "button", "name": "Attach" }, "file": "policy.docx" });
    let after_upload = |action: serde_json::Value| {
        script(
            Some("Policies"),
            serde_json::json!([
                { "step_number": 1, "actions": [upload.clone()] },
                { "step_number": 2, "actions": [action] }
            ]),
        )
    };
    let shows = |t: &str| serde_json::json!({ "kind": "expect_visible", "selector": { "text": t } });
    let named = |n: &str| click(serde_json::json!({ "role": "button", "name": n }));
    for action in [named("Download policy.docx"), named("policy.docx"), shows("240.0 KB"), shows("policy.docx (0.2 MB)")] {
        let s = after_upload(action.clone());
        assert_eq!(check_seen_with_files(&map, &none(), &s, &[], None, &files), Ok(()), "{action}");
    }
    // The name and the size need the Test files to say them.
    assert_eq!(check_seen(&map, &none(), &after_upload(shows("240.0 KB")), &[], None), Err(refusal(2, "text \"240.0 KB\"")));
    assert_eq!(
        check_seen(&map, &none(), &after_upload(named("Download policy.docx")), &[], None),
        Err(refusal(2, "button \"Download policy.docx\""))
    );
    // Not before the step that uploads it.
    let before = script(
        Some("Policies"),
        serde_json::json!([
            { "step_number": 1, "actions": [named("Download policy.docx")] },
            { "step_number": 2, "actions": [upload.clone()] }
        ]),
    );
    assert_eq!(
        check_seen_with_files(&map, &none(), &before, &[], None, &files),
        Err(refusal(1, "button \"Download policy.docx\""))
    );
    // An upload naming no Test file exempts nothing.
    let not_a_file = script(
        Some("Policies"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "upload", "selector": { "role": "button", "name": "Attach" }, "file": "Approve" }] },
            { "step_number": 2, "actions": [named("Approve")] }
        ]),
    );
    assert_eq!(check_seen_with_files(&map, &none(), &not_a_file, &[], None, &files), Err(refusal(2, "button \"Approve\"")));
    // A file the script does not upload, and its size, are not its own.
    for (action, what) in [
        (named("Download other.pdf"), "button \"Download other.pdf\""),
        (shows("2.1 MB"), "text \"2.1 MB\""),
        (shows("1240.0 KB"), "text \"1240.0 KB\""),
    ] {
        assert_eq!(check_seen_with_files(&map, &none(), &after_upload(action), &[], None, &files), Err(refusal(2, what)));
    }
}

#[test]
fn only_a_picked_date_is_exempt() {
    let map = map_with(
        "Leave",
        "/leave",
        &[role("textbox", "Start date"), role("dialog", "Choose date"), role("dialog", "Update record")],
    );
    let day = |name: &str| click(serde_json::json!({ "role": "button", "name": name }));
    let typed_then = |value: &str, action: serde_json::Value| {
        script(
            Some("Leave"),
            serde_json::json!([
                { "step_number": 1, "actions": [{ "kind": "fill", "selector": { "role": "textbox", "name": "Start date" }, "value": value }] },
                { "step_number": 2, "actions": [action] }
            ]),
        )
    };
    // Nothing picked or typed: an ordinary unseen button.
    assert_eq!(
        check_seen(&map, &none(), &one_step("Leave", serde_json::json!([day("15/01/2027")])), &[], None),
        Err(refusal(1, "button \"15/01/2027\""))
    );
    // Typed, in this form or another.
    for typed in ["15/01/2027", "2027-01-15", "15.1.2027"] {
        assert_eq!(check_seen(&map, &none(), &typed_then(typed, day("15/01/2027")), &[], None), Ok(()), "{typed}");
    }
    assert_eq!(
        check_seen(&map, &none(), &typed_then("16/01/2027", day("15/01/2027")), &[], None),
        Err(refusal(2, "button \"15/01/2027\""))
    );
    // Picked: a component's text input.
    let have = components(serde_json::json!([{
        "name": "Pick a day", "description": "d", "version": 1,
        "inputs": [{ "name": "day", "kind": "text", "description": "" }],
        "actions": [{ "kind": "click", "selector": { "role": "gridcell", "name": "{{day}}" } }]
    }]));
    let pick = |d: &str| {
        one_step("Leave", serde_json::json!([{ "kind": "use_component", "component": "Pick a day", "inputs": { "day": d } }]))
    };
    assert_eq!(check_seen(&map, &have, &pick("15/01/2027"), &[], None), Ok(()));
    assert_eq!(check_seen(&map, &have, &pick("Fifteen"), &[], None), Err(refusal(1, "gridcell \"Fifteen\"")));
    // Any dd/mm/yyyy day inside a date picker that was seen.
    let in_dialog = |dialog: &str, name: &str| {
        one_step("Leave", serde_json::json!([click(serde_json::json!([
            { "role": "dialog", "name": dialog }, { "role": "button", "name": name }
        ]))]))
    };
    assert_eq!(check_seen(&map, &none(), &in_dialog("Choose date", "15/01/2027"), &[], None), Ok(()));
    for (dialog, name) in [("Choose date", "Delete"), ("Choose date", "15/1/2027"), ("Choose date", "31/13/2027"), ("Update record", "15/01/2027")] {
        assert_eq!(
            check_seen(&map, &none(), &in_dialog(dialog, name), &[], None),
            Err(refusal(1, &format!("button \"{name}\" in dialog \"{dialog}\""))),
            "{dialog} {name}"
        );
    }
    // Only a day's role inside a picker's role: not a cell of a table
    // naming a date, nor a text.
    let mut tables = map.clone();
    tables.areas[0].pages[0].elements.push(SeenElement {
        key: role("table", "Due date").seen_key().unwrap(),
        locator: Target::One(role("table", "Due date")),
        role: "table".to_string(),
        name: "Due date".to_string(),
        kind: "table".to_string(),
        required: false,
        seen_at: 0,
    });
    let cell = one_step("Leave", serde_json::json!([click(serde_json::json!([
        { "role": "table", "name": "Due date" }, { "role": "cell", "name": "15/01/2027" }
    ]))]));
    assert_eq!(check_seen(&tables, &none(), &cell, &[], None), Err(refusal(1, "cell \"15/01/2027\" in table \"Due date\"")));
    let as_text = one_step("Leave", serde_json::json!([click(serde_json::json!([
        { "role": "dialog", "name": "Choose date" }, { "text": "15/01/2027" }
    ]))]));
    assert_eq!(check_seen(&map, &none(), &as_text, &[], None), Err(refusal(1, "text \"15/01/2027\" in dialog \"Choose date\"")));
    // The picker itself must have been seen.
    let unseen_picker = map_with("Leave", "/leave", &[]);
    assert_eq!(
        check_seen(&unseen_picker, &none(), &in_dialog("Choose date", "15/01/2027"), &[], None),
        Err(refusal(1, "button \"15/01/2027\" in dialog \"Choose date\""))
    );
}

#[test]
fn dashes_spaces_and_case_do_not_refuse_a_name() {
    let map = map_with(
        "Cycles",
        "/cycles",
        &[role("progressbar", "Step 1 of 9 \u{2013} Cycle Setup"), text("Step 2 of 9 \u{2014} Eval Rules")],
    );
    let bar = |n: &str| one_step("Cycles", serde_json::json!([{ "kind": "expect_visible", "selector": { "role": "progressbar", "name": n } }]));
    let says = |t: &str| one_step("Cycles", serde_json::json!([{ "kind": "expect_visible", "selector": { "text": t } }]));
    for n in ["Step 1 of 9 \u{2014} Cycle Setup", "step 1 of 9 - cycle  setup", "Step 1 of 9\u{2014}Cycle Setup", " STEP 1 OF 9 -CYCLE SETUP"] {
        assert_eq!(check_seen(&map, &none(), &bar(n), &[], None), Ok(()), "{n}");
    }
    for t in ["Step 2 of 9 - Eval Rules", "step 2 of 9 \u{2013} eval   rules"] {
        assert_eq!(check_seen(&map, &none(), &says(t), &[], None), Ok(()), "{t}");
    }
    assert_eq!(
        check_seen(&map, &none(), &bar("Step 1 of 8 - Cycle Setup"), &[], None),
        Err(refusal_hinted(
            1,
            "progressbar \"Step 1 of 8 - Cycle Setup\"",
            "did you mean progressbar \"Step 1 of 9 \u{2013} Cycle Setup\"?"
        ))
    );
}

#[test]
fn the_suggestion_comes_from_the_same_area() {
    let map = joined(
        map_with("Ratings", "/ratings", &[role("button", "Publish"), role("link", "Publsh"), text("Step 1 of 9 \u{2013} Cycle Setup")]),
        map_with("Payroll", "/payroll", &[role("button", "Publsh!"), role("button", "Archive it")]),
    );
    let in_area = |area: &str, selector: serde_json::Value| one_step(area, serde_json::json!([click(selector)]));
    let button = |n: &str| serde_json::json!({ "role": "button", "name": n });
    // Only the same role, though another role has a closer name; never Payroll's.
    assert_eq!(
        check_seen(&map, &none(), &in_area("Ratings", button("Publsh")), &[], None),
        Err(refusal_hinted(1, "button \"Publsh\"", "did you mean button \"Publish\"?"))
    );
    // Never another role: with no same-role name close, nothing is offered.
    assert_eq!(
        check_seen(&map, &none(), &in_area("Ratings", serde_json::json!({ "role": "progressbar", "name": "Step 1 of 9 - Cycle Setup" })), &[], None),
        Err(refusal(1, "progressbar \"Step 1 of 9 - Cycle Setup\""))
    );
    // Payroll's names are close, but not in this script's areas.
    assert_eq!(
        check_seen(&map, &none(), &in_area("Ratings", button("Archive")), &[], None),
        Err(refusal(1, "button \"Archive\""))
    );
    assert_eq!(check_seen(&map, &none(), &in_area("Leave", button("Publsh")), &[], None), Err(refusal(1, "button \"Publsh\"")));
    // A quote in the seen name is escaped.
    let quoted = map_with("Ratings", "/ratings", &[role("button", "Say \"hi\"")]);
    assert_eq!(
        check_seen(&quoted, &none(), &in_area("Ratings", button("Say \"hl\"")), &[], None),
        Err(refusal_hinted(1, "button \"Say \"hl\"\"", "did you mean button \"Say \\\"hi\\\"\"?"))
    );
    // An import lists it too.
    let hinted = check_seen_all_hinted(&map, &none(), &in_area("Ratings", button("Publsh")), &[], None, &[]);
    assert_eq!(hinted.len(), 1);
    assert_eq!(hinted[0].1.as_deref(), Some("did you mean button \"Publish\"?"));
    // And a component's save.
    let actions: Vec<v2_lib::browser::actions::Action> =
        serde_json::from_value(serde_json::json!([click(button("Publsh"))])).unwrap();
    assert_eq!(
        check_component_seen(&map, Some("Ratings"), &actions),
        Err("Action 1: button \"Publsh\" was never seen on the live app; did you mean button \"Publish\"? Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again.".to_string())
    );
    assert_eq!(
        check_component_seen(&map, Some("Leave"), &actions),
        Err("Action 1: button \"Publsh\" was never seen on the live app. Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again.".to_string())
    );
}

/// None of the above lets through a locator unseen in every respect.
#[test]
fn an_unseen_locator_is_still_refused() {
    let map = map_with("Cycles", "/cycles", &[role("button", "Save"), css(".phr-mc-card")]);
    for (selector, what) in [
        (serde_json::json!({ "role": "button", "name": "Ghost" }), "button \"Ghost\""),
        (serde_json::json!({ "css": ".nowhere:checked" }), ".nowhere:checked"),
        (serde_json::json!({ "css": ".nowhere:not(.phr-mc-card)" }), ".nowhere:not(.phr-mc-card)"),
        (serde_json::json!({ "css": "div[data-cycle-id=\"{{setup.cycle_id}}\"]" }), "div[data-cycle-id=\"{{setup.cycle_id}}\"]"),
        (serde_json::json!({ "role": "dialog", "name": "{{setup.cycle_name}}" }), "dialog \"{{setup.cycle_name}}\""),
        (serde_json::json!({ "text": "240.0 KB" }), "text \"240.0 KB\""),
        (serde_json::json!({ "role": "button", "name": "15/01/2027" }), "button \"15/01/2027\""),
        (serde_json::json!({ "text": "report.pdf" }), "text \"report.pdf\""),
    ] {
        let s = one_step("Cycles", serde_json::json!([click(selector)]));
        assert_eq!(check_seen(&map, &none(), &s, &[], None), Err(refusal(1, what)), "{what}");
    }
}

/// A placeholder a step writes into its own locator is checked once the
/// run fills it in, like a component input: by the seen value's shape.
#[test]
fn a_placeholder_in_a_scripts_own_locator_is_checked_when_filled() {
    let map = map_with(
        "Cycles",
        "/cycles",
        &[
            css("div[data-cycle-id=\"10066\"]"),
            css("button[aria-label^=\"Edit\"]"),
            css(".phr-mc-card"),
            css(".phr-mc-card[data-cycle-id=\"10066\"]"),
            role("button", "Edit Cycle 12"),
        ],
    );
    let saved = one_step("Cycles", serde_json::json!([click(edit_in_card("{{setup.cycle_id}}"))]));
    assert_eq!(check_seen(&map, &none(), &saved, &[], None), Ok(()));
    assert!(has_data_placeholders(&saved.steps));
    let run = |saved: &CaseScript, filled: &CaseScript| check_resolved_inputs(&map, &none(), &["Cycles"], &saved.steps, &filled.steps);
    let filled = |id: &str| one_step("Cycles", serde_json::json!([click(edit_in_card(id))]));
    assert_eq!(run(&saved, &filled("10071")), Ok(()));
    let blocked = |id: &str| {
        format!(
            "Step 1: button[aria-label^=\"Edit\"] in div[data-cycle-id=\"{id}\"], as filled in, does not fit what was seen on the live app. Explore that screen again with discovery, or check the value the setup or fixture gives."
        )
    };
    assert_eq!(run(&saved, &filled("draft-7")), Err(blocked("draft-7")));
    assert_eq!(run(&saved, &filled("{{setup.cycle_id}}")), Err(blocked("{{setup.cycle_id}}")), "never filled in");
    // Inside a :not filter too.
    let not_this = |id: &str| {
        one_step("Cycles", serde_json::json!([click(serde_json::json!({ "css": format!(".phr-mc-card:not([data-cycle-id=\"{id}\"])") }))]))
    };
    let saved_not = not_this("{{setup.cycle_id}}");
    assert_eq!(check_seen(&map, &none(), &saved_not, &[], None), Ok(()));
    assert_eq!(run(&saved_not, &not_this("10071")), Ok(()));
    assert_eq!(
        run(&saved_not, &not_this("x7")),
        Err("Step 1: .phr-mc-card:not([data-cycle-id=\"x7\"]), as filled in, does not fit what was seen on the live app. Explore that screen again with discovery, or check the value the setup or fixture gives.".to_string())
    );
    // In a name.
    let named = |n: &str| one_step("Cycles", serde_json::json!([click(serde_json::json!({ "role": "button", "name": n }))]));
    let saved_name = named("Edit Cycle {{setup.cycle_no}}");
    assert_eq!(run(&saved_name, &named("Edit Cycle 40")), Ok(()));
    assert_eq!(
        run(&saved_name, &named("Edit Cycle forty")),
        Err("Step 1: button \"Edit Cycle forty\", as filled in, does not fit what was seen on the live app. Explore that screen again with discovery, or check the value the setup or fixture gives.".to_string())
    );
    // A locator with no placeholder has nothing to check.
    let plain = one_step("Cycles", serde_json::json!([click(edit_in_card("10066"))]));
    assert!(!has_data_placeholders(&plain.steps));
}

/// A placeholder inside an id or class token, beside a literal part of it,
/// stands for that token's seen characters: a fresh setup draft's id is
/// never seen exactly, but its shape is.
#[test]
fn a_placeholder_inside_an_id_or_class_keeps_the_seen_tokens_shape() {
    let map = map_with("Cycles", "/cycles", &[css("#c274"), css(".row-alpha")]);
    let click_css = |c: &str| one_step("Cycles", serde_json::json!([click(serde_json::json!({ "css": c }))]));
    assert_eq!(check_seen(&map, &none(), &click_css("#c{{setup.cycle_id}}"), &[], None), Ok(()));
    assert_eq!(check_seen(&map, &none(), &click_css(".row-{{fixture.rows.name}}"), &[], None), Ok(()));
    // The literal part must be as seen, and the kind of token.
    for c in ["#d{{setup.cycle_id}}", ".c{{setup.cycle_id}}", "#c{{setup.cycle_id}}x", "#{{setup.x}}", ".{{setup.x}}"] {
        assert_eq!(check_seen(&map, &none(), &click_css(c), &[], None), Err(refusal(1, c)), "{c}");
    }
    // At run time, the filled value keeps the seen token's shape.
    let saved = click_css("#c{{setup.cycle_id}}");
    let run = |filled: &str| check_resolved_inputs(&map, &none(), &["Cycles"], &saved.steps, &click_css(filled).steps);
    assert_eq!(run("#c10071"), Ok(()));
    for filled in ["#c10071 .x", "#cdraft-7", "#c10071.x", "#c10071#y"] {
        assert_eq!(
            run(filled),
            Err(format!("Step 1: {filled}, as filled in, does not fit what was seen on the live app. Explore that screen again with discovery, or check the value the setup or fixture gives.")),
            "{filled}"
        );
    }
    // A class seen with letters takes any token characters, but nothing else.
    let saved = click_css(".row-{{fixture.rows.name}}");
    let run = |filled: &str| check_resolved_inputs(&map, &none(), &["Cycles"], &saved.steps, &click_css(filled).steps);
    assert_eq!(run(".row-beta_2"), Ok(()));
    assert_eq!(
        run(".row-beta > .x"),
        Err("Step 1: .row-beta > .x, as filled in, does not fit what was seen on the live app. Explore that screen again with discovery, or check the value the setup or fixture gives.".to_string())
    );
}

/// A placeholder is found in the name as written, the way the run finds
/// it: one the run would not fill (`{{Setup.x}}`) is literal text, never a
/// wildcard.
#[test]
fn a_placeholder_the_run_would_not_fill_is_literal_text() {
    let map = map_with("Cycles", "/cycles", &[role("button", "Save"), text("Saved")]);
    let with = |selector: serde_json::Value| one_step("Cycles", serde_json::json!([click(selector)]));
    for (selector, what) in [
        (serde_json::json!({ "role": "button", "name": "{{Setup.x}}" }), "button \"{{Setup.x}}\""),
        (serde_json::json!({ "role": "button", "name": "{{SETUP.x}}" }), "button \"{{SETUP.x}}\""),
        (serde_json::json!({ "text": "{{FIXTURE.a.b}}" }), "text \"{{FIXTURE.a.b}}\""),
    ] {
        let s = with(selector);
        assert!(!has_data_placeholders(&s.steps), "{what}");
        assert_eq!(check_seen(&map, &none(), &s, &[], None), Err(refusal(1, what)), "{what}");
    }
    // As the run writes it, it is a wildcard as before.
    assert_eq!(check_seen(&map, &none(), &with(serde_json::json!({ "role": "button", "name": "{{setup.x}}" })), &[], None), Ok(()));
    assert_eq!(check_seen(&map, &none(), &with(serde_json::json!({ "text": "{{fixture.a.b}}" })), &[], None), Ok(()));
}

/// A date picker is known by what was seen of it, never by a word a
/// placeholder holds.
#[test]
fn a_placeholder_never_names_a_date_picker() {
    let map = map_with("Leave", "/leave", &[role("dialog", "Leave request"), css("div.panel[data-x=\"abc\"]")]);
    for (picker, describe) in [
        (serde_json::json!({ "role": "dialog", "name": "{{setup.date}}" }), "dialog \"{{setup.date}}\""),
        (serde_json::json!({ "role": "dialog", "name": "{{setup.calendar}}" }), "dialog \"{{setup.calendar}}\""),
        (serde_json::json!({ "css": "div.panel[data-x=\"{{setup.calendar}}\"]" }), "div.panel[data-x=\"{{setup.calendar}}\"]"),
    ] {
        let s = one_step("Leave", serde_json::json!([click(serde_json::json!([picker, { "role": "button", "name": "15/01/2027" }]))]));
        assert_eq!(
            check_seen(&map, &none(), &s, &[], None),
            Err(refusal(1, &format!("button \"15/01/2027\" in {describe}"))),
            "{describe}"
        );
    }
}

/// A filled value cannot leave the quotes or the bracket it was put in,
/// and an escaped quote never opens or closes a value.
#[test]
fn a_filled_value_cannot_break_out_of_its_quotes() {
    let map = map_with("Cycles", "/cycles", &[css("[x=\"abc\"]"), css("[y=\\\"abc\\\"]")]);
    let at = |c: &str| one_step("Cycles", serde_json::json!([click(serde_json::json!({ "css": c }))]));
    let saved = at("[x=\"{{setup.v}}\"]");
    assert_eq!(check_seen(&map, &none(), &saved, &[], None), Ok(()));
    let run = |filled: &str| check_resolved_inputs(&map, &none(), &["Cycles"], &saved.steps, &at(filled).steps);
    assert_eq!(run("[x=\"ok-1\"]"), Ok(()));
    // Inside real quotes a space, a comma or a `]` is part of the value:
    // the whole of `a], .evil, [y=` is one attribute value, not a list.
    for filled in ["[x=\"a b\"]", "[x=\"a,b\"]", "[x=\"a], .evil, [y=\"]"] {
        assert_eq!(run(filled), Ok(()), "{filled}");
    }
    for filled in ["[x=\"a\\\\\"]", "[x=\"a\\\"b\"]", "[x=\"a\nb\"]"] {
        assert_eq!(
            run(filled),
            Err(format!("Step 1: {filled}, as filled in, does not fit what was seen on the live app. Explore that screen again with discovery, or check the value the setup or fixture gives.")),
            "{filled}"
        );
    }
    // Outside a real quote (here escaped), the placeholder is literal.
    let escaped = "[y=\\\"{{setup.v}}\\\"]";
    assert_eq!(check_seen(&map, &none(), &at(escaped), &[], None), Err(refusal(1, escaped)));
}

/// What a save refused in discovery checks on the page: every locator the
/// check refuses as unseen, in step order, and none at all when it also
/// refuses a page address, which no probe can find.
#[test]
fn unseen_targets_name_every_unseen_locator_or_none_when_a_page_is_refused() {
    let map = map_with("Ratings", "/ratings", &[role("button", "Save")]);
    let two = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Save" } }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Publish" } }] },
            { "step_number": 3, "actions": [{ "kind": "click", "selector": { "css": "#pager-2" } }] }
        ]),
    );
    let found = unseen_targets(&map, &none(), &two, &[], None, &[]).expect("only locators were refused");
    let named: Vec<String> = found.iter().map(Target::describe).collect();
    assert_eq!(named, vec!["button \"Publish\"".to_string(), "#pager-2".to_string()]);

    let seen = script(
        Some("Ratings"),
        serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Save" } }] }]),
    );
    assert_eq!(unseen_targets(&map, &none(), &seen, &[], None, &[]), Some(Vec::new()));

    let away = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://app.example/payroll" }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": { "css": "#pager-2" } }] }
        ]),
    );
    assert_eq!(unseen_targets(&map, &none(), &away, &[], None, &[]), None);
}

/// The same for a component's own locators: its unseen ones, and none at
/// all beside a page it never saw. The first refusal is still the check's.
#[test]
fn unseen_component_targets_name_its_unseen_locators_or_none_when_a_page_is_refused() {
    let map = map_with("Ratings", "/ratings", &[role("button", "Save")]);
    let actions: Vec<v2_lib::browser::actions::Action> = serde_json::from_value(serde_json::json!([
        { "kind": "click", "selector": { "role": "button", "name": "Save" } },
        { "kind": "click", "selector": { "css": "#pager-2" } },
        { "kind": "click", "selector": { "css": "#pager-3" } }
    ]))
    .unwrap();
    let found = unseen_component_targets(&map, Some("Ratings"), &actions).expect("only locators were refused");
    let named: Vec<String> = found.iter().map(Target::describe).collect();
    assert_eq!(named, vec!["#pager-2".to_string(), "#pager-3".to_string()]);
    let first = check_component_seen(&map, Some("Ratings"), &actions).unwrap_err();
    assert!(first.starts_with("Action 2: #pager-2 was never seen"), "{first}");

    let away: Vec<v2_lib::browser::actions::Action> = serde_json::from_value(serde_json::json!([
        { "kind": "navigate", "url": "https://app.example/payroll" },
        { "kind": "click", "selector": { "css": "#pager-2" } }
    ]))
    .unwrap();
    assert_eq!(unseen_component_targets(&map, Some("Ratings"), &away), None);
}

// ---- the script's own data, at save and at run time ----

/// A check of a text holding a placeholder that only the case's text
/// would let through is refused at save: the run has no case text to let
/// it through again, so it would be Blocked on every run.
#[test]
fn a_placeholder_link_passed_only_by_the_case_text_is_refused_at_save() {
    let map = map_with("Cycles", "/cycles", &[role("button", "Save")]);
    let expect = one_step(
        "Cycles",
        serde_json::json!([{ "kind": "expect_visible", "selector": { "text": "Draft {{setup.cycle_name}} Pending" } }]),
    );
    let case_text = vec!["Draft {{setup.cycle_name}} Pending is listed".to_string()];
    assert_eq!(
        check_seen(&map, &none(), &expect, &case_text, None),
        Err(refusal(1, "text \"Draft {{setup.cycle_name}} Pending\""))
    );
    // Without a placeholder the case's text still exempts a check.
    let plain = one_step("Cycles", serde_json::json!([{ "kind": "expect_visible", "selector": { "text": "Draft cycle Pending" } }]));
    assert_eq!(check_seen(&map, &none(), &plain, &["Draft cycle Pending is listed".to_string()], None), Ok(()));
}

/// A link the save let through as the script's own data runs: the value
/// typed earlier, filled in, is in the filled link; an uploaded Test file
/// is too. What the run left unfilled, or a value nobody typed, still
/// Blocks.
#[test]
fn a_placeholder_link_that_is_the_scripts_own_data_passes_at_run_time() {
    use v2_lib::autorun::seen_check::check_resolved_inputs_with;
    use v2_lib::test_files::TestFile;
    let map = map_with("Cycles", "/cycles", &[css("#name"), role("button", "Attach")]);
    let steps = |name: &str, shown: &str| {
        script(
            Some("Cycles"),
            serde_json::json!([
                { "step_number": 1, "actions": [{ "kind": "fill", "selector": { "css": "#name" }, "value": name }] },
                { "step_number": 2, "actions": [{ "kind": "expect_visible", "selector": { "text": shown } }] }
            ]),
        )
    };
    let saved = steps("{{setup.cycle_name}}", "Draft {{setup.cycle_name}} Pending");
    assert_eq!(check_seen(&map, &none(), &saved, &[], None), Ok(()));
    let run = |filled: &CaseScript| check_resolved_inputs_with(&map, &none(), &["Cycles"], &[], &saved.steps, &filled.steps, &[]);
    assert_eq!(run(&steps("AUTOTEST cycle", "Draft AUTOTEST cycle Pending")), Ok(()));
    // The earlier step can also come in as `before`, the way a supervised
    // step is checked on its own.
    assert_eq!(
        check_resolved_inputs_with(
            &map,
            &none(),
            &["Cycles"],
            &steps("AUTOTEST cycle", "x").steps[..1],
            &saved.steps[1..],
            &steps("AUTOTEST cycle", "Draft AUTOTEST cycle Pending").steps[1..],
            &[]
        ),
        Ok(())
    );
    // Without it, nothing typed exempts the link.
    assert!(check_resolved_inputs(&map, &none(), &["Cycles"], &saved.steps[1..], &steps("AUTOTEST cycle", "Draft AUTOTEST cycle Pending").steps[1..])
        .unwrap_err()
        .starts_with("Step 2: text \"Draft AUTOTEST cycle Pending\", as filled in, does not fit"));
    // A value the run left unfilled is never the script's own.
    assert!(run(&steps("{{setup.cycle_name}}", "Draft {{setup.cycle_name}} Pending")).is_err());

    // An uploaded Test file, named by a placeholder in the file and the row.
    let files = vec![TestFile { name: "policy.docx".to_string(), size: 245_760, modified: String::new() }];
    let upload = |file: &str, row: &str| {
        script(
            Some("Cycles"),
            serde_json::json!([{ "step_number": 1, "actions": [
                { "kind": "upload", "selector": { "role": "button", "name": "Attach" }, "file": file },
                { "kind": "expect_visible", "selector": { "role": "row", "name": row } }
            ] }]),
        )
    };
    let saved = upload("{{setup.file}}", "{{setup.file}} attached");
    let run = |filled: &CaseScript, files: &[TestFile]| {
        check_resolved_inputs_with(&map, &none(), &["Cycles"], &[], &saved.steps, &filled.steps, files)
    };
    assert_eq!(run(&upload("policy.docx", "policy.docx attached"), &files), Ok(()));
    assert!(run(&upload("policy.docx", "policy.docx attached"), &[]).is_err(), "not one of the project's Test files");
}

// ------------------------------------------ a saved script's own links

use v2_lib::autorun::seen_check::add_saved_scripts;

fn expect_bar(name: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "expect_visible", "selector": { "role": "progressbar", "name": name } })
}

/// A link a saved script of the area uses, written as it is there, is
/// seen though the map has let it go: the saved script being replaced
/// counts too. Another name is not, and a saved placeholder widens
/// nothing.
#[test]
fn a_locator_used_by_a_saved_script_of_the_area_is_seen() {
    use v2_lib::autorun::seen_check::load_checked_map;
    let dir = tempfile::tempdir().unwrap();
    let eval = "Step 2 of 9 \u{2014} Eval Rules";
    let open = serde_json::json!({ "role": "button", "name": "{{setup.cycle_name}} Open" });
    // Case 7 as a checked save of this project left it: the script about
    // to be replaced.
    let mut saved = one_step("Cycles", serde_json::json!([expect_bar(eval), click(open.clone())]));
    saved.organization = Some("Acme".to_string());
    saved.project = Some("Web".to_string());
    saved.checked = true;
    v2_lib::autorun::store::save_script(dir.path(), &saved).unwrap();
    let map = load_checked_map(dir.path(), "Acme", "Web").unwrap();

    let again = one_step("Cycles", serde_json::json!([expect_bar(eval)]));
    assert_eq!(check_seen(&map, &none(), &again, &[], None), Ok(()));
    let other = one_step("Cycles", serde_json::json!([expect_bar("Step 3 of 9 \u{2014} Eval Rules")]));
    assert!(check_seen(&map, &none(), &other, &[], None).is_err());
    let placeholder = one_step("Cycles", serde_json::json!([click(open)]));
    assert!(check_seen(&map, &none(), &placeholder, &[], None).is_err(), "a saved placeholder widened the check");
    // Nothing is written to the map.
    assert!(!v2_lib::autorun::discovery_map::map_path(dir.path(), "Acme", "Web").exists());

    // Another case of the area, checked against a map built in memory.
    let mut map = DiscoveryMap::default();
    let mut eleventh = one_step("cycles", serde_json::json!([expect_bar(eval)]));
    eleventh.case_id = 11;
    add_saved_scripts(&mut map, &[eleventh]);
    assert_eq!(check_seen(&map, &none(), &again, &[], None), Ok(()));
}

/// Only the area's own scripts count: a link another area's script uses,
/// or a script with no area, is still never seen here.
#[test]
fn a_locator_from_another_areas_script_is_not_seen() {
    let eval = "Step 2 of 9 \u{2014} Eval Rules";
    let mut payroll = one_step("Payroll", serde_json::json!([expect_bar(eval)]));
    payroll.case_id = 21;
    let mut nowhere = script(None, serde_json::json!([{ "step_number": 1, "actions": [expect_bar(eval)] }]));
    nowhere.case_id = 22;
    let mut map = map_with("Cycles", "/cycles", &[]);
    add_saved_scripts(&mut map, &[payroll, nowhere]);
    let here = one_step("Cycles", serde_json::json!([expect_bar(eval)]));
    assert_eq!(
        check_seen(&map, &none(), &here, &[], None),
        Err(refusal(1, "progressbar \"Step 2 of 9 \u{2014} Eval Rules\""))
    );
    let there = one_step("Payroll", serde_json::json!([expect_bar(eval)]));
    assert_eq!(check_seen(&map, &none(), &there, &[], None), Ok(()));
}

/// "Did you mean" offers only a name in the refused locator's own role: a
/// text offers text, a progressbar a progressbar, and nothing when no name
/// in that role is close.
#[test]
fn did_you_mean_keeps_to_the_same_role() {
    let map = map_with("Cycles", "/cycles", &[role("progressbar", "Step 3 of 9"), text("Step 4 of 9")]);
    let says = |t: &str| one_step("Cycles", serde_json::json!([{ "kind": "expect_visible", "selector": { "text": t } }]));
    assert_eq!(
        check_seen(&map, &none(), &says("Step 2 of 9"), &[], None),
        Err(refusal_hinted(1, "text \"Step 2 of 9\"", "did you mean text \"Step 4 of 9\"?"))
    );
    assert_eq!(
        check_seen(&map, &none(), &one_step("Cycles", serde_json::json!([expect_bar("Step 2 of 9")])), &[], None),
        Err(refusal_hinted(1, "progressbar \"Step 2 of 9\"", "did you mean progressbar \"Step 3 of 9\"?"))
    );
    let only_text = map_with("Cycles", "/cycles", &[text("Step 4 of 9")]);
    assert_eq!(
        check_seen(&only_text, &none(), &one_step("Cycles", serde_json::json!([expect_bar("Step 2 of 9")])), &[], None),
        Err(refusal(1, "progressbar \"Step 2 of 9\""))
    );
    let only_bar = map_with("Cycles", "/cycles", &[role("progressbar", "Step 3 of 9")]);
    assert_eq!(check_seen(&only_bar, &none(), &says("Step 2 of 9"), &[], None), Err(refusal(1, "text \"Step 2 of 9\"")));
}

/// "Did you mean" prefers a name seen on the page the refused step's area
/// arrives on (`add_area_pages`) over a closer one seen only on another
/// page, an element's page and a kept sighting's alike; still only in the
/// same role. With the area's page unknown it offers the fewest edits.
#[test]
fn did_you_mean_prefers_the_steps_area_page() {
    use v2_lib::autorun::discovery_map::Sighting;
    use v2_lib::autorun::seen_check::add_area_pages;
    let nav: v2_lib::autorun::nav::NavFile = serde_json::from_value(serde_json::json!({
        "modules": [{ "area": "Cycles", "module": "PE", "clicks": [], "arrived": "/cycles/edit/12", "recorded": "2026-10-10T00:00:00Z" }]
    }))
    .unwrap();
    let says = |t: &str| one_step("Cycles", serde_json::json!([{ "kind": "expect_visible", "selector": { "text": t } }]));
    let hinted = |hint: &str| Err(refusal_hinted(1, "text \"Step 2 of 9\"", hint));

    // Elements on pages.
    let mut map = joined(
        map_with("Cycles", "/dashboard", &[text("Step 2 of 99")]),
        map_with("Cycles", "/cycles/edit/77", &[text("Step 4 of 99"), role("progressbar", "Step 2 of 9x")]),
    );
    assert_eq!(check_seen(&map, &none(), &says("Step 2 of 9"), &[], None), hinted("did you mean text \"Step 2 of 99\"?"));
    add_area_pages(&mut map, &nav);
    assert_eq!(check_seen(&map, &none(), &says("Step 2 of 9"), &[], None), hinted("did you mean text \"Step 4 of 99\"?"));

    // Kept sightings, by the page each was last seen on.
    let sighting = |t: &str, page: &str| Sighting { key: text(t).seen_key().unwrap(), link: text(t), page: page.to_string(), last_seen: 1 };
    let mut kept = DiscoveryMap {
        areas: vec![AreaMap {
            area: "Cycles".into(),
            sightings: vec![sighting("Step 2 of 99", "/dashboard"), sighting("Step 4 of 99", "/cycles/edit/:id")],
            ..AreaMap::default()
        }],
    };
    assert_eq!(check_seen(&kept, &none(), &says("Step 2 of 9"), &[], None), hinted("did you mean text \"Step 2 of 99\"?"));
    add_area_pages(&mut kept, &nav);
    assert_eq!(check_seen(&kept, &none(), &says("Step 2 of 9"), &[], None), hinted("did you mean text \"Step 4 of 99\"?"));

    // Nothing close on the page: the closest elsewhere is still offered.
    let mut elsewhere = joined(map_with("Cycles", "/dashboard", &[text("Step 2 of 99")]), map_with("Cycles", "/cycles/edit/1", &[text("Totally different")]));
    add_area_pages(&mut elsewhere, &nav);
    assert_eq!(check_seen(&elsewhere, &none(), &says("Step 2 of 9"), &[], None), hinted("did you mean text \"Step 2 of 99\"?"));
}

// ------------------------------------- who vouches for a saved locator

const EVAL: &str = "Step 2 of 9 \u{2014} Eval Rules";

/// A script of `area` that looks for the Eval Rules bar, as case `id`.
fn eval_script(id: i32, area: &str) -> CaseScript {
    let mut s = one_step(area, serde_json::json!([expect_bar(EVAL)]));
    s.case_id = id;
    s
}

/// Saved as a checked save (the assistant's, or an import) of `org`/`project`.
fn save_checked(root: &std::path::Path, id: i32, area: &str, org: &str, project: &str) {
    let mut s = eval_script(id, area);
    s.organization = Some(org.to_string());
    s.project = Some(project.to_string());
    s.checked = true;
    v2_lib::autorun::store::save_script(root, &s).unwrap();
}

/// Records area `name` in `org`/`project`, as the Areas dialog does.
fn record_area(root: &std::path::Path, org: &str, project: &str, name: &str) {
    v2_lib::autorun::nav::put_path(
        root,
        org,
        project,
        v2_lib::autorun::nav::ModulePath {
            area: name.to_string(),
            module: name.to_string(),
            clicks: vec![Target::One(role("link", name))],
            arrived: "/cycles".to_string(),
            recorded: "2026-10-10T10:00:00Z".to_string(),
            start: String::new(),
            made_by: v2_lib::autorun::nav::MadeBy::Person,
        },
    )
    .unwrap();
}

/// Is the Eval Rules bar seen for a new script of `area` in `org`/`project`?
fn eval_seen(root: &std::path::Path, org: &str, project: &str, area: &str) -> bool {
    let map = v2_lib::autorun::seen_check::load_checked_map(root, org, project).unwrap();
    check_seen(&map, &none(), &eval_script(99, area), &[], None).is_ok()
}

/// Two projects with an area of the same name: a checked script of one
/// vouches nowhere in the other, and adds no area to its map. The project
/// compares as its files are named, ignoring case and spaces at the ends.
#[test]
fn two_projects_sharing_an_area_name_do_not_share_seen_locators() {
    let dir = tempfile::tempdir().unwrap();
    save_checked(dir.path(), 7, "Cycles", "Acme", "Web");
    assert!(eval_seen(dir.path(), "Acme", "Web", "Cycles"));
    assert!(eval_seen(dir.path(), " acme", "WEB ", "Cycles"));
    assert!(!eval_seen(dir.path(), "Acme", "Mobile", "Cycles"));
    assert!(!eval_seen(dir.path(), "Other", "Web", "Cycles"));
    let map = v2_lib::autorun::seen_check::load_checked_map(dir.path(), "Acme", "Mobile").unwrap();
    assert!(map.areas.is_empty(), "another project's script made an area");
}

/// A script saved from the editor ran no seen check: its links vouch for
/// nothing, even in its own project and area.
#[test]
fn an_editor_saved_script_does_not_vouch() {
    let dir = tempfile::tempdir().unwrap();
    record_area(dir.path(), "Acme", "Web", "Cycles");
    v2_lib::commands::autorun::save_script_from_editor(dir.path(), "Acme", "Web", eval_script(7, "Cycles")).unwrap();
    let saved = v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!((saved.organization.as_deref(), saved.project.as_deref(), saved.checked), (Some("Acme"), Some("Web"), false));
    assert!(!eval_seen(dir.path(), "Acme", "Web", "Cycles"));
}

/// A script that once vouched stops when a person saves it from the
/// editor, whatever the editor sends.
#[test]
fn an_editor_save_of_a_checked_script_removes_its_vouching() {
    let dir = tempfile::tempdir().unwrap();
    record_area(dir.path(), "Acme", "Web", "Cycles");
    save_checked(dir.path(), 7, "Cycles", "Acme", "Web");
    assert!(eval_seen(dir.path(), "Acme", "Web", "Cycles"));
    let mut sent = v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap();
    sent.checked = true;
    v2_lib::commands::autorun::save_script_from_editor(dir.path(), "Acme", "Web", sent).unwrap();
    assert!(!v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap().checked);
    assert!(!eval_seen(dir.path(), "Acme", "Web", "Cycles"));
}

/// A script saved before scripts were stamped names no project: it counts
/// only while exactly one project has recorded areas, and that project is
/// the one being checked.
#[test]
fn legacy_unstamped_scripts_count_only_when_one_project_has_areas() {
    let dir = tempfile::tempdir().unwrap();
    v2_lib::autorun::store::save_script(dir.path(), &eval_script(7, "Cycles")).unwrap();
    assert!(!eval_seen(dir.path(), "Acme", "Web", "Cycles"), "no project has areas");
    record_area(dir.path(), "Acme", "Web", "Cycles");
    assert!(eval_seen(dir.path(), "Acme", "Web", "Cycles"));
    assert!(eval_seen(dir.path(), "ACME", "web", "Cycles"));
    assert!(!eval_seen(dir.path(), "Acme", "Mobile", "Cycles"), "not the project with areas");
    record_area(dir.path(), "Acme", "Mobile", "Cycles");
    assert!(!eval_seen(dir.path(), "Acme", "Web", "Cycles"), "two projects have areas");
    assert!(!eval_seen(dir.path(), "Acme", "Mobile", "Cycles"));
}

/// An areas file that cannot be read could be any project's, so scripts
/// saved before the stamp count nowhere while one is there, even beside
/// the one project with areas, or as that project's own file.
#[test]
fn a_corrupt_areas_file_turns_the_legacy_rule_off() {
    let dir = tempfile::tempdir().unwrap();
    v2_lib::autorun::store::save_script(dir.path(), &eval_script(7, "Cycles")).unwrap();
    record_area(dir.path(), "Acme", "Web", "Cycles");
    assert!(eval_seen(dir.path(), "Acme", "Web", "Cycles"));
    let stray = dir.path().join("projects").join("someone-else.nav.json");
    std::fs::write(&stray, "{ not json").unwrap();
    assert!(!eval_seen(dir.path(), "Acme", "Web", "Cycles"), "a corrupt areas file beside it");
    std::fs::remove_file(&stray).unwrap();
    assert!(eval_seen(dir.path(), "Acme", "Web", "Cycles"));
    std::fs::write(v2_lib::autorun::nav::nav_path(dir.path(), "Acme", "Web"), "{ not json").unwrap();
    assert!(!eval_seen(dir.path(), "Acme", "Web", "Cycles"), "its own areas file is corrupt");
}

/// What a save that keeps steps of an earlier script unchecked carries
/// over: a checked script of this project vouches still; one saved before
/// the stamp only when the legacy rule holds; anything else does not.
#[test]
fn an_unstamped_script_carries_over_only_under_the_legacy_rule() {
    use v2_lib::autorun::seen_check::vouch_carries_over;
    let legacy = eval_script(7, "Cycles");
    assert!(vouch_carries_over(&legacy, "Acme", "Web", true));
    assert!(!vouch_carries_over(&legacy, "Acme", "Web", false));
    let mut checked = eval_script(7, "Cycles");
    checked.organization = Some("Acme".into());
    checked.project = Some("Web".into());
    checked.checked = true;
    assert!(vouch_carries_over(&checked, "acme", "web", false));
    assert!(!vouch_carries_over(&checked, "Acme", "Mobile", true));
    let editor = CaseScript { checked: false, ..checked };
    assert!(!vouch_carries_over(&editor, "Acme", "Web", true));
}

// ---- a refused save names every problem at once

/// A refused save names every unseen locator in step order, then action
/// order, one line each, each with its own "did you mean" of the same role
/// only: never a link for a button, never a button for a text.
#[test]
fn a_refusal_lists_every_unseen_locator_in_order() {
    let map = map_with(
        "Ratings",
        "/ratings",
        &[role("button", "Publish"), role("button", "Save"), role("link", "Archive"), text("Rating saved")],
    );
    let s = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [
                click(serde_json::json!({ "role": "button", "name": "Publsh" })),
                click(serde_json::json!({ "role": "button", "name": "Save" })),
                { "kind": "expect_visible", "selector": { "text": "Rating savd" } }
            ] },
            { "step_number": 2, "actions": [
                click(serde_json::json!({ "role": "button", "name": "Archive" })),
                click(serde_json::json!([
                    { "role": "dialog", "name": "Confirm" },
                    { "role": "button", "name": "Publsh" }
                ]))
            ] }
        ]),
    );
    let expected = [
        refusal_hinted(1, "button \"Publsh\"", "did you mean button \"Publish\"?"),
        refusal_hinted(1, "text \"Rating savd\"", "did you mean text \"Rating saved\"?"),
        refusal(2, "button \"Archive\""),
        refusal(2, "button \"Publsh\" in dialog \"Confirm\""),
    ];
    assert_eq!(check_seen(&map, &none(), &s, &[], None), Err(expected.join("\n")));
    assert_eq!(
        seen_verdict(&map, &none(), &s, &[], None, &[]),
        SeenVerdict::Unseen(expected.to_vec()),
        "the same lines, one by one"
    );
    // One unseen locator is answered exactly as it always was.
    let one = one_step("Ratings", serde_json::json!([click(serde_json::json!({ "role": "button", "name": "Publsh" }))]));
    assert_eq!(
        check_seen(&map, &none(), &one, &[], None),
        Err(refusal_hinted(1, "button \"Publsh\"", "did you mean button \"Publish\"?"))
    );
}

/// Past fifty, the refusal lists fifty and says how many more there are.
#[test]
fn the_refusal_list_is_capped_at_fifty() {
    assert_eq!(MAX_LISTED, 50);
    let map = map_with("Ratings", "/ratings", &[role("button", "Save")]);
    let clicks: Vec<serde_json::Value> =
        (1..=53).map(|i| click(serde_json::json!({ "css": format!("#unseen-{i}") }))).collect();
    let s = one_step("Ratings", serde_json::Value::Array(clicks));
    let refused = check_seen(&map, &none(), &s, &[], None).unwrap_err();
    let lines: Vec<&str> = refused.lines().collect();
    assert_eq!(lines.len(), 51, "{refused}");
    assert_eq!(lines[0], refusal(1, "#unseen-1"));
    assert_eq!(lines[49], refusal(1, "#unseen-50"));
    assert_eq!(lines[50], "and 3 more");

    // Exactly fifty: no "and" line.
    let fifty: Vec<String> = (1..=50).map(|i| format!("line {i}")).collect();
    assert_eq!(refusal_list(&fifty).lines().count(), 50);
    assert_eq!(refusal_list(&fifty[..1]), "line 1");
}

/// A refusal that is not of a locator never seen (a component the project
/// does not have, a page address never seen) comes back alone, as it
/// always did, however many unseen locators the script also names.
#[test]
fn a_refusal_that_is_not_an_unseen_locator_comes_back_alone() {
    let map = map_with("Ratings", "/ratings", &[role("button", "Save")]);
    let s = script(
        Some("Ratings"),
        serde_json::json!([
            { "step_number": 1, "actions": [click(serde_json::json!({ "css": "#first" }))] },
            { "step_number": 2, "actions": [
                { "kind": "use_component", "component": "Missing", "inputs": {} },
                click(serde_json::json!({ "css": "#second" }))
            ] }
        ]),
    );
    let alone = format!("Step 2: {}", v2_lib::autorun::components::not_saved("Missing"));
    assert_eq!(check_seen(&map, &none(), &s, &[], None), Err(alone.clone()));
    assert_eq!(seen_verdict(&map, &none(), &s, &[], None, &[]), SeenVerdict::Other(alone));

    // A component's page address never seen, beside its unseen locators.
    let actions: Vec<v2_lib::browser::actions::Action> = serde_json::from_value(serde_json::json!([
        click(serde_json::json!({ "css": "#first" })),
        { "kind": "navigate", "url": "/elsewhere" },
        click(serde_json::json!({ "css": "#second" }))
    ]))
    .unwrap();
    assert_eq!(
        check_component_seen(&map, Some("Ratings"), &actions),
        Err("Action 2: /elsewhere was never seen on the live app. Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again.".to_string())
    );
}

/// A component's save names every unseen locator too, in action order,
/// each with its own hint of the same role.
#[test]
fn a_component_refusal_lists_every_unseen_locator_in_order() {
    let map = map_with("Ratings", "/ratings", &[role("button", "Publish"), role("button", "Save"), role("link", "Archive")]);
    let actions: Vec<v2_lib::browser::actions::Action> = serde_json::from_value(serde_json::json!([
        click(serde_json::json!({ "role": "button", "name": "Publsh" })),
        click(serde_json::json!({ "role": "button", "name": "Save" })),
        click(serde_json::json!({ "role": "button", "name": "Archive" }))
    ]))
    .unwrap();
    let line = |i: usize, what: &str, hint: Option<&str>| match hint {
        Some(h) => format!("Action {i}: {what} was never seen on the live app; {h} Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again."),
        None => format!("Action {i}: {what} was never seen on the live app. Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again."),
    };
    let expected =
        [line(1, "button \"Publsh\"", Some("did you mean button \"Publish\"?")), line(3, "button \"Archive\"", None)];
    assert_eq!(check_component_seen(&map, Some("Ratings"), &actions), Err(expected.join("\n")));
    assert_eq!(component_verdict(&map, Some("Ratings"), &actions), SeenVerdict::Unseen(expected.to_vec()));
}
