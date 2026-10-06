//! One connection to the whole browser, with a flattened session per tab.
//!
//! A fake browser socket that speaks flattened sessions: it answers every
//! command at once (on the session it came on), and a test feeds it the
//! events a real browser would send. Everything here is about WHICH tab a
//! command goes to and which tab an event reaches: `main` is the only tab
//! a script acts in, and a tab the page opens is set up like `main` -
//! guarded included - before it is let run.

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::Duration;
use v2_lib::browser::cdp::{Cdp, CdpError, Transport, TAB_UNGUARDED};

const MAIN: &str = "S-main";

/// The browser's own socket. Every command is answered as it is sent; a
/// method in `refuse` is answered with an error.
struct FakeBrowser {
    incoming: VecDeque<String>,
    sent: Vec<Value>,
    refuse: Vec<String>,
}

impl FakeBrowser {
    fn new() -> Self {
        FakeBrowser { incoming: VecDeque::new(), sent: vec![], refuse: vec![] }
    }
}

impl Transport for FakeBrowser {
    async fn send(&mut self, text: String) -> Result<(), String> {
        let v: Value = serde_json::from_str(&text).expect("the client sent a frame that is not JSON");
        let id = v["id"].as_u64().expect("every frame the client sends has an id");
        let method = v["method"].as_str().unwrap_or("").to_string();
        let mut reply = if self.refuse.contains(&method) {
            json!({ "id": id, "error": { "code": -32000, "message": "refused by the test" } })
        } else if method == "Target.attachToTarget" {
            json!({ "id": id, "result": { "sessionId": MAIN } })
        } else {
            json!({ "id": id, "result": {} })
        };
        if let Some(s) = v.get("sessionId") {
            reply["sessionId"] = s.clone();
        }
        self.sent.push(v);
        self.incoming.push_back(reply.to_string());
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, String>> {
        match self.incoming.pop_front() {
            Some(f) => Some(Ok(f)),
            None => {
                std::future::pending::<()>().await;
                None
            }
        }
    }
}

/// A client driving `main`, as `Cdp::connect` leaves it.
async fn browser() -> Cdp<FakeBrowser> {
    let mut cdp = Cdp::over(FakeBrowser::new());
    cdp.drive_first_page("T-main", "https://hr.example/home?token=hunter2").await.expect("main was not attached");
    cdp
}

fn sent(cdp: &Cdp<FakeBrowser>) -> &[Value] {
    &cdp.transport().sent
}

/// What was sent on this session, by method, in order.
fn sent_on(cdp: &Cdp<FakeBrowser>, session: &str) -> Vec<String> {
    sent(cdp)
        .iter()
        .filter(|f| f["sessionId"].as_str() == Some(session))
        .map(|f| f["method"].as_str().unwrap_or("").to_string())
        .collect()
}

fn feed(cdp: &mut Cdp<FakeBrowser>, frames: impl IntoIterator<Item = Value>) {
    cdp.transport_mut().incoming.extend(frames.into_iter().map(|f| f.to_string()));
}

/// Read and handle whatever is waiting.
async fn settle(cdp: &mut Cdp<FakeBrowser>) {
    cdp.pump(Duration::from_millis(50)).await;
}

fn attached(session: &str, target: &str, kind: &str, url: &str, waiting: bool) -> Value {
    json!({ "method": "Target.attachedToTarget", "params": {
        "sessionId": session,
        "targetInfo": { "targetId": target, "type": kind, "url": url, "attached": true },
        "waitingForDebugger": waiting
    } })
}

fn on(session: &str, method: &str, params: Value) -> Value {
    json!({ "method": method, "params": params, "sessionId": session })
}

fn paused(session: &str, id: &str, method: &str, url: &str) -> Value {
    on(session, "Fetch.requestPaused", json!({
        "requestId": id, "resourceType": "XHR",
        "request": { "method": method, "url": url, "headers": {} }
    }))
}

fn answer_to<'a>(cdp: &'a Cdp<FakeBrowser>, request_id: &str) -> Option<&'a Value> {
    sent(cdp).iter().find(|f| f["params"]["requestId"] == request_id)
}

const SAVE_STOPPED: &str =
    "this script must not save, but the page tried to send POST /api/Save - it was stopped before it reached the server";

