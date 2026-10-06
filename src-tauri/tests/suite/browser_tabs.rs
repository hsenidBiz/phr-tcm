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
use v2_lib::browser::cdp::{Cdp, CdpError, Transport, TAB_HELD_UNGUARDED, TAB_OPEN_UNGUARDED};

const MAIN: &str = "S-main";

/// The browser's own socket. Every command is answered as it is sent.
/// Each list below names a command as `method` (on any session) or
/// `method@session`:
/// - `refuse`: answered with an error;
/// - `withhold`: its answer goes to `withheld`, for the test to hand over
///   later;
/// - `before_reply`: these frames arrive just ahead of its answer, once.
/// - `after_reply`: these frames arrive just after its answer, once.
/// - `withhold_first`: like `withhold`, for the first such command only.
///
/// `Target.createTarget` answers with the target `T-new`, and
/// `Target.getTargetInfo` says main is in the browser context `C-1`.
struct FakeBrowser {
    incoming: VecDeque<String>,
    sent: Vec<Value>,
    refuse: Vec<String>,
    withhold: Vec<String>,
    withheld: Vec<String>,
    before_reply: Vec<(String, Value)>,
    after_reply: Vec<(String, Value)>,
    withhold_first: Vec<String>,
}

impl FakeBrowser {
    fn new() -> Self {
        FakeBrowser {
            incoming: VecDeque::new(),
            sent: vec![],
            refuse: vec![],
            withhold: vec![],
            withheld: vec![],
            before_reply: vec![],
            after_reply: vec![],
            withhold_first: vec![],
        }
    }
}

/// Does `name` (`method` or `method@session`) name this command?
fn names(name: &str, method: &str, session: &str) -> bool {
    name == method || name == format!("{method}@{session}")
}

impl Transport for FakeBrowser {
    async fn send(&mut self, text: String) -> Result<(), String> {
        let v: Value = serde_json::from_str(&text).expect("the client sent a frame that is not JSON");
        let id = v["id"].as_u64().expect("every frame the client sends has an id");
        let method = v["method"].as_str().unwrap_or("").to_string();
        let session = v["sessionId"].as_str().unwrap_or("").to_string();
        while let Some(i) = self.before_reply.iter().position(|(n, _)| names(n, &method, &session)) {
            let (_, frame) = self.before_reply.remove(i);
            self.incoming.push_back(frame.to_string());
        }
        let mut reply = if self.refuse.iter().any(|n| names(n, &method, &session)) {
            json!({ "id": id, "error": { "code": -32000, "message": "refused by the test" } })
        } else if method == "Target.attachToTarget" {
            json!({ "id": id, "result": { "sessionId": MAIN } })
        } else if method == "Target.createTarget" {
            json!({ "id": id, "result": { "targetId": "T-new" } })
        } else if method == "Target.getTargetInfo" {
            json!({ "id": id, "result": { "targetInfo": { "targetId": v["params"]["targetId"], "browserContextId": "C-1" } } })
        } else {
            json!({ "id": id, "result": {} })
        };
        if let Some(s) = v.get("sessionId") {
            reply["sessionId"] = s.clone();
        }
        self.sent.push(v);
        let once = self.withhold_first.iter().position(|n| names(n, &method, &session));
        if let Some(i) = once {
            self.withhold_first.remove(i);
        }
        if once.is_some() || self.withhold.iter().any(|n| names(n, &method, &session)) {
            self.withheld.push(reply.to_string());
        } else {
            self.incoming.push_back(reply.to_string());
        }
        while let Some(i) = self.after_reply.iter().position(|(n, _)| names(n, &method, &session)) {
            let (_, frame) = self.after_reply.remove(i);
            self.incoming.push_back(frame.to_string());
        }
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

/// The run's armed `expect_dialog` is the run's, not a tab's: a dialog the
/// popup opens is claimed and answered as it asks, on the popup, and is
/// not noted anywhere as one nobody expected. With nothing armed, one is
/// accepted as always.
#[tokio::test]
async fn an_armed_expectation_claims_a_dialog_in_another_tab() {
    use v2_lib::browser::cdp::Driver;
    use v2_lib::browser::dialogs::DialogPlan;
    let mut cdp = browser().await;
    feed(&mut cdp, [attached("S-pop", "T-pop", "page", "https://hr.example/pop", false)]);
    settle(&mut cdp).await;
    Driver::dialog_book(&mut cdp)
        .unwrap()
        .arm(vec![DialogPlan { id: 0, accept: true, prompt_text: Some("Kim".into()) }]);
    feed(&mut cdp, [on("S-pop", "Page.javascriptDialogOpening", json!({ "type": "prompt", "message": "Your name?" }))]);
    settle(&mut cdp).await;
    let answered = sent(&cdp).iter().rev().find(|f| f["method"] == "Page.handleJavaScriptDialog").unwrap().clone();
    assert_eq!(answered["sessionId"], "S-pop");
    assert_eq!(answered["params"], json!({ "accept": true, "promptText": "Kim" }));
    let book = Driver::dialog_book(&mut cdp).unwrap();
    assert_eq!(book.claimed(0).map(|s| s.message.as_str()), Some("Your name?"));
    assert!(!book.is_armed());
    assert!(cdp.take_dialogs().is_empty());

    // Nothing armed: accepted, and noted on main.
    feed(&mut cdp, [on(MAIN, "Page.javascriptDialogOpening", json!({ "type": "beforeunload", "message": "" }))]);
    settle(&mut cdp).await;
    let answered = sent(&cdp).iter().rev().find(|f| f["method"] == "Page.handleJavaScriptDialog").unwrap().clone();
    assert_eq!(answered["sessionId"], MAIN);
    assert_eq!(answered["params"], json!({ "accept": true }));
    assert_eq!(cdp.take_dialogs(), ["beforeunload: "]);
}

/// A dialog on a session no tab is registered for yet (a popup whose
/// attach has not been read) is still answered on that session: accepted
/// with nothing armed, as an armed expectation asks otherwise.
#[tokio::test]
async fn a_dialog_from_a_tab_not_yet_registered_is_answered() {
    use v2_lib::browser::cdp::Driver;
    use v2_lib::browser::dialogs::DialogPlan;
    let mut cdp = browser().await;
    feed(&mut cdp, [on("S-new", "Page.javascriptDialogOpening", json!({ "type": "alert", "message": "hello" }))]);
    settle(&mut cdp).await;
    let answered = sent(&cdp).iter().rev().find(|f| f["method"] == "Page.handleJavaScriptDialog").expect("the dialog was left open").clone();
    assert_eq!(answered["sessionId"], "S-new");
    assert_eq!(answered["params"], json!({ "accept": true }));
    assert!(cdp.take_dialogs().is_empty(), "a dialog from another tab reached main");
    assert_eq!(Driver::dialog_book(&mut cdp).unwrap().seen().last().map(|s| s.message.as_str()), Some("hello"));

    Driver::dialog_book(&mut cdp).unwrap().arm(vec![DialogPlan { id: 0, accept: false, prompt_text: None }]);
    feed(&mut cdp, [on("S-new", "Page.javascriptDialogOpening", json!({ "type": "confirm", "message": "Leave?" }))]);
    settle(&mut cdp).await;
    let answered = sent(&cdp).iter().rev().find(|f| f["method"] == "Page.handleJavaScriptDialog").unwrap().clone();
    assert_eq!(answered["sessionId"], "S-new");
    assert_eq!(answered["params"], json!({ "accept": false }));
    assert_eq!(Driver::dialog_book(&mut cdp).unwrap().claimed(0).map(|s| s.message.as_str()), Some("Leave?"));
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
    assert_eq!(cdp.take_save_blocked().as_deref(), Some(TAB_HELD_UNGUARDED));
}

/// A tab that was already running when it was attached could not be held,
/// and its case says so.
#[tokio::test]
async fn an_already_open_tab_that_cannot_be_guarded_says_so() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    cdp.transport_mut().refuse.push("Fetch.enable@S-old".to_string());
    feed(&mut cdp, [attached("S-old", "T-old", "page", "https://hr.example/old", false)]);
    settle(&mut cdp).await;
    assert_eq!(cdp.take_save_blocked().as_deref(), Some(TAB_OPEN_UNGUARDED));
}

/// Review fix 1: a tab that opens while main's interception is still being
/// switched on is guarded before it runs, not let run unguarded.
#[tokio::test]
async fn a_tab_that_opens_while_the_guard_is_switched_on_is_guarded_before_it_runs() {
    let mut cdp = browser().await;
    let pop = attached("S-pop", "T-pop", "page", "https://hr.example/pop", true);
    cdp.transport_mut().before_reply.push(("Fetch.enable@S-main".to_string(), pop));
    cdp.guard_saves(&[]).await.unwrap();
    settle(&mut cdp).await;
    let setup = sent_on(&cdp, "S-pop");
    let fetch = setup.iter().position(|m| m == "Fetch.enable").unwrap_or_else(|| panic!("never guarded: {setup:?}"));
    let run = setup
        .iter()
        .position(|m| m == "Runtime.runIfWaitingForDebugger")
        .unwrap_or_else(|| panic!("never let run: {setup:?}"));
    assert!(fetch < run, "{setup:?}");
    feed(&mut cdp, [paused("S-pop", "r1", "POST", "https://hr.example/api/Save")]);
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.failRequest");
}

/// A guard that could not be switched on leaves the run as it was: a tab
/// opened afterwards is not intercepted.
#[tokio::test]
async fn a_refused_guard_leaves_the_run_unguarded() {
    let mut cdp = browser().await;
    cdp.transport_mut().refuse.push("Fetch.enable@S-main".to_string());
    assert!(cdp.guard_saves(&[]).await.is_err());
    assert!(!cdp.is_guarding_saves());
    feed(&mut cdp, [attached("S-pop", "T-pop", "page", "https://hr.example/pop", true)]);
    settle(&mut cdp).await;
    assert!(!sent_on(&cdp, "S-pop").iter().any(|m| m == "Fetch.enable"));
}

/// Review fix 2: a tab that opens while the guard is lifted is not guarded.
#[tokio::test]
async fn a_tab_that_opens_while_the_guard_is_lifted_is_not_guarded() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    let pop = attached("S-pop", "T-pop", "page", "https://hr.example/pop", true);
    cdp.transport_mut().before_reply.push(("Fetch.disable@S-main".to_string(), pop));
    cdp.stop_guarding_saves().await.unwrap();
    settle(&mut cdp).await;
    let setup = sent_on(&cdp, "S-pop");
    assert!(!setup.iter().any(|m| m == "Fetch.enable"), "{setup:?}");
    assert_eq!(setup.last().map(String::as_str), Some("Runtime.runIfWaitingForDebugger"));
    assert!(!cdp.is_guarding_saves());
}

