//! Recording a module's menu path: clicks reported by the page become the
//! locators a run clicks with, and a recording ends with a path or with
//! the reason nothing was saved.

mod common;

use common::ScriptedDriver;
use serde_json::json;
use std::sync::atomic::AtomicBool;
use v2_lib::autorun::nav::{check_path, ModulePath};
use v2_lib::autorun::recorder::{
    arm, ax_chain, capture, finish, locate, locator_from_ax, locator_from_hints, next_click, AxLink, Captured,
    ClickHints, ClickPayload, Ended, BINDING, BROWSER_CLOSED, CANCELLED, LISTENER_JS, NO_CLICKS, UNREADABLE,
};
use v2_lib::browser::cdp::{CdpError, Event};
use v2_lib::browser::locator::{LocatorStep, Target};

fn exact_role(role: &str, name: &str) -> Target {
    Target::One(LocatorStep { role: Some(role.into()), name: Some(name.into()), exact: true, ..LocatorStep::default() })
}

fn exact_text(text: &str) -> Target {
    Target::One(LocatorStep { text: Some(text.into()), exact: true, ..LocatorStep::default() })
}

fn node(role: &str, name: &str) -> AxLink {
    AxLink { role: role.into(), name: name.into(), ignored: false }
}

fn hints(role: &str, label: &str, text: &str) -> ClickHints {
    ClickHints { tag: "a".into(), role: role.into(), label: label.into(), text: text.into() }
}

/// A page's report of one click, the way the binding delivers it.
fn clicked(i: u32, text: &str) -> Event {
    Event {
        method: "Runtime.bindingCalled".into(),
        params: json!({
            "name": BINDING,
            "payload": json!({ "doc": "d1", "i": i, "tag": "a", "role": "", "label": "", "text": text }).to_string()
        }),
    }
}

/// An accessibility tree around the clicked element (backend 42): the
/// words inside a link "Leave", inside the menu.
fn leave_tree() -> serde_json::Value {
    json!({ "nodes": [
        { "nodeId": "1", "role": { "value": "RootWebArea" }, "name": { "value": "HR" }, "childIds": ["2"] },
        { "nodeId": "2", "role": { "value": "navigation" }, "name": { "value": "" }, "parentId": "1", "childIds": ["3"] },
        { "nodeId": "3", "role": { "value": "link" }, "name": { "value": " Leave " }, "parentId": "2", "childIds": ["4"], "backendDOMNodeId": 41 },
        { "nodeId": "4", "role": { "value": "StaticText" }, "name": { "value": "Leave" }, "parentId": "3", "backendDOMNodeId": 42 }
    ] })
}

/// A page whose held elements are still there (`held[i]`) or gone.
fn page_with(held: Vec<bool>, href: &'static str) -> ScriptedDriver {
    let mut last_i = 0usize;
    ScriptedDriver::new(move |method, params| {
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => json!({ "result": { "value": href } }),
            "Runtime.callFunctionOn" => {
                last_i = params["arguments"][1]["value"].as_u64().unwrap_or(0) as usize;
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                if held.get(last_i).copied().unwrap_or(false) {
                    json!({ "result": [{ "name": "0", "value": { "objectId": "el" } }] })
                } else {
                    json!({ "result": [] })
                }
            }
            "DOM.describeNode" => json!({ "node": { "backendNodeId": 42 } }),
            "Accessibility.getPartialAXTree" => leave_tree(),
            _ => json!({}),
        })
    })
}

#[test]
fn the_clicked_elements_own_role_and_name_make_the_locator() {
    assert_eq!(locator_from_ax(&[node("link", "Apply   Leave"), node("navigation", "")]), Some(exact_role("link", "Apply Leave")));
}

