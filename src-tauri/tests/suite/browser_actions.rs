//! What each action does to the browser, and how the executor reports it.
//! A described fake page stands in for Edge, so every rule here is pinned
//! without opening a window. `tests/suite/browser_live.rs` checks the same things
//! against the real one.

use crate::common;

use common::{ready_probe, FakePage, ScriptedDriver};
use serde_json::json;
use v2_lib::browser::actions::{
    execute_in, execute_with, not_noted, url_equals, Action, DialogAnswer, Policy, HIGHLIGHT_JS, NOTE_NAME_RULE, RESOLVE_URL_JS,
    URL_ONE_OF,
};
use v2_lib::browser::cdp::{CdpError, Event};
use v2_lib::browser::input::PROBE_JS;
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
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
    assert!(!out.harness, "the page answered - this is not a harness problem");
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
    assert!(out.harness, "a dropped socket is the harness's fault, not the page's");
}

/// `harness` is process-internal bookkeeping for whether to bother asking a
/// dead browser for a screenshot - it must never appear in a serialized
/// outcome, not on the IPC boundary and not in a saved run file.
#[tokio::test]
async fn a_harness_outcome_serializes_without_a_harness_key() {
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    let out = execute_with(&mut d, &Action::CheckText { value: "Dashboard".into() }, &quick()).await;
    assert!(out.harness);
    let v = serde_json::to_value(&out).unwrap();
    assert!(v.get("harness").is_none(), "{v}");
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
    // The third wait loop hands its deadline back too, or every call the
    // next action makes is capped at the floor.
    assert!(d.deadline_was_cleared(), "{:?}", d.deadlines.len());
    assert!(d.deadlines.first().is_some_and(Option::is_some), "it never set one");
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
    let out = execute_with(&mut d, &Action::CheckUrl { contains: "/home".into(), equals: None, same_as: None, timeout_ms: None }, &quick()).await;
    assert!(out.ok && out.detail.contains("https://app.example/home"), "{}", out.detail);

    let mut d = FakePage { body_has_text: false, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &Action::CheckText { value: "Dashboard".into() }, &quick()).await;
    assert!(!out.ok && out.detail.contains("does NOT contain"), "{}", out.detail);
    assert!(!execute_with(&mut d, &Action::CheckUrl { contains: "/login".into(), equals: None, same_as: None, timeout_ms: None }, &quick()).await.ok);
}

/// A page that pops an alert no longer freezes the run; the person is told
/// it happened.
#[tokio::test]
async fn a_dialog_the_page_showed_is_mentioned() {
    let mut d = FakePage::default().driver();
    d.dialogs.push("alert: Saved!".into());
    let out = execute_with(&mut d, &Action::Click { selector: "#go".into() }, &quick()).await;
    assert!(out.ok);
    assert!(out.detail.ends_with(" (an alert dialog was accepted: \"Saved!\")"), "{}", out.detail);
}

/// A navigation landing between the readiness wait and the highlight
/// yields "Cannot find context with specified id" - a refusal from the
/// PAGE, on a browser that is alive and answering. Called a harness
/// failure it says the wrong thing AND suppresses the screenshot, which
/// is the one piece of evidence a person could have used.
#[tokio::test]
async fn a_page_refusal_during_the_highlight_is_the_pages_doing() {
    let mut d = ScriptedDriver::new(|method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if f == HIGHLIGHT_JS => Err(CdpError::Protocol {
                method: "Runtime.callFunctionOn".into(),
                message: "Cannot find context with specified id".into(),
            }),
            "Runtime.callFunctionOn" if f == PROBE_JS => {
                Ok(json!({ "result": { "value": ready_probe() } }))
            }
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => {
                Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el-0" } } ] }))
            }
            _ => Ok(json!({})),
        }
    });
    let out = execute_with(&mut d, &Action::Click { selector: "#go".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("the page refused"), "{}", out.detail);
    assert!(out.detail.contains("Cannot find context"), "{}", out.detail);
    assert!(!out.harness, "the browser answered: {}", out.detail);
}

/// The same division for the checks that ask the page directly.
#[tokio::test]
async fn a_page_refusal_during_a_check_is_the_pages_doing() {
    let refusing = || {
        ScriptedDriver::new(|_, _| {
            Err(CdpError::Protocol {
                method: "Runtime.evaluate".into(),
                message: "Cannot find context with specified id".into(),
            })
        })
    };
    for action in [
        Action::CheckText { value: "Dashboard".into() },
        Action::CheckUrl { contains: "/home".into(), equals: None, same_as: None, timeout_ms: None },
    ] {
        let mut d = refusing();
        let out = execute_with(&mut d, &action, &quick()).await;
        assert!(!out.ok);
        assert!(out.detail.contains("the page refused"), "{}", out.detail);
        assert!(!out.harness, "{}", out.detail);
    }
}

/// And a dead socket is still the harness's fault, at every one of those
/// sites - the point of the split is that it is a split, not a rename.
#[tokio::test]
async fn a_closed_browser_at_those_same_sites_is_still_the_harness() {
    for action in [
        Action::Click { selector: "#go".into() },
        Action::CheckText { value: "Dashboard".into() },
        Action::CheckUrl { contains: "/home".into(), equals: None, same_as: None, timeout_ms: None },
        Action::Navigate { url: "https://app.example/x".into() },
    ] {
        let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
        let out = execute_with(&mut d, &action, &quick()).await;
        assert!(!out.ok);
        assert!(out.harness, "{action:?} lost its harness flag: {}", out.detail);
        assert!(out.detail.contains("browser"), "{}", out.detail);
    }
}

/// A relative url is what `location.href = "/dashboard"` always allowed,
/// and saving validates every action - so refusing one here refuses a
/// whole bundle for containing a single such script. Another scheme is
/// still refused.
#[test]
fn navigate_takes_a_relative_reference_but_not_another_scheme() {
    let check = |u: &str| {
        serde_json::from_value::<Action>(json!({ "kind": "navigate", "url": u }))
            .unwrap()
            .validate()
    };
    for u in [
        "/dashboard",
        "dashboard",
        "./a/b?x=1#y",
        "../up",
        "https://app.example/x",
        "http://a/",
        "file:///C:/x.html",
    ] {
        assert!(check(u).is_ok(), "{u} should be accepted: {:?}", check(u));
    }
    for u in ["javascript:alert(1)", "data:text/html,x", "about:blank", "chrome://settings", "", "   "] {
        let why = check(u).unwrap_err();
        assert!(why.contains("http, https or file"), "{u}: {why}");
    }
}

/// At run time the page resolves it, against its own address, with the
/// script's value passed as an ARGUMENT. The resolved address is what the
/// browser is sent to and what the outcome names.
#[tokio::test]
async fn a_relative_navigate_is_resolved_in_the_page() {
    let mut d =
        FakePage { resolved_url: "https://app.example/dashboard", ..FakePage::default() }.driver();
    d.on_call_events.push((
        "Page.navigate".into(),
        Event {
            method: "Page.lifecycleEvent".into(),
            params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }),
        },
    ));
    let out = execute_with(&mut d, &Action::Navigate { url: "/dashboard".into() }, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(d.calls_to("Page.navigate")[0]["url"], "https://app.example/dashboard");
    assert_eq!(out.detail, "loaded https://app.example/dashboard");

    let resolve = d
        .calls
        .iter()
        .find(|(m, p)| m == "Runtime.callFunctionOn" && p["functionDeclaration"] == RESOLVE_URL_JS)
        .expect("the relative url was never resolved in the page");
    assert_eq!(resolve.1["arguments"][0]["value"], "/dashboard");
    for (method, params) in &d.calls {
        let source =
            params["functionDeclaration"].as_str().or(params["expression"].as_str()).unwrap_or("");
        assert!(!source.contains("/dashboard"), "{method} carried the url in its source");
    }
}

/// A relative reference that resolves to something that is not a page
/// address is refused at run time, with the same words, and nothing is
/// navigated to.
#[tokio::test]
async fn a_relative_navigate_that_resolves_to_another_scheme_is_refused() {
    let mut d = FakePage { resolved_url: "about:blank", ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &Action::Navigate { url: "blank".into() }, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("http, https or file"), "{}", out.detail);
    assert!(d.calls_to("Page.navigate").is_empty(), "{:?}", d.methods());
}

#[tokio::test]
async fn an_invalid_action_fails_without_touching_the_browser() {
    let a: Action = serde_json::from_value(json!({ "kind": "click", "selector": {} })).unwrap();
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &a, &quick()).await;
    assert!(!out.ok && out.detail.contains("role, text or css"), "{}", out.detail);
    assert!(d.calls.is_empty());
}