/// A popup that will not stop intercepting does not keep main guarded, and
/// leaves the run counted as guarded so the lift is asked again.
#[tokio::test]
async fn a_popup_that_will_not_stop_intercepting_is_asked_again() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    feed(&mut cdp, [attached("S-pop", "T-pop", "page", "https://hr.example/pop", true)]);
    settle(&mut cdp).await;
    cdp.transport_mut().refuse.push("Fetch.disable@S-pop".to_string());
    assert!(cdp.stop_guarding_saves().await.is_err());
    assert!(sent_on(&cdp, MAIN).iter().any(|m| m == "Fetch.disable"), "main was not lifted");
    assert!(cdp.is_guarding_saves(), "a tab still intercepts, so the lift must be asked again");
    cdp.transport_mut().refuse.clear();
    cdp.stop_guarding_saves().await.unwrap();
    assert!(!cdp.is_guarding_saves());
}

/// After the lift, a case that may save is not intercepted, in main or in
/// a tab it opens.
#[tokio::test]
async fn a_later_case_that_saves_is_not_intercepted() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    cdp.stop_guarding_saves().await.unwrap();
    feed(
        &mut cdp,
        [
            attached("S-pop", "T-pop", "page", "https://hr.example/pop", true),
            paused(MAIN, "r1", "POST", "https://hr.example/api/Save"),
        ],
    );
    settle(&mut cdp).await;
    assert!(!sent_on(&cdp, "S-pop").iter().any(|m| m == "Fetch.enable"));
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.continueRequest");
    assert_eq!(cdp.take_save_blocked(), None);
}

/// Review fix 3: a tab held for its guard is let run once the guard is
/// lifted, even when its interception's refusal arrives during the lift.
#[tokio::test]
async fn a_held_tab_is_let_run_once_the_guard_is_lifted() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    cdp.transport_mut().refuse.push("Fetch.enable@S-pop".to_string());
    cdp.transport_mut().withhold.push("Fetch.enable@S-pop".to_string());
    feed(&mut cdp, [attached("S-pop", "T-pop", "page", "https://hr.example/pop", true)]);
    settle(&mut cdp).await;
    assert!(!sent_on(&cdp, "S-pop").iter().any(|m| m == "Runtime.runIfWaitingForDebugger"));
    let late = std::mem::take(&mut cdp.transport_mut().withheld);
    cdp.transport_mut().incoming.extend(late);
    cdp.stop_guarding_saves().await.unwrap();
    settle(&mut cdp).await;
    let runs = sent_on(&cdp, "S-pop").iter().filter(|m| *m == "Runtime.runIfWaitingForDebugger").count();
    assert_eq!(runs, 1);
    assert_eq!(cdp.take_save_blocked(), None, "a refusal read after the lift failed the next case");
}

