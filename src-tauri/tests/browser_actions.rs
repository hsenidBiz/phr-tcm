//! What each action does to the browser, and how the executor reports it.
//! A described fake page stands in for Edge, so every rule here is pinned
//! without opening a window. `tests/browser_live.rs` checks the same things
//! against the real one.

mod common;

use common::{ready_probe, FakePage, ScriptedDriver};
use serde_json::json;
use v2_lib::browser::actions::{execute_with, Action};
use v2_lib::browser::cdp::{CdpError, Event};
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0 }
}

/// Every script saved before locators existed has string selectors. They
/// must keep parsing, and keep meaning what they meant.
#[test]
fn a_script_written_before_locators_still_parses() {
    let old = json!([
        { "kind": "navigate", "url": "https://app.example/login" },
        { "kind": "wait_for", "selector": "#user", "timeout_ms": 5000 },
        { "kind": "fill", "selector": "#user", "value": "tester" },
        { "kind": "click", "selector": "text=Sign in" },
        { "kind": "check_text", "value": "Dashboard" },
        { "kind": "check_url", "contains": "/home" }
    ]);
    let actions: Vec<Action> = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(actions.len(), 6);
    assert_eq!(serde_json::to_value(&actions).unwrap(), old, "and they go back out unchanged");
}

#[test]
fn an_action_can_point_with_a_locator() {
    let a: Action = serde_json::from_value(json!({
        "kind": "click",
        "selector": [{ "role": "dialog", "name": "Add Rating Method" }, { "role": "button", "name": "Add Method" }]
    }))
    .unwrap();
    assert!(a.validate().is_ok());
}

#[test]
fn validation_catches_what_would_only_fail_at_run_time() {
    let bad = |v: serde_json::Value| serde_json::from_value::<Action>(v).unwrap().validate().unwrap_err();
    assert!(bad(json!({ "kind": "click", "selector": {} })).contains("role, text or css"));
    assert!(bad(json!({ "kind": "navigate", "url": "javascript:alert(1)" })).contains("http"));
    assert!(bad(json!({ "kind": "navigate", "url": "" })).contains("http"));
    assert!(bad(json!({ "kind": "check_text", "value": " " })).contains("empty"));
    assert!(bad(json!({ "kind": "check_url", "contains": "" })).contains("empty"));
}

/// Highlight first so the watcher sees WHERE, then a real click at the
/// point the probe measured.
#[tokio::test]
async fn a_click_highlights_then_sends_real_mouse_events() {
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &Action::Click { selector: "#go".into() }, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "clicked #go");
    let methods = d.methods();
    let first_mouse = methods.iter().position(|m| m == "Input.dispatchMouseEvent").unwrap();
    let highlight = d
        .calls
        .iter()
        .position(|(_, p)| p["functionDeclaration"] == v2_lib::browser::actions::HIGHLIGHT_JS)
        .expect("never highlighted");
    assert!(highlight < first_mouse, "the highlight must come before the click");
    let mouse = d.calls_to("Input.dispatchMouseEvent");
    assert_eq!(mouse.len(), 3);
    assert_eq!((mouse[1]["x"].as_f64(), mouse[1]["y"].as_f64()), (Some(10.0), Some(20.0)));
}

/// The human is the oracle, so the executor's job is to report faithfully:
/// a missing element is a plain false with a reason, and nothing is clicked.
#[tokio::test]
async fn a_missing_element_fails_with_the_reason_and_clicks_nothing() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &Action::Click { selector: "#nope".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("not found") && out.detail.contains("#nope"), "{}", out.detail);
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty());
}

#[tokio::test]
async fn a_covered_element_is_not_clicked_through() {
    let mut covered = ready_probe();
    covered["hit"] = json!(false);
    covered["covered_by"] = json!("div.modal-backdrop");
    let mut d = FakePage { probes: vec![covered], ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &Action::Click { selector: "#go".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("covered by div.modal-backdrop"), "{}", out.detail);
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty());
}

