//! What the assistant may read of a sign-in page: shapes and captions,
//! never a value, never a host.

use crate::common::{quick, stateful_app, ScriptedDriver};
use serde_json::{json, Value};
use v2_lib::autorun::recipe::PasswordResetPage;
use v2_lib::autorun::signin_inspect::{
    action_locator, control_locator, read_sign_in_page, reset_fields_showing, RawAction, RawControl, CHANGE_PASSWORD_HINT, INSPECT_JS,
    PASSWORD_FIELDS_JS,
};
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::locator::Target;

/// A driver whose page is at `href`, titled `title`, and answers the inspect
/// script with `read`.
fn page_reading(href: &'static str, read: Value) -> ScriptedDriver {
    ScriptedDriver::new(|_, _| Ok(json!({}))).answering_first(move |m, p| {
        if m != "Runtime.evaluate" {
            return None;
        }
        let value = match p["expression"].as_str().unwrap_or("") {
            e if e == INSPECT_JS => read.clone(),
            "location.href" => json!(href),
            "document.title" => json!("Sign in"),
            _ => return None,
        };
        Some(Ok(json!({ "result": { "value": value } })))
    })
}

fn control(id: &str, password: bool) -> Value {
    json!({ "tag": "input", "type": if password { "password" } else { "text" }, "id": id, "name": "", "label": "", "placeholder": "", "password": password, "role": "" })
}

fn target_json(t: &Target) -> Value {
    serde_json::to_value(t).unwrap()
}

#[test]
fn inspect_js_never_reads_a_value() {
    assert!(!INSPECT_JS.contains(".value"));
    assert!(!PASSWORD_FIELDS_JS.contains(".value"));
    assert_eq!(INSPECT_JS.matches("getAttribute('value')").count(), 1);
}

#[tokio::test]
async fn inspect_answer_carries_no_value_even_when_the_page_sends_one() {
    let mut c = control("txtUser", false);
    c["value"] = json!("s3cret-typed");
    let mut d = page_reading("https://hr.example.internal/login", json!({ "controls": [c], "actions": [] }));
    let view = read_sign_in_page(&mut d).await.unwrap();
    assert_eq!(view.controls.len(), 1);
    assert!(!serde_json::to_string(&view).unwrap().contains("s3cret-typed"));
}

fn action(text: &str) -> Value {
    json!({ "kind": "button", "text": text, "id": "" })
}

#[tokio::test]
async fn forty_one_actions_are_cut_to_forty() {
    let many: Vec<Value> = (0..41).map(|i| action(&format!("b{i}"))).collect();
    let mut d = page_reading("https://hr.example.internal/login", json!({ "controls": [], "actions": many }));
    let view = read_sign_in_page(&mut d).await.unwrap();
    assert!(view.actions.len() == 40 && view.cut);
}

#[tokio::test]
async fn an_action_with_no_id_and_no_text_is_left_out() {
    let acts = json!([action(""), action("Sign in")]);
    let mut d = page_reading("https://hr.example.internal/login", json!({ "controls": [], "actions": acts }));
    let view = read_sign_in_page(&mut d).await.unwrap();
    assert_eq!(view.actions.len(), 1);
    assert_eq!(view.actions[0].text, "Sign in");
}

#[tokio::test]
async fn a_failed_read_names_no_address() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({}))).answering_first(|m, p| {
        (m == "Runtime.evaluate" && p["expression"] == INSPECT_JS)
            .then(|| Err(CdpError::Transport("connect ws://127.0.0.1:9333/devtools failed".into())))
    });
    let err = read_sign_in_page(&mut d).await.unwrap_err();
    assert!(!err.contains("ws://") && !err.contains("127.0.0.1"), "{err}");
}

#[tokio::test]
async fn an_answer_that_is_not_controls_and_actions_is_an_error() {
    for bad in [json!(null), json!({ "controls": [] }), json!("text")] {
        let mut d = page_reading("https://hr.example.internal/login", bad);
        assert!(read_sign_in_page(&mut d).await.is_err());
    }
}

#[test]
fn a_labelled_select_is_a_combobox() {
    let c = control_locator(&RawControl { tag: "select".into(), label: "Language".into(), ..Default::default() });
    assert_eq!(target_json(&c), json!({ "role": "combobox", "name": "Language", "exact": true }));
}

