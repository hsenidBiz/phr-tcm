//! Recording a module's menu path: clicks reported by the page become the
//! locators a run clicks with, and a recording ends with a path or with
//! the reason nothing was saved.

mod common;

use common::ScriptedDriver;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use v2_lib::autorun::nav::{check_path, ModulePath, SIGN_IN_BROWSER_SILENT, SIGN_IN_FAILED};
use v2_lib::autorun::recorder::{
    arm, ax_chain, capture, finish, locate, locator_from_ax, locator_from_hints, next_click, AxLink, Captured,
    ClickHints, ClickPayload, Ended, BINDING, BROWSER_CLOSED, CANCELLED, LISTENER_JS, NO_CLICKS, UNREADABLE,
};
use v2_lib::browser::cdp::{CdpError, Driver, Event};
use v2_lib::browser::launch::Browser;
use v2_lib::browser::locator::{LocatorStep, Target};
use v2_lib::commands::autorun_record::{
    auto_run_record_cancel, auto_run_recording_is_open, listen, open_the_recording, prepare_to_record,
    recording_is_going, recording_is_open, refuse_to_record_now, refuse_while_recording, unless_cancelled,
    RecorderClaim, RecordingFor, ALREADY_RECORDING, RECORDING_BUSY,
};
use v2_lib::commands::autorun_replay::OneAtATime;
use v2_lib::events::RecordingEvent;

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

/// One recording at a time; a recording and a run never together. Every
/// claim in this binary is taken in this one test, so parallel tests can
/// never see each other's.
#[tokio::test]
async fn a_recording_waits_for_a_run_and_a_run_waits_for_a_recording() {
    assert!(refuse_to_record_now().await.is_ok());
    let run = OneAtATime::claim().expect("nothing is running");
    assert_eq!(refuse_to_record_now().await.unwrap_err(), "an unattended run is going - wait for it, or stop it first");
    drop(run);

    let rec = RecorderClaim::claim().expect("nothing is recording");
    assert!(recording_is_going());
    assert!(RecorderClaim::claim().is_none(), "one recording at a time");
    assert_eq!(refuse_to_record_now().await.unwrap_err(), ALREADY_RECORDING);
    assert_eq!(refuse_while_recording().unwrap_err(), RECORDING_BUSY);
    drop(rec);
    assert!(!recording_is_going());
    assert!(refuse_while_recording().is_ok());

    // Review focus 1: closing the recording browser ends the recording and
    // frees the slot by itself - nobody has to press Stop or Cancel first.
    let rec = RecorderClaim::claim().expect("free again");
    let mut d = page_with(vec![false], "https://hr.example.internal/hr/leave");
    d.closed_when_drained = true;
    d.events.push_back(clicked(0, "Leave"));
    let mut heard: Vec<RecordingEvent> = vec![];
    let (captured, kept) =
        listen(&mut d, rec, &AtomicBool::new(false), &AtomicBool::new(false), &mut |e| heard.push(e)).await;
    assert_eq!(captured.ended, Ended::Closed);
    assert!(kept.is_none(), "a closed recording keeps no claim");
    assert!(!recording_is_going());
    assert!(refuse_while_recording().is_ok());
    assert_eq!(heard.len(), 2, "{heard:?}");
    assert_eq!((heard[0].kind.as_str(), heard[0].index), ("click", 1));
    assert!(heard[0].readable.contains("Leave"), "{heard:?}");
    assert_eq!((heard[1].kind.as_str(), heard[1].detail.as_str()), ("closed", BROWSER_CLOSED));
    for e in &heard {
        assert!(!format!("{e:?}").contains("://"), "an event names no address: {e:?}");
    }

    // Stop keeps the claim: the check in a fresh browser still has to run.
    let rec = RecorderClaim::claim().expect("free again");
    let mut d = page_with(vec![false], "https://hr.example.internal/hr/leave");
    d.events.push_back(clicked(0, "Leave"));
    let (captured, kept) = listen(&mut d, rec, &AtomicBool::new(true), &AtomicBool::new(false), &mut |_| {}).await;
    assert!(matches!(captured.ended, Ended::Stopped { .. }), "{captured:?}");
    assert!(kept.is_some() && recording_is_going(), "the claim outlives Stop until the check is done");
    drop(kept);
    assert!(!recording_is_going());

    // A recording that panics still frees the slot as it unwinds.
    let rec = RecorderClaim::claim().expect("free again");
    let died = tokio::spawn(async move {
        let _held = rec;
        panic!("the recorder fell over");
    })
    .await;
    assert!(died.is_err());
    assert!(!recording_is_going());

    // A Cancel pressed while Start is still signing in wins: nothing opens,
    // the recording browser is closed and the slot is free.
    let rec = RecorderClaim::claim().expect("free again");
    auto_run_record_cancel().await.unwrap();
    let (closed, spawned) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let err = open_the_recording(rec, about(), fake_recording(ClosesOnDrop(closed.clone()), spawned.clone()))
        .await
        .unwrap_err();
    assert_eq!(err, CANCELLED);
    assert!(!spawned.load(Ordering::SeqCst), "a cancelled recording never starts listening");
    assert!(closed.load(Ordering::SeqCst), "its browser is closed");
    assert!(!recording_is_going());
    assert!(!recording_is_open().await);

    // A Cancel left over from something else (a Try) does not cancel the
    // next recording, and a Cancel once it is open ends it as before.
    let rec = RecorderClaim::claim().expect("free again");
    auto_run_record_cancel().await.unwrap();
    drop(rec);
    let rec = RecorderClaim::claim().expect("free again");
    let (closed, spawned) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    open_the_recording(rec, about(), fake_recording(ClosesOnDrop(closed.clone()), spawned.clone()))
        .await
        .expect("nothing cancelled this one");
    assert!(spawned.load(Ordering::SeqCst) && recording_is_going() && recording_is_open().await);
    auto_run_record_cancel().await.unwrap();
    assert!(closed.load(Ordering::SeqCst));
    assert!(!recording_is_going());
    assert!(!recording_is_open().await);

    // Review M3: a Cancel during the check after Stop (or during a Try)
    // finds no recording to end. It is kept, and the check gives up at its
    // next look: what it was doing is dropped, which closes its browser.
    let rec = RecorderClaim::claim().expect("free again");
    let closed = Arc::new(AtomicBool::new(false));
    let browser = ClosesOnDrop(closed.clone());
    let check = tokio::spawn(unless_cancelled(async move {
        let _browser = browser;
        tokio::time::sleep(Duration::from_secs(600)).await;
        Ok::<String, String>("/hr/leave".into())
    }));
    tokio::time::sleep(Duration::from_millis(20)).await;
    auto_run_record_cancel().await.unwrap();
    let out = tokio::time::timeout(Duration::from_secs(5), check).await.expect("a cancelled check ends at once").unwrap();
    assert_eq!(out, Err(CANCELLED.to_string()));
    assert!(closed.load(Ordering::SeqCst), "the check's browser is closed");
    // The Cancel is used up: the next check runs to its end.
    assert_eq!(unless_cancelled(async { Ok::<_, String>("/hr/leave") }).await, Ok("/hr/leave"));
    drop(rec);
    assert!(!recording_is_going());

    // Review M5: a dialog opened afresh can ask whether something still
    // holds the recorder, and Cancel it.
    assert!(!auto_run_recording_is_open().await);
    let rec = RecorderClaim::claim().expect("free again");
    assert!(auto_run_recording_is_open().await, "a Start, a recording, a check or a Try holds it");
    drop(rec);
    assert!(!auto_run_recording_is_open().await);
}

