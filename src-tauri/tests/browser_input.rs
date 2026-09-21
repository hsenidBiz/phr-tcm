//! "Can a person actually click this now?" - and real input once they can.

mod common;

use common::ScriptedDriver;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::input::{click, fill, wait_ready, Blocked, Ready, FOCUS_JS, PROBE_JS};
use v2_lib::browser::locator::Target;
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0 }
}

fn probe(over: Value) -> Value {
    let mut base = json!({
        "visible": true, "enabled": true, "editable": true, "stable": true,
        "hit": true, "x": 40.5, "y": 12.0, "covered_by": ""
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

#[tokio::test]
async fn a_ready_element_comes_back_with_where_to_click() {
    let (mut d, _) = page(1, vec![probe(json!({}))]);
    let ready = wait_ready(&mut d, &css("#go"), false, &quick()).await.ok().unwrap();
    assert_eq!(ready.handle, "el-0");
    assert_eq!((ready.x, ready.y), (40.5, 12.0));
}

/// The whole point: a button that is disabled while the page loads is
/// waited for, not clicked blind and not failed at once.
#[tokio::test]
async fn it_waits_for_a_disabled_element_to_become_enabled() {
    let (mut d, asked) = page(
        1,
        vec![probe(json!({ "enabled": false })), probe(json!({ "enabled": false })), probe(json!({}))],
    );
    assert!(wait_ready(&mut d, &css("#go"), false, &quick()).await.is_ok());
    assert_eq!(asked.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn each_reason_is_said_in_words() {
    let cases = [
        (json!({ "visible": false }), "is not visible"),
        (json!({ "stable": false }), "is still moving"),
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

#[tokio::test]
async fn typing_needs_something_that_takes_text() {
    let (mut d, _) = page(1, vec![probe(json!({ "editable": false }))]);
    match wait_ready(&mut d, &css("#go"), true, &quick()).await {
        Err(Blocked::Page(msg)) => assert!(msg.contains("cannot be typed into"), "{msg}"),
        _ => panic!("expected a page reason"),
    }
    // The same element is fine to CLICK.
    let (mut d, _) = page(1, vec![probe(json!({ "editable": false }))]);
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

#[tokio::test]
async fn a_click_is_three_real_mouse_events_at_the_probed_point() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    click(&mut d, &Ready { handle: "el".into(), x: 40.5, y: 12.0 }).await.unwrap();
    let sent = d.calls_to("Input.dispatchMouseEvent");
    let kinds: Vec<&str> = sent.iter().map(|p| p["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["mouseMoved", "mousePressed", "mouseReleased"]);
    assert!(sent.iter().all(|p| p["x"] == 40.5 && p["y"] == 12.0));
    assert_eq!(sent[1]["button"], "left");
    assert_eq!(sent[1]["clickCount"], 1);
}

/// Measured on real Edge: Input.insertText fires a genuine input event,
/// which is what a framework-controlled field listens for.
#[tokio::test]
async fn a_fill_selects_what_is_there_and_types_over_it() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "text" } })),
        _ => Ok(json!({})),
    });
    fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "Custom 4-Point").await.ok().unwrap();
    let focus = &d.calls_to("Runtime.callFunctionOn")[0];
    assert_eq!(focus["functionDeclaration"], FOCUS_JS);
    assert_eq!(focus["objectId"], "el");
    assert_eq!(d.calls_to("Input.insertText")[0]["text"], "Custom 4-Point");
}

#[tokio::test]
async fn filling_with_nothing_clears_the_field() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": "text" } })),
        _ => Ok(json!({})),
    });
    fill(&mut d, &Ready { handle: "el".into(), x: 0.0, y: 0.0 }, "").await.ok().unwrap();
    assert!(d.calls_to("Input.insertText").is_empty());
    let keys = d.calls_to("Input.dispatchKeyEvent");
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["type"], "keyDown");
    assert_eq!(keys[0]["key"], "Backspace");
    assert_eq!(keys[1]["type"], "keyUp");
}

/// A native <select> is set in the page (typing into one does nothing),
/// and an option that is not there is said plainly.
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
}