#[test]
fn locators_are_in_the_recipe_target_format() {
    let by_id = control_locator(&RawControl { id: "txtUser".into(), tag: "input".into(), ..Default::default() });
    assert_eq!(target_json(&by_id), json!({ "css": "#txtUser" }));
    let by_name = control_locator(&RawControl { name: "pwd".into(), tag: "input".into(), ..Default::default() });
    assert_eq!(target_json(&by_name), json!({ "css": "input[name=\"pwd\"]" }));
    let password = control_locator(&RawControl { password: true, tag: "input".into(), ..Default::default() });
    assert_eq!(target_json(&password), json!({ "css": "input[type=\"password\"]" }));
    let labelled = control_locator(&RawControl { label: "Email".into(), tag: "input".into(), ..Default::default() });
    assert_eq!(target_json(&labelled), json!({ "role": "textbox", "name": "Email", "exact": true }));
    let button = action_locator(&RawAction { kind: "button".into(), text: "Sign in".into(), id: String::new() });
    assert_eq!(target_json(&button), json!({ "role": "button", "name": "Sign in", "exact": true }));
    let by_action_id = action_locator(&RawAction { kind: "button".into(), text: "Go".into(), id: "btn-go".into() });
    assert_eq!(target_json(&by_action_id), json!({ "css": "#btn-go" }));

    for t in [by_id, by_name, password, labelled, button, by_action_id] {
        let back: Target = serde_json::from_value(target_json(&t)).unwrap();
        assert!(back.validate().is_ok());
    }
}

#[tokio::test]
async fn two_password_fields_give_the_hint_and_the_path_has_no_host() {
    let read = json!({ "controls": [control("old", true), control("new", true)], "actions": [] });
    let mut d = page_reading("https://hr.example.internal/account/ChangePassword?x=1", read);
    let view = read_sign_in_page(&mut d).await.unwrap();
    assert_eq!(view.path, "/account/ChangePassword");
    assert_eq!(view.change_password_hint.as_deref(), Some(CHANGE_PASSWORD_HINT));
    assert!(!serde_json::to_string(&view).unwrap().contains("hr.example.internal"));

    let one = json!({ "controls": [control("only", true)], "actions": [] });
    let mut d = page_reading("https://hr.example.internal/login", one);
    assert!(read_sign_in_page(&mut d).await.unwrap().change_password_hint.is_none());
}

#[tokio::test]
async fn forty_one_controls_are_cut_to_forty() {
    let many: Vec<Value> = (0..41).map(|i| control(&format!("c{i}"), false)).collect();
    let mut d = page_reading("https://hr.example.internal/login", json!({ "controls": many, "actions": [] }));
    let view = read_sign_in_page(&mut d).await.unwrap();
    assert!(view.controls.len() == 40 && view.cut);

    let mut d = page_reading("https://hr.example.internal/login", json!({ "controls": [control("a", false)], "actions": [] }));
    assert!(!read_sign_in_page(&mut d).await.unwrap().cut);
}

fn reset_page() -> PasswordResetPage {
    serde_json::from_value(json!({ "path_contains": "/security/PasswordReset", "field": { "css": "#txtOldPassword" } })).unwrap()
}

/// A browser on `href` where every element is there except `missing`.
fn reset_app(href: &'static str, missing: Option<&'static str>) -> ScriptedDriver {
    let (d, _state) = stateful_app(false, missing);
    d.answering_first(move |m, p| (m == "Runtime.evaluate" && p["expression"] == "location.href").then(|| Ok(json!({ "result": { "value": href } }))))
}

#[tokio::test]
async fn reset_fields_showing_needs_every_field() {
    let href = "https://hr.example.internal/security/PasswordReset";
    let mut d = reset_app(href, None);
    assert!(reset_fields_showing(&mut d, &reset_page(), &quick()).await);

    let mut d = reset_app(href, Some("#btnsubmit"));
    assert!(!reset_fields_showing(&mut d, &reset_page(), &quick()).await);

    let mut d = reset_app("https://hr.example.internal/home", None);
    assert!(!reset_fields_showing(&mut d, &reset_page(), &quick()).await);
}
