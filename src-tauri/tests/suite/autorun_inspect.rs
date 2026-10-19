//! `inspect_autorun_element`: one element's computed styles, event
//! listeners, animations and structure, read without touching the page
//! (`browser::inspect`, the `/autorun-inspect` route).
//!
//! The browser is a scripted fake. Every locator function hands back an
//! element list whose size a test chooses; each of the inspect's own page
//! functions answers what a real page would, and the listeners and the
//! script addresses come from the protocol calls a real browser answers.
//! Every call it was asked is recorded, so a test sees that nothing which
//! changes the page (a click, a key, a focus, a scroll) was ever sent.

use crate::common::ScriptedDriver;
use serde_json::{json, Value};
use v2_lib::ai_bridge::inspect_page;
use v2_lib::browser::inspect::{
    capture_above, ANCESTORS_JS, ANIMATIONS_JS, ATTRIBUTE_HANDLERS_JS, COVER_AT_JS, HANDLER_NOT_SHOWN, NODE_NAME_JS,
    STRUCTURE_JS, STYLES_JS, TEXT_MAX,
};
use v2_lib::browser::page::GROUP;
use v2_lib::browser::input::PROBE_JS;
use v2_lib::browser::locator::VISIBLE_JS;

/// The sign-in's token, as a page could print it in an element's text.
const TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";

/// What the element's click point looks like to a click: reached.
fn free_point() -> Value {
    json!({ "visible": true, "onscreen": true, "enabled": true, "editable": false, "hit": true, "nospot": false,
            "x": 40.0, "y": 60.0, "covered_by": "", "rect": [0.0, 0.0, 80.0, 24.0], "stable": false })
}

/// Its click point taken by a backdrop over it.
fn covered_point() -> Value {
    json!({ "visible": true, "onscreen": true, "enabled": true, "editable": false, "hit": false, "nospot": false,
            "x": 40.0, "y": 60.0, "covered_by": "div#overlay.backdrop", "rect": [0.0, 0.0, 80.0, 24.0], "stable": false })
}

/// A page as the inspect sees it.
struct Page {
    /// How many elements every locator matches.
    found: usize,
    point: Value,
    styles: Value,
    animations: Value,
    structure: Value,
    /// The element's own accessible name, and its cover's.
    name: String,
    cover_name: String,
}

impl Default for Page {
    fn default() -> Self {
        Page {
            found: 1,
            point: free_point(),
            styles: json!({ "color": "rgb(220, 38, 38)", "border-color": "rgb(220, 38, 38)", "opacity": "1" }),
            animations: json!({
                "running": [
                    { "kind": "CSSTransition", "name": "border-color", "duration": 200, "delay": 0, "iterations": 1, "play_state": "running" },
                    { "kind": "CSSAnimation", "name": "shake", "duration": 400, "delay": 50, "iterations": "infinite", "play_state": "paused" },
                ],
                "computed": { "transition-property": "border-color", "transition-duration": "0.2s", "animation-name": "shake" },
            }),
            structure: json!({
                "tag": "input",
                "attributes": [["type", "password"], ["name", "pwd"], ["value", "hunter2-Secret"], ["data-session-id", "abc"], ["class", "field invalid"], ["onblur", "checkPwd(this)"]],
                "text": "",
                "children": {},
                "frames": [],
                "path": "/hr/login",
            }),
            name: "Save".into(),
            cover_name: "Confirm delete".into(),
        }
    }
}