fn only(origins: &[&str]) -> Policy {
    Policy::only(origins.iter().map(|s| s.to_string()).collect())
}

#[test]
fn a_policy_judges_an_address_by_its_origin() {
    let p = only(&["https://hr.example.internal", "http://127.0.0.1:8080"]);
    assert!(p.allows("https://HR.example.internal/a/b?c=1"));
    assert!(p.allows("http://127.0.0.1:8080/x"));
    assert!(!p.allows("http://127.0.0.1:9090/x"), "the port is part of the origin");
    assert!(!p.allows("https://evil.example/"));
    assert!(!p.allows("file:///C:/x.html"), "file addresses need file:// in the list");
    assert!(only(&["file://"]).allows("file:///C:/x.html"));
    assert!(Policy::open().allows("https://anywhere.example/"));
}

#[tokio::test]
async fn navigate_outside_the_allowed_origins_is_refused_before_the_browser_is_asked() {
    let mut d = FakePage::default().driver();
    let out = execute_in(
        &mut d,
        &Action::Navigate { url: "https://evil.example/x".into() },
        &quick(),
        &only(&["https://hr.example.internal"]),
    )
    .await;
    assert!(!out.ok && !out.harness);
    assert!(out.detail.contains("https://evil.example") && out.detail.contains("allowed origins"), "{}", out.detail);
    assert!(d.calls_to("Page.navigate").is_empty());
}

/// A relative or protocol-relative address is judged by where it GOES.
/// FakePage answers the resolve call with its `resolved_url` field (see
/// `tests/suite/common.rs`), not `href`, which is what `location.href`
/// answers with for `check_url` - the brief's test used `href` for both.
#[tokio::test]
async fn a_relative_address_is_checked_after_it_is_resolved() {
    let mut d =
        FakePage { resolved_url: "https://evil.example/landing", ..FakePage::default() }.driver();
    let out = execute_in(
        &mut d,
        &Action::Navigate { url: "//evil.example/landing".into() },
        &quick(),
        &only(&["https://hr.example.internal"]),
    )
    .await;
    assert!(!out.ok, "{}", out.detail);
    assert!(d.calls_to("Page.navigate").is_empty());
}

/// The browser treats `\` in an http(s) authority as `/`, so it sends this
/// address to `evil.example` (path `/@hr.example.internal/`), not to
/// `hr.example.internal`. The check has to agree with the browser about
/// where the address goes, or the allowlist is not a boundary at all.
#[tokio::test]
async fn navigate_via_a_backslash_authority_trick_is_refused() {
    let mut d = FakePage::default().driver();
    let out = execute_in(
        &mut d,
        &Action::Navigate { url: "https://evil.example\\@hr.example.internal/".into() },
        &quick(),
        &only(&["https://hr.example.internal"]),
    )
    .await;
    assert!(!out.ok, "{}", out.detail);
    assert!(d.calls_to("Page.navigate").is_empty());
}

/// A tab hidden inside the address is silently stripped by a real
/// browser, so the checked address and the loaded one would disagree;
/// refusing it is the fail-closed answer.
#[tokio::test]
async fn navigate_with_an_embedded_control_character_is_refused() {
    let mut d = FakePage::default().driver();
    let out = execute_in(
        &mut d,
        &Action::Navigate { url: "https://hr.example.internal/\tlogin".into() },
        &quick(),
        &only(&["https://hr.example.internal"]),
    )
    .await;
    assert!(!out.ok, "{}", out.detail);
    assert!(d.calls_to("Page.navigate").is_empty());
}

#[tokio::test]
async fn with_no_policy_everything_works_as_before() {
    let mut d = FakePage::default().driver();
    d.on_call_events.push((
        "Page.navigate".into(),
        Event {
            method: "Page.lifecycleEvent".into(),
            params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }),
        },
    ));
    let out =
        execute_in(&mut d, &Action::Navigate { url: "https://anywhere.example/".into() }, &quick(), &Policy::open())
            .await;
    assert!(out.ok, "{}", out.detail);
}

/// `sign_in` is validated by the ordinary rules (a usable account key), but
/// carried out by the runner, which alone has the accounts, the recipe and
/// the disk. This driver must never be touched for it.
#[tokio::test]
async fn sign_in_is_validated_here_but_carried_out_by_the_runner() {
    let bad: Action = serde_json::from_value(json!({ "kind": "sign_in", "account": "Not A Key" })).unwrap();
    assert!(bad.validate().is_err());
    let good: Action = serde_json::from_value(json!({ "kind": "sign_in", "account": "hr.supervisor" })).unwrap();
    assert!(good.validate().is_ok());
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &good, &quick()).await;
    assert!(!out.ok && out.detail.contains("runner"), "{}", out.detail);
    assert!(d.calls.is_empty(), "the driver must not touch the browser for it");
}

/// `is_check` is what `replay::propose` uses to tell a script that JUDGES
/// something from one that only drives or waits - `wait_for` waits, it
/// does not judge, so it must read as false alongside the ordinary
/// actions. One sample of every kind the executor understands, the same
/// list `autorun_guide.rs`'s drift test builds.
#[test]
fn only_checks_and_expectations_are_checks() {
    use v2_lib::autorun::guide::ACTION_KINDS;

    let samples = vec![
        Action::Navigate { url: "u".into() },
        Action::Click { selector: "s".into() },
        Action::Fill { selector: "s".into(), value: "v".into() },
        Action::WaitFor { selector: "s".into(), timeout_ms: 1 },
        Action::CheckText { value: "v".into() },
        Action::CheckUrl { contains: "c".into(), equals: None, same_as: None, timeout_ms: None },
        Action::RememberUrl { name: "a".into() },
        Action::ExpectVisible { selector: "s".into(), timeout_ms: None },
        Action::ExpectHidden { selector: "s".into(), timeout_ms: None },
        Action::ExpectText { selector: "s".into(), equals: Some("v".into()), matches: None, timeout_ms: None },
        Action::ExpectContainsText { selector: "s".into(), value: "v".into(), timeout_ms: None },
        Action::ExpectCount { selector: "s".into(), equals: Some(1), at_least: None, at_most: None, timeout_ms: None },
        Action::ExpectAttribute { selector: "s".into(), name: "n".into(), equals: Some("v".into()), matches: None, absent: false, timeout_ms: None },
        serde_json::from_value(json!({ "kind": "expect_style", "selector": "s", "property": "color", "equals": "red" })).unwrap(),
        Action::ExpectEnabled { selector: "s".into(), timeout_ms: None },
        Action::ExpectDisabled { selector: "s".into(), timeout_ms: None },
        Action::ExpectChecked { selector: "s".into(), checked: true, timeout_ms: None },
        Action::SignIn { account: "a".into() },
        Action::Upload { selector: "s".into(), file: "f.pdf".into() },
        Action::ExpectResponse { method: None, url_contains: "/x".into(), status: 200, json: None, timeout_ms: None, stray: Default::default() },
        Action::ApiRequest { path: "/api/x".into(), query: Default::default(), expect: Default::default(), stray: Default::default() },
        // A guard is a tidy-up, never a check - even holding one.
        Action::WhenVisible { selector: "s".into(), within_ms: None, then: vec![Action::ExpectVisible { selector: "s".into(), timeout_ms: None }] },
        Action::RepeatWhileVisible { selector: v2_lib::browser::locator::Target::One(v2_lib::browser::locator::LocatorStep { css: Some(".delete".into()), ..Default::default() }), max: Some(3), except: vec![], then: vec![Action::Click { selector: "s".into() }] },
        Action::ExpectDownload {
            name: "report.csv".into(),
            within_ms: None,
            sheet: None,
            headers: None,
            cells: None,
            contains_text: None,
            pdf: None,
            stray: Default::default(),
        },
        Action::ExpectTab { name: "r".into(), url_contains: None, within_ms: None },
        Action::OpenTab { name: "r".into(), url: "/x".into() },
        Action::SwitchTab { name: "r".into() },
        Action::CloseTab { name: "r".into() },
        Action::ExpectTabClosed { name: "r".into(), within_ms: None },
        Action::Drag { from: "s".into(), to: "t".into(), position: None, within_ms: None },
        Action::ExpectDialog { text: None, contains: None, answer: DialogAnswer::Accept, prompt_text: None, within_ms: None },
        serde_json::from_value(serde_json::json!({ "kind": "expect_row", "table": "t", "cells": { "A": "b" } })).unwrap(),
        serde_json::from_value(serde_json::json!({ "kind": "expect_no_row", "table": "t", "cells": { "A": "b" } })).unwrap(),
        serde_json::from_value(serde_json::json!({ "kind": "expect_sorted", "table": "t", "column": "A", "order": "ascending" })).unwrap(),
        serde_json::from_value(serde_json::json!({ "kind": "expect_row_count", "table": "t", "equals": 1 })).unwrap(),
        serde_json::from_value(serde_json::json!({ "kind": "expect_each", "selector": "s", "not_empty": true })).unwrap(),
        serde_json::from_value(serde_json::json!({ "kind": "expect_order", "selector": "s", "texts": ["a", "b"] })).unwrap(),
        serde_json::from_value(serde_json::json!({ "kind": "expect_position", "selector": "s", "relative_to": "t", "relation": "inside" })).unwrap(),
        serde_json::from_value(serde_json::json!({ "kind": "page_until", "next": "n", "until": "u", "max_pages": 2 })).unwrap(),
        Action::UseComponent { component: "c".into(), inputs: serde_json::Map::new() },
    ];
    assert_eq!(samples.len(), ACTION_KINDS.len(), "this list has drifted from ACTION_KINDS");

    let is_a_check = |kind: &str| {
        matches!(
            kind,
            "check_text"
                | "check_url"
                | "expect_visible"
                | "expect_hidden"
                | "expect_text"
                | "expect_contains_text"
                | "expect_count"
                | "expect_attribute"
                | "expect_style"
                | "expect_enabled"
                | "expect_disabled"
                | "expect_checked"
                | "expect_response"
                | "api_request"
                | "expect_download"
                | "expect_tab"
                | "expect_tab_closed"
                | "expect_dialog"
                | "expect_row"
                | "expect_no_row"
                | "expect_sorted"
                | "expect_row_count"
                | "expect_each"
                | "expect_order"
                | "expect_position"
                | "page_until"
        )
    };
    for (action, kind) in samples.iter().zip(ACTION_KINDS.iter()) {
        assert_eq!(action.is_check(), is_a_check(kind), "`{kind}` disagreed with is_check()");
    }
}

