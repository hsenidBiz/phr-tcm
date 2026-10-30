//! The PBI selected in Test Case Manager's top bar reaches the assistants:
//! the webview pushes it with the bridge context, `get_current_pbi` reads it,
//! and the starting commands ask the person to confirm it before using it.

use std::collections::BTreeSet;
use std::sync::Mutex;
use v2_lib::ai_bridge::{route, BridgeContext, CurrentPbi};
use v2_lib::ai_tools::COMMANDS;
use v2_lib::commands::ai_bridge::{push_context_with, BridgeSwitches};

const CONFIRM: &str = "is selected in TCM - is that the one?";

fn pbi() -> CurrentPbi {
    CurrentPbi { id: 4321, title: "Leave balance carry-over".into(), project: "HRM".into() }
}

#[test]
fn the_switches_carry_the_pbi_as_the_webview_sends_it() {
    let with: BridgeSwitches = serde_json::from_str(
        r#"{"dbWrites":false,"apiWrites":false,"pbi":{"id":4321,"title":"Leave balance carry-over","project":"HRM"}}"#,
    )
    .unwrap();
    assert_eq!(with.pbi, Some(pbi()));
    let cleared: BridgeSwitches = serde_json::from_str(r#"{"dbWrites":false,"apiWrites":false,"pbi":null}"#).unwrap();
    assert_eq!(cleared.pbi, None);
    let older: BridgeSwitches = serde_json::from_str(r#"{"dbWrites":false,"apiWrites":false}"#).unwrap();
    assert_eq!(older.pbi, None, "a push that names no PBI leaves none");
}

#[test]
fn a_push_sets_the_pbi_and_a_later_push_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let slot = Mutex::new(BridgeContext::default());
    let next = BridgeContext { org: "acme".into(), current_pbi: Some(pbi()), ..BridgeContext::default() };
    push_context_with(&slot, Some(dir.path()), next, "");
    assert_eq!(slot.lock().unwrap().current_pbi, Some(pbi()));
    push_context_with(&slot, Some(dir.path()), BridgeContext { org: "acme".into(), ..BridgeContext::default() }, "");
    assert_eq!(slot.lock().unwrap().current_pbi, None);
}

#[tokio::test]
async fn get_current_pbi_answers_the_selected_pbi() {
    let ctx = BridgeContext { current_pbi: Some(pbi()), ..BridgeContext::default() };
    let (status, body) = route(&ctx, None, "GET", "/current-pbi", "", "1.0.0").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["id"], 4321);
    assert_eq!(v["title"], "Leave balance carry-over");
    assert_eq!(v["project"], "HRM");
}

#[tokio::test]
async fn get_current_pbi_says_plainly_when_none_is_selected() {
    let (status, body) = route(&BridgeContext::default(), None, "GET", "/current-pbi", "", "1.0.0").await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("No PBI is selected in TCM"), "{body}");
    assert!(serde_json::from_str::<serde_json::Value>(&body).is_err(), "a sentence, not JSON: {body}");
}

#[test]
fn the_tool_is_core_and_no_module_state_refuses_its_route() {
    assert!(v2_lib::ai_tools::CORE_TOOLS.contains(&"get_current_pbi"));
    assert_eq!(v2_lib::ai_tools::module_of_tool("get_current_pbi"), None);
    assert!(v2_lib::ai_bridge::module_guard("/current-pbi", "", &BTreeSet::new()).is_none());
}

#[test]
fn the_tool_is_listed_and_calls_the_bridge_read_only() {
    let seen = std::cell::RefCell::new(vec![]);
    let call = |m: &str, p: &str, _b: &str| {
        seen.borrow_mut().push((m.to_string(), p.to_string()));
        Ok((200, "No PBI is selected in TCM.".to_string()))
    };
    let call = &crate::common::dev_app(&call);
    let list = v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&list).unwrap();
    let tool = v["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "get_current_pbi").cloned();
    assert!(tool.is_some(), "get_current_pbi is listed");
    let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_current_pbi","arguments":{}}}"#;
    let resp = v2_lib::mcp::handle_message(req, "1.0.0", call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["content"][0]["text"], "No PBI is selected in TCM.");
    assert!(seen.borrow().contains(&("GET".to_string(), "/current-pbi".to_string())));
}

#[test]
fn the_starting_commands_check_tcms_pbi_and_wait_for_the_persons_yes() {
    for stem in ["gen-tests", "discover", "begin-test-case-writing"] {
        let text = COMMANDS.iter().find(|c| c.stem == stem).unwrap().body.join("\n");
        assert!(text.contains("get_current_pbi"), "{stem}: {text}");
        assert!(text.contains(CONFIRM), "{stem} names the selected PBI and asks: {text}");
        assert!(text.contains("wait for") && text.contains("yes"), "{stem} waits for the yes: {text}");
        assert!(text.contains("search_pbis"), "{stem} still finds a PBI when TCM has none: {text}");
        assert!(!text.contains("as before"), "{stem} names the concrete step: {text}");
    }
}

fn body_of(stem: &str) -> String {
    COMMANDS.iter().find(|c| c.stem == stem).unwrap().body.join("
")
}

#[test]
fn explore_never_asks_about_a_pbi() {
    let text = body_of("explore");
    assert!(!text.contains("get_current_pbi") && !text.contains("PBI"), "{text}");
}

#[test]
fn discover_lets_a_named_case_settle_it() {
    let text = body_of("discover");
    assert!(text.contains("case id named") && text.contains("ask nothing"), "{text}");
    assert!(text.contains("neither a case nor a PBI"), "{text}");
}

#[test]
fn setup_offers_tcms_pbi_first_and_gen_tests_has_one_rule() {
    let setup = body_of("setup");
    assert!(setup.contains("get_current_pbi") && setup.contains(CONFIRM), "{setup}");
    assert!(!body_of("gen-tests").contains("Ask for the"), "one rule for a missing PBI");
}

#[test]
fn only_the_coordinator_asks_and_subagents_use_the_pbi_they_were_given() {
    let begin = body_of("begin-test-case-writing");
    assert!(begin.contains("slice subagent") && begin.contains("never asks"), "{begin}");
    assert!(begin.contains("specification alone"), "a job with no PBI is not forced to search: {begin}");
    let guide = v2_lib::autorun::guide::autorun_guide();
    assert!(guide.contains("a subagent never asks about the PBI"), "{guide}");
}

#[test]
fn clearing_takes_the_pbi_off_the_context_and_debug_shows_it() {
    let slot = Mutex::new(BridgeContext { current_pbi: Some(pbi()), ..BridgeContext::default() });
    assert!(format!("{:?}", slot.lock().unwrap()).contains("4321"));
    v2_lib::commands::ai_bridge::clear_current_pbi(&slot);
    assert_eq!(slot.lock().unwrap().current_pbi, None);
}

#[test]
fn the_guide_states_the_same_rule_briefly() {
    let guide = v2_lib::autorun::guide::autorun_guide();
    assert!(guide.contains("get_current_pbi"));
    assert!(guide.contains(CONFIRM));
    assert!(guide.contains("search_pbis"));
}

#[tokio::test]
async fn the_writing_start_points_at_get_current_pbi_first() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = BridgeContext { working_dir: Some(dir.path().to_string_lossy().into_owned()), ..BridgeContext::default() };
    let (status, body) = route(&ctx, None, "POST", "/begin?feature=x", "", "1.0.0").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let note = v["note"].as_str().unwrap();
    assert!(note.contains("get_current_pbi") && note.contains(CONFIRM), "{note}");
}