/// The page as a scripted browser: `el-N` are the matched elements, `cover`
/// the element at the click point.
fn driver(p: Page) -> ScriptedDriver {
    ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let on = params["objectId"].as_str().unwrap_or("");
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => json!({ "result": { "value": p.point.clone() } }),
            "Runtime.callFunctionOn" if f == STYLES_JS => {
                // The properties asked for, when any were; else every one.
                let asked: Vec<String> = params["arguments"][0]["value"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect();
                let all = p.styles.as_object().unwrap().clone();
                let chosen: serde_json::Map<String, Value> = if asked.is_empty() {
                    all
                } else {
                    asked.iter().map(|k| (k.clone(), all.get(k).cloned().unwrap_or(json!("")))).collect()
                };
                json!({ "result": { "value": chosen } })
            }
            "Runtime.callFunctionOn" if f == ANIMATIONS_JS => json!({ "result": { "value": p.animations.clone() } }),
            "Runtime.callFunctionOn" if f == STRUCTURE_JS => json!({ "result": { "value": p.structure.clone() } }),
            "Runtime.callFunctionOn" if f == COVER_AT_JS => json!({ "result": { "objectId": "cover-arr" } }),
            // Above the element: a form, the app's root, the document and the window.
            "Runtime.callFunctionOn" if f == ANCESTORS_JS => json!({ "result": { "objectId": "anc-arr" } }),
            "Runtime.callFunctionOn" if f == NODE_NAME_JS => {
                let name = match on { "anc-form" => "form#leave", "anc-root" => "div#root", "anc-doc" => "document", _ => "window" };
                json!({ "result": { "value": name } })
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS => json!({ "result": { "value": true } }),
            // The element carries an onchange attribute, and nothing else.
            "Runtime.callFunctionOn" if f == ATTRIBUTE_HANDLERS_JS => {
                let asked = params["arguments"][0]["value"].as_array().cloned().unwrap_or_default();
                json!({ "result": { "value": asked.into_iter().filter(|t| t == "change").collect::<Vec<_>>() } })
            }
            // A role locator: the accessibility tree's nodes, then each one's element.
            "Accessibility.queryAXTree" => json!({ "nodes": (0..p.found)
                .map(|i| json!({ "nodeId": i.to_string(), "role": { "value": "button" }, "name": { "value": "Save" }, "backendDOMNodeId": 10 + i }))
                .collect::<Vec<_>>() }),
            "DOM.resolveNode" => {
                let i = params["backendNodeId"].as_u64().unwrap_or(10) - 10;
                json!({ "object": { "objectId": format!("el-{i}") } })
            }
            // Any locator function: an array of elements.
            "Runtime.callFunctionOn" => json!({ "result": { "objectId": "arr" } }),
            "Runtime.getProperties" if on == "anc-arr" => json!({ "result": (["anc-form", "anc-root", "anc-doc", "win"])
                .iter()
                .enumerate()
                .map(|(i, id)| json!({ "name": i.to_string(), "value": { "objectId": id } }))
                .collect::<Vec<_>>() }),
            "Runtime.getProperties" if on == "cover-arr" => {
                json!({ "result": [{ "name": "0", "value": { "objectId": "cover" } }] })
            }
            "Runtime.getProperties" => json!({ "result": (0..p.found)
                .map(|i| json!({ "name": i.to_string(), "value": { "objectId": format!("el-{i}") } }))
                .collect::<Vec<_>>() }),
            "DOM.describeNode" => json!({ "node": { "backendNodeId": if on == "cover" { 20 } else { 10 } } }),
            "Accessibility.getPartialAXTree" if on == "cover" => json!({ "nodes": [
                { "nodeId": "1", "role": { "value": "dialog" }, "name": { "value": p.cover_name.clone() }, "ignored": false }
            ] }),
            "Accessibility.getPartialAXTree" => json!({ "nodes": [
                { "nodeId": "1", "role": { "value": "button" }, "name": { "value": p.name.clone() }, "ignored": false }
            ] }),
            // The form submits on bubbling; the root takes clicks in capture
            // and bubbling (and keys, not counted); the document only keys;
            // the window a pointerdown in capture.
            "DOMDebugger.getEventListeners" if on == "anc-form" => json!({ "listeners": [
                { "type": "submit", "useCapture": false, "passive": false, "once": false, "scriptId": "12", "lineNumber": 1, "columnNumber": 1 },
                { "type": "input", "useCapture": false, "passive": false, "once": false, "scriptId": "12", "lineNumber": 1, "columnNumber": 1 },
            ] }),
            "DOMDebugger.getEventListeners" if on == "anc-root" => json!({ "listeners": [
                { "type": "click", "useCapture": true, "passive": false, "once": false, "scriptId": "12", "lineNumber": 1, "columnNumber": 1,
                  "handler": { "type": "function", "description": "function stopAll(e) { e.stopPropagation() }", "objectId": "h-2" } },
                { "type": "click", "useCapture": true, "passive": false, "once": false, "scriptId": "12", "lineNumber": 2, "columnNumber": 1 },
                { "type": "click", "useCapture": false, "passive": false, "once": false, "scriptId": "12", "lineNumber": 3, "columnNumber": 1 },
                { "type": "keydown", "useCapture": true, "passive": false, "once": false, "scriptId": "12", "lineNumber": 4, "columnNumber": 1 },
            ] }),
            "DOMDebugger.getEventListeners" if on == "anc-doc" => json!({ "listeners": [
                { "type": "keydown", "useCapture": false, "passive": false, "once": false, "scriptId": "12", "lineNumber": 5, "columnNumber": 1 },
            ] }),
            "DOMDebugger.getEventListeners" if on == "win" => json!({ "listeners": [
                { "type": "pointerdown", "useCapture": true, "passive": true, "once": false, "scriptId": "12", "lineNumber": 6, "columnNumber": 1 },
            ] }),
            "DOMDebugger.getEventListeners" if on == "cover" => json!({ "listeners": [
                { "type": "click", "useCapture": true, "passive": false, "once": false, "scriptId": "31", "lineNumber": 0, "columnNumber": 4120 }
            ] }),
            "DOMDebugger.getEventListeners" => json!({ "listeners": [
                { "type": "click", "useCapture": false, "passive": false, "once": true, "scriptId": "12", "lineNumber": 41, "columnNumber": 9 },
                { "type": "touchstart", "useCapture": false, "passive": true, "once": false, "scriptId": "99", "lineNumber": 2, "columnNumber": 0 },
                { "type": "input", "useCapture": false, "passive": false, "once": false, "scriptId": "40", "lineNumber": 0, "columnNumber": 0 },
                { "type": "change", "useCapture": false, "passive": false, "once": false, "scriptId": "77", "lineNumber": 12, "columnNumber": 30,
                  "handler": { "type": "function", "description": "function onchange(event) {\nsecretWork()\n}", "objectId": "h-1" } },
            ] }),
            "Profiler.getBestEffortCoverage" => json!({ "result": [
                { "scriptId": "12", "url": "https://hr.example/js/app.js?v=3&session=abc", "functions": [] },
                { "scriptId": "31", "url": "https://cdn.vendor.example/lib/modal.min.js#x", "functions": [] },
                { "scriptId": "40", "url": "", "functions": [] },
            ] }),
            _ => json!({}),
        })
    })
}

