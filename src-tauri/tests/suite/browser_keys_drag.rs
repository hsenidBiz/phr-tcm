//! Key combinations (`press_key` with Ctrl, Alt, Shift and Meta, and
//! `times`) and `drag`, against a scripted fake. `browser_live` proves the
//! same against a real browser.

use crate::common;

use common::{ready_probe, ScriptedDriver};
use serde_json::{json, Value};
use std::time::Duration;
use v2_lib::autorun::patterns::{action_target, classify, ErrorClass};
use v2_lib::autorun::report::action_words;
use v2_lib::browser::actions::{execute_with, Action, ActionOutcome, FOCUS_NOW_JS, WITHIN_MS_ZERO};
use v2_lib::browser::cdp::{CdpError, Event, MAIN_TAB};
use v2_lib::browser::drag::{did_not_finish, nothing_to_drag, nowhere_to_drop, DRAGS_STILL_HELD, NEXT_FRAME_JS, RECT_JS};
use v2_lib::browser::input::PROBE_JS;
use v2_lib::browser::keys::{self, TIMES_RULE};
use v2_lib::browser::locator::{CSS_JS, FRAME_DOC_JS, FRAME_JS};
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn action(v: Value) -> Action {
    serde_json::from_value(v).expect("an action")
}

// ---- press_key: what a script may say ----------------------------------------

#[test]
fn each_modifier_and_combination_is_read_whatever_its_case_and_order() {
    for (written, name, mask) in [
        ("Ctrl+ArrowUp", "Ctrl+ArrowUp", 2),
        ("Shift+Tab", "Shift+Tab", 8),
        ("Ctrl+Shift+End", "Ctrl+Shift+End", 10),
        ("Alt+ArrowDown", "Alt+ArrowDown", 1),
        ("Meta+Home", "Meta+Home", 4),
        ("ctrl+ArrowUp", "Ctrl+ArrowUp", 2),
        ("SHIFT+ctrl+End", "Ctrl+Shift+End", 10),
        (" Ctrl + ArrowLeft ", "Ctrl+ArrowLeft", 2),
        ("Meta+Shift+Alt+Ctrl+Tab", "Ctrl+Alt+Shift+Meta+Tab", 15),
        ("Enter", "Enter", 0),
    ] {
        let combo = keys::parse(written).unwrap_or_else(|e| panic!("{written}: {e}"));
        assert_eq!(combo.name(), name, "{written}");
        assert_eq!(combo.mask(), mask, "{written}");
        assert!(action(json!({ "kind": "press_key", "key": written })).validate().is_ok(), "{written}");
    }
}

#[test]
fn each_refusal_is_said_in_its_own_words() {
    let refused = |key: &str| action(json!({ "kind": "press_key", "key": key })).validate().unwrap_err();
    assert_eq!(
        refused("Hyper+ArrowUp"),
        "press_key: \"Hyper\" is not a modifier - use Ctrl, Shift, Alt or Meta"
    );
    assert_eq!(refused("Ctrl+"), "press_key: \"Ctrl+\" has no key after its modifiers");
    assert_eq!(refused("Ctrl+Shift"), "press_key: \"Ctrl+Shift\" has no key after its modifiers");
    assert_eq!(refused("Alt"), "press_key: \"Alt\" has no key after its modifiers");
    assert_eq!(refused("Ctrl+ctrl+ArrowUp"), "press_key: \"Ctrl\" is given twice");
    assert_eq!(refused("Shift+Shift"), "press_key: \"Shift\" is given twice");
    // The key keeps its own spelling, as it always has.
    let why = refused("Ctrl+F5");
    assert!(why.starts_with("press_key \"F5\" is not a key it presses - use one of Tab, Enter"), "{why}");
    assert!(refused("Ctrl+arrowup").contains("\"arrowup\""));
}

#[test]
fn times_is_one_to_fifty() {
    let with = |times: Value| action(json!({ "kind": "press_key", "key": "Ctrl+ArrowUp", "times": times })).validate();
    assert!(with(json!(1)).is_ok());
    assert!(with(json!(50)).is_ok());
    assert_eq!(with(json!(0)).unwrap_err(), TIMES_RULE);
    assert_eq!(with(json!(51)).unwrap_err(), TIMES_RULE);
    assert_eq!(TIMES_RULE, "press_key: times must be 1 to 50");
    // Past what the field can hold, the script does not even read.
    assert!(serde_json::from_value::<Action>(json!({ "kind": "press_key", "key": "Tab", "times": 300 })).is_err());
}