#[test]
fn the_nearest_ancestor_with_a_preferred_role_wins_over_the_words_inside_it() {
    let chain = [node("StaticText", "Apply Leave"), node("generic", ""), node("link", "Apply Leave"), node("listitem", ""), node("navigation", "")];
    assert_eq!(locator_from_ax(&chain), Some(exact_role("link", "Apply Leave")));
    let chain = [node("generic", ""), node("menuitem", "Leave"), node("menu", "")];
    assert_eq!(locator_from_ax(&chain), Some(exact_role("menuitem", "Leave")));
    let ignored = [AxLink { role: "button".into(), name: "Hidden".into(), ignored: true }, node("tab", "Leave")];
    assert_eq!(locator_from_ax(&ignored), Some(exact_role("tab", "Leave")));
}

#[test]
fn a_named_role_that_is_not_preferred_is_used_when_nothing_better_is_near() {
    assert_eq!(locator_from_ax(&[node("StaticText", "Leave"), node("heading", "Leave"), node("main", "")]), Some(exact_role("heading", "Leave")));
}

#[test]
fn the_climb_stops_at_a_container_and_finds_nothing_beyond_it() {
    assert_eq!(locator_from_ax(&[node("generic", ""), node("navigation", "Main"), node("link", "Home")]), None);
    assert_eq!(locator_from_ax(&[]), None);
}

#[test]
fn hints_give_a_role_from_aria_label_or_else_the_visible_words() {
    assert_eq!(locator_from_hints(&hints("menuitem", " Leave ", "L")), Some(exact_role("menuitem", "Leave")));
    assert_eq!(locator_from_hints(&hints("", "", "  Apply \n Leave ")), Some(exact_text("Apply Leave")));
    assert_eq!(locator_from_hints(&hints("", "", "")), None);
    assert_eq!(locator_from_hints(&hints("", "", &"x".repeat(81))), None, "a whole panel's text names nothing");
}

#[test]
fn wrappers_the_accessibility_tree_ignores_do_not_use_up_the_climb() {
    let mut chain: Vec<AxLink> = (0..6).map(|_| AxLink { role: "none".into(), name: String::new(), ignored: true }).collect();
    chain.push(node("link", "Leave"));
    assert_eq!(locator_from_ax(&chain), Some(exact_role("link", "Leave")));
}

#[test]
fn a_role_attribute_is_read_the_way_the_browser_reads_it() {
    assert_eq!(locator_from_hints(&hints("menuitem button", "Leave", "")), Some(exact_role("menuitem", "Leave")), "the first token");
    assert_eq!(locator_from_hints(&hints("none", "Leave", "Leave")), Some(exact_text("Leave")), "no role at all");
}

#[test]
fn ax_chain_walks_up_by_parent_id_or_else_by_child_ids() {
    let chain = ax_chain(&leave_tree(), 42);
    assert_eq!(chain.iter().map(|n| n.role.as_str()).collect::<Vec<_>>(), vec!["StaticText", "link", "navigation", "RootWebArea"]);
    let no_parent_ids = json!({ "nodes": [
        { "nodeId": "a", "role": { "value": "link" }, "name": { "value": "Leave" }, "childIds": ["b"] },
        { "nodeId": "b", "role": { "value": "StaticText" }, "name": { "value": "Leave" }, "backendDOMNodeId": 7 }
    ] });
    assert_eq!(ax_chain(&no_parent_ids, 7).iter().map(|n| n.role.as_str()).collect::<Vec<_>>(), vec!["StaticText", "link"]);
    assert!(ax_chain(&no_parent_ids, 99).is_empty());
}

#[tokio::test]
async fn arming_adds_the_binding_and_the_listener_to_every_document() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "result": {} })));
    arm(&mut d).await.unwrap();
    assert_eq!(d.methods(), vec!["Runtime.enable", "Runtime.addBinding", "Page.addScriptToEvaluateOnNewDocument", "Runtime.evaluate"]);
    assert_eq!(d.calls_to("Runtime.addBinding")[0]["name"], BINDING);
    assert_eq!(d.calls_to("Page.addScriptToEvaluateOnNewDocument")[0]["source"], LISTENER_JS);
    assert!(LISTENER_JS.contains("addEventListener('click'") && LISTENER_JS.contains(", true)"), "capture phase");
}

