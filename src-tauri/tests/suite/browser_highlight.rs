//! What the Auto Test browser draws over a step: the colour for each kind of
//! action, the label (cut, and never a secret), that the box is drawn
//! whether or not the step pauses, and that the result call never
//! changes what a step reports.

use crate::common::{FakePage, ScriptedDriver};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use v2_lib::autorun::setup::{resolve, FixtureValues};
use v2_lib::autorun::CaseScript;
use v2_lib::browser::actions::{execute_in, execute_with, Action, Policy, HIGHLIGHT_JS};
use v2_lib::browser::expect::IS_PASSWORD_JS;
use v2_lib::browser::cdp::{CdpError, Driver};
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
    assert_eq!(
        say(json!({ "kind": "upload", "selector": "#a", "file": "C:\\files\\Leave Dates.xlsx" })),
        "Upload 'Leave Dates.xlsx'"
    );
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

/// The box is always drawn: the Pause before each action choice only sets
/// whether the step waits on it. With the pause off (`highlight_ms` 0) the
/// box is still shown, with the right colour and label, nothing idles, and
/// the result still follows, for a click, a fill, a hover and a check alike.
#[tokio::test]
async fn with_the_pause_off_the_box_is_still_drawn_and_nothing_waits() {
    let cases = [
        (json!({ "kind": "click", "selector": "#a" }), "click", "Click"),
        (json!({ "kind": "fill", "selector": "#a", "value": "x" }), "type", "Type 'x'"),
        (json!({ "kind": "hover", "selector": "#a" }), "quiet", "Hover"),
        (json!({ "kind": "expect_visible", "selector": "#a" }), "check", "Check: visible"),
    ];
    for (step, kind, label) in cases {
        let clock = Arc::new(AtomicU64::new(0));
        let mut d = FakePage::default().driver();
        d.idle_clock = Some(clock.clone());
        let out = execute_with(&mut d, &act(step.clone()), &off()).await;
        assert!(out.ok, "{step}: {}", out.detail);
        assert_eq!(drawn(&d), vec![(kind.to_string(), label.to_string())], "{step}");
        assert_eq!(results(&d), vec![true], "{step}");
        // A hover has its own short settle; the pause is what must be absent.
        if step["kind"] != "hover" {
            assert_eq!(clock.load(Ordering::SeqCst), 0, "{step}: the step waited with the pause off");
        }
    }
}

/// With a pause of 350 ms: the box, then the 350 ms pause, then the action,
/// then the result.
#[tokio::test]
async fn with_a_350_ms_pause_the_box_comes_first_then_the_wait_then_the_action_then_the_result() {
    let clock = Arc::new(AtomicU64::new(0));
    let at_mouse = Arc::new(AtomicU64::new(u64::MAX));
    let (c, m) = (clock.clone(), at_mouse.clone());
    let page = FakePage::default();
    let mut d = ScriptedDriver::new(move |method, p| {
        if method == "Input.dispatchMouseEvent" {
            let _ = m.compare_exchange(u64::MAX, c.load(Ordering::SeqCst), Ordering::SeqCst, Ordering::SeqCst);
        }
        page.answer(method, p)
    });
    d.idle_clock = Some(clock.clone());
    let timing = Timing { highlight_ms: 350, ..off() };
    let out = execute_with(&mut d, &act(json!({ "kind": "click", "selector": "#go" })), &timing).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(at_mouse.load(Ordering::SeqCst), 350, "the pause was not over before the click");
    let show = position(&d, |_, p| p["functionDeclaration"] == HIGHLIGHT_JS);
    let first_mouse = position(&d, |m, _| m == "Input.dispatchMouseEvent");
    let result = position(&d, is_result);
    assert!(show < first_mouse && first_mouse < result);
    assert_eq!(results(&d), vec![true]);
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
    // The box is drawn, then the click itself is refused.
    let page = FakePage::default();
    let mut d = ScriptedDriver::new(move |m, p| {
        if m == "Input.dispatchMouseEvent" {
            return Err(CdpError::Protocol { method: m.into(), message: "Cannot find context with specified id".into() });
        }
        page.answer(m, p)
    });
    let out = execute_with(&mut d, &act(json!({ "kind": "click", "selector": "#go" })), &on()).await;
    assert!(!out.ok);
    assert_eq!(results(&d), vec![false]);
}

