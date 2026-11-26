//! What the Auto Test browser draws over a step: the colour for each kind of
//! action, the label (cut, and never a secret), that nothing is sent or
//! waited for with the highlight off, and that the result call never
//! changes what a step reports.

use crate::common::{FakePage, ScriptedDriver};
use serde_json::{json, Value};
use v2_lib::browser::actions::{execute_with, Action, HIGHLIGHT_JS};
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::highlight::{self, kind_of, Kind, Say, CHECK_HIDDEN, RESULT_JS, SHOW_JS, TYPE_HIDDEN, TYPE_PASSWORD};
use v2_lib::browser::timing::Timing;

fn off() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn on() -> Timing {
    Timing { highlight_ms: 5, ..off() }
}

fn act(v: Value) -> Action {
    serde_json::from_value(v).unwrap_or_else(|e| panic!("not an action: {e}"))
}

/// Every call that drew a box: its colour key and its label.
fn drawn(d: &ScriptedDriver) -> Vec<(String, String)> {
    d.calls
        .iter()
        .filter(|(_, p)| p["functionDeclaration"] == HIGHLIGHT_JS)
        .map(|(_, p)| {
            let a = &p["arguments"];
            (a[0]["value"].as_str().unwrap().to_string(), a[1]["value"].as_str().unwrap().to_string())
        })
        .collect()
}

fn is_result(m: &str, p: &Value) -> bool {
    m == "Runtime.evaluate" && p["expression"].as_str().is_some_and(|e| e.contains("tcm.action-highlight"))
}

/// The `ok` of every call that set a result.
fn results(d: &ScriptedDriver) -> Vec<bool> {
    d.calls
        .iter()
        .filter(|(m, p)| is_result(m, p))
        .map(|(_, p)| {
            let e = p["expression"].as_str().unwrap();
            assert!(e.starts_with(&format!("({RESULT_JS})(")), "{e}");
            e.ends_with("(true)")
        })
        .collect()
}

fn position(d: &ScriptedDriver, pick: impl Fn(&str, &Value) -> bool) -> usize {
    d.calls.iter().position(|(m, p)| pick(m, p)).expect("the call was never made")
}

/// Every kind of action has a colour: blue for a click, a key press or a
/// drag; green for a value put in; amber for a check; grey for hover and the
/// steps that only look or note.
#[test]
fn every_kind_of_action_has_its_colour() {
    let table: Vec<(Value, Kind)> = vec![
        (json!({ "kind": "click", "selector": "#a" }), Kind::Click),
        (json!({ "kind": "press_key", "key": "Enter" }), Kind::Click),
        (json!({ "kind": "drag", "from": "#a", "to": "#b" }), Kind::Click),
        (json!({ "kind": "page_until", "next": "#n", "until": "#u", "max_pages": 3 }), Kind::Click),
        (json!({ "kind": "navigate", "url": "https://app.example/x" }), Kind::Click),
        (json!({ "kind": "reload" }), Kind::Click),
        (json!({ "kind": "open_tab", "name": "t", "url": "https://app.example/x" }), Kind::Click),
        (json!({ "kind": "switch_tab", "name": "t" }), Kind::Click),
        (json!({ "kind": "close_tab", "name": "t" }), Kind::Click),
        (json!({ "kind": "fill", "selector": "#a", "value": "x" }), Kind::Type),
        (json!({ "kind": "select_option", "selector": "#a", "label": "x" }), Kind::Type),
        (json!({ "kind": "upload", "selector": "#a", "file": "f.xlsx" }), Kind::Type),
        (json!({ "kind": "sign_in", "account": "tester" }), Kind::Type),
        (json!({ "kind": "check_text", "value": "x" }), Kind::Check),
        (json!({ "kind": "check_url", "contains": "x" }), Kind::Check),
        (json!({ "kind": "wait_for", "selector": "#a", "timeout_ms": 1000 }), Kind::Check),
        (json!({ "kind": "expect_visible", "selector": "#a" }), Kind::Check),
        (json!({ "kind": "expect_hidden", "selector": "#a" }), Kind::Check),
        (json!({ "kind": "expect_text", "selector": "#a", "equals": "x" }), Kind::Check),
        (json!({ "kind": "expect_contains_text", "selector": "#a", "value": "x" }), Kind::Check),
        (json!({ "kind": "expect_count", "selector": "#a", "equals": 2 }), Kind::Check),
        (json!({ "kind": "expect_enabled", "selector": "#a" }), Kind::Check),
        (json!({ "kind": "expect_no_change", "selector": "#a" }), Kind::Check),
        (json!({ "kind": "expect_focused", "selector": "#a" }), Kind::Check),
        (json!({ "kind": "hover", "selector": "#a" }), Kind::Quiet),
        (json!({ "kind": "remember_url", "as": "u" }), Kind::Quiet),
        (json!({ "kind": "remember_text", "selector": "#a", "as": "t" }), Kind::Quiet),
    ];
    for (json, want) in table {
        let kind = kind_of(&act(json.clone()));
        assert_eq!(kind, want, "{json}");
    }
    let names: Vec<_> = [Kind::Click, Kind::Type, Kind::Check, Kind::Quiet].iter().map(|k| k.colour()).collect();
    assert_eq!(names, ["blue", "green", "amber", "grey"]);
}

