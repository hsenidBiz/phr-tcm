//! The routine `/tools` poll no longer fills the log: a successful, unchanged
//! answer is silent; a failure, a changed answer and every other route log.

use v2_lib::ai_bridge::{route, BridgeContext};

fn ctx(disabled: &[&str]) -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        disabled_tools: disabled.iter().map(|s| s.to_string()).collect(),
        ..BridgeContext::default()
    }
}

fn tools_lines() -> Vec<String> {
    v2_lib::applog::recent(2000)
        .into_iter()
        .map(|l| l.message)
        .filter(|m| m.starts_with("Bridge call: GET /tools "))
        .collect()
}

async fn poll(c: &BridgeContext) {
    let (status, _) = route(c, None, "GET", "/tools", "", "1.0.0").await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn identical_tools_answers_are_silent_a_changed_one_logs_once() {
    let _log = crate::serial::log_tail();
    let a = ctx(&[]);
    poll(&a).await; // may be the first answer, or a change from an earlier test
    let before = tools_lines().len();
    poll(&a).await;
    poll(&a).await;
    assert_eq!(tools_lines().len(), before, "two identical answers wrote a line");

    poll(&ctx(&["get_guide"])).await;
    let after = tools_lines();
    assert_eq!(after.len(), before + 1, "{after:?}");
    let line = after.last().unwrap();
    assert_eq!(line, "Bridge call: GET /tools answered 200 (the tool list changed)");
    assert!(!line.contains("get_guide"), "the payload must never be logged");
}

#[test]
fn a_failed_tools_call_still_logs() {
    let line = v2_lib::ai_bridge::call_log_line("GET", "/tools", 500, "x", 3).expect("a failure is logged");
    assert_eq!(line, "Bridge call: GET /tools answered 500 in 3 ms");
}

#[tokio::test]
async fn another_route_still_logs_its_line() {
    let _log = crate::serial::log_tail();
    route(&ctx(&[]), None, "GET", "/ping", "", "1.0.0").await;
    assert!(v2_lib::applog::recent(500).into_iter().any(|l| l.message.starts_with("Bridge call: GET /ping answered 200 in ")));
}