/// A step that never got as far as a box (the element was covered) has no
/// box to turn red, so no result call is made.
#[tokio::test]
async fn a_step_blocked_before_any_box_sends_no_result() {
    let mut covered = crate::common::ready_probe();
    covered["hit"] = json!(false);
    covered["covered_by"] = json!("div.modal-backdrop");
    let mut d = FakePage { probes: vec![covered], ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &act(json!({ "kind": "click", "selector": "#go" })), &on()).await;
    assert!(!out.ok);
    assert!(drawn(&d).is_empty());
    assert!(results(&d).is_empty());
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
    // A page whose CSP has no 'unsafe-inline' for styles blocks the style
    // attribute, leaving the box as bare text; the CSSOM is not blocked.
    assert!(!SHOW_JS.contains("setAttribute('style'"), "styles go through the CSSOM");
    assert!(SHOW_JS.contains("e.style.cssText = t"));
    assert!(RESULT_JS.contains("800") && RESULT_JS.contains("2000"), "about 0.8 s on success, 2 s on failure");
}

/// The calls that carry `value` anywhere in them.
fn carrying<'a>(d: &'a ScriptedDriver, value: &str) -> Vec<&'a (String, Value)> {
    d.calls.iter().filter(|(_, p)| p.to_string().contains(value)).collect()
}

/// Nothing about the highlight - the box, its label, its result - may carry
/// `value`. The fill itself has to (it types it): that is the one place it
/// goes, and it is not the highlight's.
fn assert_value_only_typed(d: &ScriptedDriver, value: &str) {
    for (m, p) in carrying(d, value) {
        assert!(p["functionDeclaration"] != HIGHLIGHT_JS, "the box carried the value: {m} {p}");
        assert!(!p.to_string().contains("tcm.action-highlight"), "a highlight call carried the value: {m} {p}");
    }
    assert!(carrying(d, value).len() <= 2, "the value went to more than the typing itself");
}

/// The app's own sign-in and reset fill a password that was resolved before
/// the step ran, so the label cannot know it by looking at the value: a
/// runner-own fill is always the hidden value, whatever the field is.
#[tokio::test]
async fn a_runner_own_fill_is_hidden_in_any_field_and_the_value_reaches_no_highlight_call() {
    let mut d = FakePage::default().driver();
    let fill = act(json!({ "kind": "fill", "selector": "#nickname", "value": "Tr0ub4dor-3" }));
    let out = execute_in(&mut d, &fill, &on(), &Policy::open().as_runner()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(drawn(&d), vec![("type".to_string(), TYPE_HIDDEN.to_string())]);
    assert_value_only_typed(&d, "Tr0ub4dor-3");
    assert_eq!(results(&d), vec![true]);
    // The same fill, not the runner's, still says what it types.
    let mut d = FakePage::default().driver();
    let _ = execute_in(&mut d, &fill, &on(), &Policy::open()).await;
    assert_eq!(drawn(&d), vec![("type".to_string(), "Type 'Tr0ub4dor-3'".to_string())]);
}

/// A page that cannot say whether the field is a password is treated as if
/// it were one.
#[tokio::test]
async fn a_failed_password_look_fails_closed() {
    let page = FakePage::default();
    let mut d = ScriptedDriver::new(move |m, p| {
        if p["functionDeclaration"] == IS_PASSWORD_JS {
            return Err(CdpError::Protocol { method: m.into(), message: "Cannot find context with specified id".into() });
        }
        page.answer(m, p)
    });
    let out = execute_with(&mut d, &act(json!({ "kind": "fill", "selector": "#a", "value": "hunter2" })), &on()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(drawn(&d), vec![("type".to_string(), TYPE_PASSWORD.to_string())]);
    assert_value_only_typed(&d, "hunter2");
    // A check likewise.
    let page = FakePage { texts: vec!["hunter2"], ..FakePage::default() };
    let mut d = ScriptedDriver::new(move |m, p| {
        if p["functionDeclaration"] == IS_PASSWORD_JS {
            return Err(CdpError::Protocol { method: m.into(), message: "gone".into() });
        }
        page.answer(m, p)
    });
    let _ = execute_with(
        &mut d,
        &act(json!({ "kind": "expect_contains_text", "selector": "#a", "value": "hunter2" })),
        &on(),
    )
    .await;
    let labels = drawn(&d);
    assert!(!labels.is_empty());
    for (_, label) in labels {
        assert_eq!(label, CHECK_HIDDEN);
    }
}

/// A box the page will not take never fails the step, and with no box there
/// is no result to send.
#[tokio::test]
async fn a_box_that_could_not_be_drawn_fails_nothing_and_owes_no_result() {
    let page = FakePage::default();
    let mut d = ScriptedDriver::new(move |m, p| {
        if p["functionDeclaration"] == HIGHLIGHT_JS {
            return Err(CdpError::Protocol { method: m.into(), message: "Cannot find context with specified id".into() });
        }
        page.answer(m, p)
    });
    for step in [
        json!({ "kind": "click", "selector": "#go" }),
        json!({ "kind": "fill", "selector": "#a", "value": "x" }),
        json!({ "kind": "hover", "selector": "#a" }),
    ] {
        let out = execute_with(&mut d, &act(step.clone()), &on()).await;
        assert!(out.ok, "{step}: {}", out.detail);
    }
    assert!(results(&d).is_empty(), "a result was sent for a box that was never drawn");
}

/// A box drawn in a frame's own window can never be reached by the result
/// call (it runs in the top window), so it is given a short life of its own:
/// the pause and two seconds. The overlay is hidden from assistive technology.
#[test]
fn a_box_in_a_frames_own_window_has_a_short_life_and_the_overlay_is_hidden_from_assistive_technology() {
    assert!(SHOW_JS.contains("function(kind, label, holdMs)"));
    assert!(SHOW_JS.contains("win === win.top ? 20000 : (Number(holdMs) || 0) + 2000"));
    assert!(SHOW_JS.contains("setAttribute('aria-hidden', 'true')"));
}

#[tokio::test]
async fn the_pause_is_handed_to_the_box() {
    let mut d = FakePage::default().driver();
    let _ = execute_with(&mut d, &act(json!({ "kind": "click", "selector": "#go" })), &on()).await;
    let shown: Vec<_> = d.calls.iter().filter(|(_, p)| p["functionDeclaration"] == HIGHLIGHT_JS).collect();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].1["arguments"][2]["value"], json!(5));
}

