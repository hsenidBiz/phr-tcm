//! The save check: a script names only what the app has seen on the live
//! page, unless the script typed it itself or the test case says it.

use v2_lib::autorun::components::{Component, ComponentFile};
use v2_lib::autorun::discovery_map::{AreaMap, DiscoveryMap, PageMap, SeenElement};
use v2_lib::autorun::edits::Edit;
use v2_lib::autorun::seen_check::{
    check_component_seen, check_resolved_inputs, check_seen, check_seen_all, check_seen_all_hinted, check_seen_with_files,
    has_placeholder_inputs, steps_to_check, Unseen,
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
    assert_eq!(check_seen(&map, &none(), &s, &[], None), Err(refusal(1, "button \"Old\"")));
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

/// An import names every unseen locator, step by step, where a save names
/// only the first.
#[test]
fn check_seen_all_lists_every_unseen_locator_and_check_seen_still_stops_at_the_first() {
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
    assert_eq!(check_seen(&map, &none(), &s, &[], None), Err(refusal(1, "button \"Publish\"")));
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
    for id in ["{{setup.cycle_id}}", "{{fixture.pc-draft-before-evaluators.cycle_id}}", "{{ prefix }}", "{{now:yyyyMMdd}}"] {
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
    // Not a placeholder the run fills in.
    refused(edit_in_card("{{cycle_id}}"), &map);
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
    // Any other brace pair is still the component's own placeholder.
    assert_eq!(
        check_seen(&map, &have, &with(serde_json::json!({ "id": "{{cycle_id}}" })), &[], None),
        Err("Step 1: Edit cycle by id got a placeholder as id".to_string())
    );
    assert!(has_placeholder_inputs(&saved.steps));

    let run = |filled: &str| {
        let f = with(serde_json::json!({ "id": filled }));
        check_resolved_inputs(&map, &have, &["Cycles"], &saved.steps, &f.steps)
    };
    // A new draft's id: digits, where digits were seen.
    assert_eq!(run("10071"), Ok(()));
    let gave = |id: &str| {
        format!(
            "Edit cycle by id: its input id gave button[aria-label^=\"Edit\"] in div[data-cycle-id=\"{id}\"], which does not fit what was seen on the live app"
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
        Err("Open a card: its input card gave div[data-cycle-id=\"x9\"], which does not fit what was seen on the live app".to_string())
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
    for c in ["#er-other input:checked", "#er-goals-checkbox input:focus-visible", "#er-goals-checkbox select:checked"] {
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
    // A selector filter: its inner selector seen in the same areas.
    let has_badge = ".phr-mc-card:has(.badge-draft)";
    assert_eq!(check_seen(&base_only, &none(), &with(has_badge), &[], None), Err(refusal(1, has_badge)));
    let badge = map_with("Cycles", "/cycles", &[css(".phr-mc-card"), css(".badge-draft")]);
    assert_eq!(check_seen(&badge, &none(), &with(has_badge), &[], None), Ok(()));
    let badge_elsewhere = joined(base_only.clone(), map_with("Payroll", "/payroll", &[css(".badge-draft")]));
    assert_eq!(check_seen(&badge_elsewhere, &none(), &with(has_badge), &[], None), Err(refusal(1, has_badge)));
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
    // The size needs the Test files to say it.
    assert_eq!(check_seen(&map, &none(), &after_upload(shows("240.0 KB")), &[], None), Err(refusal(2, "text \"240.0 KB\"")));
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
    // The same role before a closer name in another role; never Payroll's.
    assert_eq!(
        check_seen(&map, &none(), &in_area("Ratings", button("Publsh")), &[], None),
        Err(refusal_hinted(1, "button \"Publsh\"", "did you mean button \"Publish\"?"))
    );
    // Another role when no same-role name is close.
    assert_eq!(
        check_seen(&map, &none(), &in_area("Ratings", serde_json::json!({ "role": "progressbar", "name": "Step 1 of 9 - Cycle Setup" })), &[], None),
        Err(refusal_hinted(
            1,
            "progressbar \"Step 1 of 9 - Cycle Setup\"",
            "did you mean text \"Step 1 of 9 \u{2013} Cycle Setup\"?"
        ))
    );
    // Payroll's names are close, but not in this script's areas.
    assert_eq!(
        check_seen(&map, &none(), &in_area("Ratings", button("Archive")), &[], None),
        Err(refusal(1, "button \"Archive\""))
    );
    assert_eq!(check_seen(&map, &none(), &in_area("Leave", button("Publsh")), &[], None), Err(refusal(1, "button \"Publsh\"")));
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