#[test]
fn expect_response_and_api_request_round_trip_with_their_defaults() {
    let a: Action =
        serde_json::from_value(json!({ "kind": "expect_response", "url_contains": "/Cycle/Save" })).unwrap();
    assert_eq!(
        a,
        Action::ExpectResponse {
            method: None,
            url_contains: "/Cycle/Save".into(),
            status: 200,
            json: None,
            timeout_ms: None,
            stray: Default::default()
        }
    );
    // Nothing optional is written back out when it is not set.
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        json!({ "kind": "expect_response", "url_contains": "/Cycle/Save", "status": 200 })
    );

    let full = json!({
        "kind": "expect_response", "method": "POST", "url_contains": "/Save",
        "status": 201, "json": { "success": true }, "timeout_ms": 5000
    });
    let a: Action = serde_json::from_value(full.clone()).unwrap();
    assert_eq!(serde_json::to_value(&a).unwrap(), full);

    let r: Action = serde_json::from_value(json!({ "kind": "api_request", "path": "/api/cycles/42" })).unwrap();
    match &r {
        Action::ApiRequest { path, query, expect, .. } => {
            assert_eq!(path, "/api/cycles/42");
            assert!(query.is_empty());
            assert_eq!(expect.status, 200);
            assert!(expect.json.is_none());
        }
        other => panic!("{other:?}"),
    }
    let written = serde_json::to_value(&r).unwrap();
    assert!(written.get("query").is_none(), "an empty query is omitted: {written}");
    assert_eq!(written["expect"], json!({ "status": 200 }));

    let full = json!({
        "kind": "api_request", "path": "/api/cycles/42", "query": { "include": "rules" },
        "expect": { "status": 200, "json": { "name": "Q4 Cycle" } }
    });
    let r: Action = serde_json::from_value(full.clone()).unwrap();
    assert_eq!(serde_json::to_value(&r).unwrap(), full);
    assert!(r.validate().is_ok());
}

#[test]
fn the_two_response_steps_say_what_is_wrong_with_them() {
    let err = |v: serde_json::Value| serde_json::from_value::<Action>(v).unwrap().validate().unwrap_err();

    assert_eq!(
        err(json!({ "kind": "expect_response", "url_contains": "  " })),
        "expect_response needs url_contains"
    );
    assert_eq!(
        err(json!({ "kind": "expect_response", "url_contains": "/x", "method": "FETCH" })),
        "expect_response method \"FETCH\" is not an HTTP method"
    );
    assert_eq!(
        err(json!({ "kind": "expect_response", "url_contains": "/x", "status": 99 })),
        "status 99 is not an HTTP status"
    );
    assert_eq!(
        err(json!({ "kind": "expect_response", "url_contains": "/x", "status": 600 })),
        "status 600 is not an HTTP status"
    );
    for m in ["get", "POST", "Put", "PATCH", "delete", "HEAD", "options"] {
        let a: Action =
            serde_json::from_value(json!({ "kind": "expect_response", "url_contains": "/x", "method": m })).unwrap();
        assert!(a.validate().is_ok(), "{m}");
    }

    for p in ["api/x", "//evil.example/x", "/a/../b", "https://evil.example/x", "/a\\b", ""] {
        assert_eq!(
            err(json!({ "kind": "api_request", "path": p })),
            // Never repeated: an unsafe path can be a whole address.
            "api_request path is not a safe path on this site - give a path such as /api/cycles/42, never an address",
        );
    }
    assert_eq!(
        err(json!({ "kind": "api_request", "path": "/api/x", "expect": { "status": 700 } })),
        "status 700 is not an HTTP status"
    );
}

/// Like `sign_in`, both are carried out by the runner (it alone holds the
/// network record and the page's cookies for a request).
#[tokio::test]
async fn the_driver_alone_never_runs_the_response_steps() {
    for a in [
        json!({ "kind": "expect_response", "url_contains": "/x" }),
        json!({ "kind": "api_request", "path": "/api/x" }),
    ] {
        let a: Action = serde_json::from_value(a).unwrap();
        let mut d = FakePage::default().driver();
        let out = execute_with(&mut d, &a, &quick()).await;
        assert!(!out.ok && out.detail.contains("runner"), "{}", out.detail);
        assert!(d.calls.is_empty());
    }
}

/// Spec 9: `check_text` reads the words of every same-origin frame too, at
/// any depth, skipping a frame no one can see and one it cannot enter.
/// Proven live in `browser_live::frame_check_text_reads_same_origin_frames_at_any_depth`.
#[test]
fn check_text_walks_into_same_origin_frames() {
    let js = v2_lib::browser::actions::CHECK_TEXT_JS;
    assert!(js.contains("contentDocument"), "{js}");
    assert!(js.contains("try"), "a frame from another site throws or answers null, and is skipped: {js}");
    assert!(js.contains("checkVisibility"), "a hidden frame's words are not on the page: {js}");
}

// ------------------------------------------------------------ expect_download

/// The spec's example, without `contains_text`: a workbook is read as cells,
/// and only a .csv or .txt file is read as text.
fn spec_download() -> serde_json::Value {
    json!({ "kind": "expect_download",
            "name": "Template*.xlsx",
            "within_ms": 15000,
            "sheet": "Employees",
            "headers": { "exact": ["Employee No", "Name", "Department"] },
            "cells": [ { "ref": "B2", "text": "Employee Name", "match": "exact" } ] })
}

fn download_refusal(v: serde_json::Value) -> String {
    let a: Action = serde_json::from_value(v).expect("the test wrote an expect_download that does not parse");
    a.validate().expect_err("this expect_download should have been refused")
}

#[test]
fn expect_download_takes_the_specs_shape_and_round_trips() {
    let a: Action = serde_json::from_value(spec_download()).unwrap();
    assert!(a.validate().is_ok(), "{:?}", a.validate());
    assert!(a.is_check());
    assert_eq!(a.kind(), "expect_download");
    assert_eq!(serde_json::to_value(&a).unwrap(), spec_download());
    // Only the name is needed; nothing left out is written back.
    let lone: Action = serde_json::from_value(json!({ "kind": "expect_download", "name": "report.csv" })).unwrap();
    assert!(lone.validate().is_ok());
    assert_eq!(serde_json::to_value(&lone).unwrap(), json!({ "kind": "expect_download", "name": "report.csv" }));
    // `match` defaults to exact; contains and text checks on a CSV are fine.
    let csv: Action = serde_json::from_value(json!({ "kind": "expect_download", "name": "errors*.CSV",
        "headers": { "contains": ["Row"] }, "cells": [ { "ref": "a2", "text": "Row 4" } ],
        "contains_text": ["Row 4: Department is required"] }))
    .unwrap();
    assert!(csv.validate().is_ok(), "{:?}", csv.validate());
    let txt: Action = serde_json::from_value(json!({ "kind": "expect_download", "name": "log.txt",
        "contains_text": ["Department is required"] }))
    .unwrap();
    assert!(txt.validate().is_ok(), "{:?}", txt.validate());
}