/// Review fix 5: a tab that refuses its page log or dialog handler is
/// still let run, and the log says which tab, without its query.
#[tokio::test]
async fn a_refused_tab_setup_is_logged() {
    let _log = crate::serial::log_tail();
    let mut cdp = browser().await;
    cdp.transport_mut().refuse.push("Page.enable@S-pop".to_string());
    feed(&mut cdp, [attached("S-pop", "T-pop", "page", "https://hr.example/pop?token=hunter2", true)]);
    settle(&mut cdp).await;
    assert!(sent_on(&cdp, "S-pop").iter().any(|m| m == "Runtime.runIfWaitingForDebugger"));
    let lines = v2_lib::applog::recent(200);
    let warned = lines
        .iter()
        .find(|l| l.level == "warn" && l.message.contains("Page.enable"))
        .unwrap_or_else(|| panic!("nothing was logged: {lines:?}"));
    assert!(warned.message.ends_with(": https://hr.example/pop"), "{}", warned.message);
    assert!(!lines.iter().any(|l| l.message.contains("hunter2")), "a query reached the log");
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

/// The draft page's pagehide beacon is paused before the sign-in asks for
/// the hold, but its pause is read after. It is judged as it was when the
/// browser paused it: refused. The hold takes effect only once the browser
/// has answered a command sent after it was asked for.
#[tokio::test]
async fn a_save_paused_before_the_hold_is_refused_even_when_read_after_it() {
    let mut cdp = browser().await;
    // The sign-in page is the page on screen when the hold is asked for.
    feed(&mut cdp, [navigated(MAIN, "L-signin", "https://hr.example/login")]);
    settle(&mut cdp).await;
    cdp.guard_saves(&[]).await.unwrap();
    cdp.hold_saves(true);
    // Read with no command since the hold: still guarded.
    feed(&mut cdp, [paused(MAIN, "r0", "POST", "https://hr.example/api/SaveDraft")]);
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r0").unwrap()["method"], "Fetch.failRequest");
    // Read ahead of the answer to the first command since the hold.
    let beacon = paused(MAIN, "r1", "POST", "https://hr.example/api/SaveDraft?token=hunter2");
    cdp.transport_mut().before_reply.push(("Runtime.evaluate@S-main".to_string(), beacon));
    cdp.call("Runtime.evaluate", json!({})).await.unwrap();
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.failRequest", "the beacon was let through");
    assert!(cdp.take_save_blocked().is_some_and(|b| b.contains("POST /api/SaveDraft")));
    // From then on the hold is on: the sign-in's own requests go through.
    feed(
        &mut cdp,
        [
            sent_by(MAIN, "n2", "L-signin", "https://hr.example/api/Save"),
            paused_from(MAIN, "r2", "https://hr.example/api/Save", "XHR", "n2"),
        ],
    );
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r2").unwrap()["method"], "Fetch.continueRequest");
    // Ending the hold takes effect at once.
    cdp.hold_saves(false);
    feed(&mut cdp, [paused(MAIN, "r3", "POST", "https://hr.example/api/Save")]);
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r3").unwrap()["method"], "Fetch.failRequest");
}

fn navigated(session: &str, loader: &str, url: &str) -> Value {
    on(session, "Page.frameNavigated", json!({ "frame": { "id": "T-main", "loaderId": loader, "url": url } }))
}

fn sent_by(session: &str, network_id: &str, loader: &str, url: &str) -> Value {
    on(session, "Network.requestWillBeSent", json!({
        "requestId": network_id, "loaderId": loader, "timestamp": 1.0,
        "request": { "method": "POST", "url": url }
    }))
}

fn paused_from(session: &str, id: &str, url: &str, kind: &str, network_id: &str) -> Value {
    on(session, "Fetch.requestPaused", json!({
        "requestId": id, "resourceType": kind, "networkId": network_id, "frameId": "T-main",
        "request": { "method": "POST", "url": url, "headers": {} }
    }))
}

/// A guarded browser that has left the draft (loader L-draft) for the
/// sign-in page (L-signin), and whose sign-in hold has taken effect.
async fn signing_in() -> Cdp<FakeBrowser> {
    let mut cdp = browser().await;
    feed(
        &mut cdp,
        [
            navigated(MAIN, "L-draft", "https://hr.example/draft"),
            navigated(MAIN, "L-signin", "https://hr.example/login"),
        ],
    );
    settle(&mut cdp).await;
    cdp.guard_saves(&["login".to_string()]).await.unwrap();
    cdp.hold_saves(true);
    cdp.call("Runtime.evaluate", json!({})).await.unwrap();
    cdp
}

/// The page being left sends its beacon late: it is paused only after the
/// hold took effect. A ping is still stopped.
#[tokio::test]
async fn a_ping_paused_after_the_hold_took_effect_is_still_refused() {
    let mut cdp = signing_in().await;
    feed(&mut cdp, [paused_from(MAIN, "r1", "https://hr.example/api/SaveDraft", "Ping", "n1")]);
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.failRequest");
    assert!(cdp.take_save_blocked().is_some_and(|b| b.contains("POST /api/SaveDraft")));
}

/// A save from the document left before the hold, paused after it took
/// effect, is still stopped - here its document is learnt only after its
/// pause is read.
#[tokio::test]
async fn a_save_from_the_page_being_left_paused_after_the_hold_is_still_refused() {
    let mut cdp = signing_in().await;
    feed(&mut cdp, [paused_from(MAIN, "r1", "https://hr.example/api/SaveDraft", "Fetch", "n1")]);
    settle(&mut cdp).await;
    assert!(answer_to(&cdp, "r1").is_none(), "answered before its document was known");
    feed(&mut cdp, [sent_by(MAIN, "n1", "L-draft", "https://hr.example/api/SaveDraft")]);
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.failRequest");
    assert!(cdp.take_save_blocked().is_some_and(|b| b.contains("POST /api/SaveDraft")));
}

/// The sign-in page's own login goes through: sent by the page the sign-in
/// arrived on, or as the navigation that page's form starts.
#[tokio::test]
async fn the_sign_in_pages_own_login_goes_through() {
    let mut cdp = signing_in().await;
    feed(
        &mut cdp,
        [
            sent_by(MAIN, "n1", "L-signin", "https://hr.example/api/login"),
            paused_from(MAIN, "r1", "https://hr.example/api/login", "XHR", "n1"),
            paused_from(MAIN, "r2", "https://hr.example/Account/login", "Document", "n2"),
            sent_by(MAIN, "n2", "L-next", "https://hr.example/Account/login"),
        ],
    );
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.continueRequest");
    assert_eq!(answer_to(&cdp, "r2").unwrap()["method"], "Fetch.continueRequest");
    assert_eq!(cdp.take_save_blocked(), None);
}

/// A sign-in page's form post is a navigation, a new document by
/// definition, so it goes through even when the browser does not say which
/// document sent it (no Network domain). Any other save it cannot place is
/// stopped.
#[tokio::test]
async fn a_sign_in_form_post_goes_through_even_unplaced() {
    let mut cdp = signing_in().await;
    let unplaced = |id: &str, kind: &str| {
        on(MAIN, "Fetch.requestPaused", json!({
            "requestId": id, "resourceType": kind, "frameId": "T-main",
            "request": { "method": "POST", "url": "https://hr.example/Account/login", "headers": {} }
        }))
    };
    feed(&mut cdp, [unplaced("r1", "Document"), unplaced("r2", "XHR")]);
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.continueRequest");
    assert_eq!(answer_to(&cdp, "r2").unwrap()["method"], "Fetch.failRequest");
}

// ---------------------------------------------------------------------------
// The tab actions: naming, opening, switching, closing and waiting for tabs.
// ---------------------------------------------------------------------------

use v2_lib::browser::actions::{execute, execute_in, Action, Policy};

fn detached(session: &str) -> Value {
    json!({ "method": "Target.detachedFromTarget", "params": { "sessionId": session } })
}

/// A popup the page opened during this step, read at once.
async fn popup(cdp: &mut Cdp<FakeBrowser>, session: &str, target: &str, url: &str) {
    feed(cdp, [attached(session, target, "page", url, false)]);
    settle(cdp).await;
}

/// `main` and a popup called `name`, the step that opened it begun.
async fn with_named(name: &str) -> Cdp<FakeBrowser> {
    let mut cdp = browser().await;
    cdp.step_began();
    popup(&mut cdp, "S-pop", "T-pop", "https://hr.example/report/7?token=hunter2").await;
    cdp.expect_tab(name, None, Duration::from_millis(500)).await.expect("the popup was not named");
    cdp
}

fn tab_failure(e: Result<impl std::fmt::Debug, CdpError>) -> String {
    match e {
        Err(CdpError::Tab(sentence)) => sentence,
        other => panic!("not a tab rule's failure: {other:?}"),
    }
}

#[tokio::test]
async fn expect_tab_names_the_newest_tab_opened_since_the_previous_step_began() {
    let mut cdp = browser().await;
    // Open before the run: never claimed.
    popup(&mut cdp, "S-old", "T-old", "https://hr.example/old").await;
    cdp.step_began();
    popup(&mut cdp, "S-one", "T-one", "https://hr.example/one").await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    popup(&mut cdp, "S-two", "T-two", "https://hr.example/two?id=9").await;
    cdp.step_began();

    let address = cdp.expect_tab("report", None, Duration::from_millis(500)).await.unwrap();
    assert_eq!(address, "https://hr.example/two", "the newest, with no query");
    let named =
        |cdp: &Cdp<FakeBrowser>, s: &str| cdp.tabs().iter().find(|t| t.session_id == s).and_then(|t| t.name.clone());
    assert_eq!(named(&cdp, "S-two").as_deref(), Some("report"));
    cdp.expect_tab("other", None, Duration::from_millis(500)).await.unwrap();
    assert_eq!(named(&cdp, "S-one").as_deref(), Some("other"));
    // The tab opened before the run is left alone.
    let none = cdp.expect_tab("third", None, Duration::from_millis(300)).await;
    assert_eq!(tab_failure(none), "no new tab opened within 0.3 seconds");
    assert_eq!(named(&cdp, "S-old"), None);
    // Naming it did not switch to it.
    assert_eq!(cdp.tab_name(), "main");
}

#[tokio::test]
async fn a_tab_opened_during_an_earlier_step_is_never_claimed() {
    let mut cdp = browser().await;
    cdp.step_began();
    popup(&mut cdp, "S-pop", "T-pop", "https://hr.example/pop").await;
    cdp.step_began();
    cdp.step_began();
    let none = cdp.expect_tab("late", None, Duration::from_millis(1000)).await;
    assert_eq!(tab_failure(none), "no new tab opened within 1 seconds");
}

#[tokio::test]
async fn expect_tab_says_each_of_its_failures_in_the_spec_words() {
    let mut cdp = browser().await;
    cdp.step_began();
    popup(&mut cdp, "S-pop", "T-pop", "https://hr.example/pop?report=1").await;
    let elsewhere = cdp.expect_tab("report", Some("/reports/"), Duration::from_millis(300)).await;
    assert_eq!(tab_failure(elsewhere), "the new tab's address does not contain \"/reports/\"");
    assert_eq!(cdp.tabs()[1].name, None, "a tab somewhere else is not named");
    cdp.expect_tab("report", None, Duration::from_millis(300)).await.unwrap();
    let taken = cdp.expect_tab("report", None, Duration::from_millis(300)).await;
    assert_eq!(tab_failure(taken), "there is already a tab report");
    let main = cdp.expect_tab("main", None, Duration::from_millis(300)).await;
    assert_eq!(tab_failure(main), "there is already a tab main");
}

/// A popup is attached blank and then goes to its own address: the wait
/// looks at where it is now.
#[tokio::test]
async fn expect_tab_matches_the_address_a_popup_goes_to() {
    let mut cdp = browser().await;
    cdp.step_began();
    popup(&mut cdp, "S-pop", "T-pop", "about:blank").await;
    feed(
        &mut cdp,
        [on(
            "S-pop",
            "Page.frameNavigated",
            json!({ "frame": { "id": "T-pop", "loaderId": "L1", "url": "https://hr.example/reports/7?token=hunter2" } }),
        )],
    );
    let address = cdp.expect_tab("report", Some("/reports/"), Duration::from_millis(500)).await.unwrap();
    assert_eq!(address, "https://hr.example/reports/7");

    cdp.step_began();
    popup(&mut cdp, "S-b", "T-b", "https://hr.example/b?x=1").await;
    let out = execute(&mut cdp, &Action::ExpectTab { name: "b".into(), url_contains: None, within_ms: Some(500) }).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.detail, "a new tab opened at /b; it is called \"b\"");
}

