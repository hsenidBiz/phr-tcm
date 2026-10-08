//! Components: the `use_component` step and the `input` placeholder a
//! component's locator leaves to the caller.

use serde_json::json;
use v2_lib::browser::actions::Action;
use v2_lib::browser::locator::{has_input_placeholder, LocatorStep, Target};

#[test]
fn scripts_without_components_still_parse() {
    let a: Action = serde_json::from_value(json!({"kind": "click", "selector": {"role": "button", "name": "Save"}})).unwrap();
    assert!(matches!(a, Action::Click { .. }));
    let t: Target = serde_json::from_value(json!({"role": "button", "name": "Save"})).unwrap();
    assert!(!has_input_placeholder(&t));
    assert!(t.validate().is_ok());
}

#[test]
fn use_component_round_trips() {
    let v = json!({"kind": "use_component", "component": "pick-date", "inputs": {"day": "5", "n": 2}});
    let a: Action = serde_json::from_value(v.clone()).unwrap();
    match &a {
        Action::UseComponent { component, inputs } => {
            assert_eq!(component, "pick-date");
            assert_eq!(inputs.len(), 2);
        }
        other => panic!("wrong variant: {other:?}"),
    }
    assert_eq!(serde_json::to_value(&a).unwrap(), v);
}

#[test]
fn an_input_placeholder_round_trips_inside_a_chain() {
    let v = json!([{"role": "dialog", "name": "Pick"}, {"input": "day"}]);
    let t: Target = serde_json::from_value(v.clone()).unwrap();
    assert!(has_input_placeholder(&t));
    assert!(t.validate().is_ok());
    assert_eq!(serde_json::to_value(&t).unwrap(), v);
}

#[test]
fn an_input_placeholder_with_a_role_is_refused() {
    for step in [
        LocatorStep { input: Some("x".into()), role: Some("button".into()), ..Default::default() },
        LocatorStep { input: Some("x".into()), text: Some("t".into()), ..Default::default() },
        LocatorStep { input: Some("x".into()), css: Some(".c".into()), ..Default::default() },
    ] {
        let err = Target::One(step).validate().unwrap_err();
        assert!(err.contains("an input placeholder stands alone"), "{err}");
    }
}

#[test]
fn use_component_has_no_targets() {
    let a = Action::UseComponent { component: "c".into(), inputs: serde_json::Map::new() };
    assert!(a.targets().is_empty());
}

// ---- the store ----

use v2_lib::autorun::components::{
    components_path, find, load_components, put, remove, reset_components, users_of, Component, ComponentInput,
    InputKind,
};
use v2_lib::autorun::store::save_script;
use v2_lib::autorun::{CaseScript, StepScript};

fn component(name: &str) -> Component {
    Component {
        name: name.into(),
        description: "pick a date".into(),
        inputs: vec![ComponentInput { name: "day".into(), kind: InputKind::Text, description: "the day".into() }],
        actions: vec![Action::CheckText { value: "x".into() }],
        tried_at: 7,
        tried_area: "Leave".into(),
        version: 1,
        changes: 0,
    }
}

fn script(case_id: i32, actions: Vec<Action>) -> CaseScript {
    CaseScript {
        case_id,
        title: "a case".into(),
        account: None,
        area: None,
        steps: vec![StepScript { step_number: 2, actions, unchecked: None }],
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None,
        ignore_page_errors: vec![],
    }
}

fn use_it(name: &str) -> Action {
    Action::UseComponent { component: name.into(), inputs: serde_json::Map::new() }
}

#[test]
fn put_then_load_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_components(dir.path(), "o", "p").unwrap().components.is_empty());
    put(dir.path(), "o", "p", component("pick-date")).unwrap();
    put(dir.path(), "o", "p", component("other")).unwrap();
    let f = load_components(dir.path(), "o", "p").unwrap();
    assert_eq!(f.components.len(), 2);
    assert_eq!(find(&f, "pick-date"), Some(&component("pick-date")));
    // Replaced by key, not added.
    let mut again = component("Pick-Date");
    again.version = 2;
    put(dir.path(), "o", "p", again).unwrap();
    let f = load_components(dir.path(), "o", "p").unwrap();
    assert_eq!(f.components.len(), 2);
    assert_eq!(find(&f, "pick-date").unwrap().version, 2);
    remove(dir.path(), "o", "p", "OTHER").unwrap();
    assert_eq!(load_components(dir.path(), "o", "p").unwrap().components.len(), 1);
}