#[test]
fn expect_download_refuses_what_could_never_be_checked() {
    // The name is required, and cannot be blank.
    assert!(serde_json::from_value::<Action>(json!({ "kind": "expect_download" })).is_err());
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "  " })),
        "expect_download needs a name, such as Template*.xlsx"
    );
    // within_ms: more than 0, at most 120000.
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "a.csv", "within_ms": 0 })),
        "within_ms must be more than 0"
    );
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "a.csv", "within_ms": 120001 })),
        "within_ms is at most 120000 (got 120001)"
    );
    let ok: Action =
        serde_json::from_value(json!({ "kind": "expect_download", "name": "a.csv", "within_ms": 120000 })).unwrap();
    assert!(ok.validate().is_ok());

    // Spreadsheet keys on a file that is not a spreadsheet.
    let sheet_keys = [
        ("sheet", json!("Errors")),
        ("headers", json!({ "exact": ["Row"] })),
        ("cells", json!([{ "ref": "A1", "text": "Row" }])),
    ];
    for name in ["notes.txt", "report.pdf"] {
        for (key, value) in &sheet_keys {
            let mut v = json!({ "kind": "expect_download", "name": name });
            v[*key] = value.clone();
            if *key == "sheet" {
                v["headers"] = json!({ "exact": ["Row"] });
            }
            assert_eq!(
                download_refusal(v),
                format!("expect_download can check {key} only in a file whose name ends in .xlsx, .xls or .csv, and \"{name}\" does not - name the file type (such as Template*.xlsx) so it is known before the file arrives, or check only name and within_ms"),
            );
        }
    }
    // Text on a file that is not text.
    for name in ["Template.xlsx", "old.XLS"] {
        assert_eq!(
            download_refusal(json!({ "kind": "expect_download", "name": name, "contains_text": ["Row 4"] })),
            format!("expect_download can check contains_text only in a file whose name ends in .csv or .txt, and \"{name}\" does not - name the file type (such as errors*.csv) so it is known before the file arrives, or check only name and within_ms"),
        );
    }
    // A name with no fixed file type carries only name and within_ms.
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "export*", "contains_text": ["x"] })),
        "expect_download can check contains_text only in a file whose name ends in .csv or .txt, and \"export*\" does not - name the file type (such as errors*.csv) so it is known before the file arrives, or check only name and within_ms",
    );
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "export*.xls*", "headers": { "exact": ["A"] } })),
        "expect_download can check headers only in a file whose name ends in .xlsx, .xls or .csv, and \"export*.xls*\" does not - name the file type (such as Template*.xlsx) so it is known before the file arrives, or check only name and within_ms",
    );
    let loose: Action =
        serde_json::from_value(json!({ "kind": "expect_download", "name": "export*", "within_ms": 5000 })).unwrap();
    assert!(loose.validate().is_ok());

    // Empty lists, a bad reference, and checks that hold of anything.
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "a.csv", "cells": [] })),
        "expect_download cells is an empty list - give at least one cell, or leave cells out"
    );
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "a.csv", "contains_text": [] })),
        "expect_download contains_text is an empty list - give at least one text, or leave contains_text out"
    );
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "a.csv", "headers": { "exact": [] } })),
        "expect_download headers is an empty list - name at least one header, or leave headers out"
    );
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "a.csv", "contains_text": ["Row", " "] })),
        "expect_download contains_text 2 is empty - every file contains nothing"
    );
    for bad in ["2B", "B0", "", "A1:B2", "ZZZZ1"] {
        assert_eq!(
            download_refusal(json!({ "kind": "expect_download", "name": "a.xlsx", "cells": [{ "ref": "A1", "text": "x" }, { "ref": bad, "text": "x" }] })),
            format!("expect_download cells 2: \"{bad}\" is not a cell reference like B2"),
        );
    }
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "a.xlsx", "cells": [{ "ref": "A1", "text": " ", "match": "contains" }] })),
        "expect_download cells 1: an empty text with match contains holds for any cell - give the text to find"
    );
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "a.xlsx", "sheet": " ", "cells": [{ "ref": "A1", "text": "x" }] })),
        "expect_download sheet is empty - leave it out for the first sheet"
    );
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "a.xlsx", "sheet": "Errors" })),
        "expect_download sheet \"Errors\" is read only for headers or cells - add one, or leave sheet out"
    );
    // A misspelt key is refused rather than dropped without a word.
    assert_eq!(
        download_refusal(json!({ "kind": "expect_download", "name": "a.xlsx", "header": { "exact": ["A"] } })),
        "expect_download has no \"header\" - it takes name, within_ms, sheet, headers, cells, contains_text and pdf"
    );
}

/// Only the runner knows where the step began, so the driver alone never
/// carries it out.
#[tokio::test]
async fn the_driver_alone_never_checks_a_download() {
    let a: Action = serde_json::from_value(json!({ "kind": "expect_download", "name": "a.csv" })).unwrap();
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &a, &quick()).await;
    assert!(!out.ok && out.detail.contains("runner"), "{}", out.detail);
    assert!(d.calls.is_empty());
}

// ---- expect_style ----------------------------------------------------------

/// An `expect_style` on `#save`'s `property`, with the rest of its keys.
fn style_check(property: &str, rest: serde_json::Value) -> Action {
    let mut v = json!({ "kind": "expect_style", "selector": { "css": "#save" }, "property": property });
    v.as_object_mut().unwrap().extend(rest.as_object().unwrap().clone());
    serde_json::from_value(v).unwrap()
}

/// A page whose style reads answer these `(value, still changing)` pairs,
/// in turn; the last repeats.
fn styled(answers: &[(&str, bool)]) -> FakePage {
    FakePage {
        styles: answers.iter().map(|(v, moving)| json!({ "value": v, "moving": moving })).collect(),
        ..FakePage::default()
    }
}

#[tokio::test]
async fn expect_style_equals_passes_and_fails_naming_the_property_and_both_values() {
    let red = style_check("color", json!({ "equals": "rgb(255, 0, 0)" }));
    let mut d = styled(&[("rgb(255, 0, 0)", false)]).driver();
    let out = execute_with(&mut d, &red, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "#save has color rgb(255, 0, 0)");

    let mut d = styled(&[("rgb(0, 0, 0)", false)]).driver();
    let out = execute_with(&mut d, &red, &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, "waited 300ms: #save color is rgb(0, 0, 0), expected rgb(255, 0, 0)");
}

/// However a colour is written, it is compared as the rgb() the browser
/// reports: a name, any hex form, rgb() or rgba() with commas or spaces,
/// and hsl().
#[tokio::test]
async fn red_equals_rgb_255_0_0_however_it_is_written() {
    for want in [
        "red",
        "RED",
        "#f00",
        "#FF0000",
        "#ff0000ff",
        "#f00f",
        "rgb(255,0,0)",
        "rgba(255, 0, 0, 1)",
        "rgb(255 0 0)",
        "rgb(100% 0% 0% / 100%)",
        "hsl(0, 100%, 50%)",
        " red ",
    ] {
        let mut d = styled(&[("rgb(255, 0, 0)", false)]).driver();
        let out = execute_with(&mut d, &style_check("color", json!({ "equals": want })), &quick()).await;
        assert!(out.ok, "{want}: {}", out.detail);
    }
    let mut d = styled(&[("rgb(255, 0, 0)", false)]).driver();
    let out = execute_with(&mut d, &style_check("color", json!({ "equals": "maroon" })), &quick()).await;
    assert!(!out.ok && out.detail.ends_with("color is rgb(255, 0, 0), expected rgb(128, 0, 0)"), "{}", out.detail);

    // Transparency: the browser's rgba() and a hex with alpha agree.
    let mut d = styled(&[("rgba(255, 0, 0, 0.5)", false)]).driver();
    assert!(execute_with(&mut d, &style_check("background-color", json!({ "equals": "#ff000080" })), &quick()).await.ok);
    let mut d = styled(&[("rgba(0, 0, 0, 0)", false)]).driver();
    assert!(execute_with(&mut d, &style_check("background-color", json!({ "equals": "transparent" })), &quick()).await.ok);
}

