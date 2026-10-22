//! `expect_no_change`: a click on a control that is not usable, and what the
//! page is then allowed to do (nothing). The words, the validation, and the
//! forced click against a scripted page; the real page is `browser_live`.

use crate::common::ScriptedDriver;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::browser::actions::{execute_with, Action};
use v2_lib::browser::expect::DISABLED_JS;
use v2_lib::browser::input::{DISABLED, PROBE_JS};
use v2_lib::browser::cdp::Event;
use v2_lib::browser::net_record::{NetEntry, NetState};
use v2_lib::browser::no_change::{
    changes, new_texts, writes_sent, Seen, COVERED_JS, DIALOGS_JS, MESSAGES_JS, NOTHING_EXPECTED, STATE_JS, TIME,
};
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn seen() -> Seen {
    Seen {
        address: "https://app.example/cycles?token=abc#top".into(),
        dialogs: 0,
        browser_dialogs: 0,
        messages: vec![],
        text: "Save".into(),
        states: json!([[0, "aria-pressed", "false"]]),
    }
}

#[test]
fn changes_say_each_thing_that_happened() {
    let before = seen();
    let after = Seen {
        address: "https://app.example/cycles/new".into(),
        dialogs: 1,
        browser_dialogs: 1,
        messages: vec!["Saved".into()],
        text: "Saved".into(),
        states: json!([[0, "aria-pressed", "true"]]),
    };
    assert_eq!(
        changes(&before, &after, "the element #cards"),
        vec![
            "the address went from /cycles to /cycles/new",
            "a dialog opened",
            "a browser dialog opened",
            "a message appeared: \"Saved\"",
            "the text of the element #cards changed",
            "a state in the element #cards changed",
        ]
    );
    // Several dialogs, a dialog that went away, and a region that is gone.
    let two = Seen { dialogs: 2, ..seen() };
    assert_eq!(changes(&seen(), &two, "r"), vec!["2 dialogs opened"]);
    assert_eq!(changes(&two, &seen(), "r"), vec!["2 dialogs closed"]);
    let gone = Seen { text: String::new(), states: Value::Null, ..seen() };
    assert_eq!(changes(&seen(), &gone, "the element #cards"), vec!["the element #cards is no longer on the page"]);
}

#[test]
fn nothing_changed_says_nothing() {
    assert!(changes(&seen(), &seen(), "the element #cards").is_empty());
}

#[test]
fn the_address_is_said_as_its_path_only() {
    let after = Seen { address: "https://other.example:8443/cycles/next?token=xyz#frag".into(), ..seen() };
    let said = changes(&seen(), &after, "r");
    assert_eq!(said, vec!["the address went from /cycles to /cycles/next"]);
    for part in ["token", "app.example", "other.example", "8443", "frag", "abc"] {
        assert!(!said[0].contains(part), "{part} in: {}", said[0]);
    }
}

#[test]
fn expect_no_change_is_validated() {
    let action = |v: Value| serde_json::from_value::<Action>(v).unwrap();
    assert!(action(json!({ "kind": "expect_no_change", "selector": "#a" })).validate().is_ok());
    assert!(action(json!({ "kind": "expect_no_change", "selector": "#a", "region": "#b", "within_ms": 200 })).validate().is_ok());
    assert!(action(json!({ "kind": "expect_no_change", "selector": "#a", "within_ms": 10000 })).validate().is_ok());
    for ms in [0, 50, 199, 10_001, 60_000] {
        let a = action(json!({ "kind": "expect_no_change", "selector": "#a", "within_ms": ms }));
        assert_eq!(a.validate(), Err(TIME.to_string()), "within_ms {ms}");
    }
    let bad_region = action(json!({ "kind": "expect_no_change", "selector": "#a", "region": "" }));
    assert_eq!(bad_region.validate(), Err("expect_no_change region: a selector is empty".to_string()));
    let bad_selector = action(json!({ "kind": "expect_no_change", "selector": "" }));
    assert_eq!(bad_selector.validate(), Err("a selector is empty".to_string()));
    // Both its locators are targets, so the save check and the rewrites see them.
    let both = action(json!({ "kind": "expect_no_change", "selector": "#a", "region": "#b" }));
    assert_eq!(both.targets().len(), 2);
    assert!(both.is_check());
}

