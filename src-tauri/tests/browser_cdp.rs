//! The DevTools wire format. Only two things need to be right: the
//! frame we send, and telling OUR reply apart from the flood of events
//! the browser volunteers.

use v2_lib::browser::cdp::{frame, reply_for};

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