#[tokio::test]
async fn steps_act_in_the_current_tab_and_switch_tab_brings_it_to_the_front() {
    let mut cdp = with_named("report").await;
    let out = execute(&mut cdp, &Action::SwitchTab { name: "report".into() }).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.detail, "switched to the \"report\" tab");
    let front = sent(&cdp).iter().find(|f| f["method"] == "Target.activateTarget").expect("not brought to the front");
    assert!(front.get("sessionId").is_none());
    assert_eq!(front["params"]["targetId"], "T-pop");
    cdp.call("Runtime.evaluate", json!({ "expression": "1" })).await.unwrap();
    assert_eq!(sent(&cdp).last().unwrap()["sessionId"], "S-pop");
    assert_eq!(cdp.tab_name(), "report");

    let none = execute(&mut cdp, &Action::SwitchTab { name: "nowhere".into() }).await;
    assert!(!none.ok && !none.harness, "{none:?}");
    assert_eq!(none.detail, "there is no tab nowhere");

    execute(&mut cdp, &Action::SwitchTab { name: "main".into() }).await;
    cdp.call("Runtime.evaluate", json!({ "expression": "1" })).await.unwrap();
    assert_eq!(sent(&cdp).last().unwrap()["sessionId"], MAIN);
}

/// Review Focus 2.
#[tokio::test]
async fn closing_the_current_tab_makes_main_current_and_nothing_waits_on_it() {
    let mut cdp = with_named("report").await;
    cdp.switch_tab("report").await.unwrap();
    let out = execute(&mut cdp, &Action::CloseTab { name: "report".into() }).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.detail, "closed the \"report\" tab; main is the current tab now");
    let closed = sent(&cdp).iter().find(|f| f["method"] == "Target.closeTarget").expect("never closed");
    assert_eq!(closed["params"]["targetId"], "T-pop");
    assert_eq!(cdp.tabs().len(), 1);
    assert_eq!(cdp.tab_name(), "main");
    // The browser's own word that it went is harmless now.
    feed(&mut cdp, [detached("S-pop")]);
    let answered =
        tokio::time::timeout(Duration::from_secs(2), cdp.call("Runtime.evaluate", json!({ "expression": "1" }))).await;
    assert!(matches!(answered, Ok(Ok(_))), "{answered:?}");
    assert_eq!(sent(&cdp).last().unwrap()["sessionId"], MAIN);
    assert_eq!(cdp.missing_tab(), None);

    let again = execute(&mut cdp, &Action::CloseTab { name: "report".into() }).await;
    assert_eq!(again.detail, "there is no tab report");
    assert_eq!(tab_failure(cdp.close_tab("main").await), "main cannot be closed");
}