/// What the scripted page is now, and what a click on it does to it.
struct World {
    href: String,
    dialogs: u64,
    messages: Vec<String>,
    text: String,
    states: Value,
    present: bool,
    /// The text the page shows once the pointer is over it (a hint).
    hover_text: Option<String>,
    /// How many element lookups have been answered, and how many of those
    /// after the first are answered with nothing found (a late region).
    lookups: usize,
    late: usize,
}

type OnClick = Box<dyn Fn(&mut World) + Send>;

/// A page where every locator finds one element (until the click removes
/// it, when `present` goes false), the probe answers `probe`, and a click's
/// release runs `on_click` on the page's state.
fn page(probe: Value, on_click: OnClick) -> (ScriptedDriver, Arc<Mutex<World>>) {
    let world = Arc::new(Mutex::new(World {
        href: "https://app.example/cycles?token=abc".into(),
        dialogs: 0,
        messages: vec![],
        text: "Save".into(),
        states: json!([]),
        present: true,
        hover_text: None,
        lookups: 0,
        late: 0,
    }));
    let w = world.clone();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let expression = params["expression"].as_str().unwrap_or("");
        let mut world = w.lock().unwrap();
        Ok(match method {
            "Runtime.evaluate" if expression == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if expression.contains("[role=alert]") => json!({ "result": { "value": world.messages } }),
            "Runtime.evaluate" if expression.contains("checkVisibility") => json!({ "result": { "value": world.dialogs } }),
            "Runtime.evaluate" => json!({ "result": { "value": world.href } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => json!({ "result": { "value": probe } }),
            "Runtime.callFunctionOn" if f == STATE_JS => {
                json!({ "result": { "value": { "text": world.text, "states": world.states } } })
            }
            "Runtime.callFunctionOn" if f == v2_lib::browser::actions::HIGHLIGHT_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" if f == DISABLED_JS => json!({ "result": { "value": "its disabled property is set" } }),
            "Runtime.callFunctionOn" => json!({ "result": { "objectId": "arr" } }),
            "Runtime.getProperties" => {
                world.lookups += 1;
                let late = world.lookups > 1 && world.lookups <= 1 + world.late;
                json!({ "result": if world.present && !late {
                    vec![json!({ "name": "0", "value": { "objectId": "el-0" } })]
                } else {
                    vec![]
                } })
            }
            "Input.dispatchMouseEvent" => {
                if params["type"] == "mouseMoved" {
                    if let Some(t) = world.hover_text.take() {
                        world.text = t;
                    }
                }
                if params["type"] == "mouseReleased" {
                    on_click(&mut world);
                }
                json!({})
            }
            _ => json!({}),
        })
    });
    (d, world)
}

fn disabled_probe() -> Value {
    json!({
        "visible": true, "onscreen": true, "enabled": false, "editable": false, "hit": true, "around": false,
        "x": 40.0, "y": 12.0, "covered_by": "", "rect": [0.0, 0.0, 80.0, 24.0], "stable": true
    })
}

fn no_change(v: Value) -> Action {
    serde_json::from_value(v).unwrap()
}

fn mouse_events(d: &ScriptedDriver) -> usize {
    d.calls_to("Input.dispatchMouseEvent").len()
}

#[tokio::test]
async fn a_forced_click_reaches_a_disabled_element_a_plain_click_refuses() {
    let (mut d, _) = page(disabled_probe(), Box::new(|_| {}));
    let plain = execute_with(&mut d, &serde_json::from_value(json!({ "kind": "click", "selector": "#save" })).unwrap(), &quick()).await;
    assert!(!plain.ok && plain.detail.contains(DISABLED), "{}", plain.detail);
    assert_eq!(mouse_events(&d), 0, "a plain click on a disabled element sent a mouse event");

    let clock = Arc::new(AtomicU64::new(0));
    d.idle_clock = Some(clock.clone());
    let out = execute_with(
        &mut d,
        &no_change(json!({ "kind": "expect_no_change", "selector": "#save", "within_ms": 2000 })),
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "clicked #save anyway and nothing happened in 2 s");
    assert_eq!(mouse_events(&d), 4, "the pointer's arrival, then moved, pressed and released");
    assert_eq!(clock.load(Ordering::SeqCst), 2000, "it waited the whole time after the click");
    assert!(d.deadline_was_cleared());
}