#[test]
fn labels_say_what_the_step_does() {
    let say = |v: Value| Say::of(&act(v)).label;
    assert_eq!(say(json!({ "kind": "click", "selector": "#a" })), "Click");
    assert_eq!(say(json!({ "kind": "hover", "selector": "#a" })), "Hover");
    assert_eq!(say(json!({ "kind": "fill", "selector": "#name", "value": "Smith" })), "Type 'Smith'");
    assert_eq!(say(json!({ "kind": "select_option", "selector": "#a", "label": "Manager" })), "Choose 'Manager'");
    assert_eq!(say(json!({ "kind": "upload", "selector": "#a", "file": "file.xlsx" })), "Upload 'file.xlsx'");
    assert_eq!(say(json!({ "kind": "check_text", "value": "Saved" })), "Check: shows 'Saved'");
    assert_eq!(say(json!({ "kind": "expect_contains_text", "selector": "#a", "value": "Saved" })), "Check: shows 'Saved'");
}

#[test]
fn a_long_value_is_cut_to_24_characters() {
    let long = "abcdefghijklmnopqrstuvwxyz0123456789";
    let label = Say::of(&act(json!({ "kind": "fill", "selector": "#name", "value": long }))).label;
    assert_eq!(label, "Type 'abcdefghijklmnopqrstuvwx\u{2026}'");
    let exact = "abcdefghijklmnopqrstuvwx";
    assert_eq!(exact.chars().count(), 24);
    assert_eq!(highlight::shown(exact), exact);
    assert_eq!(highlight::shown("a  b\n c"), "a b c");
}

#[test]
fn a_secret_is_never_in_a_label() {
    // A field named like a secret.
    let say = Say::of(&act(json!({ "kind": "fill", "selector": "#password", "value": "hunter2" })));
    assert_eq!(say.label, TYPE_HIDDEN);
    // A text noted from a password field carries the marker.
    let noted = format!("{}hunter2", v2_lib::browser::expect::NOTED_SECRET);
    let say = Say::of(&act(json!({ "kind": "fill", "selector": "#a", "value": noted })));
    assert_eq!(say.label, TYPE_HIDDEN);
    // The sign-in recipe's placeholder, should one ever reach the page unresolved.
    let say = Say::of(&act(json!({ "kind": "fill", "selector": "#a", "value": "{{password}}" })));
    assert_eq!(say.label, TYPE_HIDDEN);
    // An ordinary field turns into a password label once the page says it is one.
    let say = Say::of(&act(json!({ "kind": "fill", "selector": "#a", "value": "hunter2" })));
    assert_eq!(say.if_password.as_deref(), Some(TYPE_PASSWORD));
}