async fn inspect(d: &mut ScriptedDriver, args: Value) -> (u16, Value) {
    let (status, text) = inspect_page(d, &args.to_string()).await;
    let v = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (status, v)
}

/// Nothing that changes the page was sent: no input, no focus, no scroll,
/// no debugger, and the click point was measured where it stands.
fn read_only(d: &ScriptedDriver) {
    for (method, params) in &d.calls {
        assert!(!method.starts_with("Input."), "{method}");
        assert!(!method.starts_with("Debugger."), "{method}");
        assert!(!matches!(method.as_str(), "DOM.focus" | "DOM.scrollIntoViewIfNeeded" | "DOM.setAttributeValue"), "{method}");
        if params["functionDeclaration"] == PROBE_JS {
            assert_eq!(params["arguments"], json!([{ "value": false }, { "value": true }]), "measured without settling or scrolling");
        }
        // The handler objects Chrome hands back go in the app's group, so
        // the inspect's release lets them go.
        if method == "DOMDebugger.getEventListeners" {
            assert_eq!(params["objectGroup"], GROUP, "{params}");
        }
    }
}

#[tokio::test]
async fn every_part_is_shown_by_default() {
    let mut d = driver(Page::default());
    let (status, v) = inspect(&mut d, json!({ "locator": { "role": "button", "name": "Save" } })).await;
    assert_eq!(status, 200, "{v}");
    for part in ["styles", "listeners", "animations", "structure"] {
        assert!(v.get(part).is_some(), "{part} in {v}");
    }
    assert_eq!(v["element"], "button \"Save\"");
    assert_eq!(v["named"], "button \"Save\"");
    read_only(&d);
}

#[tokio::test]
async fn styles_are_every_computed_value_or_only_those_asked() {
    let mut d = driver(Page::default());
    let (status, v) = inspect(&mut d, json!({ "locator": "#email", "show": ["styles"] })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["styles"]["color"], "rgb(220, 38, 38)");
    assert_eq!(v["styles"]["opacity"], "1");
    assert!(v.get("listeners").is_none() && v.get("structure").is_none() && v.get("animations").is_none(), "{v}");
    assert!(!d.calls.iter().any(|(m, _)| m == "DOMDebugger.getEventListeners"), "listeners were not asked");

    let mut d = driver(Page::default());
    let (status, v) = inspect(&mut d, json!({ "locator": "#email", "show": ["styles"], "properties": ["border-color"] })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["styles"], json!({ "border-color": "rgb(220, 38, 38)" }));
    let sent = d.calls.iter().find(|(_, p)| p["functionDeclaration"] == STYLES_JS).expect("styles read").1.clone();
    assert_eq!(sent["arguments"][0]["value"], json!(["border-color"]), "the names travel as an argument, never as source");
    read_only(&d);
}