#[tokio::test]
async fn a_forced_click_says_the_default_wait() {
    let (mut d, _) = page(disabled_probe(), Box::new(|_| {}));
    let clock = Arc::new(AtomicU64::new(0));
    d.idle_clock = Some(clock.clone());
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(out.detail.ends_with("nothing happened in 1 s"), "{}", out.detail);
    assert_eq!(clock.load(Ordering::SeqCst), 1000);
}

#[tokio::test]
async fn a_forced_click_never_clicks_an_overlay() {
    let covered = json!({
        "visible": true, "onscreen": true, "enabled": false, "editable": false, "hit": false, "around": false,
        "x": 40.0, "y": 12.0, "covered_by": "div.modal", "rect": [0.0, 0.0, 80.0, 24.0], "stable": true
    });
    let (mut d, _) = page(covered, Box::new(|_| {}));
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("is covered by div.modal"), "{}", out.detail);
    assert_eq!(mouse_events(&d), 0, "an overlay was clicked");
    assert!(d.deadline_was_cleared());
}

#[tokio::test]
async fn a_hit_on_an_element_around_it_holds_a_plain_click_only() {
    // The point lands on the container (the card lets the click through).
    let around = json!({
        "visible": true, "onscreen": true, "enabled": false, "editable": false, "hit": false, "around": true,
        "x": 40.0, "y": 12.0, "covered_by": "div.holder", "rect": [0.0, 0.0, 80.0, 24.0], "stable": true
    });
    let (mut d, _) = page(around, Box::new(|_| {}));
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#card" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(mouse_events(&d), 4);
}

#[tokio::test]
async fn the_forced_wait_still_wants_the_element_still_and_in_view() {
    for over in [json!({ "visible": false }), json!({ "onscreen": false }), json!({ "stable": false })] {
        let mut probe = disabled_probe();
        for (k, v) in over.as_object().unwrap() {
            probe[k] = v.clone();
        }
        let (mut d, _) = page(probe, Box::new(|_| {}));
        let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
        assert!(!out.ok, "{over} was clicked: {}", out.detail);
        assert_eq!(mouse_events(&d), 0, "{over}");
    }
}

#[tokio::test]
async fn a_dialog_that_opens_is_a_change() {
    let (mut d, _) = page(disabled_probe(), Box::new(|w| w.dialogs += 1));
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, format!("{NOTHING_EXPECTED}a dialog opened"));
}

#[tokio::test]
async fn the_address_the_text_and_a_state_are_each_a_change() {
    let (mut d, _) = page(
        disabled_probe(),
        Box::new(|w| {
            w.href = "https://app.example/cycles/next?token=abc".into();
            w.text = "Saved".into();
            w.states = json!([[0, "aria-expanded", "true"]]);
        }),
    );
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    let action = no_change(json!({ "kind": "expect_no_change", "selector": "#save", "region": "#cards" }));
    let out = execute_with(&mut d, &action, &quick()).await;
    assert!(!out.ok);
    assert_eq!(
        out.detail,
        format!(
            "{NOTHING_EXPECTED}the address went from /cycles to /cycles/next, the text of #cards changed, a state in #cards changed"
        )
    );
    assert!(!out.detail.contains("token") && !out.detail.contains("app.example"), "{}", out.detail);
}

#[tokio::test]
async fn a_browser_dialog_is_a_change_and_is_answered() {
    let (mut d, _) = page(disabled_probe(), Box::new(|_| {}));
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    // Opened by the click itself: the release, not the pointer's arrival.
    d.dialog_when = Some((
        Box::new(|method, params| method == "Input.dispatchMouseEvent" && params["type"] == "mouseReleased"),
        "alert".into(),
        "Saved!".into(),
    ));
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(!out.ok);
    // The runner's own note on a dialog nobody expected follows the sentence.
    assert!(out.detail.starts_with(&format!("{NOTHING_EXPECTED}a browser dialog opened")), "{}", out.detail);
    assert!(!d.calls_to("Page.handleJavaScriptDialog").is_empty(), "the dialog was left open");
}