#[tokio::test]
async fn the_first_page_is_attached_over_the_browsers_socket_and_every_command_carries_its_session() {
    let mut cdp = browser().await;
    let f = sent(&cdp);
    assert_eq!(f[0]["method"], "Target.attachToTarget");
    assert_eq!(f[0]["params"], json!({ "targetId": "T-main", "flatten": true }));
    assert!(f[0].get("sessionId").is_none(), "attaching is the browser's own call");
    assert_eq!(sent_on(&cdp, MAIN), ["Page.enable", "Page.setLifecycleEventsEnabled"]);
    let auto = f.iter().find(|f| f["method"] == "Target.setAutoAttach").expect("auto-attach was never asked for");
    assert!(auto.get("sessionId").is_none(), "auto-attach is asked of the browser, not of the page");
    assert_eq!(auto["params"], json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true }));

    cdp.call("Runtime.evaluate", json!({ "expression": "1" })).await.unwrap();
    cdp.guard_saves(&[]).await.unwrap();
    let last: Vec<&Value> = sent(&cdp).iter().rev().take(3).collect();
    for f in last {
        assert_eq!(f["sessionId"], MAIN, "{f}");
    }

    let tabs = cdp.tabs();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].name.as_deref(), Some("main"));
    assert_eq!(tabs[0].target_id, "T-main");
    assert_eq!(tabs[0].url_without_query, "https://hr.example/home");
    assert_eq!(cdp.current().map(|t| t.session_id.as_str()), Some(MAIN));
}

/// Downloads are switched on for the whole browser, so they are asked of
/// the browser itself; their events, which carry no session, are main's.
#[tokio::test]
async fn downloads_are_asked_of_the_browser_and_their_events_are_mains() {
    let dir = tempfile::tempdir().unwrap();
    let mut cdp = browser().await;
    cdp.enable_downloads(dir.path()).await.unwrap();
    let asked = sent(&cdp).last().unwrap();
    assert_eq!(asked["method"], "Browser.setDownloadBehavior");
    assert!(asked.get("sessionId").is_none());
    feed(&mut cdp, [json!({ "method": "Browser.downloadWillBegin", "params": {
        "guid": "g-1", "suggestedFilename": "a.csv", "frameId": "T-main", "url": "https://hr.example/a.csv"
    } })]);
    settle(&mut cdp).await;
    let got = cdp.downloads();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].name, "a.csv");
}

#[tokio::test]
async fn an_event_for_another_tab_never_reaches_main() {
    let mut cdp = browser().await;
    feed(&mut cdp, [attached("S-pop", "T-pop", "page", "https://hr.example/pop", false)]);
    settle(&mut cdp).await;
    assert_eq!(cdp.tabs().len(), 2);
    feed(
        &mut cdp,
        [
            on("S-pop", "Page.loadEventFired", json!({ "timestamp": 1 })),
            on("S-pop", "Page.javascriptDialogOpening", json!({ "type": "alert", "message": "from the popup" })),
            on("S-pop", "Network.requestWillBeSent", json!({
                "requestId": "n1", "timestamp": 1.0,
                "request": { "method": "GET", "url": "https://hr.example/api/List" }
            })),
            on("S-pop", "Network.loadingFailed", json!({ "requestId": "n1", "timestamp": 2.0, "errorText": "net::ERR_FAILED" })),
        ],
    );
    settle(&mut cdp).await;
    assert!(cdp.take_dialogs().is_empty(), "the popup's dialog reached main");
    assert!(cdp.page_log().is_empty(), "the popup's failed request reached main's page log: {:?}", cdp.page_log());
    assert!(cdp.net_since(0).is_empty(), "the popup's request reached main's network record");
    let waited = cdp.wait_event("Page.loadEventFired", Duration::from_millis(50)).await;
    assert!(matches!(waited, Err(CdpError::Timeout { .. })), "the popup's load reached main: {waited:?}");
    // The popup's dialog is still answered, on the popup.
    let answered = sent(&cdp).iter().find(|f| f["method"] == "Page.handleJavaScriptDialog").expect("the dialog was left open");
    assert_eq!(answered["sessionId"], "S-pop");

    // The same events on main's own session do reach it.
    feed(
        &mut cdp,
        [
            on(MAIN, "Page.loadEventFired", json!({ "timestamp": 2 })),
            on(MAIN, "Page.javascriptDialogOpening", json!({ "type": "alert", "message": "from main" })),
        ],
    );
    let load = cdp.wait_event("Page.loadEventFired", Duration::from_millis(200)).await.unwrap();
    assert_eq!(load.params["timestamp"], 2);
    settle(&mut cdp).await;
    assert_eq!(cdp.take_dialogs(), ["alert: from main"]);
}