/// A script saved before combinations existed is written back byte for
/// byte, and the new keys appear only when set.
#[test]
fn old_and_new_scripts_round_trip_unchanged() {
    for text in [
        r#"{"kind":"press_key","key":"Tab"}"#,
        r#"{"kind":"press_key","key":"Shift+Tab"}"#,
        r#"{"kind":"press_key","key":"Ctrl+ArrowUp","times":3}"#,
        r##"{"kind":"drag","from":{"css":"#a"},"to":{"css":"#b"}}"##,
        r##"{"kind":"drag","from":{"role":"row","name":"C"},"to":[{"css":"iframe"},{"css":"#b"}],"position":"before","within_ms":5000}"##,
    ] {
        let read: Action = serde_json::from_str(text).unwrap();
        assert_eq!(serde_json::to_string(&read).unwrap(), text);
    }
}

// ---- press_key: what reaches the page ---------------------------------------

/// A page whose focus is on `button "Up"`.
fn key_page() -> ScriptedDriver {
    ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" if params["functionDeclaration"] == FOCUS_NOW_JS => {
            Ok(json!({ "result": { "value": "button \"Up\"" } }))
        }
        _ => Ok(json!({})),
    })
}

fn key_events(d: &ScriptedDriver) -> Vec<(String, String, String, i64, i64)> {
    d.calls_to("Input.dispatchKeyEvent")
        .iter()
        .map(|e| {
            (
                e["type"].as_str().unwrap().to_string(),
                e["key"].as_str().unwrap().to_string(),
                e["code"].as_str().unwrap().to_string(),
                e["windowsVirtualKeyCode"].as_i64().unwrap(),
                e["modifiers"].as_i64().unwrap(),
            )
        })
        .collect()
}

fn ev(kind: &str, key: &str, code: &str, vk: i64, mask: i64) -> (String, String, String, i64, i64) {
    (kind.to_string(), key.to_string(), code.to_string(), vk, mask)
}

