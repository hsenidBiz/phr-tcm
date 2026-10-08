//! The save check: a script names only what the app has seen on the live
//! page, unless the script typed it itself or the test case says it.

use v2_lib::autorun::components::{Component, ComponentFile};
use v2_lib::autorun::discovery_map::{AreaMap, DiscoveryMap, PageMap, SeenElement};
use v2_lib::autorun::edits::Edit;
use v2_lib::autorun::seen_check::{check_seen, check_seen_all, steps_to_check, Unseen};
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