fn resolved(value: &str, fixtures: &BTreeMap<String, FixtureValues>, setup: BTreeMap<String, Value>) -> Action {
    let sc: CaseScript = serde_json::from_value(json!({
        "case_id": 1, "title": "case 1", "setup": { "fixture": "own" },
        "steps": [{ "step_number": 1, "actions": [
            { "kind": "fill", "selector": { "css": "#a" }, "value": value }
        ] }]
    }))
    .unwrap();
    let out = resolve(&sc, fixtures, &setup).unwrap();
    out.steps[0].actions[0].clone()
}

/// A setup's or a fixture's output named like a secret is filled in before
/// the step runs; the resolved fill carries that, and its label hides it.
#[test]
fn a_setup_or_fixture_secret_is_hidden_once_resolved() {
    let fixtures = BTreeMap::from([(
        "api".to_string(),
        FixtureValues {
            name: "Api".into(),
            outputs: Some([("token".to_string(), json!("tok-9f8e7d")), ("cycle".to_string(), json!("Q3"))].into()),
        },
    )]);
    let setup: BTreeMap<String, Value> =
        [("password".to_string(), json!("S3cret-pw")), ("cycle_name".to_string(), json!("Draft 7"))].into();

    let a = resolved("{{setup.password}}", &fixtures, setup.clone());
    assert!(matches!(a, Action::Fill { sensitive: true, ref value, .. } if value == "S3cret-pw"), "{a:?}");
    assert_eq!(Say::of(&a).label, TYPE_HIDDEN);

    let a = resolved("Bearer {{fixture.api.token}}", &fixtures, setup.clone());
    assert!(matches!(a, Action::Fill { sensitive: true, .. }), "{a:?}");
    assert_eq!(Say::of(&a).label, TYPE_HIDDEN);

    // Nothing secret-named: shown as ever, and not marked.
    let a = resolved("{{setup.cycle_name}}", &fixtures, setup.clone());
    assert!(matches!(a, Action::Fill { sensitive: false, .. }), "{a:?}");
    assert_eq!(Say::of(&a).label, "Type 'Draft 7'");
    let a = resolved("{{fixture.api.cycle}}", &fixtures, setup);
    assert_eq!(Say::of(&a).label, "Type 'Q3'");
}

