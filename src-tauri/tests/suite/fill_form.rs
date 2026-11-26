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

// ------------------------------------------------------------ matching a label

use serde_json::Value;
use v2_lib::browser::actions::Action;
use v2_lib::browser::cdp::{CdpError, Driver, Event};
use v2_lib::browser::fill_form::{plan_field, within_target, Planned};
use v2_lib::browser::locator::LocatorStep;

/// One control the fake page holds.
struct Control {
    role: &'static str,
    name: &'static str,
    tag: &'static str,
    kind: &'static str,
    editable: bool,
    checked: bool,
}

fn control(role: &'static str, name: &'static str, tag: &'static str, kind: &'static str) -> Control {
    Control { role, name, tag, kind, editable: false, checked: false }
}

/// A page of controls (and containers) that answers the accessibility
/// query, node resolution and the calls on a handle.
struct Page(Vec<Control>);

impl Driver for Page {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        let on = params["objectId"].as_str().unwrap_or("").to_string();
        Ok(match method {
            "Runtime.evaluate" => json!({"result": {"objectId": "doc"}}),
            "Accessibility.queryAXTree" => {
                let role = params["role"].as_str().unwrap_or("");
                let nodes: Vec<Value> = self
                    .0
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.role == role)
                    .map(|(i, c)| json!({"name": {"value": c.name}, "backendDOMNodeId": i as i64}))
                    .collect();
                json!({ "nodes": nodes })
            }
            "DOM.resolveNode" => json!({"object": {"objectId": format!("el{}", params["backendNodeId"])}}),
            "DOM.describeNode" => {
                json!({"node": {"backendNodeId": on.trim_start_matches("el").parse::<i64>().unwrap_or(0)}})
            }
            "Runtime.callFunctionOn" => {
                let f = params["functionDeclaration"].as_str().unwrap_or("");
                let c = on.strip_prefix("el").and_then(|n| n.parse::<usize>().ok()).and_then(|i| self.0.get(i));
                let value = if f.contains("isContentEditable") {
                    let c = c.expect("a control");
                    json!({"tag": c.tag, "type": c.kind, "editable": c.editable, "checked": c.checked})
                } else if f.contains("IFRAME") {
                    json!("element")
                } else {
                    json!(true)
                };
                json!({"result": {"value": value}})
            }
            other => panic!("not expected: {other}"),
        })
    }
    async fn wait_event(&mut self, method: &str, _limit: std::time::Duration) -> Result<Event, CdpError> {
        panic!("not expected: {method}");
    }
    fn forget_events(&mut self) {}
    fn take_dialogs(&mut self) -> Vec<String> {
        vec![]
    }
    fn set_deadline(&mut self, _deadline: Option<std::time::Instant>) {}
}

fn text(s: &str) -> FieldValue {
    FieldValue::Text(s.to_string())
}

async fn plan(controls: Vec<Control>, label: &str, value: FieldValue) -> Planned {
    plan_field(&mut Page(controls), None, label, &value).await.unwrap()
}

fn one(target: &Target) -> &LocatorStep {
    match target {
        Target::One(s) => s,
        other => panic!("expected one link, got {other:?}"),
    }
}

