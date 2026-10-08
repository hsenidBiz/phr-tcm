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