#[test]
fn names_match_ignoring_case_and_spaces() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "o", "p", component("Pick  The Date")).unwrap();
    let f = load_components(dir.path(), "o", "p").unwrap();
    assert!(find(&f, "pick the date").is_some());
    assert!(find(&f, "  PICK THE  DATE ").is_some());
    assert!(find(&f, "pick date").is_none());
}

#[test]
fn a_corrupt_file_names_itself_and_reset_moves_it_aside() {
    let dir = tempfile::tempdir().unwrap();
    // A file that reads cannot be reset.
    put(dir.path(), "o", "p", component("c")).unwrap();
    assert!(reset_components(dir.path(), "o", "p").unwrap_err().contains("nothing to reset"));
    let path = components_path(dir.path(), "o", "p");
    std::fs::write(&path, "{ not json").unwrap();
    let err = load_components(dir.path(), "o", "p").unwrap_err();
    assert!(err.contains("the components file projects/"), "{err}");
    assert!(err.contains("-components.json could not be read; Reset it in Auto Run"), "{err}");
    assert!(!err.contains(dir.path().to_str().unwrap()), "{err}");
    assert!(put(dir.path(), "o", "p", component("d")).is_err());
    let aside = reset_components(dir.path(), "o", "p").unwrap();
    assert!(aside.starts_with("projects/") && aside.contains("-components.corrupt-") && aside.ends_with(".json"), "{aside}");
    assert!(!path.exists());
    assert_eq!(std::fs::read_to_string(dir.path().join(&aside)).unwrap(), "{ not json");
    assert!(load_components(dir.path(), "o", "p").unwrap().components.is_empty());
}

#[test]
fn put_without_change_does_not_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "o", "p", component("c")).unwrap();
    let path = components_path(dir.path(), "o", "p");
    // Mark the file so a rewrite would show.
    let before = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{before}

")).unwrap();
    put(dir.path(), "o", "p", component("c")).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{before}

"));
    remove(dir.path(), "o", "p", "absent").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{before}

"));
    let mut changed = component("c");
    changed.changes = 1;
    put(dir.path(), "o", "p", changed).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), serde_json::to_string_pretty(&load_components(dir.path(), "o", "p").unwrap()).unwrap());
}

#[test]
fn users_of_lists_the_scripts_that_use_it() {
    let dir = tempfile::tempdir().unwrap();
    let guarded = Action::WhenVisible {
        selector: Target::One(LocatorStep { role: Some("dialog".into()), ..Default::default() }),
        within_ms: None,
        then: vec![use_it("Pick-Date")],
    };
    save_script(dir.path(), &script(12, vec![use_it("pick-date")])).unwrap();
    save_script(dir.path(), &script(5, vec![guarded])).unwrap();
    save_script(dir.path(), &script(9, vec![use_it("something else")])).unwrap();
    save_script(dir.path(), &script(3, vec![])).unwrap();
    let u = users_of(dir.path(), "pick-date");
    assert_eq!(u.cases, vec![5, 12]);
    let none = users_of(dir.path(), "never used");
    assert!(none.cases.is_empty());
}

// ---- expansion ----

use v2_lib::autorun::components::expand;

fn made(name: &str, inputs: serde_json::Value, actions: serde_json::Value) -> Component {
    serde_json::from_value(json!({ "name": name, "description": "d", "inputs": inputs, "actions": actions, "version": 1 }))
        .expect("a component")
}

fn given(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    v.as_object().cloned().expect("an object")
}

fn actions(v: serde_json::Value) -> Vec<Action> {
    serde_json::from_value(v).expect("actions")
}

#[test]
fn text_inputs_fill_values() {
    let c = made(
        "Pick a date",
        json!([{ "name": "day", "kind": "text", "description": "" }]),
        json!([
            { "kind": "fill", "selector": { "role": "textbox", "name": "Day" }, "value": "{{day}}" },
            { "kind": "check_text", "value": "picked {{day}} and {{day}}, not {{other}}" },
            { "kind": "click", "selector": { "role": "gridcell", "name": "{{day}}" } }
        ]),
    );
    let out = expand(&c, &given(json!({ "day": "5" }))).unwrap();
    assert_eq!(
        out,
        actions(json!([
            { "kind": "fill", "selector": { "role": "textbox", "name": "Day" }, "value": "5" },
            { "kind": "check_text", "value": "picked 5 and 5, not {{other}}" },
            { "kind": "click", "selector": { "role": "gridcell", "name": "5" } }
        ]))
    );
    // A value that itself reads like a placeholder is typed as it is.
    let out = expand(&c, &given(json!({ "day": "{{day}}" }))).unwrap();
    assert_eq!(out[0], actions(json!([{ "kind": "fill", "selector": { "role": "textbox", "name": "Day" }, "value": "{{day}}" }]))[0]);
}