#[tokio::test]
async fn a_tab_opened_while_guarded_is_set_up_and_guarded_before_it_runs() {
    let _log = crate::serial::log_tail();
    let mut cdp = browser().await;
    cdp.enable_downloads(tempfile::tempdir().unwrap().path()).await.unwrap();
    // A seed script added to main is given to the new tab too.
    cdp.call("Page.addScriptToEvaluateOnNewDocument", json!({ "source": "seed()" })).await.unwrap();
    cdp.guard_saves(&["recalc".to_string()]).await.unwrap();
    feed(&mut cdp, [attached("S-pop", "T-pop", "page", "https://hr.example/report?token=hunter2#x", true)]);
    settle(&mut cdp).await;

    let setup = sent_on(&cdp, "S-pop");
    let at = |m: &str| setup.iter().position(|s| s == m).unwrap_or_else(|| panic!("{m} was never sent: {setup:?}"));
    let run = at("Runtime.runIfWaitingForDebugger");
    for m in [
        "Network.setBypassServiceWorker",
        "Fetch.enable",
        "Network.enable",
        "Runtime.enable",
        "Page.enable",
        "Page.setLifecycleEventsEnabled",
        "Page.addScriptToEvaluateOnNewDocument",
    ] {
        assert!(at(m) < run, "{m} came after the tab was let run: {setup:?}");
    }
    assert!(at("Network.setBypassServiceWorker") < at("Fetch.enable"));
    assert_eq!(setup.iter().filter(|m| *m == "Runtime.runIfWaitingForDebugger").count(), 1);
    let seed = sent(&cdp)
        .iter()
        .find(|f| f["sessionId"] == "S-pop" && f["method"] == "Page.addScriptToEvaluateOnNewDocument")
        .unwrap();
    assert_eq!(seed["params"]["source"], "seed()");

    let tab = &cdp.tabs()[1];
    assert_eq!(tab.session_id, "S-pop");
    assert_eq!(tab.name, None);
    assert_eq!(tab.url_without_query, "https://hr.example/report");
    let lines: Vec<String> = v2_lib::applog::recent(200).into_iter().map(|l| l.message).collect();
    assert!(lines.iter().any(|l| l == "a tab opened: https://hr.example/report"), "{lines:?}");
    assert!(!lines.iter().any(|l| l.contains("hunter2")), "a query reached the log");
    // Actions still go to main.
    assert_eq!(cdp.current().map(|t| t.session_id.as_str()), Some(MAIN));
}

#[tokio::test]
async fn a_guarded_tab_whose_interception_is_refused_is_held_and_fails_the_case() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    cdp.transport_mut().refuse.push("Fetch.enable".to_string());
    feed(&mut cdp, [attached("S-pop", "T-pop", "page", "https://hr.example/pop", true)]);
    settle(&mut cdp).await;
    assert!(
        !sent_on(&cdp, "S-pop").iter().any(|m| m == "Runtime.runIfWaitingForDebugger"),
        "a tab that could not be guarded was let run"
    );
    assert_eq!(cdp.take_save_blocked().as_deref(), Some(TAB_UNGUARDED));
}

#[tokio::test]
async fn a_tab_opened_while_not_guarded_gets_no_interception_and_runs_at_once() {
    let mut cdp = browser().await;
    feed(&mut cdp, [attached("S-pop", "T-pop", "page", "about:blank", true)]);
    settle(&mut cdp).await;
    let setup = sent_on(&cdp, "S-pop");
    assert!(!setup.iter().any(|m| m == "Fetch.enable"), "{setup:?}");
    assert_eq!(setup.last().map(String::as_str), Some("Runtime.runIfWaitingForDebugger"));
    assert!(setup.iter().any(|m| m == "Network.enable") && setup.iter().any(|m| m == "Page.enable"));
}

