//! "Can a person actually click this now?" - and real input once they can.

use crate::common;

use common::ScriptedDriver;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::input::{
    click, fill, wait_ready, Blocked, Ready, FOCUS_JS, HAS_FOCUS_JS, PROBE_JS,
};
use v2_lib::browser::locator::Target;
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0 }
}

/// A field that answers both halves of a fill: `FOCUS_JS` says what kind
/// of control it is, and the focus check says it still holds the focus
/// when the text is about to be sent.
fn field(kind: &'static str) -> ScriptedDriver {
    ScriptedDriver::new(move |method, params| match method {
        "Runtime.callFunctionOn" if params["functionDeclaration"] == HAS_FOCUS_JS => {
            Ok(json!({ "result": { "value": true } }))
        }
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": kind } })),
        _ => Ok(json!({})),
    })
}

fn probe(over: Value) -> Value {
    let mut base = json!({
        "visible": true, "enabled": true, "editable": true, "onscreen": true,
        "hit": true, "x": 40.5, "y": 12.0, "covered_by": "", "rect": [0.0, 0.0, 80.0, 24.0]
    });
    for (k, v) in over.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

/// A page where a css locator finds `found` elements and the probe gives
/// `probes` in turn, repeating the last.
fn page(found: usize, probes: Vec<Value>) -> (ScriptedDriver, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let d = ScriptedDriver::new(move |method, params| match method {
        "Runtime.releaseObjectGroup" => Ok(json!({})),
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" if params["functionDeclaration"] == PROBE_JS => {
            let i = counter.fetch_add(1, Ordering::SeqCst).min(probes.len() - 1);
            Ok(json!({ "result": { "value": probes[i] } }))
        }
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
        "Runtime.getProperties" => Ok(json!({ "result": (0..found)
            .map(|i| json!({ "name": i.to_string(), "value": { "objectId": format!("el-{i}") } }))
            .collect::<Vec<_>>() })),
        other => panic!("unexpected {other}"),
    });
    (d, asked)
}

fn css(sel: &str) -> Target {
    serde_json::from_value(json!({ "css": sel })).unwrap()
}

/// A ready element still needs to be SEEN holding still: the same rect on
/// two consecutive looks.
#[tokio::test]
async fn a_ready_element_comes_back_with_where_to_click() {
    let (mut d, asked) = page(1, vec![probe(json!({})), probe(json!({}))]);
    let ready = wait_ready(&mut d, &css("#go"), false, &quick()).await.ok().unwrap();
    assert_eq!(ready.handle, "el-0");
    assert_eq!((ready.x, ready.y), (40.5, 12.0));
    assert_eq!(asked.load(Ordering::SeqCst), 2);
}

/// The whole point: a button that is disabled while the page loads is
/// waited for, not clicked blind and not failed at once.
#[tokio::test]
async fn it_waits_for_a_disabled_element_to_become_enabled() {
    let (mut d, asked) = page(
        1,
        vec![
            probe(json!({ "enabled": false })),
            probe(json!({ "enabled": false })),
            probe(json!({})),
            probe(json!({})),
        ],
    );
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_ok());
    // Two disabled looks, then two matching-rect looks once it is enabled.
    assert_eq!(asked.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn each_reason_is_said_in_words() {
    let cases = [
        (json!({ "visible": false }), "is not visible"),
        (json!({ "onscreen": false }), "is outside the visible part of the page"),
        (json!({ "enabled": false }), "is disabled"),
        (json!({ "hit": false, "covered_by": "div.overlay" }), "is covered by div.overlay"),
    ];
    for (over, words) in cases {
        let (mut d, _) = page(1, vec![probe(over)]);
        match wait_ready(&mut d, &css("#go"), false, &quick()).await {
            Err(Blocked::Page(msg)) => {
                assert!(msg.contains(words), "{msg}");
                assert!(msg.contains("#go"), "the target is missing from: {msg}");
                assert!(msg.contains("300ms"), "the wait is missing from: {msg}");
            }
            _ => panic!("expected a page reason containing {words:?}"),
        }
    }
}

/// The Rust-side fallback for an unnamed cause must never say "nothing":
/// that reads as "covered by nothing", which is nonsense to a person.
#[tokio::test]
async fn a_covered_element_with_no_named_cause_still_gets_a_reason() {
    let mut raw = probe(json!({ "hit": false }));
    raw.as_object_mut().unwrap().remove("covered_by");
    let (mut d, _) = page(1, vec![raw]);
    match wait_ready(&mut d, &css("#go"), false, &quick()).await {
        Err(Blocked::Page(msg)) => {
            assert!(msg.contains("is covered by another element"), "{msg}");
            assert!(!msg.contains("nothing"), "{msg}");
        }
        _ => panic!("expected a page reason"),
    }
}

/// Two consecutive looks with the SAME rect: ready on the second.
#[tokio::test]
async fn readiness_needs_the_same_rect_twice_in_a_row() {
    let rect_a = json!([0.0, 0.0, 80.0, 24.0]);
    let (mut d, asked) =
        page(1, vec![probe(json!({ "rect": rect_a.clone() })), probe(json!({ "rect": rect_a }))]);
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_ok());
    assert_eq!(asked.load(Ordering::SeqCst), 2);
}