/// The listener sits on pages where people type passwords: it reports
/// locator words only, and never changes what a click does.
#[test]
fn the_listener_only_listens_and_never_reads_what_was_typed() {
    for never in ["preventDefault", "stopPropagation", "stopImmediatePropagation", ".value"] {
        assert!(!LISTENER_JS.contains(never), "the listener must not use {never}");
    }
    assert!(LISTENER_JS.contains("isContentEditable"), "text typed into an editable area is not sent");
}

#[tokio::test]
async fn a_reported_click_is_read_back_and_another_binding_is_ignored() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    d.events.push_back(Event { method: "Runtime.bindingCalled".into(), params: json!({ "name": "other", "payload": "{}" }) });
    d.events.push_back(clicked(3, "Leave"));
    assert_eq!(next_click(&mut d, std::time::Duration::from_millis(10)).await.unwrap(), None);
    let got = next_click(&mut d, std::time::Duration::from_millis(10)).await.unwrap().unwrap();
    assert_eq!(got, ClickPayload { doc: "d1".into(), i: 3, hints: hints("", "", "Leave") });
    assert_eq!(next_click(&mut d, std::time::Duration::from_millis(10)).await.unwrap(), None, "nothing more: a timeout is not an error");
}

#[tokio::test]
async fn locate_asks_the_accessibility_tree_about_the_clicked_element() {
    let mut d = page_with(vec![true], "https://hr.example.internal/hr/leave");
    let click = ClickPayload { doc: "d1".into(), i: 0, hints: hints("", "", "something else") };
    assert_eq!(locate(&mut d, &click).await, Ok(exact_role("link", "Leave")));
    assert_eq!(d.calls_to("Accessibility.getPartialAXTree")[0]["backendNodeId"], 42);
}

#[tokio::test]
async fn locate_falls_back_to_the_hints_when_the_element_has_gone() {
    let mut d = page_with(vec![false], "https://hr.example.internal/hr/leave");
    let click = ClickPayload { doc: "d1".into(), i: 0, hints: hints("", "", "Apply Leave") };
    assert_eq!(locate(&mut d, &click).await, Ok(exact_text("Apply Leave")));
    assert!(d.calls_to("Accessibility.getPartialAXTree").is_empty());
    let nothing = ClickPayload { doc: "d1".into(), i: 0, hints: hints("", "", "") };
    assert_eq!(locate(&mut d, &nothing).await, Err(UNREADABLE.to_string()));
}

#[tokio::test]
async fn capture_keeps_every_reported_click_then_stops_where_the_page_is() {
    let mut d = page_with(vec![true, false], "https://hr.example.internal/hr/leave/apply?tab=2#top");
    d.events.push_back(clicked(0, "Leave"));
    d.events.push_back(clicked(1, "Apply Leave"));
    let (stop, cancel) = (AtomicBool::new(true), AtomicBool::new(false));
    let mut seen: Vec<String> = vec![];
    let captured = capture(&mut d, &stop, &cancel, &mut |c| {
        seen.push(match c {
            Ok(t) => t.describe(),
            Err(why) => why.to_string(),
        })
    })
    .await;
    assert_eq!(captured.clicks, vec![exact_role("link", "Leave"), exact_text("Apply Leave")]);
    assert_eq!(seen, vec!["link \"Leave\"".to_string(), "text \"Apply Leave\"".to_string()]);
    assert_eq!(captured.ended, Ended::Stopped { href: "https://hr.example.internal/hr/leave/apply?tab=2#top".into() });
    let path = finish(" Leave ", captured, "2026-09-24T10:00:00Z").unwrap();
    assert_eq!(path.module, "Leave");
    assert_eq!(path.arrived, "/hr/leave/apply");
    assert_eq!(path.clicks.len(), 2);
}