/// The mark is carried into a fill nested in a guard as well.
#[test]
fn the_mark_reaches_a_fill_inside_a_guard() {
    let sc: CaseScript = serde_json::from_value(json!({
        "case_id": 1, "title": "case 1", "setup": { "fixture": "own" },
        "steps": [{ "step_number": 1, "actions": [
            { "kind": "when_visible", "selector": { "css": "#b" }, "then": [
                { "kind": "fill", "selector": { "css": "#c" }, "value": "{{setup.password}}" }
            ] }
        ] }]
    }))
    .unwrap();
    let out = resolve(&sc, &BTreeMap::new(), &[("password".to_string(), json!("S3cret-pw"))].into()).unwrap();
    let Action::WhenVisible { then, .. } = &out.steps[0].actions[0] else { panic!("not a guard") };
    assert!(matches!(then[0], Action::Fill { sensitive: true, .. }), "{:?}", then[0]);
}

#[test]
fn the_drag_label_uses_the_drop_position_wording() {
    use v2_lib::browser::actions::DropAt;
    assert_eq!(highlight::drag_label(DropAt::Onto), "Drag");
    let before = highlight::drag_label(DropAt::Before);
    assert_eq!(before, format!("Drag {}", DropAt::Before.word()));
    assert_eq!(
        Say::of(&act(json!({ "kind": "drag", "from": "#a", "to": "#b", "position": "before" }))).label,
        before
    );
}

/// A check of a secret-named or password element says nothing of its text.
#[test]
fn a_check_of_a_secret_shows_no_value() {
    let say = |v: Value| Say::of(&act(v)).label;
    for step in [
        json!({ "kind": "expect_contains_text", "selector": "#password", "value": "hunter2" }),
        json!({ "kind": "expect_text", "selector": "#api-token", "equals": "hunter2" }),
        json!({ "kind": "expect_text", "selector": "#password", "same_as": "kept" }),
        json!({ "kind": "check_text", "value": "{{password}}" }),
        json!({ "kind": "check_text", "value": format!("{}hunter2", v2_lib::browser::expect::NOTED_SECRET) }),
    ] {
        let label = say(step.clone());
        assert_eq!(label, CHECK_HIDDEN, "{step}");
        assert!(!label.contains("hunter2"));
    }
    // Any other element: the text is shown, until the page says it is a password.
    assert_eq!(say(json!({ "kind": "expect_text", "selector": "#a", "equals": "Saved" })), "Check: text is 'Saved'");
}

fn resolved_action(action: Value) -> Action {
    let sc: CaseScript = serde_json::from_value(json!({
        "case_id": 1, "title": "case 1", "setup": { "fixture": "own" },
        "steps": [{ "step_number": 1, "actions": [action] }]
    }))
    .unwrap();
    let fixtures = BTreeMap::from([(
        "api".to_string(),
        FixtureValues { name: "Api".into(), outputs: Some([("token".to_string(), json!("tok-9f8e7d"))].into()) },
    )]);
    let setup: BTreeMap<String, Value> = [("password".to_string(), json!("S3cret-pw"))].into();
    resolve(&sc, &fixtures, &setup).unwrap().steps[0].actions[0].clone()
}

/// A check of a setup's or a fixture's secret is marked once resolved, so no
/// highlight call carries what it compares.
#[tokio::test]
async fn a_check_of_a_setup_or_fixture_secret_shows_no_value() {
    let steps = [
        (json!({ "kind": "expect_text", "selector": "#generated-key", "equals": "{{fixture.api.token}}" }), "tok-9f8e7d"),
        (json!({ "kind": "check_text", "value": "{{setup.password}}" }), "S3cret-pw"),
        (json!({ "kind": "expect_contains_text", "selector": "#note", "value": "k {{fixture.api.token}}" }), "tok-9f8e7d"),
    ];
    for (step, secret) in steps {
        let action = resolved_action(step.clone());
        assert_eq!(Say::of(&action).label, CHECK_HIDDEN, "{step}");
        let mut d = FakePage { texts: vec!["tok-9f8e7d", "S3cret-pw", "k tok-9f8e7d"], ..FakePage::default() }.driver();
        let _ = execute_with(&mut d, &action, &on()).await;
        for (_, p) in &d.calls {
            let text = p.to_string();
            if text.contains(secret) {
                assert!(
                    p["functionDeclaration"] != HIGHLIGHT_JS && !text.contains("tcm.action-highlight"),
                    "a highlight call carried the value: {p}"
                );
            }
        }
    }
}

/// A box drawn in a frame's own window also clears one the top window still
/// holds (where it can be read), so an earlier step's box is never the one
/// the result recolours.
#[test]
fn a_box_in_a_frame_clears_the_one_the_top_window_still_holds() {
    assert!(SHOW_JS.contains("if (win !== win.top)"));
    assert!(SHOW_JS.contains("win.top[KEY]"));
    assert!(SHOW_JS.contains("above.stop()"));
}