/// Rect A, then rect B twice: ready on the third look, once B repeats.
#[tokio::test]
async fn a_settling_rect_change_still_reaches_ready() {
    let rect_a = json!([0.0, 0.0, 80.0, 24.0]);
    let rect_b = json!([5.0, 0.0, 80.0, 24.0]);
    let (mut d, asked) = page(
        1,
        vec![
            probe(json!({ "rect": rect_a })),
            probe(json!({ "rect": rect_b.clone() })),
            probe(json!({ "rect": rect_b })),
        ],
    );
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_ok());
    assert_eq!(asked.load(Ordering::SeqCst), 3);
}

/// A rect that never repeats within the action budget never counts as
/// ready: the last reason reported is "still moving", not a false pass.
#[tokio::test]
async fn a_never_settling_rect_times_out_as_still_moving() {
    let n = Arc::new(AtomicUsize::new(0));
    let c = n.clone();
    let mut d = ScriptedDriver::new(move |method, params| match method {
        "Runtime.releaseObjectGroup" => Ok(json!({})),
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" if params["functionDeclaration"] == PROBE_JS => {
            let i = c.fetch_add(1, Ordering::SeqCst) as f64;
            Ok(json!({ "result": { "value": probe(json!({ "rect": [i, 0.0, 80.0, 24.0] })) } }))
        }
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
        "Runtime.getProperties" => {
            Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el-0" } } ] }))
        }
        other => panic!("unexpected {other}"),
    });
    match wait_ready(&mut d, &css("#go"), false, &quick()).await {
        Err(Blocked::Page(msg)) => assert!(msg.contains("is still moving"), "{msg}"),
        other => panic!("expected a still-moving timeout, got {other:?}"),
    }
}

#[tokio::test]
async fn typing_needs_something_that_takes_text() {
    let (mut d, _) = page(1, vec![probe(json!({ "editable": false }))]);
    match wait_ready(&mut d, &css("#go"), true, &quick()).await {
        Err(Blocked::Page(msg)) => assert!(msg.contains("cannot be typed into"), "{msg}"),
        _ => panic!("expected a page reason"),
    }
    // The same element is fine to CLICK.
    let (mut d, _) =
        page(1, vec![probe(json!({ "editable": false })), probe(json!({ "editable": false }))]);
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_ok());
}

#[tokio::test]
async fn nothing_found_and_too_many_found_are_both_reasons() {
    let (mut d, _) = page(0, vec![probe(json!({}))]);
    match wait_ready(&mut d, &css("#nope"), false, &quick()).await {
        Err(Blocked::Page(msg)) => assert!(msg.contains("not found"), "{msg}"),
        _ => panic!("expected a page reason"),
    }
    // A structured locator has to mean ONE element, or the click is a guess.
    let (mut d, _) = page(3, vec![probe(json!({}))]);
    match wait_ready(&mut d, &css("button"), false, &quick()).await {
        Err(Blocked::Page(msg)) => assert!(msg.contains("matched 3 elements"), "{msg}"),
        _ => panic!("expected a page reason"),
    }
}

/// A page between two documents refuses calls for a moment. That is a
/// reason to look again, not a harness failure.
#[tokio::test]
async fn a_refused_call_is_retried() {
    let n = Arc::new(AtomicUsize::new(0));
    let c = n.clone();
    let mut d = ScriptedDriver::new(move |method, params| match method {
        "Runtime.releaseObjectGroup" => Ok(json!({})),
        "Runtime.evaluate" => {
            if c.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(CdpError::Protocol {
                    method: "Runtime.evaluate".into(),
                    message: "Cannot find context with specified id".into(),
                })
            } else {
                Ok(json!({ "result": { "objectId": "doc" } }))
            }
        }
        "Runtime.callFunctionOn" if params["functionDeclaration"] == PROBE_JS => {
            Ok(json!({ "result": { "value": probe(json!({})) } }))
        }
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
        "Runtime.getProperties" => Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el-0" } } ] })),
        other => panic!("unexpected {other}"),
    });
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_ok());
}