#[tokio::test]
async fn closing_a_tab_that_is_not_current_leaves_the_current_tab_alone() {
    let mut cdp = with_named("report").await;
    let out = execute(&mut cdp, &Action::CloseTab { name: "report".into() }).await;
    assert_eq!(out.detail, "closed the \"report\" tab");
    assert_eq!(cdp.tab_name(), "main");
}

/// Review Focus 3: a print preview that closes itself while it is current.
#[tokio::test]
async fn a_current_tab_that_closes_itself_fails_the_next_call_with_its_name() {
    let mut cdp = with_named("preview").await;
    cdp.switch_tab("preview").await.unwrap();
    feed(&mut cdp, [detached("S-pop")]);
    settle(&mut cdp).await;

    let call =
        tokio::time::timeout(Duration::from_secs(2), cdp.call("Runtime.evaluate", json!({ "expression": "1" }))).await;
    assert_eq!(call.expect("the call waited on a closed tab"), Err(CdpError::Tab("there is no tab preview".into())));
    let event =
        tokio::time::timeout(Duration::from_secs(2), cdp.wait_event("Page.loadEventFired", Duration::from_secs(1))).await;
    assert_eq!(event.expect("the wait hung"), Err(CdpError::Tab("there is no tab preview".into())));
    assert_eq!(cdp.missing_tab().as_deref(), Some("preview"));
    assert_eq!(cdp.tab_name(), "preview");

    // An action that acts in the page says so plainly, not as the browser
    // having stopped answering.
    let out = execute(&mut cdp, &Action::CheckUrl { contains: "x".into() }).await;
    assert!(!out.ok && !out.harness, "{out:?}");
    assert_eq!(out.detail, "there is no tab preview");

    // The tab actions still run: its closing is what the case checks.
    let closed = execute(&mut cdp, &Action::ExpectTabClosed { name: "preview".into(), within_ms: Some(500) }).await;
    assert!(closed.ok, "{closed:?}");
    assert_eq!(closed.detail, "the \"preview\" tab closed");
    assert_eq!(cdp.tab_name(), "main");
    cdp.call("Runtime.evaluate", json!({ "expression": "1" })).await.unwrap();
    assert_eq!(sent(&cdp).last().unwrap()["sessionId"], MAIN);
}

/// A call in flight when the current tab closes ends with its name too.
#[tokio::test]
async fn a_call_in_flight_when_the_current_tab_closes_ends_with_its_name() {
    let mut cdp = with_named("preview").await;
    cdp.switch_tab("preview").await.unwrap();
    cdp.transport_mut().withhold.push("Runtime.evaluate@S-pop".to_string());
    cdp.transport_mut().after_reply.push(("Runtime.evaluate@S-pop".to_string(), detached("S-pop")));
    let call =
        tokio::time::timeout(Duration::from_secs(2), cdp.call("Runtime.evaluate", json!({ "expression": "1" }))).await;
    assert_eq!(call.expect("the call hung"), Err(CdpError::Tab("there is no tab preview".into())));
}

#[tokio::test]
async fn a_current_tab_that_closes_itself_can_be_left_with_switch_tab() {
    let mut cdp = with_named("preview").await;
    cdp.switch_tab("preview").await.unwrap();
    feed(&mut cdp, [detached("S-pop")]);
    settle(&mut cdp).await;
    let gone = execute(&mut cdp, &Action::SwitchTab { name: "preview".into() }).await;
    assert_eq!(gone.detail, "there is no tab preview");
    assert!(execute(&mut cdp, &Action::SwitchTab { name: "main".into() }).await.ok);
    assert_eq!(cdp.missing_tab(), None);
    cdp.call("Runtime.evaluate", json!({ "expression": "1" })).await.unwrap();
    assert_eq!(sent(&cdp).last().unwrap()["sessionId"], MAIN);
}

#[tokio::test]
async fn expect_tab_closed_waits_for_the_page_to_close_it_and_says_when_it_did_not() {
    let mut cdp = with_named("preview").await;
    let open = cdp.expect_tab_closed("preview", Duration::from_millis(300)).await;
    assert_eq!(tab_failure(open), "the \"preview\" tab did not close within 0.3 seconds");
    feed(&mut cdp, [detached("S-pop")]);
    cdp.expect_tab_closed("preview", Duration::from_millis(500)).await.unwrap();
    // Claimed once: the name is free again, and no such tab is open.
    let again = cdp.expect_tab_closed("preview", Duration::from_millis(100)).await;
    assert_eq!(tab_failure(again), "there is no tab preview");
    assert_eq!(tab_failure(cdp.expect_tab_closed("main", Duration::from_millis(100)).await), "main cannot be closed");
    assert_eq!(
        tab_failure(cdp.expect_tab_closed("nowhere", Duration::from_millis(100)).await),
        "there is no tab nowhere"
    );
    let ten = execute(&mut cdp, &Action::ExpectTabClosed { name: "nowhere".into(), within_ms: None }).await;
    assert_eq!(ten.detail, "there is no tab nowhere");
}

/// `open_tab` makes a blank tab in main's browser context, which is set up
/// (guarded first) before anything is sent there, then sends it to the
/// address. A save from it is stopped like main's.
#[tokio::test]
async fn open_tab_sets_the_new_tab_up_and_guards_it_before_it_goes_anywhere() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    let new_tab = attached("S-new", "T-new", "page", "about:blank", true);
    cdp.transport_mut().after_reply.push(("Target.createTarget".to_string(), new_tab));
    let out = execute(
        &mut cdp,
        &Action::OpenTab { name: "second".into(), url: "https://hr.example/hr/employee/42?token=hunter2".into() },
    )
    .await;
    assert!(out.ok, "{out:?}");
    assert_eq!(out.detail, "opened the \"second\" tab at /hr/employee/42");

    let made = sent(&cdp).iter().find(|f| f["method"] == "Target.createTarget").expect("no tab was made");
    assert!(made.get("sessionId").is_none());
    assert_eq!(made["params"], json!({ "url": "about:blank", "browserContextId": "C-1" }));
    let setup = sent_on(&cdp, "S-new");
    let at = |m: &str| setup.iter().position(|s| s == m).unwrap_or_else(|| panic!("{m} was never sent: {setup:?}"));
    assert!(at("Fetch.enable") < at("Runtime.runIfWaitingForDebugger"), "{setup:?}");
    assert!(at("Runtime.runIfWaitingForDebugger") < at("Page.navigate"), "{setup:?}");
    let went = sent(&cdp).iter().find(|f| f["method"] == "Page.navigate").unwrap();
    assert_eq!(went["sessionId"], "S-new");
    assert_eq!(cdp.tab_name(), "second");

    feed(&mut cdp, [paused("S-new", "r1", "POST", "https://hr.example/api/Save")]);
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.failRequest");
    assert_eq!(cdp.take_save_blocked().as_deref(), Some(SAVE_STOPPED));
}