/// Review focus 1.
#[tokio::test]
async fn a_closed_recording_browser_ends_the_recording_and_saves_nothing() {
    let mut d = page_with(vec![false], "https://hr.example.internal/hr/leave");
    d.closed_when_drained = true;
    d.events.push_back(clicked(0, "Leave"));
    let (stop, cancel) = (AtomicBool::new(false), AtomicBool::new(false));
    let captured = capture(&mut d, &stop, &cancel, &mut |_| {}).await;
    assert_eq!(captured.clicks.len(), 1);
    assert_eq!(captured.ended, Ended::Closed);
    assert_eq!(finish("Leave", captured, "t").unwrap_err(), BROWSER_CLOSED);
}

#[tokio::test]
async fn cancel_ends_at_once_and_stop_with_no_clicks_saves_nothing() {
    let mut d = page_with(vec![true], "https://hr.example.internal/hr/home/index");
    d.events.push_back(clicked(0, "Leave"));
    let captured = capture(&mut d, &AtomicBool::new(false), &AtomicBool::new(true), &mut |_| {}).await;
    assert_eq!(captured, Captured { clicks: vec![], ended: Ended::Cancelled });
    assert_eq!(finish("Leave", captured, "t").unwrap_err(), CANCELLED);
    let none = Captured { clicks: vec![], ended: Ended::Stopped { href: "https://hr.example.internal/hr/home/index".into() } };
    assert_eq!(finish("Leave", none, "t").unwrap_err(), NO_CLICKS);
}

fn leave_path() -> ModulePath {
    serde_json::from_value(json!({
        "module": "Leave",
        "clicks": [ { "role": "link", "name": "Leave", "exact": true }, { "role": "link", "name": "Apply Leave", "exact": true } ],
        "arrived": "/hr/leave/apply",
        "recorded": "2026-09-24T10:00:00Z"
    }))
    .unwrap()
}

#[tokio::test]
async fn a_path_is_checked_by_signing_in_fresh_and_walking_it_to_where_it_ended() {
    let dir = tempfile::tempdir().unwrap();
    let menu: &[(&str, &str, &str)] = &[("link", "Leave", "/hr/leave"), ("link", "Apply Leave", "/hr/leave/apply")];
    let (mut d, _app) = common::menu_app(menu, "/hr/welcome", 0);
    let ok = check_path(&mut d, dir.path(), &common::menu_recipe(), &common::account(), &leave_path(), &common::quick()).await;
    assert_eq!(ok, Ok("/hr/leave/apply".to_string()));

    let (mut d, _app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/welcome", 0);
    let err = check_path(&mut d, dir.path(), &common::menu_recipe(), &common::account(), &leave_path(), &common::quick())
        .await
        .unwrap_err();
    assert!(err.starts_with("click 2, link \"Apply Leave\": "), "{err}");
}

/// The dialog shows this sentence: the sign-in's own words can name the
/// application's address, so they go to the log and not to the person.
#[tokio::test]
async fn a_sign_in_page_that_will_not_load_is_reported_without_its_address() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|method, _| {
        Ok(match method {
            "Page.navigate" => json!({ "errorText": "net::ERR_NAME_NOT_RESOLVED" }),
            _ => json!({}),
        })
    });
    let err = check_path(&mut d, dir.path(), &common::menu_recipe(), &common::account(), &leave_path(), &common::quick())
        .await
        .unwrap_err();
    assert!(!err.contains("://"), "{err}");
    assert_eq!(err, "the sign-in did not work: check the account and the sign-in recipe, and see Settings, Logs for the details");
}

#[tokio::test]
async fn a_browser_that_stops_answering_during_the_sign_in_is_told_apart() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    let err = check_path(&mut d, dir.path(), &common::menu_recipe(), &common::account(), &leave_path(), &common::quick())
        .await
        .unwrap_err();
    assert!(!err.contains("://"), "{err}");
    assert_eq!(err, "the sign-in did not work: the browser did not respond - try again, and see Settings, Logs if it keeps happening");
}