fn failed(p: Planned) -> String {
    match p {
        Planned::Failed(s) => s,
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[tokio::test]
async fn a_text_box_gets_fill_with_an_exact_locator() {
    let p = plan(vec![control("textbox", "First name", "input", "text")], "First name", text("Ann")).await;
    let Planned::Act { action: Action::Fill { selector, value, .. }, locator, matched } = p else { panic!("{p:?}") };
    assert_eq!(value, "Ann");
    assert_eq!(matched, 1);
    assert_eq!(selector, locator);
    let s = one(&locator);
    assert_eq!((s.role.as_deref(), s.name.as_deref(), s.exact), (Some("textbox"), Some("First name"), true));
}

#[tokio::test]
async fn a_native_select_gets_select_option_by_label() {
    let p = plan(vec![control("combobox", "Department", "select", "")], "Department", text("HR")).await;
    let Planned::Act { action: Action::SelectOption { label, value, within_ms, .. }, .. } = p else {
        panic!("{p:?}")
    };
    assert_eq!((label.as_deref(), value, within_ms), (Some("HR"), None, None));
}

#[tokio::test]
async fn a_ticked_checkbox_asked_true_is_already_set() {
    let mut c = control("checkbox", "Active", "input", "checkbox");
    c.checked = true;
    assert_eq!(plan(vec![c], "Active", FieldValue::Bool(true)).await, Planned::AlreadySet);
}

#[tokio::test]
async fn an_unticked_checkbox_asked_true_is_clicked() {
    let p = plan(vec![control("checkbox", "Active", "input", "checkbox")], "Active", FieldValue::Bool(true)).await;
    assert!(matches!(p, Planned::Act { action: Action::Click { .. }, .. }), "{p:?}");
}

#[tokio::test]
async fn radio_false_fails() {
    let msg = failed(plan(vec![control("radio", "Male", "input", "radio")], "Male", FieldValue::Bool(false)).await);
    assert!(msg.contains("turned off by choosing another"), "{msg}");
}

#[tokio::test]
async fn password_fails() {
    let msg = failed(plan(vec![control("textbox", "Password", "input", "password")], "Password", text("hunter2")).await);
    assert_eq!(msg, "passwords are never typed by an assistant");
}

#[tokio::test]
async fn a_custom_combobox_fails_naming_its_role() {
    let msg = failed(plan(vec![control("combobox", "Country", "div", "")], "Country", text("LK")).await);
    assert!(msg.contains("combobox") && msg.contains("saved component"), "{msg}");
}

#[tokio::test]
async fn a_label_matching_in_two_roles_fails_with_matched_2() {
    let msg = failed(
        plan(
            vec![control("textbox", "Name", "input", "text"), control("combobox", "Name", "select", "")],
            "Name",
            text("x"),
        )
        .await,
    );
    assert!(msg.starts_with("2 form controls match \"Name\""), "{msg}");
}

#[tokio::test]
async fn exact_first_then_contains() {
    let p = plan(vec![control("textbox", "Work email", "input", "email")], "Email", text("a@b.c")).await;
    let Planned::Act { locator, .. } = p else { panic!("{p:?}") };
    let s = one(&locator);
    assert_eq!((s.name.as_deref(), s.exact), (Some("Email"), false));
}

#[tokio::test]
async fn nothing_named_and_wrong_value_types_fail_without_the_value() {
    let msg = failed(plan(vec![], "Ghost", text("secret")).await);
    assert_eq!(msg, "no form control is named \"Ghost\"");
    let msg = failed(plan(vec![control("textbox", "Age", "input", "number")], "Age", FieldValue::Bool(true)).await);
    assert!(msg.contains("text"), "{msg}");
    let msg = failed(plan(vec![control("checkbox", "Active", "input", "checkbox")], "Active", text("yes")).await);
    assert!(msg.contains("true or false") && !msg.contains("yes"), "{msg}");
}

#[tokio::test]
async fn within_matching_two_writes_nth_0_and_says_2() {
    let dialog = || control("dialog", "Add", "div", "");
    let within =
        Target::One(LocatorStep { role: Some("dialog".into()), name: Some("Add".into()), ..Default::default() });
    let mut page = Page(vec![dialog(), dialog()]);
    let (target, n) = within_target(&mut page, &within).await.unwrap().unwrap();
    assert_eq!(n, 2);
    assert_eq!(one(&target).nth, Some(0));

    let mut page = Page(vec![dialog()]);
    let (target, n) = within_target(&mut page, &within).await.unwrap().unwrap();
    assert_eq!((n, one(&target).nth), (1, None));

    let mut page = Page(vec![]);
    assert!(within_target(&mut page, &within).await.unwrap().is_err());
    assert!(within_target(&mut page, &Target::Legacy("#a".into())).await.unwrap().is_err());
}

#[tokio::test]
async fn within_is_the_head_of_the_written_chain() {
    let within = Target::One(LocatorStep {
        role: Some("dialog".into()),
        name: Some("Add".into()),
        nth: Some(0),
        ..Default::default()
    });
    let mut page = Page(vec![control("dialog", "Add", "div", ""), control("textbox", "Name", "input", "text")]);
    let p = plan_field(&mut page, Some(&within), "Name", &text("Ann")).await.unwrap();
    let Planned::Act { locator: Target::Chain(links), .. } = p else { panic!("{p:?}") };
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].role.as_deref(), Some("dialog"));
    assert_eq!(links[1].role.as_deref(), Some("textbox"));
}

