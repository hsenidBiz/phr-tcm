//! The DevTools wire format. Only two things need to be right: the
//! frame we send, and telling OUR reply apart from the flood of events
//! the browser volunteers.

use std::collections::VecDeque;
use std::time::{Duration, Instant};
use v2_lib::browser::cdp::{
    event_of, frame, reply_for, Cdp, CdpError, Transport, CALL_TIMEOUT, MIN_CALL_TIMEOUT,
};

#[test]
fn a_frame_carries_its_id_method_and_params() {
    let f = frame(7, "Runtime.evaluate", serde_json::json!({ "expression": "1+1" }));
    let v: serde_json::Value = serde_json::from_str(&f).unwrap();
    assert_eq!(v["id"], 7);
    assert_eq!(v["method"], "Runtime.evaluate");
    assert_eq!(v["params"]["expression"], "1+1");
}

/// The socket also carries EVENTS - frames with no id at all. Treating
/// one as an answer would hang the caller on the next reply, or worse,
/// return another request's result.
#[test]
fn events_and_other_ids_are_not_mistaken_for_our_reply() {
    assert!(reply_for(7, r#"{"method":"Page.loadEventFired","params":{}}"#).is_none());
    assert!(reply_for(7, r#"{"id":8,"result":{"x":1}}"#).is_none());
    assert!(reply_for(7, "not json at all").is_none());
}

#[test]
fn our_reply_returns_its_result() {
    let got = reply_for(7, r#"{"id":7,"result":{"result":{"value":42}}}"#).unwrap().unwrap();
    assert_eq!(got["result"]["value"], 42);
}

/// A protocol error must surface as an error, not as an empty success -
/// a silently-empty result reads as "the check found nothing", which is
/// a false negative in a test runner.
#[test]
fn a_protocol_error_is_an_error_with_its_message() {
    let got = reply_for(7, r#"{"id":7,"error":{"code":-32000,"message":"No node found"}}"#)
        .unwrap()
        .unwrap_err();
    assert!(got.contains("No node found"), "got: {got}");
}

/// Stands in for the socket: hands back canned frames in order and keeps
/// what was sent. When it runs dry it either reports the socket closed or,
/// for the timeout test, never answers at all.
struct FakeTransport {
    incoming: VecDeque<String>,
    sent: Vec<String>,
    hang_when_empty: bool,
}

impl FakeTransport {
    fn new(frames: &[&str]) -> Self {
        FakeTransport {
            incoming: frames.iter().map(|f| f.to_string()).collect(),
            sent: vec![],
            hang_when_empty: false,
        }
    }
    fn hanging(frames: &[&str]) -> Self {
        FakeTransport { hang_when_empty: true, ..FakeTransport::new(frames) }
    }
}

impl Transport for FakeTransport {
    async fn send(&mut self, text: String) -> Result<(), String> {
        self.sent.push(text);
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, String>> {
        match self.incoming.pop_front() {
            Some(f) => Some(Ok(f)),
            None if self.hang_when_empty => {
                std::future::pending::<()>().await;
                None
            }
            None => None,
        }
    }
}

#[test]
fn an_event_is_a_frame_with_a_method_and_no_id() {
    let ev = event_of(r#"{"method":"Page.loadEventFired","params":{"timestamp":1}}"#).unwrap();
    assert_eq!(ev.method, "Page.loadEventFired");
    assert_eq!(ev.params["timestamp"], 1);
    assert!(event_of(r#"{"id":3,"result":{}}"#).is_none());
    assert!(event_of("not json").is_none());
}

/// Events that arrive while a call is waiting are kept, not thrown away:
/// the load event for a navigation usually lands before anyone asks for it.
#[tokio::test]
async fn a_call_skips_events_and_keeps_them_for_later() {
    let t = FakeTransport::new(&[
        r#"{"method":"Page.loadEventFired","params":{"timestamp":1}}"#,
        r#"{"id":1,"result":{"ok":true}}"#,
    ]);
    let mut cdp = Cdp::over(t);
    let got = cdp.call("Page.enable", serde_json::json!({})).await.unwrap();
    assert_eq!(got["ok"], true);
    let ev = cdp.wait_event("Page.loadEventFired", Duration::from_millis(50)).await.unwrap();
    assert_eq!(ev.params["timestamp"], 1);
}

#[tokio::test]
async fn wait_event_reads_the_socket_when_nothing_is_buffered() {
    let t = FakeTransport::new(&[
        r#"{"method":"Network.dataReceived","params":{}}"#,
        r#"{"method":"Page.loadEventFired","params":{"timestamp":2}}"#,
    ]);
    let mut cdp = Cdp::over(t);
    let ev = cdp.wait_event("Page.loadEventFired", Duration::from_millis(200)).await.unwrap();
    assert_eq!(ev.params["timestamp"], 2);
}

/// The old client waited forever. A browser that has stopped answering is
/// a harness failure with a name and a number, not a frozen screen.
#[tokio::test]
async fn a_call_that_is_never_answered_times_out_and_says_what_it_was() {
    let mut cdp = Cdp::over(FakeTransport::hanging(&[]));
    let err = cdp
        .call_within("Runtime.evaluate", serde_json::json!({}), Duration::from_millis(50))
        .await
        .unwrap_err();
    assert_eq!(err, CdpError::Timeout { what: "Runtime.evaluate".into(), ms: 50 });
    assert!(err.to_string().contains("Runtime.evaluate"), "{err}");
    assert!(!err.is_transient());
}

/// A wait loop's budget is pushed down into the client, because checking
/// the deadline only BETWEEN calls leaves each call capped at the full
/// 30s `CALL_TIMEOUT`: a browser that accepts frames and stops answering
/// turns a 15s click into 30s and a ten-action step into minutes, all
/// while the session is locked, so "Close browser" cannot act.
#[tokio::test]
async fn a_deadline_shortens_a_call_without_starving_it() {
    let mut cdp = Cdp::over(FakeTransport::hanging(&[]));
    cdp.set_deadline(Some(Instant::now() + Duration::from_millis(100)));
    let started = Instant::now();
    let err = cdp.call("Runtime.evaluate", serde_json::json!({})).await.unwrap_err();
    match err {
        CdpError::Timeout { ms, .. } => assert!(ms <= 250, "capped at the deadline, not 30s: {ms}"),
        other => panic!("expected a timeout, got {other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(1), "it waited {:?}", started.elapsed());

    // The floor: a call made right at the edge of a budget still gets a
    // moment to answer rather than being cancelled before it can.
    cdp.set_deadline(Some(Instant::now() - Duration::from_secs(5)));
    let err = cdp.call("Runtime.evaluate", serde_json::json!({})).await.unwrap_err();
    assert_eq!(
        err,
        CdpError::Timeout {
            what: "Runtime.evaluate".into(),
            ms: MIN_CALL_TIMEOUT.as_millis() as u64
        }
    );
}

/// And with no deadline a call is back to the full `CALL_TIMEOUT` - the
/// floor is a cap on a shortened call, never a new limit of its own.
#[tokio::test]
async fn clearing_the_deadline_restores_the_full_call_timeout() {
    assert_eq!(CALL_TIMEOUT, Duration::from_secs(30));
    let mut cdp = Cdp::over(FakeTransport::hanging(&[]));
    cdp.set_deadline(Some(Instant::now() - Duration::from_secs(5)));
    cdp.set_deadline(None);
    // Observed without waiting 30s: the call is still pending long after
    // the 250ms floor a stale deadline would have imposed.
    let pending = tokio::time::timeout(
        Duration::from_millis(400),
        cdp.call("Runtime.evaluate", serde_json::json!({})),
    )
    .await;
    assert!(pending.is_err(), "the call gave up early: {pending:?}");
}

#[tokio::test]
async fn a_closed_socket_is_its_own_error() {
    let mut cdp = Cdp::over(FakeTransport::new(&[]));
    let err = cdp.call("Page.enable", serde_json::json!({})).await.unwrap_err();
    assert_eq!(err, CdpError::Closed);
    assert!(err.to_string().contains("browser"), "{err}");
}

/// A refusal names the method. It is also the one kind worth retrying: a
/// page between two documents refuses calls for a moment.
#[tokio::test]
async fn a_protocol_error_names_the_method_and_is_transient() {
    let t = FakeTransport::new(&[r#"{"id":1,"error":{"code":-32000,"message":"No node found"}}"#]);
    let mut cdp = Cdp::over(t);
    let err = cdp.call("DOM.resolveNode", serde_json::json!({})).await.unwrap_err();
    assert_eq!(
        err,
        CdpError::Protocol { method: "DOM.resolveNode".into(), message: "No node found".into() }
    );
    assert!(err.is_transient());
}

/// Measured on real Edge: an alert() leaves every later call pending until
/// the dialog is handled. The client accepts it at once and remembers what
/// it said, so the run carries on and the person is told.
#[tokio::test]
async fn a_javascript_dialog_is_accepted_and_remembered() {
    let t = FakeTransport::new(&[
        r#"{"method":"Page.javascriptDialogOpening","params":{"type":"alert","message":"Saved!"}}"#,
        r#"{"id":1,"result":{"done":true}}"#,
    ]);
    let mut cdp = Cdp::over(t);
    let got = cdp.call("Runtime.evaluate", serde_json::json!({})).await.unwrap();
    assert_eq!(got["done"], true);

    let sent: Vec<serde_json::Value> = cdp
        .transport()
        .sent
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let handled = sent
        .iter()
        .find(|f| f["method"] == "Page.handleJavaScriptDialog")
        .expect("the dialog was never handled");
    assert_eq!(handled["params"]["accept"], true);

    assert_eq!(cdp.take_dialogs(), vec!["alert: Saved!".to_string()]);
    assert!(cdp.take_dialogs().is_empty(), "taking the dialogs must empty the list");
}

/// A page stuck in an alert loop must not grow the dialog list forever:
/// only the most recent dialogs are worth reporting, and every one of them
/// is still accepted regardless of the cap.
#[tokio::test]
async fn the_dialog_list_is_capped_to_the_most_recent() {
    let mut frames: Vec<String> = (0..25)
        .map(|i| {
            format!(
                r#"{{"method":"Page.javascriptDialogOpening","params":{{"type":"alert","message":"m{i}"}}}}"#
            )
        })
        .collect();
    frames.push(r#"{"id":1,"result":{"done":true}}"#.to_string());
    let refs: Vec<&str> = frames.iter().map(|s| s.as_str()).collect();
    let t = FakeTransport::new(&refs);
    let mut cdp = Cdp::over(t);
    let got = cdp.call("Runtime.evaluate", serde_json::json!({})).await.unwrap();
    assert_eq!(got["done"], true);

    let sent: Vec<serde_json::Value> = cdp
        .transport()
        .sent
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let handled_count = sent.iter().filter(|f| f["method"] == "Page.handleJavaScriptDialog").count();
    assert_eq!(handled_count, 25, "every dialog must still be accepted, cap or no cap");

    let dialogs = cdp.take_dialogs();
    assert_eq!(dialogs.len(), 20);
    assert_eq!(dialogs.first().unwrap(), "alert: m5");
    assert_eq!(dialogs.last().unwrap(), "alert: m24");
}

#[tokio::test]
async fn forgetting_events_drops_what_was_buffered() {
    let t = FakeTransport::new(&[
        r#"{"method":"Page.loadEventFired","params":{}}"#,
        r#"{"id":1,"result":{}}"#,
    ]);
    let mut cdp = Cdp::over(t);
    cdp.call("Page.enable", serde_json::json!({})).await.unwrap();
    cdp.forget_events();
    let err = cdp.wait_event("Page.loadEventFired", Duration::from_millis(20)).await.unwrap_err();
    assert_eq!(err, CdpError::Closed, "the fake socket is empty, so this reads as closed");
}

#[tokio::test]
#[ignore = "starts a real Edge window"]
async fn eval_round_trips_against_a_real_browser() {
    let mut b = v2_lib::browser::launch::launch().unwrap();
    // Give the browser a moment to bind its port.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let mut cdp = v2_lib::browser::cdp::Cdp::connect(b.port).await.unwrap();
    let v = cdp.eval("1 + 1").await.unwrap();
    assert_eq!(v["result"]["value"], 2);
    let _ = b.child.kill();
}