/// A value with quotes and a closing script tag travels as data. It never
/// appears inside any JavaScript source this app sends.
#[tokio::test]
async fn a_fill_types_the_value_and_never_puts_it_in_source() {
    let nasty = "he said \"hi\"\n</script>";
    let mut d = FakePage::default().driver();
    let out = execute_with(
        &mut d,
        &Action::Fill { selector: "#user".into(), value: nasty.into() },
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "filled #user");
    assert_eq!(d.calls_to("Input.insertText")[0]["text"], nasty);
    for (method, params) in &d.calls {
        let source = params["functionDeclaration"].as_str().or(params["expression"].as_str()).unwrap_or("");
        assert!(!source.contains("he said"), "{method} carried the value in its source");
    }
}

#[tokio::test]
async fn a_fill_into_something_that_takes_no_text_says_so() {
    let mut not_editable = ready_probe();
    not_editable["editable"] = json!(false);
    let mut d = FakePage { probes: vec![not_editable], ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &Action::Fill { selector: "#logo".into(), value: "x".into() },
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.contains("cannot be typed into"), "{}", out.detail);
}

/// A dropped socket must not read as a failed assertion about the app
/// under test - it is a failure of the harness, and it says so.
#[tokio::test]
async fn a_transport_error_is_reported_as_a_harness_problem() {
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    let out = execute_with(&mut d, &Action::CheckText { value: "Dashboard".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("browser"), "{}", out.detail);
}

/// Waiting polls rather than sleeping a fixed guess, and gives up with a
/// verdict instead of hanging the run.
#[tokio::test]
async fn wait_for_polls_until_it_appears() {
    let mut d = FakePage { appears_on_look: 3, ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &Action::WaitFor { selector: "#late".into(), timeout_ms: 2000 },
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(d.calls_to("Runtime.getProperties").len(), 3);
}

/// `wait_for` calls `resolve` directly rather than going through
/// `wait_ready`, so a structured locator needs its own coverage alongside
/// the legacy-string case above.
#[tokio::test]
async fn wait_for_polls_until_a_structured_locator_appears() {
    let mut d = FakePage { appears_on_look: 3, ..FakePage::default() }.driver();
    let selector: v2_lib::browser::locator::Target =
        serde_json::from_value(json!({ "css": "#late" })).unwrap();
    let out = execute_with(
        &mut d,
        &Action::WaitFor { selector, timeout_ms: 2000 },
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(d.calls_to("Runtime.getProperties").len(), 3);
}

#[tokio::test]
async fn wait_for_gives_up_after_its_own_timeout() {
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &Action::WaitFor { selector: "#never".into(), timeout_ms: 120 },
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert!(out.detail.contains("120ms") && out.detail.contains("#never"), "{}", out.detail);
}

#[tokio::test]
async fn navigate_waits_for_the_page_to_load() {
    let mut d = FakePage::default().driver();
    // FakePage's default navigate reply is { frameId: "F", loaderId: "L" }.
    d.on_call_events.push((
        "Page.navigate".into(),
        Event {
            method: "Page.lifecycleEvent".into(),
            params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }),
        },
    ));
    // A stale lifecycle event from an earlier navigation must not satisfy
    // this one.
    d.events.push_back(Event {
        method: "Page.lifecycleEvent".into(),
        params: json!({ "frameId": "F", "loaderId": "OLD", "name": "load" }),
    });
    let out = execute_with(&mut d, &Action::Navigate { url: "https://app.example/login".into() }, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(d.calls_to("Page.navigate")[0]["url"], "https://app.example/login");
    assert!(d.events.is_empty(), "the navigation's own load event should have been consumed");
}

/// A load still in flight from an EARLIER navigation, or from a sub-frame
/// of this one, must not satisfy the wait; only the reply's own loader id,
/// under a `"load"` lifecycle event, does.
#[tokio::test]
async fn navigate_skips_a_sub_frames_load_and_an_earlier_lifecycle_stage() {
    let mut d = FakePage::default().driver();
    for (loader, name) in [("OTHER", "load"), ("L", "DOMContentLoaded"), ("L", "load")] {
        d.on_call_events.push((
            "Page.navigate".into(),
            Event {
                method: "Page.lifecycleEvent".into(),
                params: json!({ "frameId": "F", "loaderId": loader, "name": name }),
            },
        ));
    }
    let out = execute_with(&mut d, &Action::Navigate { url: "https://app.example/login".into() }, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(d.events.is_empty(), "all three lifecycle events should have been consumed");
}

/// A sub-frame's own load can arrive again and again; without the loader
/// id filter it would satisfy the wait and report a page that never
/// actually finished loading.
#[tokio::test]
async fn navigate_gives_up_when_only_a_sub_frames_load_arrives() {
    let mut d = FakePage::default().driver();
    d.on_call_events.push((
        "Page.navigate".into(),
        Event {
            method: "Page.lifecycleEvent".into(),
            params: json!({ "frameId": "F", "loaderId": "OTHER", "name": "load" }),
        },
    ));
    let out = execute_with(&mut d, &Action::Navigate { url: "https://app.example/login".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("did not finish loading"), "{}", out.detail);
}

/// A same-document navigation (a #fragment) returns no `loaderId` and
/// fires no lifecycle event at all - waiting for one would just time out.
#[tokio::test]
async fn navigate_within_the_same_document_does_not_wait_for_a_load() {
    let mut d = FakePage { navigate_reply: json!({ "frameId": "F" }), ..FakePage::default() }.driver();
    let out = execute_with(
        &mut d,
        &Action::Navigate { url: "https://app.example/login#section".into() },
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert!(out.detail.starts_with("moved to"), "{}", out.detail);
}

#[tokio::test]
async fn navigate_reports_a_page_that_would_not_load() {
    let mut d = FakePage {
        navigate_reply: json!({ "frameId": "F", "errorText": "net::ERR_NAME_NOT_RESOLVED" }),
        ..FakePage::default()
    }
    .driver();
    let out = execute_with(&mut d, &Action::Navigate { url: "https://nope.invalid/".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("ERR_NAME_NOT_RESOLVED"), "{}", out.detail);
}

#[tokio::test]
async fn navigate_that_never_finishes_loading_says_so() {
    let mut d = FakePage::default().driver(); // no load event will come
    let out = execute_with(&mut d, &Action::Navigate { url: "https://app.example/slow".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("did not finish loading"), "{}", out.detail);
}

/// A scheme that is not a page is refused before the browser is asked.
#[tokio::test]
async fn navigate_refuses_a_javascript_url_without_touching_the_browser() {
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &Action::Navigate { url: "javascript:alert(1)".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(d.calls.is_empty(), "{:?}", d.methods());
}

#[tokio::test]
async fn check_text_and_check_url_answer_from_the_page() {
    let mut d = FakePage::default().driver();
    assert!(execute_with(&mut d, &Action::CheckText { value: "Dashboard".into() }, &quick()).await.ok);
    let out = execute_with(&mut d, &Action::CheckUrl { contains: "/home".into() }, &quick()).await;
    assert!(out.ok && out.detail.contains("https://app.example/home"), "{}", out.detail);

    let mut d = FakePage { body_has_text: false, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &Action::CheckText { value: "Dashboard".into() }, &quick()).await;
    assert!(!out.ok && out.detail.contains("does NOT contain"), "{}", out.detail);
    assert!(!execute_with(&mut d, &Action::CheckUrl { contains: "/login".into() }, &quick()).await.ok);
}

/// A page that pops an alert no longer freezes the run; the person is told
/// it happened.
#[tokio::test]
async fn a_dialog_the_page_showed_is_mentioned() {
    let mut d = FakePage::default().driver();
    d.dialogs.push("alert: Saved!".into());
    let out = execute_with(&mut d, &Action::Click { selector: "#go".into() }, &quick()).await;
    assert!(out.ok);
    assert!(out.detail.contains("alert: Saved!") && out.detail.contains("accepted"), "{}", out.detail);
}

#[tokio::test]
async fn an_invalid_action_fails_without_touching_the_browser() {
    let a: Action = serde_json::from_value(json!({ "kind": "click", "selector": {} })).unwrap();
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &a, &quick()).await;
    assert!(!out.ok && out.detail.contains("role, text or css"), "{}", out.detail);
    assert!(d.calls.is_empty());
}