#[tokio::test]
async fn open_tab_keeps_navigates_rules_and_refusals() {
    let mut cdp = browser().await;
    let policy = Policy::only(vec!["https://hr.example".into()]);
    let out = execute_in(
        &mut cdp,
        &Action::OpenTab { name: "second".into(), url: "https://elsewhere.example/x".into() },
        &Default::default(),
        &policy,
    )
    .await;
    assert!(!out.ok);
    assert_eq!(
        out.detail,
        "https://elsewhere.example is not one of this project's allowed origins - add it to the sign-in recipe if the test really goes there"
    );
    assert!(!sent(&cdp).iter().any(|f| f["method"] == "Target.createTarget"), "a refused address opened a tab");

    let bad = execute(&mut cdp, &Action::OpenTab { name: "second".into(), url: "javascript:alert(1)".into() }).await;
    assert_eq!(bad.detail, "this action cannot run: open_tab needs an http, https or file address, not \"javascript:alert(1)\"");

    assert_eq!(tab_failure(cdp.open_tab("main").await), "there is already a tab main");
}

/// Review Focus 5: a download a named tab started is that tab's; one whose
/// frame no tab shows is the current tab's.
#[tokio::test]
async fn a_download_belongs_to_the_tab_whose_frame_started_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut cdp = with_named("report").await;
    cdp.enable_downloads(dir.path()).await.unwrap();
    feed(
        &mut cdp,
        [
            json!({ "method": "Browser.downloadWillBegin", "params": { "guid": "g-pop", "suggestedFilename": "report.csv", "frameId": "T-pop" } }),
            json!({ "method": "Browser.downloadWillBegin", "params": { "guid": "g-main", "suggestedFilename": "main.csv", "frameId": "T-main" } }),
        ],
    );
    settle(&mut cdp).await;
    let names = |d: Vec<v2_lib::browser::downloads::DownloadEntry>| d.into_iter().map(|e| e.name).collect::<Vec<_>>();
    assert_eq!(names(cdp.downloads()), ["main.csv"], "the report tab's download was taken for main's");
    cdp.switch_tab("report").await.unwrap();
    assert_eq!(names(cdp.downloads()), ["report.csv"]);
    assert_eq!(names(cdp.all_downloads()), ["report.csv", "main.csv"]);
    // A frame no tab is known to show: the current tab's.
    feed(
        &mut cdp,
        [json!({ "method": "Browser.downloadWillBegin", "params": { "guid": "g-x", "suggestedFilename": "x.csv", "frameId": "F-unknown" } })],
    );
    settle(&mut cdp).await;
    assert_eq!(names(cdp.downloads()), ["report.csv", "x.csv"]);
}

/// Review Focus 5: the page log and the network record a step reads are
/// the current tab's, and a mark taken in `main` still dates a request in
/// the tab the step moved to.
#[tokio::test]
async fn the_page_log_and_the_network_record_follow_the_current_tab() {
    let mut cdp = with_named("report").await;
    let request = |s: &str, id: &str, path: &str| {
        on(
            s,
            "Network.requestWillBeSent",
            json!({ "requestId": id, "timestamp": 1.0, "request": { "method": "GET", "url": format!("https://hr.example{path}") } }),
        )
    };
    feed(&mut cdp, [request("S-pop", "n0", "/api/before")]);
    settle(&mut cdp).await;
    let mark = cdp.net_mark();
    feed(
        &mut cdp,
        [
            request("S-pop", "n1", "/api/after"),
            on("S-pop", "Network.loadingFailed", json!({ "requestId": "n1", "timestamp": 2.0, "errorText": "net::ERR_FAILED" })),
        ],
    );
    settle(&mut cdp).await;
    assert!(cdp.page_log().is_empty(), "the report tab's failure reached main's log");
    cdp.switch_tab("report").await.unwrap();
    assert!(cdp.page_log().iter().any(|l| l.contains("/api/after")), "{:?}", cdp.page_log());
    let since: Vec<String> = cdp.net_since(mark).into_iter().map(|e| e.path_query).collect();
    assert_eq!(since.len(), 1, "{since:?}");
    assert!(since[0].contains("/api/after"), "{since:?}");
}

/// Review Focus 4: the end of a case closes every tab but `main`, named or
/// not, and starts the next case's tabs afresh.
#[tokio::test]
async fn the_end_of_a_case_closes_every_tab_but_main() {
    let mut cdp = with_named("report").await;
    popup(&mut cdp, "S-other", "T-other", "https://hr.example/other").await;
    cdp.switch_tab("report").await.unwrap();
    cdp.close_other_tabs().await;
    let closed: Vec<&Value> = sent(&cdp).iter().filter(|f| f["method"] == "Target.closeTarget").collect();
    assert_eq!(closed.len(), 2, "{closed:?}");
    assert_eq!(cdp.tabs().len(), 1);
    assert_eq!(cdp.tab_name(), "main");
    cdp.call("Runtime.evaluate", json!({ "expression": "1" })).await.unwrap();
    assert_eq!(sent(&cdp).last().unwrap()["sessionId"], MAIN);
    // The next case claims nothing from this one.
    let none = cdp.expect_tab("report", None, Duration::from_millis(200)).await;
    assert_eq!(tab_failure(none), "no new tab opened within 0.2 seconds");
}

#[tokio::test]
async fn a_tab_no_step_expects_is_logged_and_left_open() {
    let _log = crate::serial::log_tail();
    let mut cdp = browser().await;
    cdp.step_began();
    popup(&mut cdp, "S-ad", "T-ad", "https://hr.example/whats-new?campaign=x").await;
    let lines: Vec<String> = v2_lib::applog::recent(200).into_iter().map(|l| l.message).collect();
    assert!(lines.iter().any(|l| l == "a tab opened: https://hr.example/whats-new"), "{lines:?}");
    let out = execute(&mut cdp, &Action::SwitchTab { name: "main".into() }).await;
    assert!(out.ok);
    assert_eq!(cdp.tabs().len(), 2, "the tab was closed");
    assert_eq!(cdp.take_save_blocked(), None);
}

