//! The discovery-only `fill_form` step: read from a raw action, carried as a
//! marked stand-in, described without values, and refused in a save.

use serde_json::json;
use v2_lib::ai_bridge::describe_action;
use v2_lib::autorun::action_help::{
    fill_form_ask, fill_form_from, refuse_raw_saved_action, FieldValue, FILL_FORM_MAX_FIELDS, SAVED_FILL_FORM,
};
use v2_lib::browser::locator::Target;

fn stand_in(raw: serde_json::Value) -> v2_lib::browser::actions::Action {
    fill_form_from(&raw).expect("is fill_form").expect("accepted")
}

#[test]
fn other_kinds_are_not_fill_form() {
    assert!(fill_form_from(&json!({"kind":"click","target":"#a"})).is_none());
}

#[test]
fn fields_keep_their_order() {
    let a = stand_in(json!({"kind":"fill_form","fields":{"B":"1","A":true,"C":3,"D":2.5}}));
    let form = fill_form_ask(&a).expect("carries a form");
    assert!(form.within.is_none());
    assert_eq!(
        form.fields,
        vec![
            ("B".to_string(), FieldValue::Text("1".into())),
            ("A".to_string(), FieldValue::Bool(true)),
            ("C".to_string(), FieldValue::Text("3".into())),
            ("D".to_string(), FieldValue::Text("2.5".into())),
        ]
    );
}

#[test]
fn within_is_read_as_a_locator() {
    let a = stand_in(json!({"kind":"fill_form","within":{"role":"dialog","name":"Add employee"},"fields":{"A":"x"}}));
    let form = fill_form_ask(&a).unwrap();
    assert!(matches!(form.within, Some(Target::One(_))));
}

#[test]
fn a_within_that_is_not_a_locator_is_refused() {
    let r = fill_form_from(&json!({"kind":"fill_form","within":5,"fields":{"A":"x"}})).unwrap();
    assert!(r.unwrap_err().contains("within"));
}

#[test]
fn stray_key_is_refused_naming_the_keys() {
    let e = fill_form_from(&json!({"kind":"fill_form","fields":{"A":"x"},"submit":true})).unwrap().unwrap_err();
    assert!(e.contains("submit"), "{e}");
    assert!(e.contains("within, fields"), "{e}");
}

#[test]
fn no_fields_and_too_many_are_refused() {
    assert!(fill_form_from(&json!({"kind":"fill_form","fields":{}})).unwrap().is_err());
    assert!(fill_form_from(&json!({"kind":"fill_form"})).unwrap().is_err());
    assert!(fill_form_from(&json!({"kind":"fill_form","fields":["A"]})).unwrap().is_err());
    let mut ok = serde_json::Map::new();
    for i in 0..FILL_FORM_MAX_FIELDS {
        ok.insert(format!("F{i}"), json!("x"));
    }
    assert!(fill_form_from(&json!({"kind":"fill_form","fields":ok.clone()})).unwrap().is_ok());
    ok.insert("one more".into(), json!("x"));
    assert!(fill_form_from(&json!({"kind":"fill_form","fields":ok})).unwrap().is_err());
}

#[test]
fn a_list_null_or_empty_label_is_refused() {
    assert!(fill_form_from(&json!({"kind":"fill_form","fields":{"A":["x"]}})).unwrap().is_err());
    assert!(fill_form_from(&json!({"kind":"fill_form","fields":{"A":null}})).unwrap().is_err());
    assert!(fill_form_from(&json!({"kind":"fill_form","fields":{"  ":"x"}})).unwrap().is_err());
}

#[test]
fn a_stand_in_is_only_read_back_when_marked() {
    use v2_lib::browser::actions::Action;
    assert!(fill_form_ask(&Action::CheckText { value: "hello".into() }).is_none());
    assert!(fill_form_ask(&Action::CheckText { value: "\u{1}fill_form:{not json".into() }).is_none());
}

#[test]
fn a_saved_script_with_fill_form_is_refused() {
    let raw = json!({"kind":"fill_form","fields":{"A":"x"}});
    assert_eq!(refuse_raw_saved_action(&raw), Err(SAVED_FILL_FORM.to_string()));
    let nested = json!({"kind":"when_visible","target":"#a","then":[raw]});
    assert_eq!(refuse_raw_saved_action(&nested), Err(SAVED_FILL_FORM.to_string()));
}

#[test]
fn a_check_text_carrying_the_stand_in_is_refused_in_a_save() {
    let a = stand_in(json!({"kind":"fill_form","fields":{"A":"x"}}));
    let raw = serde_json::to_value(&a).unwrap();
    assert_eq!(refuse_raw_saved_action(&raw), Err(SAVED_FILL_FORM.to_string()));
    let nested = json!({"kind":"when_visible","target":"#a","then":[raw]});
    assert_eq!(refuse_raw_saved_action(&nested), Err(SAVED_FILL_FORM.to_string()));
}

#[test]
fn describe_never_shows_a_value() {
    let a = stand_in(json!({"kind":"fill_form","fields":{"Name":"secret-one","Pass":"secret-two"}}));
    let d = describe_action(&a);
    assert_eq!(d, "fill_form 2 fields");
    let one = stand_in(json!({"kind":"fill_form","fields":{"Name":"secret-one"}}));
    assert!(!describe_action(&one).contains("secret"));
}