#[tokio::test]
async fn a_dead_browser_is_a_harness_failure_at_once() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.releaseObjectGroup" => Ok(json!({})),
        _ => Err(CdpError::Closed),
    });
    match wait_ready(&mut d, &css("#go"), false, &quick()).await {
        Err(Blocked::Harness(msg)) => assert!(msg.contains("browser"), "{msg}"),
        _ => panic!("expected a harness failure"),
    }
    assert!(d.calls.len() <= 3, "it must not keep polling a dead browser: {:?}", d.methods());
}

/// `click` re-probes the handle before it acts: the coordinate in `Ready`
/// is stale by the time a highlight pause has run, so the fresh point is
/// what actually gets clicked.
#[tokio::test]
async fn a_click_re_probes_then_sends_three_real_mouse_events() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.callFunctionOn" if params["functionDeclaration"] == PROBE_JS => {
            Ok(json!({ "result": { "value": probe(json!({ "x": 40.5, "y": 12.0 })) } }))
        }
        _ => Ok(json!({})),
    });
    click(&mut d, &Ready { handle: "el".into(), x: 999.0, y: 999.0 }).await.unwrap();

    let probed = &d.calls_to("Runtime.callFunctionOn")[0];
    assert_eq!(probed["functionDeclaration"], PROBE_JS);
    assert_eq!(probed["objectId"], "el");

    let sent = d.calls_to("Input.dispatchMouseEvent");
    let kinds: Vec<&str> = sent.iter().map(|p| p["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["mouseMoved", "mousePressed", "mouseReleased"]);
    // The FRESH point from the re-probe, not the stale one in `Ready`.
    assert!(sent.iter().all(|p| p["x"] == 40.5 && p["y"] == 12.0));
    let buttons: Vec<i64> = sent.iter().map(|p| p["buttons"].as_i64().unwrap()).collect();
    assert_eq!(buttons, vec![0, 1, 0]);
    assert_eq!(sent[1]["button"], "left");
    assert_eq!(sent[1]["clickCount"], 1);
}

#[tokio::test]
async fn a_click_is_refused_when_the_re_probe_finds_it_covered() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.callFunctionOn" if params["functionDeclaration"] == PROBE_JS => Ok(json!({
            "result": { "value": probe(json!({ "hit": false, "covered_by": "div.overlay" })) }
        })),
        _ => Ok(json!({})),
    });
    match click(&mut d, &Ready { handle: "el".into(), x: 40.5, y: 12.0 }).await {
        Err(Blocked::Page(msg)) => {
            assert!(msg.contains("just before the click"), "{msg}");
            assert!(msg.contains("is covered by div.overlay"), "{msg}");
        }
        other => panic!("expected a page reason, got {other:?}"),
    }
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty());
}

/// Measured on real Edge: Input.insertText fires a genuine input event,
/// which is what a framework-controlled field listens for.
#[tokio::test]
async fn a_fill_selects_what_is_there_and_types_over_it() {
    let mut d = field("text");
    fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "Custom 4-Point").await.ok().unwrap();
    let focus = &d.calls_to("Runtime.callFunctionOn")[0];
    assert_eq!(focus["functionDeclaration"], FOCUS_JS);
    assert_eq!(focus["objectId"], "el");
    assert_eq!(d.calls_to("Input.insertText")[0]["text"], "Custom 4-Point");
}

#[tokio::test]
async fn filling_with_nothing_clears_the_field() {
    let mut d = field("text");
    fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "").await.ok().unwrap();
    assert!(d.calls_to("Input.insertText").is_empty());
    let keys = d.calls_to("Input.dispatchKeyEvent");
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["type"], "keyDown");
    assert_eq!(keys[0]["key"], "Backspace");
    assert_eq!(keys[1]["type"], "keyUp");
}