#[tokio::test]
async fn expect_style_contains_passes_and_fails() {
    let shadow = "rgb(0, 0, 0) 0px 1px 2px 0px";
    for want in ["2px", "black", "#000"] {
        let mut d = styled(&[(shadow, false)]).driver();
        let out = execute_with(&mut d, &style_check("box-shadow", json!({ "contains": want })), &quick()).await;
        assert!(out.ok, "{want}: {}", out.detail);
    }
    let mut d = styled(&[(shadow, false)]).driver();
    let out = execute_with(&mut d, &style_check("box-shadow", json!({ "contains": "3px" })), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.ends_with(&format!("#save box-shadow is {shadow}, expected it to contain 3px")), "{}", out.detail);
}

#[tokio::test]
async fn expect_style_not_equals_passes_when_the_value_differs() {
    let mut d = styled(&[("none", false)]).driver();
    let out = execute_with(&mut d, &style_check("display", json!({ "not_equals": "block" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "#save has display none, not block");

    // Keywords ignore case; a colour is compared as rgb().
    let mut d = styled(&[("none", false)]).driver();
    let out = execute_with(&mut d, &style_check("display", json!({ "not_equals": "NONE" })), &quick()).await;
    assert!(!out.ok && out.detail.ends_with("#save display is none, expected anything but NONE"), "{}", out.detail);
    let mut d = styled(&[("rgb(255, 0, 0)", false)]).driver();
    let out = execute_with(&mut d, &style_check("color", json!({ "not_equals": "red" })), &quick()).await;
    assert!(
        !out.ok && out.detail.ends_with("color is rgb(255, 0, 0), expected anything but rgb(255, 0, 0)"),
        "{}",
        out.detail
    );
}

/// A value read mid-transition is the transition's, not the page's: the
/// check looks again until the property stops changing, within its
/// timeout - and a `not_equals` never passes on an in-between colour.
#[tokio::test]
async fn expect_style_waits_for_a_transition_to_settle() {
    let mut d = styled(&[("rgb(40, 0, 0)", true), ("rgb(200, 0, 0)", true), ("rgb(255, 0, 0)", false)]).driver();
    let out = execute_with(&mut d, &style_check("color", json!({ "equals": "red" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);

    let mut d = styled(&[("rgb(10, 0, 0)", true), ("rgb(0, 0, 0)", false)]).driver();
    let out = execute_with(&mut d, &style_check("color", json!({ "not_equals": "black" })), &quick()).await;
    assert!(!out.ok && out.detail.ends_with("expected anything but rgb(0, 0, 0)"), "{}", out.detail);

    // Still changing when the time runs out: said so, with the value so far.
    let mut d = styled(&[("rgb(9, 0, 0)", true)]).driver();
    let out = execute_with(&mut d, &style_check("color", json!({ "equals": "red", "timeout_ms": 50 })), &quick()).await;
    assert_eq!(out.detail, "waited 50ms: #save color is still changing (rgb(9, 0, 0) so far)");
}

#[test]
fn expect_style_takes_exactly_one_comparison_and_a_css_property_name() {
    use v2_lib::browser::actions::STYLE_NEEDS_ONE;
    let bad = |v: serde_json::Value| serde_json::from_value::<Action>(v).unwrap().validate().unwrap_err();
    let base = |rest: serde_json::Value| {
        let mut v = json!({ "kind": "expect_style", "selector": "#s", "property": "color" });
        v.as_object_mut().unwrap().extend(rest.as_object().unwrap().clone());
        v
    };
    assert_eq!(bad(base(json!({}))), STYLE_NEEDS_ONE);
    assert_eq!(bad(base(json!({ "equals": "red", "contains": "r" }))), STYLE_NEEDS_ONE);
    assert_eq!(bad(base(json!({ "equals": "red", "not_equals": "blue" }))), STYLE_NEEDS_ONE);
    assert!(bad(base(json!({ "contains": " " }))).contains("empty"));
    assert!(bad(json!({ "kind": "expect_style", "selector": "#s", "property": " ", "equals": "x" })).contains("empty property"));
    let camel = bad(json!({ "kind": "expect_style", "selector": "#s", "property": "backgroundColor", "equals": "red" }));
    assert!(camel.contains("\"background-color\""), "{camel}");
    assert!(bad(json!({ "kind": "expect_style", "selector": "#s", "property": "color;x", "equals": "red" })).contains("not a CSS property"));
    assert!(bad(json!({ "kind": "expect_style", "selector": {}, "property": "color", "equals": "red" })).contains("role, text or css"));
    for good in [
        base(json!({ "equals": "red" })),
        base(json!({ "contains": "0px" })),
        base(json!({ "not_equals": "none" })),
        json!({ "kind": "expect_style", "selector": "#s", "property": "--brand-Colour", "equals": "#123" }),
        json!({ "kind": "expect_style", "selector": "#s", "property": "-webkit-line-clamp", "equals": "2" }),
    ] {
        let a: Action = serde_json::from_value(good.clone()).unwrap();
        assert_eq!(a.validate(), Ok(()), "{good}");
        // Only what was given is written back.
        assert_eq!(serde_json::to_value(&a).unwrap(), good);
    }
}

#[test]
fn expect_each_is_validated() {
    use v2_lib::browser::actions::EACH_NEEDS_ONE;
    assert_eq!(
        EACH_NEEDS_ONE,
        "expect_each takes exactly one of \"visible\": true, \"equals\", \"contains\", \"matches\" or \"not_empty\": true"
    );
    let bad = |v: serde_json::Value| serde_json::from_value::<Action>(v).unwrap().validate().unwrap_err();
    let base = |rest: serde_json::Value| {
        let mut v = json!({ "kind": "expect_each", "selector": { "css": "li" } });
        v.as_object_mut().unwrap().extend(rest.as_object().unwrap().clone());
        v
    };
    // The selector is checked first.
    assert!(bad(json!({ "kind": "expect_each", "selector": {}, "not_empty": true })).contains("role, text or css"));
    // A selector string reaches only the first match, so it cannot be a list.
    assert_eq!(
        bad(json!({ "kind": "expect_each", "selector": "div.card", "not_empty": true })),
        "expect_each needs a locator object for selector, not a selector string - a string reaches only the first match, and this reads every item"
    );
    assert_eq!(bad(json!({ "kind": "expect_each", "selector": " ", "not_empty": true })), "a selector is empty");
    // The list must be seen holding still, which takes time.
    for t in [0, 300, 1499] {
        assert_eq!(
            bad(base(json!({ "not_empty": true, "timeout_ms": t }))),
            "expect_each: needs at least 1500 ms to see the list hold still",
            "{t}"
        );
    }
    for t in [1500, 5000] {
        let a: Action = serde_json::from_value(base(json!({ "not_empty": true, "timeout_ms": t }))).unwrap();
        assert_eq!(a.validate(), Ok(()), "{t}");
    }
    // `min`.
    assert_eq!(
        bad(base(json!({ "not_empty": true, "min": 0 }))),
        "expect_each needs \"min\" of 1 or more - a list of none would pass any check"
    );
    assert_eq!(
        bad(base(json!({ "not_empty": true, "min": 201 }))),
        "expect_each reads at most 200 items, so \"min\" cannot be 201"
    );
    // Exactly one of the five.
    for rest in [
        json!({}),
        json!({ "equals": "a", "contains": "a" }),
        json!({ "equals": "a", "matches": "a" }),
        json!({ "not_empty": true, "equals": "a" }),
        json!({ "visible": true, "child": { "css": "h3" }, "not_empty": true }),
        json!({ "visible": false }),
        json!({ "not_empty": false }),
    ] {
        assert_eq!(bad(base(rest.clone())), EACH_NEEDS_ONE, "{rest}");
    }
    assert_eq!(
        bad(base(json!({ "visible": true }))),
        "expect_each: \"visible\": true needs a \"child\" - every item of the list is already visible"
    );
    assert_eq!(
        bad(base(json!({ "visible": true, "child": { "css": "h3" }, "attribute": "title" }))),
        "expect_each: \"visible\" reads no attribute - leave \"attribute\" out"
    );
    assert_eq!(bad(base(json!({ "equals": "a", "attribute": " " }))), "expect_each has an empty attribute name");
    assert_eq!(
        bad(base(json!({ "contains": " " }))),
        "expect_each has an empty contains - everything contains nothing"
    );
    // A pattern goes through the one check every `matches` has.
    assert_eq!(
        bad(base(json!({ "matches": "" }))),
        v2_lib::browser::pattern::check_pattern("expect_each", "").unwrap_err()
    );
    assert!(bad(base(json!({ "matches": ".*" }))).starts_with("expect_each: \"matches\" fits any text"));
    assert!(bad(base(json!({ "matches": "(?i)a" }))).starts_with("expect_each: "));
    // The child.
    assert_eq!(
        bad(base(json!({ "not_empty": true, "child": "h3" }))),
        "expect_each's child is a locator object or a list of them, not a selector string: it is looked for inside each item"
    );
    assert_eq!(
        bad(base(json!({ "not_empty": true, "child": { "input": "title" } }))),
        "expect_each's child cannot hold a component input - put the input in the list's locator"
    );
    assert_eq!(
        bad(base(json!({ "not_empty": true, "child": [{ "css": "h3" }, { "input": "x" }] }))),
        "expect_each's child cannot hold a component input - put the input in the list's locator"
    );
    assert_eq!(
        bad(base(json!({ "not_empty": true, "child": { "role": "button", "css": "x" } }))),
        "expect_each child: a locator takes only one of role, text or css"
    );
    // A component input in the LIST's locator is fine.
    assert_eq!(
        serde_json::from_value::<Action>(base(json!({ "not_empty": true, "selector": { "input": "list" } }))).unwrap().validate(),
        Ok(())
    );
    // Valid shapes round trip exactly, and only what was given is written.
    for good in [
        json!({ "kind": "expect_each", "selector": { "css": "div[data-cycle-id]" }, "child": { "css": "h3" }, "matches": "^AUTOTEST " }),
        json!({ "kind": "expect_each", "selector": { "css": "li" }, "not_empty": true }),
        json!({ "kind": "expect_each", "selector": { "css": "li" }, "min": 3, "equals": "a", "timeout_ms": 1500 }),
        json!({ "kind": "expect_each", "selector": { "css": "li" }, "child": [{ "css": "a" }, { "css": "i" }], "visible": true }),
        json!({ "kind": "expect_each", "selector": { "css": "li" }, "attribute": "href", "contains": "/x", "min": 200 }),
    ] {
        let a: Action = serde_json::from_value(good.clone()).unwrap();
        assert_eq!(a.validate(), Ok(()), "{good}");
        assert_eq!(serde_json::to_value(&a).unwrap(), good);
    }
    // A check, and its targets are the list then the child.
    let a: Action = serde_json::from_value(json!({ "kind": "expect_each", "selector": { "css": "li" }, "child": { "css": "h3" }, "not_empty": true })).unwrap();
    assert!(a.is_check());
    assert_eq!(a.targets().len(), 2);
}

#[test]
fn expect_order_and_position_are_validated() {
    let bad = |v: serde_json::Value| serde_json::from_value::<Action>(v).unwrap().validate().unwrap_err();
    let order = |texts: serde_json::Value| json!({ "kind": "expect_order", "selector": { "role": "option" }, "texts": texts });
    // The selector is checked first, and is a locator object: a string
    // reaches only the first match.
    assert!(bad(json!({ "kind": "expect_order", "selector": {}, "texts": ["a", "b"] })).contains("role, text or css"));
    assert_eq!(
        bad(json!({ "kind": "expect_order", "selector": "li", "texts": ["a", "b"] })),
        "expect_order needs a locator object for selector, not a selector string - a string reaches only the first match, and this reads every item"
    );
    assert_eq!(bad(order(json!([]))), "expect_order needs at least two texts - for one, use expect_contains_text");
    assert_eq!(bad(order(json!(["a"]))), "expect_order needs at least two texts - for one, use expect_contains_text");
    let fifty_one: Vec<String> = (0..51).map(|i| format!("t{i}")).collect();
    assert_eq!(bad(order(json!(fifty_one))), "expect_order takes at most 50 texts");
    assert_eq!(bad(order(json!(["a", " "]))), "expect_order has an empty text - everything contains nothing");
    assert_eq!(bad(order(json!(["", "b"]))), "expect_order has an empty text - everything contains nothing");
    // The list must be seen holding still, which takes time.
    for t in [0, 300, 1499] {
        let mut v = order(json!(["a", "b"]));
        v["timeout_ms"] = json!(t);
        assert_eq!(bad(v), "expect_order: needs at least 1500 ms to see the list hold still", "{t}");
    }
    let fifty: Vec<String> = (0..50).map(|i| format!("t{i}")).collect();
    for good in [
        json!({ "kind": "expect_order", "selector": { "role": "option" }, "texts": ["All", "Draft", "Published", "Unpublished"] }),
        json!({ "kind": "expect_order", "selector": { "css": "li" }, "texts": ["a", "b"], "timeout_ms": 1500 }),
        json!({ "kind": "expect_order", "selector": [{ "role": "list" }, { "role": "listitem" }], "texts": fifty }),
        json!({ "kind": "expect_order", "selector": { "input": "list" }, "texts": ["a", "b"] }),
    ] {
        let a: Action = serde_json::from_value(good.clone()).unwrap();
        assert_eq!(a.validate(), Ok(()), "{good}");
        assert_eq!(serde_json::to_value(&a).unwrap(), good);
        assert!(a.is_check());
        assert_eq!(a.targets().len(), 1);
    }

    let pos = |relation: serde_json::Value| {
        json!({ "kind": "expect_position", "selector": { "role": "button", "name": "New Cycle", "exact": true }, "relative_to": { "css": "#bar" }, "relation": relation })
    };
    // Every relation, written as the guide writes it.
    for relation in
        ["left_of", "right_of", "above", "below", "inside", "in_top_left", "in_top_right", "in_bottom_left", "in_bottom_right"]
    {
        let good = pos(json!(relation));
        let a: Action = serde_json::from_value(good.clone()).unwrap();
        assert_eq!(a.validate(), Ok(()), "{relation}");
        assert_eq!(serde_json::to_value(&a).unwrap(), good);
        assert!(a.is_check());
        assert_eq!(a.targets().len(), 2);
    }
    // Both boxes must be seen at rest, which takes time.
    for t in [0, 500, 1499] {
        let mut v = pos(json!("above"));
        v["timeout_ms"] = json!(t);
        assert_eq!(bad(v), "expect_position: needs at least 1500 ms to see both elements at rest", "{t}");
    }
    let mut with_time = pos(json!("above"));
    with_time["timeout_ms"] = json!(1500);
    let a: Action = serde_json::from_value(with_time.clone()).unwrap();
    assert_eq!(a.validate(), Ok(()));
    assert_eq!(serde_json::to_value(&a).unwrap(), with_time);
    // A relation it does not know is not read.
    let err = serde_json::from_value::<Action>(pos(json!("near"))).unwrap_err().to_string();
    assert!(err.contains("unknown variant `near`"), "{err}");
    // Each locator is checked, the reference under its own name.
    assert!(bad(json!({ "kind": "expect_position", "selector": {}, "relative_to": { "css": "#bar" }, "relation": "above" }))
        .contains("role, text or css"));
    assert_eq!(
        bad(json!({ "kind": "expect_position", "selector": { "css": "#a" }, "relative_to": {}, "relation": "above" })),
        "expect_position relative_to: a locator needs one of role, text or css"
    );
    assert_eq!(
        bad(json!({ "kind": "expect_position", "selector": { "css": "#a" }, "relative_to": " ", "relation": "above" })),
        "expect_position relative_to: a selector is empty"
    );
    // The same locator twice compares an element with itself, however written.
    for (selector, relative_to) in [
        (json!({ "css": "#a" }), json!({ "css": "#a" })),
        (json!({ "css": "#a" }), json!([{ "css": "#a" }])),
        (json!("#a"), json!({ "css": "#a" })),
    ] {
        assert_eq!(
            bad(json!({ "kind": "expect_position", "selector": selector, "relative_to": relative_to, "relation": "inside" })),
            "expect_position compares two elements - relative_to is the same locator as selector"
        );
    }
    // A different locator for the same words is a different element.
    let ok: Action = serde_json::from_value(
        json!({ "kind": "expect_position", "selector": { "css": "li", "nth": 0 }, "relative_to": { "css": "li", "nth": 1 }, "relation": "above" }),
    )
    .unwrap();
    assert_eq!(ok.validate(), Ok(()));
}

/// The colour rule on its own: the CSS basic colours and `transparent`,
/// every hex form, rgb()/rgba() with commas, spaces, a slash or percents,
/// and hsl(); anything else is not a colour.
#[test]
fn colours_normalise_to_rgb_or_rgba() {
    use v2_lib::browser::style::colour;
    let named = [
        ("black", "rgb(0, 0, 0)"),
        ("silver", "rgb(192, 192, 192)"),
        ("gray", "rgb(128, 128, 128)"),
        ("grey", "rgb(128, 128, 128)"),
        ("white", "rgb(255, 255, 255)"),
        ("maroon", "rgb(128, 0, 0)"),
        ("red", "rgb(255, 0, 0)"),
        ("purple", "rgb(128, 0, 128)"),
        ("fuchsia", "rgb(255, 0, 255)"),
        ("green", "rgb(0, 128, 0)"),
        ("lime", "rgb(0, 255, 0)"),
        ("olive", "rgb(128, 128, 0)"),
        ("yellow", "rgb(255, 255, 0)"),
        ("navy", "rgb(0, 0, 128)"),
        ("blue", "rgb(0, 0, 255)"),
        ("teal", "rgb(0, 128, 128)"),
        ("aqua", "rgb(0, 255, 255)"),
        ("orange", "rgb(255, 165, 0)"),
        ("transparent", "rgba(0, 0, 0, 0)"),
    ];
    for (name, rgb) in named {
        assert_eq!(colour(name).as_deref(), Some(rgb), "{name}");
    }
    for (written, rgb) in [
        ("#abc", "rgb(170, 187, 204)"),
        ("#ABCD", "rgba(170, 187, 204, 0.867)"),
        ("#1a2b3c", "rgb(26, 43, 60)"),
        ("#1a2b3c80", "rgba(26, 43, 60, 0.5)"),
        ("#00000000", "rgba(0, 0, 0, 0)"),
        ("rgb(1,2,3)", "rgb(1, 2, 3)"),
        ("rgba(1, 2, 3, 0.25)", "rgba(1, 2, 3, 0.25)"),
        ("rgba(1 2 3 / 25%)", "rgba(1, 2, 3, 0.25)"),
        ("RGB( 1 , 2 , 3 )", "rgb(1, 2, 3)"),
        ("rgb(300, -5, 12.6)", "rgb(255, 0, 13)"),
        ("rgb(50%, 0%, 100%)", "rgb(128, 0, 255)"),
        ("rgba(0, 0, 0, 1)", "rgb(0, 0, 0)"),
        ("hsl(120, 100%, 25%)", "rgb(0, 128, 0)"),
        ("hsl(240deg 100% 50% / 0.5)", "rgba(0, 0, 255, 0.5)"),
        ("hsla(0.5turn, 100%, 50%, 1)", "rgb(0, 255, 255)"),
    ] {
        assert_eq!(colour(written).as_deref(), Some(rgb), "{written}");
    }
    for not_one in ["", "none", "block", "#12", "#ggg", "rgb(1, 2)", "rgb(1, 2, 3, 4, 5)", "2px solid red", "url(#a)", "rebecca"] {
        assert_eq!(colour(not_one), None, "{not_one}");
    }
}

/// Values that are not colours: trimmed, spaces collapsed, case ignored -
/// except inside quotes, where the case is the page's text.
#[test]
fn other_style_values_compare_trimmed_and_ignoring_the_case_of_keywords() {
    use v2_lib::browser::style::{judge, Want};
    assert!(judge("display", "block", Want::Equals(" Block ")).is_ok());
    assert!(judge("font-family", "\"Segoe UI\", Arial", Want::Equals("\"Segoe UI\",  Arial")).is_ok());
    assert!(judge("content", "\"Hello\"", Want::Equals("\"hello\"")).is_err());
    assert!(judge("font-family", "\"Segoe UI\", Arial", Want::Contains("arial")).is_err(), "quoted: the case is the page's");
    assert!(judge("transition", "color 0.2s ease 0s", Want::Contains("COLOR 0.2S")).is_ok());
    assert_eq!(judge("--x", "", Want::Equals("1")).unwrap_err(), "--x is (empty), expected 1");
}

/// Every CSS named colour is read, not only the basic ones: the table is
/// the whole CSS Color 4 list, one entry per name.
#[test]
fn every_css_named_colour_normalises() {
    use v2_lib::browser::style::{colour, NAMED_COLOURS};
    assert_eq!(NAMED_COLOURS.len(), 148);
    let names: Vec<&str> = NAMED_COLOURS.iter().map(|(n, _)| *n).collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(names, sorted, "alphabetical, each name once");
    assert_eq!(colour("rebeccapurple").as_deref(), Some("rgb(102, 51, 153)"));
    assert_eq!(colour("Tomato").as_deref(), Some("rgb(255, 99, 71)"));
    assert_eq!(colour("lightgoldenrodyellow").as_deref(), Some("rgb(250, 250, 210)"));
    for (name, [r, g, b]) in NAMED_COLOURS {
        assert_eq!(colour(name), Some(format!("rgb({r}, {g}, {b})")), "{name}");
    }
}

/// `currentcolor` computes to the actual colour, so a check against it
/// could never be right: it is refused where the script is written.
#[test]
fn expect_style_refuses_currentcolor() {
    use v2_lib::browser::actions::STYLE_CURRENTCOLOR;
    for (how, value) in [("equals", "currentColor"), ("contains", "1px solid currentcolor"), ("not_equals", "CURRENTCOLOR")] {
        let a: Action = serde_json::from_value(json!({ "kind": "expect_style", "selector": "#s", "property": "border", how: value })).unwrap();
        assert_eq!(a.validate().unwrap_err(), STYLE_CURRENTCOLOR, "{how}");
    }
    assert!(STYLE_CURRENTCOLOR.contains("write that colour instead"));
}

/// The browser writes a computed list with one space after each comma:
/// a value written without them, or with spaces before them, is the same.
#[test]
fn comma_spacing_is_normalised() {
    use v2_lib::browser::style::{judge, normal, Want};
    let computed = "matrix(1, 0, 0, 1, 0, 0)";
    assert_eq!(normal("matrix(1,0,0,1,0,0)"), computed);
    assert_eq!(normal(" matrix(1 ,0,  0,1,0 , 0) "), computed);
    assert!(judge("transform", computed, Want::Equals("matrix(1,0,0,1,0,0)")).is_ok());
    assert!(judge("transform", computed, Want::Contains("1,0,0")).is_ok());
    assert!(judge("transform", computed, Want::NotEquals("matrix(1,0,0,1,0,0)")).is_err());
}

#[tokio::test]
async fn check_url_looks_again_until_the_address_holds() {
    let reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = reads.clone();
    let mut d = ScriptedDriver::new(move |method, _| {
        Ok(match method {
            "Runtime.evaluate" => {
                let n = seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                json!({ "result": { "value": if n < 2 { "https://app.example/login" } else { "https://app.example/home" } } })
            }
            _ => json!({}),
        })
    });
    let out = execute_with(&mut d, &Action::CheckUrl { contains: "/home".into(), equals: None, same_as: None, timeout_ms: Some(2000) }, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "url is https://app.example/home");
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn check_url_still_fails_naming_the_address_it_last_saw() {
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &Action::CheckUrl { contains: "/login".into(), equals: None, same_as: None, timeout_ms: None }, &quick()).await;
    assert!(!out.ok && !out.harness, "{}", out.detail);
    assert_eq!(out.detail, "url is https://app.example/home");
}

#[test]
fn the_new_check_options_are_validated() {
    use v2_lib::browser::actions::{ATTRIBUTE_NEEDS_ONE, ATTRIBUTE_ONE_OF, COUNT_NEEDS_ONE, TEXT_NEEDS_ONE};
    let bad = |v: serde_json::Value| serde_json::from_value::<Action>(v).unwrap().validate().unwrap_err();
    assert_eq!(bad(json!({ "kind": "expect_text", "selector": "s", "equals": "a", "matches": "b" })), TEXT_NEEDS_ONE);
    assert_eq!(bad(json!({ "kind": "expect_text", "selector": "s" })), TEXT_NEEDS_ONE);
    assert_eq!(bad(json!({ "kind": "expect_attribute", "selector": "s", "name": "n", "matches": "a", "equals": "b" })), ATTRIBUTE_ONE_OF);
    assert_eq!(bad(json!({ "kind": "expect_attribute", "selector": "s", "name": "n", "matches": "a", "absent": true })), ATTRIBUTE_ONE_OF);
    assert_eq!(bad(json!({ "kind": "expect_attribute", "selector": "s", "name": "n" })), ATTRIBUTE_NEEDS_ONE);
    assert_eq!(bad(json!({ "kind": "expect_count", "selector": "s" })), COUNT_NEEDS_ONE);
    assert_eq!(bad(json!({ "kind": "expect_count", "selector": "s", "equals": 1, "at_least": 1 })), COUNT_NEEDS_ONE);
    assert!(bad(json!({ "kind": "expect_text", "selector": "s", "matches": ".*" })).contains("fits any text"));
    assert!(bad(json!({ "kind": "expect_attribute", "selector": "s", "name": "n", "matches": "(?i)x" })).starts_with("expect_attribute: "));

    for old in [
        json!({ "kind": "expect_count", "selector": "s", "equals": 3 }),
        json!({ "kind": "expect_text", "selector": "s", "equals": "v" }),
        json!({ "kind": "check_url", "contains": "/x" }),
    ] {
        let a: Action = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(a.validate(), Ok(()));
        // The same bytes, key for key and in order, not just the same map.
        assert_eq!(serde_json::to_string(&a).unwrap(), old.to_string());
    }
}

/// `check_url` waits for its own `timeout_ms`, not the default: the page
/// turns after `quick()`'s 300 ms, and only a longer wait sees it.
#[tokio::test]
async fn check_url_waits_for_its_own_timeout() {
    let began = std::time::Instant::now();
    let mut d = ScriptedDriver::new(move |method, _| {
        Ok(match method {
            "Runtime.evaluate" => {
                let late = began.elapsed() >= std::time::Duration::from_millis(450);
                json!({ "result": { "value": if late { "https://app.example/home" } else { "https://app.example/login" } } })
            }
            _ => json!({}),
        })
    });
    let out = execute_with(&mut d, &Action::CheckUrl { contains: "/home".into(), equals: None, same_as: None, timeout_ms: Some(2000) }, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(began.elapsed() >= std::time::Duration::from_millis(450), "{:?}", began.elapsed());

    // The same page with the default wait gives up first.
    let began = std::time::Instant::now();
    let mut d = ScriptedDriver::new(move |method, _| {
        Ok(match method {
            "Runtime.evaluate" => {
                let late = began.elapsed() >= std::time::Duration::from_millis(450);
                json!({ "result": { "value": if late { "https://app.example/home" } else { "https://app.example/login" } } })
            }
            _ => json!({}),
        })
    });
    let out = execute_with(&mut d, &Action::CheckUrl { contains: "/home".into(), equals: None, same_as: None, timeout_ms: None }, &quick()).await;
    assert!(!out.ok && out.detail == "url is https://app.example/login", "{}", out.detail);
    assert!(began.elapsed() < std::time::Duration::from_millis(450), "{:?}", began.elapsed());
}

/// An address that never comes fails after about the wait asked for.
#[tokio::test]
async fn check_url_that_never_holds_fails_after_its_own_timeout() {
    let mut d = FakePage::default().driver();
    let began = std::time::Instant::now();
    let out = execute_with(&mut d, &Action::CheckUrl { contains: "/login".into(), equals: None, same_as: None, timeout_ms: Some(700) }, &quick()).await;
    let took = began.elapsed();
    assert!(!out.ok && out.detail == "url is https://app.example/home", "{}", out.detail);
    assert!(took >= std::time::Duration::from_millis(700) && took < std::time::Duration::from_millis(1500), "{took:?}");
}

/// A browser that never answers is the harness's doing, said with the
/// time waited and what for.
#[tokio::test]
async fn check_url_on_a_browser_that_never_answers_is_a_harness_failure() {
    let mut d = ScriptedDriver::new(|method, _| Err(CdpError::Timeout { what: method.to_string(), ms: 50 }));
    let out = execute_with(&mut d, &Action::CheckUrl { contains: "/home".into(), equals: None, same_as: None, timeout_ms: Some(300) }, &quick()).await;
    assert!(!out.ok && out.harness, "{}", out.detail);
    assert!(out.detail.contains("the page address") && out.detail.contains("300ms"), "{}", out.detail);
}

// ---- check_url equals and same_as, and remember_url ------------------------

#[test]
fn check_url_takes_exactly_one_of_contains_equals_and_same_as() {
    let check = |v: serde_json::Value| serde_json::from_value::<Action>(v).unwrap().validate();
    assert_eq!(check(json!({ "kind": "check_url", "contains": "/a", "equals": "/a" })).unwrap_err(), URL_ONE_OF);
    assert_eq!(check(json!({ "kind": "check_url", "equals": "/a", "same_as": "before" })).unwrap_err(), URL_ONE_OF);
    assert!(check(json!({ "kind": "check_url" })).unwrap_err().contains("empty"));
    assert!(check(json!({ "kind": "check_url", "equals": "" })).unwrap_err().contains("empty"));
    assert!(check(json!({ "kind": "check_url", "same_as": "has space" }))
        .unwrap_err()
        .starts_with("check_url same_as: a noted address's name"));
    assert!(check(json!({ "kind": "remember_url", "as": "" })).unwrap_err().contains(NOTE_NAME_RULE));
    assert!(check(json!({ "kind": "check_url", "equals": "/a" })).is_ok());
    assert!(check(json!({ "kind": "check_url", "same_as": "before" })).is_ok());
    assert!(check(json!({ "kind": "remember_url", "as": "before" })).is_ok());
    let back = serde_json::to_value(serde_json::from_value::<Action>(json!({ "kind": "check_url", "contains": "/home" })).unwrap()).unwrap();
    assert_eq!(back, json!({ "kind": "check_url", "contains": "/home" }));
    assert!(back.get("equals").is_none() && back.get("same_as").is_none());
}

#[test]
fn url_equals_compares_a_path_with_the_path_query_and_fragment() {
    assert!(url_equals("https://app.example/hr/cycles?id=4#top", "/hr/cycles?id=4#top"));
    assert!(!url_equals("https://app.example/hr/cycles?id=4", "/hr/cycles"));
    assert!(url_equals("https://app.example/x", "https://app.example/x"));
    assert!(!url_equals("https://app.example/x/", "https://app.example/x"));
    assert!(url_equals("https://app.example", "/"));
    assert!(url_equals("https://app.example?a=1", "/?a=1"));
}

#[tokio::test]
async fn check_url_equals_passes_only_on_the_exact_address() {
    let eq = |want: &str| Action::CheckUrl { contains: String::new(), equals: Some(want.into()), same_as: None, timeout_ms: None };
    let mut d = FakePage::default().driver();
    let out = execute_with(&mut d, &eq("/home"), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "url is https://app.example/home");
    let out = execute_with(&mut d, &eq("/hom"), &quick()).await;
    assert!(!out.ok && !out.harness, "{}", out.detail);
    assert!(out.detail.starts_with("url is https://app.example/home"), "{}", out.detail);
    assert_eq!(out.detail, "url is https://app.example/home, not \"/hom\"");
}

#[tokio::test]
async fn remember_url_notes_the_address_for_a_later_same_as() {
    let mut d = FakePage::default().driver();
    let remember = Action::RememberUrl { name: "before".into() };
    let same = Action::CheckUrl { contains: String::new(), equals: None, same_as: Some("before".into()), timeout_ms: None };
    let out = execute_with(&mut d, &remember, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "noted the address as \"before\": /home");
    assert_eq!(d.noted.get("before").map(String::as_str), Some("https://app.example/home"));
    let out = execute_with(&mut d, &same, &quick()).await;
    assert!(out.ok, "{}", out.detail);
}

#[tokio::test]
async fn check_url_same_as_fails_when_the_address_changed() {
    let same = |name: &str| Action::CheckUrl { contains: String::new(), equals: None, same_as: Some(name.into()), timeout_ms: None };
    let mut d = FakePage::default().driver();
    d.noted.insert("before".into(), "https://app.example/start".into());
    let out = execute_with(&mut d, &same("before"), &quick()).await;
    assert!(!out.ok && !out.harness, "{}", out.detail);
    assert!(out.detail.contains("not the address noted as \"before\""), "{}", out.detail);
    let calls = d.calls.len();
    let out = execute_with(&mut d, &same("never"), &quick()).await;
    assert!(!out.ok, "{}", out.detail);
    assert_eq!(out.detail, not_noted("never"));
    assert_eq!(d.calls.len(), calls, "nothing noted is no look at the page");
}