#[tokio::test]
async fn ctrl_arrow_up_holds_ctrl_for_the_key_with_the_bitmask_on_every_event() {
    let mut d = key_page();
    let out = execute_with(&mut d, &action(json!({ "kind": "press_key", "key": "Ctrl+ArrowUp" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(
        key_events(&d),
        [
            ev("rawKeyDown", "Control", "ControlLeft", 17, 2),
            ev("rawKeyDown", "ArrowUp", "ArrowUp", 38, 2),
            ev("keyUp", "ArrowUp", "ArrowUp", 38, 2),
            ev("keyUp", "Control", "ControlLeft", 17, 0),
        ]
    );
    assert_eq!(out.detail, "pressed Ctrl+ArrowUp; the focus is on button \"Up\"");
}

/// All four, written in any order: down Ctrl, Alt, Shift, Meta, each adding
/// its bit; the key with all of them; up in reverse, each taking its own
/// bit away.
#[tokio::test]
async fn every_modifier_goes_down_in_order_and_up_in_reverse() {
    let mut d = key_page();
    let out = execute_with(&mut d, &action(json!({ "kind": "press_key", "key": "meta+shift+alt+ctrl+End" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(
        key_events(&d),
        [
            ev("rawKeyDown", "Control", "ControlLeft", 17, 2),
            ev("rawKeyDown", "Alt", "AltLeft", 18, 3),
            ev("rawKeyDown", "Shift", "ShiftLeft", 16, 11),
            ev("rawKeyDown", "Meta", "MetaLeft", 91, 15),
            ev("rawKeyDown", "End", "End", 35, 15),
            ev("keyUp", "End", "End", 35, 15),
            ev("keyUp", "Meta", "MetaLeft", 91, 11),
            ev("keyUp", "Shift", "ShiftLeft", 16, 3),
            ev("keyUp", "Alt", "AltLeft", 18, 2),
            ev("keyUp", "Control", "ControlLeft", 17, 0),
        ]
    );
    assert!(out.detail.starts_with("pressed Ctrl+Alt+Shift+Meta+End;"), "{}", out.detail);
}

#[tokio::test]
async fn a_key_that_types_keeps_its_text_under_a_modifier() {
    let mut d = key_page();
    assert!(execute_with(&mut d, &action(json!({ "kind": "press_key", "key": "Ctrl+Enter" })), &quick()).await.ok);
    let sent = d.calls_to("Input.dispatchKeyEvent");
    assert_eq!(sent[1]["type"], "keyDown");
    assert_eq!(sent[1]["text"], "\r");
    assert_eq!(sent[1]["modifiers"], 2);
}

#[tokio::test]
async fn times_presses_the_whole_combination_again() {
    let mut d = key_page();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "press_key", "key": "Ctrl+ArrowUp", "times": 3 })),
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    let sent = key_events(&d);
    assert_eq!(sent.len(), 12);
    for press in sent.chunks(4) {
        assert_eq!(press, &sent[..4], "each press is the same four events");
    }
    assert_eq!(out.detail, "pressed Ctrl+ArrowUp 3 times; the focus is on button \"Up\"");

    // One time is said as a single press.
    let mut d = key_page();
    let out = execute_with(&mut d, &action(json!({ "kind": "press_key", "key": "Tab", "times": 1 })), &quick()).await;
    assert_eq!(key_events(&d).len(), 2);
    assert_eq!(out.detail, "pressed Tab; the focus is on button \"Up\"");
}

/// A key pressed alone sends what it always sent: down and up, no bits.
#[tokio::test]
async fn a_single_key_is_pressed_exactly_as_before() {
    let mut d = key_page();
    assert!(execute_with(&mut d, &action(json!({ "kind": "press_key", "key": "ArrowUp" })), &quick()).await.ok);
    assert_eq!(
        key_events(&d),
        [ev("rawKeyDown", "ArrowUp", "ArrowUp", 38, 0), ev("keyUp", "ArrowUp", "ArrowUp", 38, 0)]
    );
}

// ---- drag: a fake page --------------------------------------------------------

/// The window `RECT_JS` reports.
const VW: f64 = 1000.0;
const VH: f64 = 800.0;

/// Where each element is: its css, and its box in the top window.
type Boxes = Vec<(&'static str, [f64; 4])>;

/// A page where each css finds one element, `el:<css>` (none for a css in
/// `missing`), ready to use, at its place in `boxes`. `iframe#f` is a
/// frame whose document is `frame-doc`. The animation frame answers at
/// once, or as `frame` says.
struct DragPage {
    boxes: Boxes,
    missing: Vec<&'static str>,
    /// What the animation-frame wait answers, by its 1-based count.
    frame: fn(usize) -> Result<Value, CdpError>,
    /// The `Input.*` call, by method and 1-based count, that the browser
    /// refuses.
    refuse: Option<(&'static str, usize, CdpError)>,
}

impl Default for DragPage {
    fn default() -> Self {
        DragPage {
            boxes: vec![("#a", [100.0, 100.0, 200.0, 40.0]), ("#b", [100.0, 300.0, 200.0, 40.0])],
            missing: vec![],
            frame: |_| Ok(json!({ "result": { "value": true } })),
            refuse: None,
        }
    }
}

impl DragPage {
    fn driver(self) -> ScriptedDriver {
        let mut frames = 0usize;
        let mut counts: std::collections::HashMap<String, usize> = Default::default();
        ScriptedDriver::new(move |method, params| {
            let f = params["functionDeclaration"].as_str().unwrap_or("");
            let on = params["objectId"].as_str().unwrap_or("");
            let n = {
                let c = counts.entry(method.to_string()).or_default();
                *c += 1;
                *c
            };
            if let Some((m, at, e)) = &self.refuse {
                if *m == method && *at == n {
                    return Err(e.clone());
                }
            }
            Ok(match method {
                "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
                "Runtime.evaluate" if params["expression"] == NEXT_FRAME_JS => {
                    frames += 1;
                    return (self.frame)(frames);
                }
                "Runtime.callFunctionOn" if f == PROBE_JS => json!({ "result": { "value": ready_probe() } }),
                "Runtime.callFunctionOn" if f == RECT_JS => {
                    let css = on.strip_prefix("el:").unwrap_or("");
                    let [left, top, width, height] =
                        self.boxes.iter().find(|(c, _)| *c == css).map(|(_, b)| *b).unwrap_or([0.0; 4]);
                    json!({ "result": { "value": {
                        "left": left, "top": top, "width": width, "height": height, "vw": VW, "vh": VH
                    } } })
                }
                "Runtime.callFunctionOn" if f == CSS_JS => {
                    let css = params["arguments"][0]["value"].as_str().unwrap_or("");
                    json!({ "result": { "objectId": format!("arr:{on}:{css}") } })
                }
                "Runtime.callFunctionOn" if f == FRAME_JS => {
                    json!({ "result": { "value": if on == "el:iframe#f" { "frame" } else { "element" } } })
                }
                "Runtime.callFunctionOn" if f == FRAME_DOC_JS => json!({ "result": { "objectId": "arr:framedoc" } }),
                "Runtime.callFunctionOn" => json!({ "result": { "value": true } }),
                "Runtime.getProperties" => {
                    let arr = params["objectId"].as_str().unwrap_or("");
                    if arr == "arr:framedoc" {
                        json!({ "result": [{ "name": "0", "value": { "objectId": "framedoc" } }] })
                    } else {
                        let css = arr.rsplit(':').next().unwrap_or("");
                        if self.missing.contains(&css) {
                            json!({ "result": [] })
                        } else {
                            json!({ "result": [{ "name": "0", "value": { "objectId": format!("el:{css}") } }] })
                        }
                    }
                }
                "DOM.describeNode" => json!({ "node": { "backendNodeId": 7 } }),
                "DOM.resolveNode" => json!({ "object": { "objectId": "frame-doc" } }),
                _ => json!({}),
            })
        })
    }
}

fn drag_action(position: Option<&str>) -> Action {
    let mut v = json!({ "kind": "drag", "from": { "css": "#a" }, "to": { "css": "#b" } });
    if let Some(p) = position {
        v["position"] = json!(p);
    }
    action(v)
}

/// The mouse events sent, as (type, x, y, buttons).
fn mouse_events(d: &ScriptedDriver) -> Vec<(String, f64, f64, i64)> {
    d.calls_to("Input.dispatchMouseEvent")
        .iter()
        .map(|e| {
            (
                e["type"].as_str().unwrap().to_string(),
                e["x"].as_f64().unwrap(),
                e["y"].as_f64().unwrap(),
                e["buttons"].as_i64().unwrap(),
            )
        })
        .collect()
}

/// Every `Input.*` call, in order, as a short word: what the gesture did.
fn input_story(d: &ScriptedDriver) -> Vec<String> {
    d.calls
        .iter()
        .filter_map(|(m, p)| match m.as_str() {
            "Input.setInterceptDrags" => Some(format!("intercept {}", p["enabled"])),
            "Input.dispatchMouseEvent" => Some(p["type"].as_str().unwrap().to_string()),
            "Input.dispatchDragEvent" => Some(p["type"].as_str().unwrap().to_string()),
            "Runtime.evaluate" if p["expression"] == NEXT_FRAME_JS => Some("frame".to_string()),
            _ => None,
        })
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ---- drag: the mouse path -------------------------------------------------------

#[tokio::test]
async fn the_mouse_path_presses_nudges_moves_in_ten_frames_and_releases_at_each_drop_point() {
    // #a's centre is (200, 120); #b spans y 300 to 340.
    for (position, drop_y) in [(Some("before"), 310.0), (Some("after"), 330.0), (Some("onto"), 320.0), (None, 320.0)] {
        let mut d = DragPage::default().driver();
        let out = execute_with(&mut d, &drag_action(position), &quick()).await;
        assert!(out.ok, "{position:?}: {}", out.detail);
        let word = position.unwrap_or("onto");
        assert_eq!(out.detail, format!("dragged #a {word} #b"));

        let moves = mouse_events(&d);
        assert_eq!(moves.len(), 14, "{moves:?}");
        assert_eq!(moves[0], ("mouseMoved".to_string(), 200.0, 120.0, 0));
        assert_eq!(moves[1], ("mousePressed".to_string(), 200.0, 120.0, 1));
        // 10 px toward the drop point (straight down here).
        assert_eq!(moves[2], ("mouseMoved".to_string(), 200.0, 130.0, 1));
        for (i, m) in moves[3..13].iter().enumerate() {
            let f = (i + 1) as f64 / 10.0;
            assert_eq!(m.0, "mouseMoved");
            assert!(close(m.1, 200.0) && close(m.2, 130.0 + (drop_y - 130.0) * f), "step {}: {m:?}", i + 1);
            assert_eq!(m.3, 1, "the button stays down");
        }
        assert_eq!(moves[13], ("mouseReleased".to_string(), 200.0, drop_y, 0));

        let mut expected = vec!["intercept true", "mouseMoved", "mousePressed", "mouseMoved"];
        for _ in 0..10 {
            expected.extend(["frame", "mouseMoved"]);
        }
        expected.extend(["frame", "mouseReleased", "intercept false"]);
        assert_eq!(input_story(&d), expected);
        assert!(d.deadline_was_cleared());
    }
}

/// Drag toward a target above: the nudge goes up, toward it.
#[tokio::test]
async fn the_nudge_goes_toward_the_drop_point() {
    let page = DragPage {
        boxes: vec![("#a", [100.0, 300.0, 200.0, 40.0]), ("#b", [100.0, 100.0, 200.0, 40.0])],
        ..DragPage::default()
    };
    let mut d = page.driver();
    assert!(execute_with(&mut d, &drag_action(Some("before")), &quick()).await.ok);
    let moves = mouse_events(&d);
    assert_eq!(moves[1].2, 320.0);
    assert_eq!(moves[2].2, 310.0);
    assert_eq!(moves.last().unwrap().2, 110.0);
}

// ---- drag: the browser's own drag and drop -------------------------------------

fn intercepted(d: &mut ScriptedDriver) {
    d.on_call_events.push((
        "Input.dispatchMouseEvent".to_string(),
        Event {
            method: "Input.dragIntercepted".to_string(),
            params: json!({ "data": { "items": [{ "mimeType": "text/plain", "data": "row-a" }], "dragOperationsMask": 1 } }),
        },
    ));
}

#[tokio::test]
async fn a_drag_the_browser_hands_over_is_finished_at_the_drop_point() {
    let mut d = DragPage::default().driver();
    intercepted(&mut d);
    let out = execute_with(&mut d, &drag_action(Some("after")), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "dragged #a after #b");
    assert_eq!(
        input_story(&d),
        [
            "intercept true",
            "mouseMoved",
            "mousePressed",
            "mouseMoved",
            "dragEnter",
            "dragOver",
            "drop",
            "mouseReleased",
            "intercept false"
        ]
    );
    for e in d.calls_to("Input.dispatchDragEvent") {
        assert_eq!((e["x"].as_f64().unwrap(), e["y"].as_f64().unwrap()), (200.0, 330.0));
        assert_eq!(e["data"]["items"][0]["data"], "row-a", "the intercepted data is handed back");
    }
    assert_eq!(mouse_events(&d).last().unwrap(), &("mouseReleased".to_string(), 200.0, 330.0, 0));
}

#[tokio::test]
async fn interception_is_turned_off_when_the_drop_fails() {
    let page = DragPage {
        refuse: Some((
            "Input.dispatchDragEvent",
            3,
            CdpError::Protocol { method: "Input.dispatchDragEvent".into(), message: "no drag in progress".into() },
        )),
        ..DragPage::default()
    };
    let mut d = page.driver();
    intercepted(&mut d);
    let out = execute_with(&mut d, &drag_action(None), &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, "the page refused: no drag in progress");
    // The button is let go, then interception is switched off, last.
    let story = input_story(&d);
    assert_eq!(&story[story.len() - 3..], ["drop", "mouseReleased", "intercept false"]);
    assert!(d.deadline_was_cleared());
}

#[tokio::test]
async fn interception_is_turned_off_when_the_mouse_path_fails() {
    let page = DragPage {
        refuse: Some((
            "Input.dispatchMouseEvent",
            6,
            CdpError::Protocol { method: "Input.dispatchMouseEvent".into(), message: "target closed".into() },
        )),
        ..DragPage::default()
    };
    let mut d = page.driver();
    let out = execute_with(&mut d, &drag_action(None), &quick()).await;
    assert_eq!(out.detail, "the page refused: target closed");
    let story = input_story(&d);
    assert_eq!(&story[story.len() - 2..], ["mouseReleased", "intercept false"]);
}

#[tokio::test]
async fn interception_is_turned_off_even_when_turning_it_on_failed() {
    let page = DragPage {
        refuse: Some((
            "Input.setInterceptDrags",
            1,
            CdpError::Protocol { method: "Input.setInterceptDrags".into(), message: "not supported".into() },
        )),
        ..DragPage::default()
    };
    let mut d = page.driver();
    let out = execute_with(&mut d, &drag_action(None), &quick()).await;
    assert_eq!(out.detail, "the page refused: not supported");
    assert_eq!(input_story(&d), ["intercept true", "intercept false"]);
}

#[tokio::test]
async fn a_browser_that_will_not_stop_intercepting_is_said() {
    // It logs a warning: the log tail is the process's.
    let _tail = crate::serial::log_tail();
    let page = DragPage {
        refuse: Some(("Input.setInterceptDrags", 2, CdpError::Transport("gone".into()))),
        ..DragPage::default()
    };
    let mut d = page.driver();
    let out = execute_with(&mut d, &drag_action(None), &quick()).await;
    assert!(out.ok);
    assert_eq!(out.detail, format!("dragged #a onto #b{DRAGS_STILL_HELD}"));
}

// ---- drag: each failure sentence --------------------------------------------------

fn sentence_of(out: &ActionOutcome) -> &str {
    assert!(!out.ok, "expected a failure: {}", out.detail);
    &out.detail
}

#[tokio::test]
async fn nothing_to_drag_when_from_is_not_there() {
    let mut d = DragPage { missing: vec!["#a"], ..DragPage::default() }.driver();
    let out = execute_with(&mut d, &drag_action(None), &quick()).await;
    let s = sentence_of(&out);
    assert!(s.starts_with(&format!("{}: ", nothing_to_drag("#a"))), "{s}");
    assert!(s.starts_with("there was nothing to drag at #a"));
    assert!(s.ends_with("not found"), "{s}");
    assert_eq!(classify(s, None), ErrorClass::NotFound);
    assert!(d.calls_to("Input.setInterceptDrags").is_empty(), "nothing was pressed");
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty());
}

#[tokio::test]
async fn nowhere_to_drop_when_to_is_not_there() {
    let mut d = DragPage { missing: vec!["#b"], ..DragPage::default() }.driver();
    let out = execute_with(&mut d, &drag_action(None), &quick()).await;
    let s = sentence_of(&out);
    assert!(s.starts_with(&format!("{}: ", nowhere_to_drop("#b"))), "{s}");
    assert!(s.starts_with("there was nowhere to drop at #b"));
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty());
}

#[tokio::test]
async fn nowhere_to_drop_when_the_drop_point_is_off_the_page() {
    let page = DragPage {
        boxes: vec![("#a", [100.0, 100.0, 200.0, 40.0]), ("#b", [100.0, 900.0, 200.0, 40.0])],
        ..DragPage::default()
    };
    let mut d = page.driver();
    let out = execute_with(&mut d, &drag_action(None), &quick()).await;
    assert_eq!(sentence_of(&out), "there was nowhere to drop at #b: is outside the visible part of the page");
}

#[tokio::test]
async fn a_drag_that_runs_out_of_time_says_so_and_lets_go() {
    // The browser stops answering partway: the drag did not finish.
    let page = DragPage {
        frame: |n| {
            if n >= 4 {
                Err(CdpError::Timeout { what: "Runtime.evaluate".into(), ms: 250 })
            } else {
                Ok(json!({ "result": { "value": true } }))
            }
        },
        ..DragPage::default()
    };
    let mut d = page.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "drag", "from": { "css": "#a" }, "to": { "css": "#b" }, "within_ms": 2000 })),
        &quick(),
    )
    .await;
    assert_eq!(sentence_of(&out), "the drag did not finish within 2 seconds");
    assert_eq!(classify(&out.detail, None), ErrorClass::TimedOut);
    let story = input_story(&d);
    assert_eq!(&story[story.len() - 2..], ["mouseReleased", "intercept false"]);
    assert!(d.deadline_was_cleared());

    // A page slower than the whole budget: the deadline ends it.
    let page = DragPage {
        frame: |_| {
            std::thread::sleep(Duration::from_millis(30));
            Ok(json!({ "result": { "value": true } }))
        },
        ..DragPage::default()
    };
    let mut d = page.driver();
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "drag", "from": { "css": "#a" }, "to": { "css": "#b" }, "within_ms": 100 })),
        &quick(),
    )
    .await;
    assert_eq!(sentence_of(&out), "the drag did not finish within 0.1 seconds");
    assert_eq!(d.calls_to("Input.setInterceptDrags").last().unwrap()["enabled"], false);
    assert_eq!(did_not_finish(10_000), "the drag did not finish within 10 seconds");
    assert_eq!(did_not_finish(2500), "the drag did not finish within 2.5 seconds");
}

