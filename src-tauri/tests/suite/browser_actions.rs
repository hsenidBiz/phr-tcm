//! What each action does to the browser, and how the executor reports it.
//! A described fake page stands in for Edge, so every rule here is pinned
//! without opening a window. `tests/suite/browser_live.rs` checks the same things
//! against the real one.

use crate::common;

use common::{ready_probe, FakePage, ScriptedDriver};
use serde_json::json;
use v2_lib::browser::actions::{execute_in, execute_with, Action, Policy, HIGHLIGHT_JS, RESOLVE_URL_JS};
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
        Action::CheckUrl { contains: "/home".into() },
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
        Action::CheckUrl { contains: "/home".into() },
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
        Action::CheckUrl { contains: "c".into() },
        Action::ExpectVisible { selector: "s".into(), timeout_ms: None },
        Action::ExpectHidden { selector: "s".into(), timeout_ms: None },
        Action::ExpectText { selector: "s".into(), equals: "v".into(), timeout_ms: None },
        Action::ExpectContainsText { selector: "s".into(), value: "v".into(), timeout_ms: None },
        Action::ExpectCount { selector: "s".into(), equals: 1, timeout_ms: None },
        Action::ExpectAttribute { selector: "s".into(), name: "n".into(), equals: "v".into(), timeout_ms: None },
        Action::SignIn { account: "a".into() },
        Action::Upload { selector: "s".into(), file: "f.pdf".into() },
        Action::ExpectResponse { method: None, url_contains: "/x".into(), status: 200, json: None, timeout_ms: None, stray: Default::default() },
        Action::ApiRequest { path: "/api/x".into(), query: Default::default(), expect: Default::default(), stray: Default::default() },
        // A guard is a tidy-up, never a check - even holding one.
        Action::WhenVisible { selector: "s".into(), within_ms: None, then: vec![Action::ExpectVisible { selector: "s".into(), timeout_ms: None }] },
        Action::ExpectDownload {
            name: "report.csv".into(),
            within_ms: None,
            sheet: None,
            headers: None,
            cells: None,
            contains_text: None,
            stray: Default::default(),
        },
        Action::ExpectTab { name: "r".into(), url_contains: None, within_ms: None },
        Action::OpenTab { name: "r".into(), url: "/x".into() },
        Action::SwitchTab { name: "r".into() },
        Action::CloseTab { name: "r".into() },
        Action::ExpectTabClosed { name: "r".into(), within_ms: None },
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
                | "expect_response"
                | "api_request"
                | "expect_download"
                | "expect_tab"
                | "expect_tab_closed"
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
        "expect_download has no \"header\" - it takes name, within_ms, sheet, headers, cells and contains_text"
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