#[tokio::test]
async fn listeners_say_type_flags_and_the_script_path_never_its_source_host_or_query() {
    let mut d = driver(Page::default());
    let (status, v) = inspect(&mut d, json!({ "locator": "#save", "show": ["listeners"] })).await;
    assert_eq!(status, 200, "{v}");
    let own = v["listeners"]["element"].as_array().expect("the element's listeners");
    assert_eq!(own.len(), 4, "{v}");
    assert_eq!(
        own[0],
        json!({ "type": "click", "use_capture": false, "once": true, "passive": false, "script": "/js/app.js", "line": 42, "column": 10 })
    );
    assert_eq!(own[1]["passive"], true);
    assert_eq!(own[1]["script"], "(address not known)", "a script the coverage did not list");
    assert_eq!(own[2]["script"], "(no address: an inline handler or a script the page built)");
    assert_eq!(own[3]["script"], "(its onchange attribute)", "a markup handler V8 has not run yet");
    let text = v.to_string();
    for leak in ["hr.example", "v=3", "session=abc", "function", "secretWork"] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }
    // The element's click point is free: no cover, so no cover listeners.
    assert!(v["listeners"].get("cover").is_none(), "{v}");
    assert!(v["covered_by"].is_null(), "{v}");
    read_only(&d);
}

#[tokio::test]
async fn the_click_listeners_above_the_element_are_counted_and_a_capture_one_is_called_out() {
    let mut d = driver(Page::default());
    let (status, v) = inspect(&mut d, json!({ "locator": "#save", "show": ["listeners"] })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(
        v["listeners"]["ancestors"],
        json!([
            { "name": "form#leave", "listeners": [{ "type": "submit", "capture": false, "count": 1 }] },
            { "name": "div#root", "listeners": [
                { "type": "click", "capture": false, "count": 1 },
                { "type": "click", "capture": true, "count": 2 },
            ] },
            { "name": "window", "listeners": [{ "type": "pointerdown", "capture": true, "count": 1 }] },
        ]),
        "only ancestors with a click kind of listener, nearest first; the document has none"
    );
    let names = vec!["div#root".to_string(), "window".to_string()];
    assert_eq!(v["listeners"]["ancestors_note"], capture_above(&names));
    assert!(capture_above(&names).contains("usual reason a click does nothing"));
    assert!(!v.to_string().contains("stopPropagation()"), "a handler's source leaked: {v}");
    read_only(&d);
}

#[tokio::test]
async fn with_no_capture_listener_above_nothing_is_called_out() {
    let mut d = ScriptedDriver::new(|method, params| {
        let on = params["objectId"].as_str().unwrap_or("");
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Runtime.evaluate" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => json!({ "result": { "value": free_point() } }),
            "Runtime.callFunctionOn" if f == ANCESTORS_JS => json!({ "result": { "objectId": "anc-arr" } }),
            "Runtime.callFunctionOn" if f == NODE_NAME_JS => json!({ "result": { "value": "ul.menu" } }),
            "Runtime.callFunctionOn" => json!({ "result": { "objectId": "arr" } }),
            "Runtime.getProperties" if on == "anc-arr" => json!({ "result": [{ "name": "0", "value": { "objectId": "anc-ul" } }] }),
            "Runtime.getProperties" => json!({ "result": [{ "name": "0", "value": { "objectId": "el-0" } }] }),
            "DOMDebugger.getEventListeners" if on == "anc-ul" => json!({ "listeners": [
                { "type": "mouseover", "useCapture": false, "passive": false, "once": false },
            ] }),
            _ => json!({}),
        })
    });
    let (status, v) = inspect(&mut d, json!({ "locator": "#item", "show": ["listeners"] })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["listeners"]["ancestors"], json!([{ "name": "ul.menu", "listeners": [{ "type": "mouseover", "capture": false, "count": 1 }] }]));
    assert!(v["listeners"].get("ancestors_note").is_none(), "{v}");
}