/// When the page script says it drew nothing (it caught an error), there is
/// no pause and no result call.
#[tokio::test]
async fn a_script_that_drew_nothing_means_no_pause_and_no_result() {
    let page = FakePage::default();
    let mut d = ScriptedDriver::new(move |m, p| {
        if p["functionDeclaration"] == HIGHLIGHT_JS {
            return Ok(json!({ "result": { "value": false } }));
        }
        page.answer(m, p)
    });
    let out = execute_with(&mut d, &act(json!({ "kind": "click", "selector": "#go" })), &on()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(results(&d).is_empty());
}

/// `sensitive` is a run-time mark: sent in, it is read; written out, it is not.
#[test]
fn the_sensitive_mark_is_never_written() {
    let sc: CaseScript = serde_json::from_value(json!({
        "case_id": 1, "title": "case 1",
        "steps": [{ "step_number": 1, "actions": [
            { "kind": "fill", "selector": { "css": "#a" }, "value": "x", "sensitive": true },
            { "kind": "check_text", "value": "y", "sensitive": true }
        ] }]
    }))
    .unwrap();
    assert!(matches!(sc.steps[0].actions[0], Action::Fill { sensitive: true, .. }));
    assert!(!serde_json::to_string(&sc).unwrap().contains("sensitive"));
}

#[test]
fn a_saved_script_and_a_kept_action_come_back_without_the_mark() {
    let dir = tempfile::tempdir().unwrap();
    let sc: CaseScript = serde_json::from_value(json!({
        "case_id": 7, "title": "case 7",
        "steps": [{ "step_number": 1, "actions": [
            { "kind": "fill", "selector": { "css": "#a" }, "value": "x", "sensitive": true }
        ] }]
    }))
    .unwrap();
    v2_lib::autorun::store::save_script(dir.path(), &sc).unwrap();
    let back = v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap();
    assert!(matches!(back.steps[0].actions[0], Action::Fill { sensitive: false, .. }));
    let file = std::fs::read_to_string(dir.path().join("autorun").join("scripts").join("case-7.json"));
    if let Ok(text) = file {
        assert!(!text.contains("sensitive"), "{text}");
    }
    let action = sc.steps[0].actions[0].clone();
    v2_lib::autorun::kept::keep(dir.path(), "s1", &action, None, None, 1, 2).unwrap();
    let list = v2_lib::autorun::kept::list_of(dir.path(), "s1").unwrap();
    for k in &list.actions {
        assert!(!serde_json::to_string(&k.action).unwrap().contains("sensitive"));
    }
}

/// A script is prepared in more than one pass (fixtures first, then the
/// setup's outputs, then later passes): a mark one pass put on a fill or a
/// check survives the next pass over the same steps.
#[test]
fn the_hidden_mark_survives_a_second_pass_over_the_steps() {
    let first: CaseScript = serde_json::from_value(json!({
        "case_id": 1, "title": "case 1", "setup": { "fixture": "own" },
        "steps": [{ "step_number": 1, "actions": [
            { "kind": "fill", "selector": { "css": "#a" }, "value": "{{fixture.api.token}}" },
            { "kind": "expect_text", "selector": { "css": "#generated-key" }, "equals": "{{fixture.api.token}}" }
        ] }]
    }))
    .unwrap();
    let fixtures = BTreeMap::from([(
        "api".to_string(),
        FixtureValues { name: "Api".into(), outputs: Some([("token".to_string(), json!("tok-9f8e7d"))].into()) },
    )]);
    let pass_one = resolve(&first, &fixtures, &BTreeMap::new()).unwrap();
    assert!(matches!(pass_one.steps[0].actions[0], Action::Fill { sensitive: true, .. }));

    // The second pass: a step that names the setup's output joins the script.
    let mut second = pass_one.clone();
    second.steps[0].actions.push(act(json!({ "kind": "fill", "selector": "#b", "value": "{{setup.cycle_name}}" })));
    let pass_two = resolve(&second, &BTreeMap::new(), &[("cycle_name".to_string(), json!("Draft 7"))].into()).unwrap();
    let actions = &pass_two.steps[0].actions;
    assert_eq!(Say::of(&actions[0]).label, TYPE_HIDDEN);
    assert_eq!(Say::of(&actions[1]).label, CHECK_HIDDEN);
    assert_eq!(Say::of(&actions[2]).label, "Type 'Draft 7'");
    // Nothing that is written out carries it.
    assert!(!serde_json::to_string(&pass_two).unwrap().contains("sensitive"));
}

/// The mark is a run-time one: a panic inside a marked pass cannot leave the
/// thread writing it.
#[test]
fn a_panic_inside_a_marked_pass_leaves_no_mark_written() {
    let fill = Action::Fill { selector: "#a".into(), value: "x".into(), sensitive: true };
    let _ = std::panic::catch_unwind(|| {
        v2_lib::browser::actions::with_run_marks(|| panic!("inside"));
    });
    assert!(!serde_json::to_string(&fill).unwrap().contains("sensitive"));
    let inside = v2_lib::browser::actions::with_run_marks(|| serde_json::to_string(&fill).unwrap());
    assert!(inside.contains("sensitive"));
}

/// What a marked check says about itself - a pass, a failure that sets the
/// expected text against what was seen - never holds what it compares.
#[tokio::test]
async fn a_marked_checks_outcome_never_holds_the_compared_text() {
    let secret = "S3cret-pw \"q\"";
    let marked = |v: Value| {
        let mut v = v;
        v["sensitive"] = json!(true);
        act(v)
    };
    let checks = [
        marked(json!({ "kind": "check_text", "value": secret })),
        marked(json!({ "kind": "expect_contains_text", "selector": "#a", "value": secret })),
        marked(json!({ "kind": "expect_text", "selector": "#a", "equals": secret })),
    ];
    for check in checks {
        // Seen text that differs (a failure) and seen text that is the same (a pass).
        for texts in [vec!["something else"], vec!["S3cret-pw \"q\""]] {
            let mut d = FakePage { texts, ..FakePage::default() }.driver();
            let out = execute_with(&mut d, &check, &off()).await;
            assert!(!out.detail.contains("S3cret-pw"), "{check:?}: {}", out.detail);
        }
    }
}

/// The failure still says what was wrong, with the hidden value in its place.
#[tokio::test]
async fn a_marked_checks_failure_says_the_hidden_value_in_place_of_the_text() {
    let check = act(json!({ "kind": "expect_contains_text", "selector": "#a", "value": "S3cret-pw", "sensitive": true }));
    let mut d = FakePage { texts: vec!["something else"], ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &check, &off()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("the hidden value") && !out.detail.contains("S3cret"), "{}", out.detail);
}

#[tokio::test]
async fn the_label_drawn_is_noted_for_the_video() {
    let mut d = FakePage { password: true, ..FakePage::default() }.driver();
    execute_with(&mut d, &act(json!({ "kind": "fill", "selector": "#a", "value": "hunter2" })), &off()).await;
    assert_eq!(d.take_drawn_label().as_deref(), Some(TYPE_PASSWORD));
    assert_eq!(d.take_drawn_label(), None, "taking it empties it");
    let mut d = FakePage::default().driver();
    execute_with(&mut d, &act(json!({ "kind": "fill", "selector": "#a", "value": "Smith" })), &off()).await;
    assert_eq!(d.take_drawn_label().as_deref(), Some("Type 'Smith'"));
}

#[tokio::test]
async fn a_box_the_page_would_not_draw_leaves_no_drawn_label() {
    let page = FakePage::default();
    let mut d = ScriptedDriver::new(move |m, p| {
        if m == "Runtime.callFunctionOn" && p["functionDeclaration"] == HIGHLIGHT_JS {
            Ok(json!({ "result": { "value": false } }))
        } else {
            page.answer(m, p)
        }
    });
    execute_with(&mut d, &act(json!({ "kind": "fill", "selector": "#a", "value": "Smith" })), &off()).await;
    assert_eq!(d.take_drawn_label(), None);
}

#[test]
fn page_level_actions_say_what_they_do_in_words() {
    let say = |v: Value| Say::of(&act(v)).label;
    assert_eq!(say(json!({ "kind": "navigate", "url": "https://app.test/" })), "Open a page");
    assert_eq!(say(json!({ "kind": "reload" })), "Reload the page");
    assert_eq!(say(json!({ "kind": "expire_session" })), "End the session");
    assert_eq!(say(json!({ "kind": "open_tab", "name": "t", "url": "https://app.test/" })), "Open a tab");
    assert_eq!(say(json!({ "kind": "switch_tab", "name": "t" })), "Switch tab");
    assert_eq!(say(json!({ "kind": "close_tab", "name": "t" })), "Close a tab");
    assert_eq!(say(json!({ "kind": "sign_in", "account": "admin" })), "Sign in");
}