#[test]
fn a_target_input_replaces_its_placeholder() {
    let c = made(
        "Open a menu",
        json!([{ "name": "field", "kind": "target", "description": "" }]),
        json!([
            { "kind": "click", "selector": { "input": "field" } },
            { "kind": "when_visible", "selector": { "role": "menu" }, "then": [
                { "kind": "click", "selector": { "input": "field" } }
            ] }
        ]),
    );
    let out = expand(&c, &given(json!({ "field": { "role": "textbox", "name": "Leave start" } }))).unwrap();
    assert_eq!(
        out,
        actions(json!([
            { "kind": "click", "selector": { "role": "textbox", "name": "Leave start" } },
            { "kind": "when_visible", "selector": { "role": "menu" }, "then": [
                { "kind": "click", "selector": { "role": "textbox", "name": "Leave start" } }
            ] }
        ]))
    );
    // A chain given for a lone placeholder becomes the whole chain.
    let out = expand(&c, &given(json!({ "field": [{ "role": "dialog" }, { "role": "button", "name": "Go" }] }))).unwrap();
    assert_eq!(out[0], actions(json!([{ "kind": "click", "selector": [{ "role": "dialog" }, { "role": "button", "name": "Go" }] }]))[0]);
    // A caller's locator is never searched for text placeholders.
    let t = made(
        "Type",
        json!([{ "name": "field", "kind": "target", "description": "" }, { "name": "v", "kind": "text", "description": "" }]),
        json!([{ "kind": "fill", "selector": { "input": "field" }, "value": "{{v}}" }]),
    );
    let out = expand(&t, &given(json!({ "field": { "text": "{{v}}" }, "v": "x" }))).unwrap();
    assert_eq!(out, actions(json!([{ "kind": "fill", "selector": { "text": "{{v}}" }, "value": "x" }])));
}

#[test]
fn a_target_input_inside_a_chain_expands_in_place() {
    let c = made(
        "Edit a row",
        json!([{ "name": "row", "kind": "target", "description": "" }]),
        json!([{ "kind": "click", "selector": [{ "css": "#grid" }, { "input": "row" }, { "role": "button", "name": "Edit" }] }]),
    );
    let one = expand(&c, &given(json!({ "row": { "role": "row", "name": "Annual" } }))).unwrap();
    assert_eq!(
        one,
        actions(json!([{ "kind": "click", "selector": [
            { "css": "#grid" }, { "role": "row", "name": "Annual" }, { "role": "button", "name": "Edit" }
        ] }]))
    );
    let chain = expand(&c, &given(json!({ "row": [{ "role": "rowgroup" }, { "role": "row", "name": "Annual" }] }))).unwrap();
    assert_eq!(
        chain,
        actions(json!([{ "kind": "click", "selector": [
            { "css": "#grid" }, { "role": "rowgroup" }, { "role": "row", "name": "Annual" }, { "role": "button", "name": "Edit" }
        ] }]))
    );
    for wrong in [json!("#row"), json!(5), json!({ "input": "row" }), json!([])] {
        let err = expand(&c, &given(json!({ "row": wrong }))).unwrap_err();
        assert_eq!(err, "Edit a row needs row to be a locator", "{wrong}");
    }
}

#[test]
fn a_missing_input_is_named() {
    let c = made(
        "Pick a date",
        json!([{ "name": "field", "kind": "target", "description": "" }, { "name": "day", "kind": "text", "description": "" }]),
        json!([{ "kind": "fill", "selector": { "input": "field" }, "value": "{{day}}" }]),
    );
    assert_eq!(expand(&c, &given(json!({ "field": { "css": "#d" } }))).unwrap_err(), "Pick a date needs day");
    assert_eq!(expand(&c, &given(json!({ "field": { "css": "#d" }, "day": null }))).unwrap_err(), "Pick a date needs day");
    assert_eq!(expand(&c, &given(json!({ "day": "5" }))).unwrap_err(), "Pick a date needs field");
    assert_eq!(
        expand(&c, &given(json!({ "field": { "css": "#d" }, "day": { "css": "#x" } }))).unwrap_err(),
        "Pick a date needs day to be text"
    );
}