#[tokio::test]
async fn a_token_in_an_accessible_name_is_hidden() {
    let mut d = driver(Page {
        point: covered_point(),
        name: format!("Save {TOKEN}"),
        cover_name: format!("Session {TOKEN}"),
        ..Page::default()
    });
    let (status, v) = inspect(&mut d, json!({ "locator": "#save", "show": ["listeners"] })).await;
    assert_eq!(status, 200, "{v}");
    assert!(!v.to_string().contains(TOKEN), "{v}");
    assert!(v["named"].as_str().unwrap().starts_with("button \"Save "), "{v}");
    assert!(v["covered_by"].as_str().unwrap().starts_with("dialog \"Session "), "{v}");
}

/// The route tells the window when a dead browser was let go (and with it
/// a discovery), as the probe's does: after the let-go.
#[test]
fn the_route_publishes_the_discovery_after_letting_a_dead_browser_go() {
    let source = include_str!("../../src/ai_bridge.rs");
    let route = &source[source.find("async fn autorun_inspect(").unwrap()..];
    let route = &route[..route.find("\n}").unwrap()];
    let let_go = route.find("let_go_if_silent(&mut slot, answer)").expect("a dead browser is let go");
    let publish = route.find("publish_discovery(&slot)").expect("the window is not told");
    assert!(let_go < publish, "published before the let-go");
}

#[tokio::test]
async fn a_covered_element_names_its_cover_and_lists_its_listeners() {
    let mut d = driver(Page { point: covered_point(), ..Page::default() });
    let (status, v) = inspect(&mut d, json!({ "locator": "#save", "show": ["listeners"] })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["covered_by"], "dialog \"Confirm delete\" (div#overlay.backdrop)");
    let cover = &v["listeners"]["cover"];
    assert_eq!(cover["name"], "dialog \"Confirm delete\" (div#overlay.backdrop)");
    assert_eq!(
        cover["listeners"],
        json!([{ "type": "click", "use_capture": true, "once": false, "passive": false, "script": "/lib/modal.min.js", "line": 1, "column": 4121 }])
    );
    assert!(!v.to_string().contains("cdn.vendor.example"), "{v}");
    // The cover was found at the click point the probe measured.
    let at = d.calls.iter().find(|(_, p)| p["functionDeclaration"] == COVER_AT_JS).expect("cover looked for").1.clone();
    assert_eq!(at["arguments"], json!([{ "value": 40.0 }, { "value": 60.0 }]));
    read_only(&d);
}

#[tokio::test]
async fn the_cover_is_named_even_when_listeners_are_not_asked() {
    let mut d = driver(Page { point: covered_point(), ..Page::default() });
    let (status, v) = inspect(&mut d, json!({ "locator": "#save", "show": ["structure"] })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["covered_by"], "dialog \"Confirm delete\" (div#overlay.backdrop)");
    assert!(v.get("listeners").is_none(), "{v}");
}

#[tokio::test]
async fn an_element_off_screen_is_not_scrolled_to_and_its_point_is_not_checked() {
    let point = json!({ "visible": true, "onscreen": false, "enabled": true, "hit": false, "nospot": false,
                        "x": 40.0, "y": 2400.0, "covered_by": "", "rect": [0.0, 2380.0, 80.0, 24.0], "stable": false });
    let mut d = driver(Page { point, ..Page::default() });
    let (status, v) = inspect(&mut d, json!({ "locator": "#save", "show": ["listeners"] })).await;
    assert_eq!(status, 200, "{v}");
    assert!(v["covered_by"].is_null(), "{v}");
    assert!(v["click_point"].as_str().unwrap().contains("never scrolls"), "{v}");
    assert!(!d.calls.iter().any(|(_, p)| p["functionDeclaration"] == COVER_AT_JS));
    read_only(&d);
}

#[tokio::test]
async fn animations_are_the_running_ones_and_the_computed_transition_and_animation_values() {
    let mut d = driver(Page::default());
    let (status, v) = inspect(&mut d, json!({ "locator": "#email", "show": ["animations"] })).await;
    assert_eq!(status, 200, "{v}");
    let running = v["animations"]["running"].as_array().unwrap();
    assert_eq!(running.len(), 2);
    assert_eq!(running[0]["name"], "border-color");
    assert_eq!(running[0]["duration"], 200);
    assert_eq!(running[1]["iterations"], "infinite");
    assert_eq!(running[1]["play_state"], "paused");
    assert_eq!(running[1]["delay"], 50);
    assert_eq!(v["animations"]["computed"]["transition-duration"], "0.2s");
    read_only(&d);
}

