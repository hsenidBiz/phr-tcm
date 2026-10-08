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
fn users_of_lists_scripts_and_fixtures() {
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
    let fx = v2_lib::api_templates::fixture_store::fixtures_dir(dir.path(), "o", "p");
    std::fs::create_dir_all(&fx).unwrap();
    std::fs::write(
        fx.join("f1.json"),
        json!({"id": "f1", "name": "Make a cycle", "steps": [{"kind": "use_component", "component": "PICK-DATE", "inputs": {}}]}).to_string(),
    )
    .unwrap();
    std::fs::write(fx.join("f2.json"), json!({"id": "f2", "name": "Unrelated", "steps": []}).to_string()).unwrap();
    std::fs::write(fx.join("f1.runs.json"), "[]").unwrap();
    let u = users_of(dir.path(), "o", "p", "pick-date");
    assert_eq!(u.cases, vec![5, 12]);
    assert_eq!(u.fixtures, vec!["Make a cycle".to_string()]);
    let none = users_of(dir.path(), "o", "p", "never used");
    assert!(none.cases.is_empty() && none.fixtures.is_empty());
}
