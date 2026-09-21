//! What each action does to the browser, and how the executor reports it.
//! A described fake page stands in for Edge, so every rule here is pinned
//! without opening a window. `tests/browser_live.rs` checks the same things
//! against the real one.

mod common;

use common::{ready_probe, FakePage, ScriptedDriver};
use serde_json::json;
use v2_lib::browser::actions::{execute_in, execute_with, Action, Policy, HIGHLIGHT_JS, RESOLVE_URL_JS};
use v2_lib::browser::cdp::{CdpError, Event};
use v2_lib::browser::input::PROBE_JS;
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
/// `tests/common/mod.rs`), not `href`, which is what `location.href`
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