// ---------------------------------------------------------------------------
// What a script may say about tabs, before any browser is involved.
// ---------------------------------------------------------------------------

fn tab_action(v: Value) -> Action {
    serde_json::from_value(v).expect("an action")
}

#[test]
fn the_tab_actions_read_from_a_script_validate_and_write_back_as_they_were() {
    for v in [
        json!({ "kind": "expect_tab", "name": "report" }),
        json!({ "kind": "expect_tab", "name": "report-2_b", "url_contains": "/reports/", "within_ms": 20000 }),
        json!({ "kind": "open_tab", "name": "second", "url": "/hr/employee/42" }),
        json!({ "kind": "open_tab", "name": "second", "url": "https://hr.example/hr/home" }),
        json!({ "kind": "switch_tab", "name": "main" }),
        json!({ "kind": "close_tab", "name": "report" }),
        json!({ "kind": "expect_tab_closed", "name": "preview" }),
        json!({ "kind": "expect_tab_closed", "name": "preview", "within_ms": 5000 }),
    ] {
        let a = tab_action(v.clone());
        assert!(a.validate().is_ok(), "{v}: {:?}", a.validate());
        assert_eq!(serde_json::to_value(&a).unwrap(), v);
    }
}

#[test]
fn a_tab_name_is_1_to_30_letters_digits_dashes_or_underscores() {
    let rule = |kind: &str, name: &str| {
        tab_action(json!({ "kind": kind, "name": name, "url": "/x" })).validate().unwrap_err()
    };
    for kind in ["expect_tab", "open_tab", "switch_tab", "close_tab", "expect_tab_closed"] {
        let long = "x".repeat(31);
        for bad in ["", "has space", "dot.ted", "ünï", long.as_str()] {
            let why = rule(kind, bad);
            assert!(why.starts_with(&format!("{kind}: a tab name is 1 to 30 letters, digits, - or _")), "{why}");
        }
        let ok = tab_action(json!({ "kind": kind, "name": "a".repeat(30), "url": "/x" })).validate();
        assert!(ok.is_ok(), "{kind}: {ok:?}");
    }
}

#[test]
fn main_cannot_be_closed_or_named_again() {
    let why = |v: Value| tab_action(v).validate().unwrap_err();
    assert_eq!(why(json!({ "kind": "close_tab", "name": "main" })), "main cannot be closed");
    assert_eq!(why(json!({ "kind": "expect_tab_closed", "name": "main" })), "main cannot be closed");
    assert_eq!(why(json!({ "kind": "expect_tab", "name": "main" })), "there is already a tab main");
    assert_eq!(why(json!({ "kind": "open_tab", "name": "main", "url": "/x" })), "there is already a tab main");
}

#[test]
fn the_tab_waits_are_bounded_and_open_tab_takes_only_an_address() {
    let why = |v: Value| tab_action(v).validate().unwrap_err();
    assert_eq!(why(json!({ "kind": "expect_tab", "name": "r", "within_ms": 0 })), "within_ms must be more than 0");
    assert_eq!(
        why(json!({ "kind": "expect_tab_closed", "name": "r", "within_ms": 60001 })),
        "expect_tab_closed waits at most 60000 ms, not 60001"
    );
    assert!(why(json!({ "kind": "expect_tab", "name": "r", "url_contains": " " })).contains("empty url_contains"));
    assert_eq!(
        why(json!({ "kind": "open_tab", "name": "r", "url": "data:text/html,x" })),
        "open_tab needs an http, https or file address, not \"data:text/html,x\""
    );
}

#[test]
fn expect_tab_and_expect_tab_closed_are_checks_and_the_rest_are_actions() {
    assert!(tab_action(json!({ "kind": "expect_tab", "name": "r" })).is_check());
    assert!(tab_action(json!({ "kind": "expect_tab_closed", "name": "r" })).is_check());
    for v in [
        json!({ "kind": "open_tab", "name": "r", "url": "/x" }),
        json!({ "kind": "switch_tab", "name": "r" }),
        json!({ "kind": "close_tab", "name": "r" }),
    ] {
        assert!(!tab_action(v.clone()).is_check(), "{v}");
    }
}

#[test]
fn a_guard_never_moves_between_tabs_and_a_recipe_holds_no_tab_action() {
    let guarded = |inner: Value| {
        tab_action(json!({ "kind": "when_visible", "selector": { "css": "#banner" }, "then": [inner] })).validate()
    };
    for inner in [
        json!({ "kind": "open_tab", "name": "r", "url": "/x" }),
        json!({ "kind": "switch_tab", "name": "r" }),
        json!({ "kind": "close_tab", "name": "r" }),
        json!({ "kind": "expect_tab", "name": "r" }),
        json!({ "kind": "expect_tab_closed", "name": "r" }),
    ] {
        let why = guarded(inner.clone()).unwrap_err();
        assert!(why.contains(inner["kind"].as_str().unwrap()), "{why}");
    }
    for k in ["expect_tab", "open_tab", "switch_tab", "close_tab", "expect_tab_closed"] {
        let r: Result<v2_lib::autorun::recipe::SignInRecipe, _> = serde_json::from_value(json!({
            "start_url": "https://hr.example.internal/login",
            "steps": [ { "kind": k, "name": "r", "url": "/x" } ],
            "signed_in": { "css": "#marker" }
        }));
        let refused = match r {
            Err(e) => e.to_string(),
            Ok(recipe) => recipe.validate().unwrap_err(),
        };
        assert!(refused.contains(&format!("a sign-in recipe cannot contain {k} - it belongs in a case script")), "{refused}");
    }
}

#[test]
fn a_tab_action_is_said_without_a_host_or_a_query() {
    use v2_lib::autorun::report::action_words;
    let open = tab_action(json!({ "kind": "open_tab", "name": "second", "url": "https://hr.example/hr/e/42?token=hunter2" }));
    assert_eq!(action_words(&open), "open a new tab \"second\" at /hr/e/42");
    assert_eq!(action_words(&tab_action(json!({ "kind": "expect_tab", "name": "r" }))), "wait for a new tab and call it \"r\"");
    assert_eq!(action_words(&tab_action(json!({ "kind": "switch_tab", "name": "r" }))), "switch to the \"r\" tab");
    assert_eq!(action_words(&tab_action(json!({ "kind": "close_tab", "name": "r" }))), "close the \"r\" tab");
    assert_eq!(action_words(&tab_action(json!({ "kind": "expect_tab_closed", "name": "r" }))), "check the \"r\" tab closes");
    let tried = v2_lib::ai_bridge::describe_try(&open, true);
    assert_eq!(tried, "AI tried open_tab second /hr/e/42 in the supervised browser: ok");
    assert_eq!(
        v2_lib::autorun::patterns::action_target(&tab_action(json!({ "kind": "switch_tab", "name": "r" }))).as_deref(),
        Some("the \"r\" tab")
    );
}

// ---------------------------------------------------------------------------
// Fix round 1.
// ---------------------------------------------------------------------------