// ---- drag: where it acts --------------------------------------------------------

#[tokio::test]
async fn a_drag_acts_in_the_current_tab() {
    let mut d = DragPage::default().driver();
    d.tabs.open.push("second".to_string());
    d.tabs.current = "second".to_string();
    let out = execute_with(&mut d, &drag_action(Some("before")), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    let there = d.tabs.called_in("second");
    assert_eq!(there.iter().filter(|m| *m == "Input.dispatchMouseEvent").count(), 14);
    assert_eq!(there.iter().filter(|m| *m == "Input.setInterceptDrags").count(), 2);
    assert!(d.tabs.called_in(MAIN_TAB).is_empty(), "nothing went to main");
}

/// Both ends inside a same-origin frame: each is searched for in the
/// frame's own document, and the points come from `RECT_JS`, which shifts
/// the box out through every enclosing frame as the click probe does.
#[tokio::test]
async fn a_drag_inside_a_frame_finds_both_ends_in_the_frame() {
    let page = DragPage {
        boxes: vec![("#a", [150.0, 180.0, 100.0, 20.0]), ("#b", [150.0, 240.0, 100.0, 20.0])],
        ..DragPage::default()
    };
    let mut d = page.driver();
    let out = execute_with(
        &mut d,
        &action(json!({
            "kind": "drag",
            "from": [{ "css": "iframe#f" }, { "css": "#a" }],
            "to": [{ "css": "iframe#f" }, { "css": "#b" }],
            "position": "after",
        })),
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "dragged #a in iframe#f after #b in iframe#f");
    // Every search for #a and #b ran in the frame's document.
    for (_, p) in d.calls.iter().filter(|(m, p)| m == "Runtime.callFunctionOn" && p["functionDeclaration"] == CSS_JS) {
        let css = p["arguments"][0]["value"].as_str().unwrap();
        let expected = if css == "iframe#f" { "doc" } else { "frame-doc" };
        assert_eq!(p["objectId"], expected, "{css}");
    }
    let moves = mouse_events(&d);
    assert_eq!(moves[1], ("mousePressed".to_string(), 200.0, 190.0, 1));
    assert_eq!(moves.last().unwrap(), &("mouseReleased".to_string(), 200.0, 255.0, 0));
    assert!(RECT_JS.contains("frameElement") && RECT_JS.contains("clientLeft") && RECT_JS.contains("paddingTop"));
}

// ---- drag: what a script may say ----------------------------------------------

#[test]
fn a_drag_is_refused_where_it_is_written() {
    let refused = |v: Value| action(v).validate().unwrap_err();
    assert_eq!(
        refused(json!({ "kind": "drag", "from": "#a", "to": "#b", "within_ms": 0 })),
        WITHIN_MS_ZERO
    );
    assert_eq!(
        refused(json!({ "kind": "drag", "from": "#a", "to": "#b", "within_ms": 60001 })),
        "drag waits at most 60000 ms, not 60001"
    );
    assert!(refused(json!({ "kind": "drag", "from": { "css": " " }, "to": "#b" })).starts_with("drag from: "));
    assert!(refused(json!({ "kind": "drag", "from": "#a", "to": { "nth": 1 } })).starts_with("drag to: "));
    assert!(serde_json::from_value::<Action>(json!({ "kind": "drag", "from": "#a", "to": "#b", "position": "beside" })).is_err());
    assert!(action(json!({ "kind": "drag", "from": "#a", "to": "#b", "within_ms": 60000 })).validate().is_ok());
    let a = drag_action(None);
    assert!(!a.is_check(), "a drag checks nothing by itself");
}

#[test]
fn reports_and_patterns_have_words_for_both() {
    assert_eq!(action_words(&drag_action(Some("before"))), "drag #a before #b");
    assert_eq!(action_words(&drag_action(None)), "drag #a onto #b");
    assert_eq!(action_words(&action(json!({ "kind": "press_key", "key": "Ctrl+ArrowUp", "times": 2 }))), "press Ctrl+ArrowUp 2 times");
    assert_eq!(action_words(&action(json!({ "kind": "press_key", "key": "Ctrl+ArrowUp" }))), "press Ctrl+ArrowUp");
    assert_eq!(action_target(&drag_action(None)).as_deref(), Some("#a"));
    assert_eq!(action_target(&action(json!({ "kind": "press_key", "key": "Ctrl+ArrowUp" }))).as_deref(), Some("the Ctrl+ArrowUp key"));
    assert_eq!(
        v2_lib::ai_bridge::describe_try(&drag_action(Some("after")), true),
        "AI tried drag #a after #b in the supervised browser: ok"
    );
}