/// A date-like field (date, time, month, week, datetime-local, color,
/// range) wants its parts in the order the machine's locale puts them,
/// while a script always writes `2026-09-21`. So it is set through its
/// native value setter and never typed into.
#[tokio::test]
async fn a_date_like_field_is_set_directly_not_typed() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "set" } })),
        _ => Ok(json!({})),
    });
    fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "2026-09-21").await.ok().unwrap();
    assert!(d.calls_to("Input.insertText").is_empty());
    assert!(d.calls_to("Input.dispatchKeyEvent").is_empty());
}

/// A native <select> is set in the page (typing into one does nothing),
/// and an option that is not there, or that cannot be picked, is said
/// plainly.
#[tokio::test]
async fn a_native_select_is_chosen_not_typed() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "select-ok" } })),
        _ => Ok(json!({})),
    });
    fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "Numeric").await.ok().unwrap();
    assert!(d.calls_to("Input.insertText").is_empty());

    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "select-missing" } })),
        _ => Ok(json!({})),
    });
    match fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "Nope").await {
        Err(Blocked::Page(msg)) => assert!(msg.contains("no option") && msg.contains("Nope"), "{msg}"),
        _ => panic!("expected a page reason"),
    }

    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "select-disabled" } })),
        _ => Ok(json!({})),
    });
    match fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "Archived").await {
        Err(Blocked::Page(msg)) => {
            assert!(msg.contains("is disabled") && msg.contains("Archived"), "{msg}")
        }
        _ => panic!("expected a page reason"),
    }
}

#[tokio::test]
async fn a_page_refusal_during_fill_is_blamed_on_the_page() {
    let mut d = ScriptedDriver::new(|_, _| {
        Err(CdpError::Protocol {
            method: "Runtime.callFunctionOn".into(),
            message: "no such element".into(),
        })
    });
    match fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "x").await {
        Err(Blocked::Page(msg)) => {
            assert!(msg.contains("the page refused"), "{msg}");
            assert!(!msg.contains("browser"), "{msg}");
        }
        other => panic!("expected a page reason, got {other:?}"),
    }
}

#[tokio::test]
async fn a_dead_browser_during_fill_is_blamed_on_the_browser() {
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    match fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "x").await {
        Err(Blocked::Harness(msg)) => assert!(msg.contains("browser"), "{msg}"),
        other => panic!("expected a harness failure, got {other:?}"),
    }
}

/// Focusing and typing are SEPARATE round trips, and the text goes to
/// whatever `document.activeElement` is by the time it lands. A page that
/// moves focus in between (an autofocusing dialog, a focus trap) would
/// otherwise take the typing into another field while this still reported
/// "filled" - a false pass. Nothing at all is sent.
#[tokio::test]
async fn nothing_is_typed_when_the_field_lost_the_focus() {
    for value in ["Custom 4-Point", ""] {
        let mut d = ScriptedDriver::new(|method, params| match method {
            "Runtime.callFunctionOn" if params["functionDeclaration"] == HAS_FOCUS_JS => {
                Ok(json!({ "result": { "value": false } }))
            }
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "text" } })),
            _ => Ok(json!({})),
        });
        match fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, value).await {
            Err(Blocked::Page(msg)) => {
                assert!(msg.contains("lost focus before it could be typed into"), "{msg}");
                assert!(!msg.contains('\u{2014}'), "no em dashes in what a person reads: {msg}");
            }
            other => panic!("expected a page reason for {value:?}, got {other:?}"),
        }
        assert!(d.calls_to("Input.insertText").is_empty(), "text was sent anyway");
        assert!(d.calls_to("Input.dispatchKeyEvent").is_empty(), "keys were sent anyway");
    }
}

/// The focus check is asked ON the element, and only after the page has
/// said what kind of control it is.
#[tokio::test]
async fn the_focus_is_checked_on_the_element_just_before_typing() {
    let mut d = field("text");
    fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "hello").await.ok().unwrap();
    let called: Vec<&str> = d
        .calls_to("Runtime.callFunctionOn")
        .iter()
        .map(|p| if p["functionDeclaration"] == HAS_FOCUS_JS { "focus?" } else { "focus!" })
        .collect();
    assert_eq!(called, vec!["focus!", "focus?"]);
    assert_eq!(d.calls_to("Runtime.callFunctionOn")[1]["objectId"], "el");
}