/// A tab `open_tab` made whose guard was refused is never sent anywhere: it
/// is closed again and the step fails with the sentence, at once.
#[tokio::test]
async fn open_tab_closes_a_tab_it_could_not_guard_and_fails_with_the_sentence() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    cdp.transport_mut().refuse.push("Fetch.enable@S-new".to_string());
    let new_tab = attached("S-new", "T-new", "page", "about:blank", true);
    cdp.transport_mut().after_reply.push(("Target.createTarget".to_string(), new_tab));
    let out = tokio::time::timeout(
        Duration::from_secs(5),
        execute(&mut cdp, &Action::OpenTab { name: "second".into(), url: "https://hr.example/hr/home".into() }),
    )
    .await
    .expect("open_tab waited on a tab it could not guard");
    assert!(!out.ok && !out.harness, "{out:?}");
    assert_eq!(out.detail, TAB_HELD_UNGUARDED);
    assert!(!sent(&cdp).iter().any(|f| f["method"] == "Page.navigate"), "the unguarded tab was sent somewhere");
    assert!(!sent_on(&cdp, "S-new").iter().any(|m| m == "Runtime.runIfWaitingForDebugger"), "it was let run");
    let closed = sent(&cdp).iter().find(|f| f["method"] == "Target.closeTarget").expect("it was left open");
    assert_eq!(closed["params"]["targetId"], "T-new");
    assert_eq!(cdp.tabs().len(), 1);
    assert_eq!(cdp.tab_name(), "main");
}

/// An accepted guard: `open_tab` hands the tab back only once its
/// `Fetch.enable` was answered, even for a tab the browser did not pause.
#[tokio::test]
async fn open_tab_waits_for_the_guards_answer_before_the_tab_goes_anywhere() {
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    cdp.transport_mut().withhold.push("Fetch.enable@S-new".to_string());
    let new_tab = attached("S-new", "T-new", "page", "about:blank", false);
    cdp.transport_mut().after_reply.push(("Target.createTarget".to_string(), new_tab));
    let early = tokio::time::timeout(Duration::from_millis(500), cdp.open_tab("second")).await;
    assert!(early.is_err(), "open_tab came back before the guard was answered: {early:?}");

    // Answered: the next open goes through, guard first, then the address.
    let mut cdp = browser().await;
    cdp.guard_saves(&[]).await.unwrap();
    let new_tab = attached("S-new", "T-new", "page", "about:blank", false);
    cdp.transport_mut().after_reply.push(("Target.createTarget".to_string(), new_tab));
    let out = execute(&mut cdp, &Action::OpenTab { name: "second".into(), url: "https://hr.example/hr/home".into() }).await;
    assert!(out.ok, "{out:?}");
    let on_new = sent_on(&cdp, "S-new");
    let at = |m: &str| on_new.iter().position(|s| s == m).unwrap_or_else(|| panic!("{m} was never sent: {on_new:?}"));
    assert!(at("Fetch.enable") < at("Page.navigate"), "{on_new:?}");
}

/// A tab whose close is not answered in time is still let go, and the
/// tabs after it are still closed.
#[tokio::test]
async fn a_close_that_times_out_does_not_stop_the_other_tabs_closing() {
    let mut cdp = with_named("report").await;
    popup(&mut cdp, "S-other", "T-other", "https://hr.example/other").await;
    cdp.transport_mut().withhold_first.push("Target.closeTarget".to_string());
    cdp.close_other_tabs().await;
    let closed: Vec<&Value> = sent(&cdp).iter().filter(|f| f["method"] == "Target.closeTarget").collect();
    assert_eq!(closed.len(), 2, "{closed:?}");
    assert_eq!(closed[1]["params"]["targetId"], "T-other");
    assert_eq!(cdp.tabs().len(), 1);
    assert_eq!(cdp.tab_name(), "main");
}

// ---------------------------------------------------------------------------
// A sign-in in one tab holds only that tab.
// ---------------------------------------------------------------------------

/// A guarded browser with `main` on its draft (loader L-main) and a second
/// tab, `second`, on the sign-in page (L-signin). The sign-in runs in
/// `second`: it is current when the hold is asked for, and the hold has
/// taken effect.
async fn signing_in_in_second() -> Cdp<FakeBrowser> {
    let mut cdp = browser().await;
    feed(&mut cdp, [navigated(MAIN, "L-main", "https://hr.example/draft")]);
    settle(&mut cdp).await;
    cdp.step_began();
    popup(&mut cdp, "S-pop", "T-pop", "about:blank").await;
    cdp.expect_tab("second", None, Duration::from_millis(500)).await.unwrap();
    feed(
        &mut cdp,
        [on("S-pop", "Page.frameNavigated", json!({ "frame": { "id": "T-pop", "loaderId": "L-signin", "url": "https://hr.example/login" } }))],
    );
    settle(&mut cdp).await;
    cdp.guard_saves(&["login".to_string()]).await.unwrap();
    cdp.switch_tab("second").await.unwrap();
    cdp.hold_saves(true);
    cdp.call("Runtime.evaluate", json!({})).await.unwrap();
    cdp
}

/// `main`'s own draft saving while the sign-in runs in `second` is still
/// stopped: the hold is `second`'s alone.
#[tokio::test]
async fn a_save_from_another_tabs_page_is_stopped_during_a_sign_in() {
    let mut cdp = signing_in_in_second().await;
    feed(
        &mut cdp,
        [
            sent_by(MAIN, "n1", "L-main", "https://hr.example/api/Save"),
            paused_from(MAIN, "r1", "https://hr.example/api/Save", "XHR", "n1"),
        ],
    );
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.failRequest");
    assert!(cdp.take_save_blocked().is_some_and(|b| b.contains("POST /api/Save")));
}

/// A form `main` submits (a top-level navigation) during a sign-in in
/// `second` is still stopped.
#[tokio::test]
async fn a_form_another_tab_submits_is_stopped_during_a_sign_in() {
    let mut cdp = signing_in_in_second().await;
    feed(&mut cdp, [paused_from(MAIN, "r1", "https://hr.example/api/Save", "Document", "n9")]);
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.failRequest");
}

/// The sign-in's own login, in `second`, goes through.
#[tokio::test]
async fn the_sign_in_tabs_own_login_goes_through() {
    let mut cdp = signing_in_in_second().await;
    feed(
        &mut cdp,
        [
            sent_by("S-pop", "n1", "L-signin", "https://hr.example/api/login"),
            paused_from("S-pop", "r1", "https://hr.example/api/login", "XHR", "n1"),
        ],
    );
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r1").unwrap()["method"], "Fetch.continueRequest");
    assert_eq!(cdp.take_save_blocked(), None);
    // Once the hold ends, `second` is stopped like any tab.
    cdp.hold_saves(false);
    feed(&mut cdp, [paused("S-pop", "r2", "POST", "https://hr.example/api/Save")]);
    settle(&mut cdp).await;
    assert_eq!(answer_to(&cdp, "r2").unwrap()["method"], "Fetch.failRequest");
}