#[tokio::test]
async fn several_contains_matches_fail_naming_the_count() {
    let msg = failed(
        plan(
            vec![control("textbox", "First name", "input", "text"), control("textbox", "Last name", "input", "text")],
            "Name",
            text("x"),
        )
        .await,
    );
    assert!(msg.starts_with("2 form controls match \"Name\""), "{msg}");
}

#[tokio::test]
async fn an_exact_single_match_wins_over_contains_matches() {
    let p = plan(
        vec![control("textbox", "Email confirm", "input", "email"), control("textbox", "Email", "input", "email")],
        "Email",
        text("a@b.c"),
    )
    .await;
    let Planned::Act { locator, .. } = p else { panic!("{p:?}") };
    assert!(one(&locator).exact);
}

#[tokio::test]
async fn a_week_input_is_filled() {
    let p = plan(vec![control("textbox", "Week", "input", "week")], "Week", text("2026-W01")).await;
    assert!(matches!(p, Planned::Act { action: Action::Fill { .. }, .. }), "{p:?}");
}

fn on(mut c: Control) -> Control {
    c.checked = true;
    c
}

fn is_click(p: &Planned) -> bool {
    matches!(p, Planned::Act { action: Action::Click { .. }, .. })
}

#[tokio::test]
async fn a_switch_is_clicked_only_when_it_is_not_already_as_asked() {
    let off = || control("switch", "Notify", "button", "");
    assert!(is_click(&plan(vec![off()], "Notify", FieldValue::Bool(true)).await));
    assert_eq!(plan(vec![on(off())], "Notify", FieldValue::Bool(true)).await, Planned::AlreadySet);
    // Turning one off is a click too, and an off one asked off is set.
    assert!(is_click(&plan(vec![on(off())], "Notify", FieldValue::Bool(false)).await));
    assert_eq!(plan(vec![off()], "Notify", FieldValue::Bool(false)).await, Planned::AlreadySet);
}

#[tokio::test]
async fn a_radio_asked_true_is_clicked_unless_it_is_chosen() {
    let radio = || control("radio", "Male", "input", "radio");
    assert!(is_click(&plan(vec![radio()], "Male", FieldValue::Bool(true)).await));
    assert_eq!(plan(vec![on(radio())], "Male", FieldValue::Bool(true)).await, Planned::AlreadySet);
}

#[tokio::test]
async fn a_ticked_checkbox_asked_false_is_clicked() {
    let ticked = on(control("checkbox", "Active", "input", "checkbox"));
    assert!(is_click(&plan(vec![ticked], "Active", FieldValue::Bool(false)).await));
}

#[tokio::test]
async fn a_textarea_and_a_content_editable_are_filled() {
    let p = plan(vec![control("textbox", "Notes", "textarea", "")], "Notes", text("hello")).await;
    let Planned::Act { action: Action::Fill { value, .. }, .. } = p else { panic!("{p:?}") };
    assert_eq!(value, "hello");
    let mut rich = control("textbox", "Body", "div", "");
    rich.editable = true;
    let p = plan(vec![rich], "Body", text("hi")).await;
    let Planned::Act { action: Action::Fill { value, .. }, .. } = p else { panic!("{p:?}") };
    assert_eq!(value, "hi");
}