#[tokio::test]
async fn structure_hides_a_password_value_and_secret_named_attributes() {
    let mut d = driver(Page::default());
    let (status, v) = inspect(&mut d, json!({ "locator": "#pwd", "show": ["structure"] })).await;
    assert_eq!(status, 200, "{v}");
    let s = &v["structure"];
    assert_eq!(s["tag"], "input");
    assert_eq!(s["attributes"]["value"], "(hidden)");
    assert_eq!(s["attributes"]["data-session-id"], "(hidden)");
    assert_eq!(s["attributes"]["type"], "password");
    assert_eq!(s["attributes"]["class"], "field invalid");
    assert_eq!(s["attributes"]["onblur"], HANDLER_NOT_SHOWN, "a handler's source is never shown");
    assert!(!v.to_string().contains("checkPwd"), "{v}");
    assert_eq!(s["frame"], "the page itself");
    assert!(!v.to_string().contains("hunter2"), "{v}");
    read_only(&d);
}

#[tokio::test]
async fn a_value_is_hidden_when_the_field_is_named_like_a_secret() {
    let structure = json!({
        "tag": "input",
        "attributes": [["type", "hidden"], ["name", "__RequestVerificationToken"], ["value", "CfDJ8-aftok"]],
        "text": "", "children": {}, "frames": [], "path": "/hr",
    });
    let mut d = driver(Page { structure, ..Page::default() });
    let (_, v) = inspect(&mut d, json!({ "locator": "input[name=x]", "show": ["structure"] })).await;
    assert_eq!(v["structure"]["attributes"]["value"], "(hidden)", "{v}");
    assert!(!v.to_string().contains("CfDJ8-aftok"), "{v}");
}

#[tokio::test]
async fn structure_text_is_cut_and_redacted_children_counted_and_the_frame_named() {
    let long = format!("Welcome {TOKEN} {}", "word ".repeat(300));
    let structure = json!({
        "tag": "div",
        "attributes": [["id", "panel"], ["data-note", format!("t={TOKEN}")]],
        "text": long,
        "children": { "span": 3, "button": 1 },
        "frames": ["iframe#emp-search"],
        "path": "/hr/search",
    });
    let mut d = driver(Page { structure, ..Page::default() });
    let (status, v) = inspect(&mut d, json!({ "locator": [{ "css": "#emp-search" }, { "css": "#panel" }], "show": ["structure"] })).await;
    assert_eq!(status, 200, "{v}");
    let s = &v["structure"];
    let text = s["text"].as_str().unwrap();
    assert!(text.starts_with("Welcome "), "{text}");
    assert!(text.chars().count() <= TEXT_MAX + 3, "{}", text.chars().count());
    assert!(!v.to_string().contains(TOKEN), "{v}");
    assert_eq!(s["children"], json!({ "span": 3, "button": 1 }));
    assert_eq!(s["frame"], "inside iframe#emp-search, showing /hr/search");
}

#[tokio::test]
async fn a_locator_that_matches_nothing_or_many_is_refused_as_a_try_refuses_it() {
    let mut d = driver(Page { found: 0, ..Page::default() });
    let (status, v) = inspect(&mut d, json!({ "locator": { "role": "button", "name": "Save" } })).await;
    assert_eq!(status, 404);
    assert_eq!(v, json!("button \"Save\" not found"));

    let mut d = driver(Page { found: 2, ..Page::default() });
    let (status, v) = inspect(&mut d, json!({ "locator": { "role": "button", "name": "Save" } })).await;
    assert_eq!(status, 409);
    assert_eq!(v, json!("button \"Save\" matched 2 elements - narrow it, or add nth"));
    assert!(!d.calls.iter().any(|(_, p)| p["functionDeclaration"] == PROBE_JS), "nothing more is read");
}

#[tokio::test]
async fn a_bad_ask_is_refused_before_the_browser_is_asked() {
    for (args, says) in [
        (json!({}), "locator"),
        (json!({ "locator": "#x", "show": ["colours"] }), "styles, listeners, animations, structure"),
        (json!({ "locator": "#x", "show": [] }), "styles, listeners, animations, structure"),
        (json!({ "locator": "#x", "properties": ["color", 3] }), "properties"),
        (json!({ "locator": { "input": "field" } }), "placeholder"),
    ] {
        let mut d = driver(Page::default());
        let (status, v) = inspect(&mut d, args.clone()).await;
        assert_eq!(status, 400, "{args}: {v}");
        assert!(v.as_str().unwrap().contains(says), "{args}: {v}");
        assert!(d.calls.is_empty(), "{args}: the browser was asked");
    }
}