fn about() -> RecordingFor {
    RecordingFor {
        organization: "acme".into(),
        project: "Web".into(),
        module: "Leave".into(),
        account: "admin".into(),
        which: Browser::Edge,
    }
}

/// Stands in for the recording browser: says when it has been closed.
struct ClosesOnDrop(Arc<AtomicBool>);

impl Drop for ClosesOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// The listening task, as Start spawns it, minus the page: it owns the
/// browser and waits for Cancel.
fn fake_recording(
    browser: ClosesOnDrop,
    spawned: Arc<AtomicBool>,
) -> impl FnOnce(RecorderClaim, Arc<AtomicBool>, Arc<AtomicBool>) -> tokio::task::JoinHandle<(Captured, Option<RecorderClaim>)>
{
    move |claim, _stop, cancel| {
        spawned.store(true, Ordering::SeqCst);
        tokio::spawn(async move {
            while !cancel.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            drop(browser);
            (Captured { clicks: vec![], ended: Ended::Cancelled }, Some(claim))
        })
    }
}

/// `menu_app` whose second navigation (the trip home after the sign-in)
/// will not load.
struct HomeWillNotLoad {
    inner: ScriptedDriver,
    navigations: usize,
}

impl Driver for HomeWillNotLoad {
    async fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, CdpError> {
        if method == "Page.navigate" {
            self.navigations += 1;
            if self.navigations > 1 {
                return Ok(json!({ "errorText": "net::ERR_CONNECTION_RESET" }));
            }
        }
        self.inner.call(method, params).await
    }
    async fn wait_event(&mut self, method: &str, limit: Duration) -> Result<Event, CdpError> {
        self.inner.wait_event(method, limit).await
    }
    fn forget_events(&mut self) {
        self.inner.forget_events()
    }
    fn take_dialogs(&mut self) -> Vec<String> {
        self.inner.take_dialogs()
    }
    fn set_deadline(&mut self, deadline: Option<std::time::Instant>) {
        self.inner.set_deadline(deadline)
    }
}

#[tokio::test]
async fn a_recording_whose_home_page_will_not_load_says_it_could_not_start() {
    let dir = tempfile::tempdir().unwrap();
    let (inner, _app) = common::menu_app(&[], "/hr/dashboard", 0);
    let mut d = HomeWillNotLoad { inner, navigations: 0 };
    let err = prepare_to_record(&mut d, dir.path(), &common::menu_recipe(), &common::account(), &common::quick())
        .await
        .unwrap_err();
    assert_eq!(err, "the recording could not start: the home page did not load");
}

/// Getting the recording browser ready names no address, just as the
/// check does: a sign-in's own words can.
#[tokio::test]
async fn a_recording_whose_sign_in_fails_says_so_without_an_address() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = ScriptedDriver::new(|method, _| {
        Ok(match method {
            "Page.navigate" => json!({ "errorText": "net::ERR_NAME_NOT_RESOLVED" }),
            _ => json!({}),
        })
    });
    let err = prepare_to_record(&mut d, dir.path(), &common::menu_recipe(), &common::account(), &common::quick())
        .await
        .unwrap_err();
    assert_eq!(err, SIGN_IN_FAILED);
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    let err = prepare_to_record(&mut d, dir.path(), &common::menu_recipe(), &common::account(), &common::quick())
        .await
        .unwrap_err();
    assert_eq!(err, SIGN_IN_BROWSER_SILENT);
}