/// Every way out of the wait hands the deadline back. A loop that left
/// one set would cap every later call at the floor, because the budget it
/// named has long since passed.
#[tokio::test]
async fn a_wait_always_hands_its_deadline_back() {
    let (mut d, _) = page(1, vec![probe(json!({})), probe(json!({}))]);
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_ok());
    assert!(d.deadlines.first().is_some_and(Option::is_some), "it never set one");
    assert!(d.deadline_was_cleared(), "after success: {:?}", d.deadlines.len());

    let (mut d, _) = page(0, vec![probe(json!({}))]);
    assert!(wait_ready(&mut d, &css("#nope"), false, &quick()).await.is_err());
    assert!(d.deadline_was_cleared(), "after a page failure");

    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.releaseObjectGroup" => Ok(json!({})),
        _ => Err(CdpError::Closed),
    });
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_err());
    assert!(d.deadline_was_cleared(), "after a harness failure");
}

/// A single call that eats the whole wait budget and then times out never
/// let a single look complete either - the wait ending mid-call rather
/// than between two of them does not change that. This used to be
/// misreported with page wording ("waited 300ms: ..."); it is the
/// browser's silence, not a slow page, and is now said that way.
#[tokio::test]
async fn a_call_that_burns_the_whole_budget_with_no_look_is_the_harness() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.releaseObjectGroup" => Ok(json!({})),
        other => {
            // The call itself burns the whole action budget, exactly as a
            // browser that takes frames and stops answering does.
            std::thread::sleep(Duration::from_millis(400));
            Err(CdpError::Timeout { what: other.to_string(), ms: 400 })
        }
    });
    match wait_ready(&mut d, &css("#go"), false, &quick()).await {
        Err(Blocked::Harness(msg)) => {
            assert!(msg.contains("browser did not answer"), "{msg}");
            assert!(msg.contains("#go"), "{msg}");
        }
        other => panic!("expected a harness failure, got {other:?}"),
    }
    assert!(d.deadline_was_cleared());
}

/// While the budget remains, a timeout is still what it always was: a
/// browser that has stopped answering.
#[tokio::test]
async fn a_timeout_while_the_budget_remains_is_still_the_harness() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.releaseObjectGroup" => Ok(json!({})),
        other => Err(CdpError::Timeout { what: other.to_string(), ms: 250 }),
    });
    match wait_ready(&mut d, &css("#go"), false, &quick()).await {
        Err(Blocked::Harness(msg)) => assert!(msg.contains("browser"), "{msg}"),
        other => panic!("expected a harness failure, got {other:?}"),
    }
    assert!(d.deadline_was_cleared());
}

fn timeout_of(method: &str) -> CdpError {
    CdpError::Timeout { what: method.to_string(), ms: 50 }
}

/// Not one look completed before the deadline - every call timed out - so
/// this is the browser's silence, not a missing element. Reporting it as
/// "not found" would send someone looking for a selector bug that was
/// never there.
#[tokio::test]
async fn a_browser_that_never_answers_is_not_reported_as_a_missing_element() {
    let mut d = ScriptedDriver::new(|method, _| Err(timeout_of(method)));
    match wait_ready(&mut d, &css("#save"), false, &quick()).await {
        Err(Blocked::Harness(msg)) => {
            assert!(msg.contains("browser did not answer"), "{msg}");
            assert!(!msg.contains("not found"), "{msg}");
        }
        other => panic!("expected a harness failure, got {other:?}"),
    }
}

/// Once one look has completed (the browser answered, even if the
/// element was not yet ready), later silence still ends the wait with
/// today's page wording, not a harness failure.
#[tokio::test]
async fn one_completed_look_keeps_the_page_wording() {
    // Not visible, so the first look never reaches `Ready` - a completed
    // look reporting "is not visible" - and every call after it (starting
    // with the very next look's own first call) goes silent. A full look
    // here is 5 calls: the loop's own `release`, then `resolve` (document,
    // the css lookup, and getProperties), then the actionability probe.
    let calls = AtomicUsize::new(0);
    let mut d = ScriptedDriver::new(move |method, params| {
        if calls.fetch_add(1, Ordering::SeqCst) >= 5 {
            return Err(timeout_of(method));
        }
        match method {
            "Runtime.releaseObjectGroup" => Ok(json!({})),
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if params["functionDeclaration"] == PROBE_JS => {
                Ok(json!({ "result": { "value": probe(json!({ "visible": false })) } }))
            }
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => {
                Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el-0" } } ] }))
            }
            _ => Ok(json!({})),
        }
    });
    match wait_ready(&mut d, &css("#go"), false, &quick()).await {
        Err(Blocked::Page(msg)) => assert!(msg.starts_with("waited"), "{msg}"),
        other => panic!("expected a page failure, got {other:?}"),
    }
}