/// Review Focus 1: with the must-not-save guard on, a save the popup sends
/// is stopped as main's would be, and the case hears of it.
#[tokio::test]
async fn a_save_paused_in_a_popup_is_refused_as_mains_would_be() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    feed(&mut cdp, [attached("S-pop", "T-pop", "page", "https://hr.example/pop", true)]);
    settle(&mut cdp).await;
    feed(
        &mut cdp,
        [
            paused("S-pop", "r1", "POST", "https://hr.example/api/Save?token=hunter2"),
            paused("S-pop", "r2", "GET", "https://hr.example/api/List"),
        ],
    );
    settle(&mut cdp).await;
    let stop = answer_to(&cdp, "r1").expect("the popup's save was left paused");
    assert_eq!(stop["method"], "Fetch.failRequest");
    assert_eq!(stop["params"]["errorReason"], "BlockedByClient");
    assert_eq!(stop["sessionId"], "S-pop", "answered on another tab");
    let go = answer_to(&cdp, "r2").unwrap();
    assert_eq!(go["method"], "Fetch.continueRequest");
    assert_eq!(go["sessionId"], "S-pop");
    assert_eq!(cdp.take_save_blocked().as_deref(), Some(SAVE_STOPPED));
    assert_eq!(cdp.take_save_blocked(), None, "reported twice");
}

/// A popup that saves and closes at once still fails the case.
#[tokio::test]
async fn a_save_stopped_in_a_popup_that_then_closed_is_still_reported() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    feed(
        &mut cdp,
        [
            attached("S-pop", "T-pop", "page", "https://hr.example/pop", true),
            paused("S-pop", "r1", "POST", "https://hr.example/api/Save"),
            json!({ "method": "Target.detachedFromTarget", "params": { "sessionId": "S-pop", "targetId": "T-pop" } }),
        ],
    );
    settle(&mut cdp).await;
    assert_eq!(cdp.tabs().len(), 1);
    assert_eq!(cdp.take_save_blocked().as_deref(), Some(SAVE_STOPPED));
}

#[tokio::test]
async fn a_worker_is_only_let_run() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    feed(&mut cdp, [attached("S-sw", "T-sw", "service_worker", "https://hr.example/sw.js", true)]);
    settle(&mut cdp).await;
    assert_eq!(sent_on(&cdp, "S-sw"), ["Runtime.runIfWaitingForDebugger"]);
    assert_eq!(cdp.tabs().len(), 1, "a worker became a tab");
}

/// The browser may attach main a second time: that session is let go,
/// and main is not driven twice.
#[tokio::test]
async fn a_second_session_on_main_is_let_go() {
    let mut cdp = browser().await;
    feed(&mut cdp, [attached("S-dup", "T-main", "page", "https://hr.example/home", false)]);
    settle(&mut cdp).await;
    assert_eq!(cdp.tabs().len(), 1);
    let detach = sent(&cdp).iter().find(|f| f["method"] == "Target.detachFromTarget").expect("the second session was kept");
    assert_eq!(detach["params"]["sessionId"], "S-dup");
    assert!(detach.get("sessionId").is_none());
}

#[tokio::test]
async fn a_popup_that_detaches_or_crashes_is_dropped() {
    let mut cdp = browser().await;
    feed(
        &mut cdp,
        [
            attached("S-a", "T-a", "page", "https://hr.example/a", false),
            attached("S-b", "T-b", "page", "https://hr.example/b", false),
        ],
    );
    settle(&mut cdp).await;
    assert_eq!(cdp.tabs().len(), 3);
    feed(
        &mut cdp,
        [
            json!({ "method": "Target.detachedFromTarget", "params": { "sessionId": "S-a", "targetId": "T-a" } }),
            on("S-b", "Inspector.targetCrashed", json!({})),
        ],
    );
    settle(&mut cdp).await;
    let left: Vec<&str> = cdp.tabs().iter().map(|t| t.session_id.as_str()).collect();
    assert_eq!(left, [MAIN]);
    // Main goes on as before.
    cdp.call("Runtime.evaluate", json!({})).await.unwrap();
}

/// Main closing is the browser closing, as it always was.
#[tokio::test]
async fn main_detaching_closes_the_connection() {
    let mut cdp = browser().await;
    feed(&mut cdp, [json!({ "method": "Target.detachedFromTarget", "params": { "sessionId": MAIN, "targetId": "T-main" } })]);
    settle(&mut cdp).await;
    assert!(cdp.current().is_none());
    assert_eq!(cdp.call("Runtime.evaluate", json!({})).await, Err(CdpError::Closed));
    assert!(matches!(cdp.wait_event("Page.loadEventFired", Duration::from_millis(50)).await, Err(CdpError::Closed)));
}