#[tokio::test]
async fn a_password_input_says_type_password_and_the_value_never_reaches_the_script() {
    let mut d = FakePage { password: true, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &act(json!({ "kind": "fill", "selector": "#a", "value": "hunter2" })), &on()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(drawn(&d), vec![("type".to_string(), TYPE_PASSWORD.to_string())]);
    for (_, p) in d.calls.iter().filter(|(_, p)| p["functionDeclaration"] == HIGHLIGHT_JS) {
        assert!(!p.to_string().contains("hunter2"), "{p}");
    }
    assert_eq!(results(&d), vec![true]);
}

#[tokio::test]
async fn a_check_of_a_password_field_shows_no_text() {
    let mut d = FakePage { password: true, texts: vec!["hunter2"], ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &act(json!({ "kind": "expect_contains_text", "selector": "#a", "value": "hunter2" })),
        &on(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(drawn(&d), vec![("check".to_string(), CHECK_HIDDEN.to_string())]);
}

/// With the highlight off nothing is drawn and no result is sent, for a
/// click, a fill, a hover and a check alike.
#[tokio::test]
async fn with_the_highlight_off_the_script_is_never_called() {
    for step in [
        json!({ "kind": "click", "selector": "#a" }),
        json!({ "kind": "fill", "selector": "#a", "value": "x" }),
        json!({ "kind": "hover", "selector": "#a" }),
        json!({ "kind": "expect_visible", "selector": "#a" }),
        json!({ "kind": "expect_no_change", "selector": "#a" }),
    ] {
        let mut d = FakePage::default().driver();
        let _ = execute_with(&mut d, &act(step.clone()), &off()).await;
        assert!(drawn(&d).is_empty(), "{step}");
        assert!(results(&d).is_empty(), "{step}");
        assert!(
            !d.calls.iter().any(|(_, p)| p.to_string().contains("tcm.action-highlight")),
            "{step}: something about the highlight was sent"
        );
    }
}

/// With it on, the box is drawn before the action, with the right colour
/// and label, and the result follows once the step is over.
#[tokio::test]
async fn with_the_highlight_on_a_click_is_drawn_first_and_its_result_follows() {
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &act(json!({ "kind": "click", "selector": "#go" })), &on()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(drawn(&d), vec![("click".to_string(), "Click".to_string())]);
    let show = position(&d, |_, p| p["functionDeclaration"] == HIGHLIGHT_JS);
    let first_mouse = position(&d, |m, _| m == "Input.dispatchMouseEvent");
    let last_mouse = d.calls.iter().rposition(|(m, _)| m == "Input.dispatchMouseEvent").unwrap();
    let result = position(&d, is_result);
    assert!(show < first_mouse && last_mouse < result);
    assert_eq!(results(&d), vec![true]);
}

#[tokio::test]
async fn a_step_that_failed_turns_the_box_red() {
    let mut covered = crate::common::ready_probe();
    covered["hit"] = json!(false);
    covered["covered_by"] = json!("div.modal-backdrop");
    let mut d = FakePage { probes: vec![covered], ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &act(json!({ "kind": "click", "selector": "#go" })), &on()).await;
    assert!(!out.ok);
    assert_eq!(results(&d), vec![false]);
}

/// A check is drawn once it has finished, over the one element, with its
/// verdict.
#[tokio::test]
async fn a_finished_check_is_drawn_amber_then_given_its_verdict() {
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &act(json!({ "kind": "expect_visible", "selector": "#a" })), &on()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(drawn(&d), vec![("check".to_string(), "Check: visible".to_string())]);
    assert_eq!(results(&d), vec![true]);
}

/// A highlight never changes what a step reports: the call that sets the
/// result may fail in any way and the outcome is the same.
#[tokio::test]
async fn a_failing_result_call_changes_nothing() {
    let page = FakePage::default();
    let mut d = ScriptedDriver::new(move |m, p| {
        if is_result(m, p) {
            return Err(CdpError::Protocol { method: m.into(), message: "Cannot find context with specified id".into() });
        }
        page.answer(m, p)
    });
    let out = execute_with(&mut d, &act(json!({ "kind": "click", "selector": "#go" })), &on()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "clicked #go");
    assert!(!out.harness);
    assert_eq!(results(&d).len(), 1, "the result was still tried");
}

/// The injected scripts, read as text: drawn over the page in a closed
/// shadow root, never on the element, nothing for the site's CSS to restyle
/// or its clicks to land on.
#[test]
fn the_overlay_cannot_be_restyled_or_clicked_and_leaves_the_element_alone() {
    for want in [
        "pointer-events: none",
        "2147483647",
        "mode: 'closed'",
        "position: fixed",
        "requestAnimationFrame",
        "host.remove()",
        "#2563eb",
        "#16a34a",
        "#d97706",
        "#64748b",
        "#dc2626",
        "documentElement",
        "frameElement",
    ] {
        assert!(SHOW_JS.contains(want), "{want}");
    }
    assert!(!SHOW_JS.contains("el.style") && !SHOW_JS.contains("this.style"), "it must not touch the element's style");
    assert!(!SHOW_JS.contains("outline"));
    assert!(!SHOW_JS.contains("innerHTML"), "the label is text, never markup");
    assert!(RESULT_JS.contains("800") && RESULT_JS.contains("2000"), "about 0.8 s on success, 2 s on failure");
}