#[tokio::test]
async fn a_dialog_seen_before_the_click_is_not_this_clicks() {
    let (mut d, _) = page(disabled_probe(), Box::new(|_| {}));
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    d.book.opened("alert", "an earlier step's");
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
}

#[tokio::test]
async fn a_region_that_goes_away_is_a_change() {
    let (mut d, _) = page(disabled_probe(), Box::new(|w| w.present = false));
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, format!("{NOTHING_EXPECTED}#save is no longer on the page"));
}

#[tokio::test]
async fn a_failed_forced_click_changes_the_class_of_the_failure() {
    use v2_lib::autorun::patterns::{classify, ErrorClass};
    let sentence = format!("{NOTHING_EXPECTED}a dialog opened, the text of button \"Save\" changed");
    assert_eq!(classify(&sentence, Some("button \"Save\"")), ErrorClass::ChangedOnClick);
    assert_eq!(classify(&sentence, None), ErrorClass::ChangedOnClick);
    assert_eq!(ErrorClass::ChangedOnClick.key(), "changed_on_click");
}

#[test]
fn the_helpers_are_functions_the_export_can_wrap() {
    assert!(STATE_JS.starts_with("function() {"));
    assert!(DIALOGS_JS.starts_with("function() {"));
    for banned in ["className", "classList", "getComputedStyle", ".style"] {
        assert!(!STATE_JS.contains(banned), "the region's state reads {banned}");
    }
}

// ---------------------------------------------------------------- what the click sends and the page says

fn ev(method: &str, params: Value) -> Event {
    Event { method: method.to_string(), params }
}

fn request(id: &str, method: &str, url: &str) -> Event {
    ev("Network.requestWillBeSent", json!({ "requestId": id, "request": { "url": url, "method": method } }))
}

/// A page that keeps a network record, and sends `events` when the mouse is
/// released over the control (the click) - or, with `at`, at the mouse event
/// of that type.
fn recording(probe: Value, on_click: OnClick, at: &'static str, events: Vec<Event>) -> (ScriptedDriver, Arc<Mutex<World>>) {
    let (d, world) = page(probe, on_click);
    let mut d = d.with_net_record();
    d.events_when = Some((
        Box::new(move |method, params| method == "Input.dispatchMouseEvent" && params["type"] == at),
        events,
    ));
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    (d, world)
}

fn net_entry(method: &str, path_query: &str) -> NetEntry {
    NetEntry {
        seq: 0,
        id: "r0".into(),
        method: method.into(),
        path_query: path_query.into(),
        status: Some(200),
        mime: None,
        state: NetState::Finished,
        redirect: None,
        kind: Some("XHR".into()),
        same_site: true,
        polling: false,
    }
}

#[test]
fn only_a_write_is_a_request_the_click_sent() {
    let sent = vec![
        net_entry("GET", "/hr/Cycle/List?x=1"),
        net_entry("post", "/hr/Cycle/Save?token=abc"),
        net_entry("POST", "/hr/Cycle/Save?token=def"),
        net_entry("DELETE", "/hr/Cycle/7"),
        net_entry("OPTIONS", "/hr/Cycle/7"),
        net_entry("PUT", "/a"),
        net_entry("PATCH", "/b"),
    ];
    // Each once, the path only, at most three.
    assert_eq!(
        writes_sent(&sent),
        vec!["the click sent POST /hr/Cycle/Save", "the click sent DELETE /hr/Cycle/7", "the click sent PUT /a"]
    );
    assert!(writes_sent(&sent[..1]).is_empty());
    assert!(writes_sent(&[]).is_empty());
}

#[test]
fn a_text_is_new_when_the_page_was_not_already_showing_it() {
    let s = |v: &[&str]| v.iter().map(|t| t.to_string()).collect::<Vec<_>>();
    assert_eq!(new_texts(&s(&["Draft"]), &s(&["Draft", "Saved"])), s(&["Saved"]));
    assert!(new_texts(&s(&["Draft"]), &s(&["Draft"])).is_empty());
    // Counted: a second "Saved" after one is new.
    assert_eq!(new_texts(&s(&["Saved"]), &s(&["Saved", "Saved"])), s(&["Saved"]));
    assert!(new_texts(&s(&["Saved", "Draft"]), &s(&["Draft"])).is_empty());
}

#[tokio::test]
async fn a_post_the_click_sends_is_a_change_even_when_the_control_looks_the_same() {
    let (mut d, _) = recording(
        disabled_probe(),
        Box::new(|_| {}),
        "mouseReleased",
        vec![request("w1", "POST", "https://app.example/hr/Cycle/Save?token=abc")],
    );
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(!out.ok, "{}", out.detail);
    assert_eq!(out.detail, format!("{NOTHING_EXPECTED}the click sent POST /hr/Cycle/Save"));
    assert!(!out.detail.contains("token") && !out.detail.contains("app.example"), "{}", out.detail);
}

#[tokio::test]
async fn a_get_or_a_write_before_the_click_is_not_the_clicks() {
    // A GET after the click.
    let (mut d, _) = recording(
        disabled_probe(),
        Box::new(|_| {}),
        "mouseReleased",
        vec![request("g1", "GET", "https://app.example/hr/Cycle/List")],
    );
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
    // A POST that the pointer's arrival (the hover) started: before the click.
    let (mut d, _) = recording(
        disabled_probe(),
        Box::new(|_| {}),
        "mouseMoved",
        vec![request("p1", "POST", "https://app.example/hr/Track")],
    );
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
}

#[tokio::test]
async fn a_message_the_click_brings_is_a_change_one_already_showing_is_not() {
    let (mut d, world) = page(disabled_probe(), Box::new(|w| w.messages.push("Cycle saved".into())));
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    world.lock().unwrap().messages.push("Draft autosaved".into());
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, format!("{NOTHING_EXPECTED}a message appeared: \"Cycle saved\""));
    // Showing before and after: nothing.
    let (mut d, world) = page(disabled_probe(), Box::new(|_| {}));
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    world.lock().unwrap().messages.push("Draft autosaved".into());
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
}

#[tokio::test]
async fn what_only_the_pointer_does_is_read_before_the_click() {
    let (mut d, world) = page(disabled_probe(), Box::new(|_| {}));
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    // A hint that shows while the pointer is over the control.
    world.lock().unwrap().hover_text = Some("Save (fill the required fields first)".into());
    let out = execute_with(&mut d, &no_change(json!({ "kind": "expect_no_change", "selector": "#save" })), &quick()).await;
    assert!(out.ok, "the hover's own change was taken for the click's: {}", out.detail);
    // The pointer arrives before the page is read, and before the press.
    let calls: Vec<(String, Value)> = d.calls.clone();
    let first_move = calls.iter().position(|(m, p)| m == "Input.dispatchMouseEvent" && p["type"] == "mouseMoved").unwrap();
    let first_read = calls
        .iter()
        .position(|(m, p)| m == "Runtime.callFunctionOn" && p["functionDeclaration"].as_str() == Some(STATE_JS))
        .unwrap();
    let press = calls.iter().position(|(m, p)| m == "Input.dispatchMouseEvent" && p["type"] == "mousePressed").unwrap();
    assert!(first_move < first_read && first_read < press, "{first_move} {first_read} {press}");
}

#[tokio::test]
async fn a_region_drawn_just_after_the_control_is_waited_for() {
    let (mut d, world) = page(disabled_probe(), Box::new(|_| {}));
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    world.lock().unwrap().late = 3;
    let action = no_change(json!({ "kind": "expect_no_change", "selector": "#save", "region": "#cards" }));
    let out = execute_with(&mut d, &action, &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert!(d.deadline_was_cleared());
    // One that never comes is "not found", after the action's time.
    let (mut d, world) = page(disabled_probe(), Box::new(|_| {}));
    d.idle_clock = Some(Arc::new(AtomicU64::new(0)));
    world.lock().unwrap().late = 100_000;
    let out = execute_with(&mut d, &action, &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("#cards") && out.detail.contains("not found"), "{}", out.detail);
    assert_eq!(mouse_events(&d), 1, "only the pointer's arrival: the click was never made");
}

#[test]
fn the_message_and_cover_helpers_are_functions_the_export_can_wrap() {
    assert!(MESSAGES_JS.starts_with("function() {"));
    assert!(COVERED_JS.starts_with("function() {"));
    for needed in ["[role=alert]", "[role=status]", "toast"] {
        assert!(MESSAGES_JS.contains(needed), "{needed}");
    }
}
