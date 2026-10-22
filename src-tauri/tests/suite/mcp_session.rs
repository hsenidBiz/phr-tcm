//! What one MCP session has been told: a role's rules once ahead of the first answer of the tool that
//! starts the role, a pointer line after, nothing for a call that never reached the app.

use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use v2_lib::autorun::roles::{self, Role};
use v2_lib::mcp::{handle_message, handle_message_in};
use v2_lib::mcp_session::{Session, IDLE_RESET_MS};

const ANSWER: &str = "canned answer";

/// A bridge that answers every tool route with `ANSWER` (or `status`, or an error).
struct Fake {
    status: Cell<u16>,
    unreachable: Cell<bool>,
    calls: RefCell<Vec<String>>,
}

impl Fake {
    fn new() -> Fake {
        Fake { status: Cell::new(200), unreachable: Cell::new(false), calls: RefCell::new(vec![]) }
    }
    fn answer(&self, method: &str, path: &str, _body: &str) -> Result<(u16, String), String> {
        if self.unreachable.get() {
            return Err("connection refused".into());
        }
        if method == "GET" && path == "/tools" {
            return Ok((200, json!({
                    "disabled": [],
                    "modules": v2_lib::modules::Module::ALL.iter().map(|m| m.key()).collect::<Vec<_>>(),
                }).to_string()));
        }
        self.calls.borrow_mut().push(path.to_string());
        Ok((self.status.get(), ANSWER.to_string()))
    }
}

fn send(fake: &Fake, session: &mut Session, tool: &str, args: Value) -> Value {
    let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": tool, "arguments": args } }).to_string();
    let call = |m: &str, p: &str, b: &str| fake.answer(m, p, b);
    let out = handle_message_in(&msg, "1.0.0", &call, session).expect("an answer");
    serde_json::from_str::<Value>(&out).unwrap()["result"].clone()
}

fn texts(result: &Value) -> Vec<String> {
    result["content"].as_array().unwrap().iter().map(|c| c["text"].as_str().unwrap().to_string()).collect()
}

fn failures(fake: &Fake, s: &mut Session) -> Vec<String> {
    texts(&send(fake, s, "get_autorun_failures", json!({})))
}

#[test]
fn the_first_failures_answer_of_a_session_opens_with_the_healers_rules() {
    let fake = Fake::new();
    let mut s = Session::new();
    s.tick(1_000);
    assert_eq!(failures(&fake, &mut s), vec![Role::Healer.text(), ANSWER.to_string()]);
}

#[test]
fn a_later_failures_answer_opens_with_the_pointer() {
    let fake = Fake::new();
    let mut s = Session::new();
    s.tick(1_000);
    failures(&fake, &mut s);
    s.tick(2_000);
    assert_eq!(failures(&fake, &mut s), vec![roles::pointer(Role::Healer), ANSWER.to_string()]);
}

#[test]
fn a_discovery_start_brings_the_writers_rules_but_a_mapping_run_does_not() {
    let fake = Fake::new();
    let mut s = Session::new();
    s.tick(1_000);
    let mapping = texts(&send(&fake, &mut s, "start_autorun_discovery", json!({ "account": "a", "mapping": true })));
    assert_eq!(mapping, vec![ANSWER.to_string()]);
    let plain = texts(&send(&fake, &mut s, "start_autorun_discovery", json!({ "account": "a" })));
    assert_eq!(plain, vec![Role::Writer.text(), ANSWER.to_string()]);
}

fn repair_args() -> Value {
    json!({ "scripts": [{ "case_id": 7, "steps": [] }], "edits": [{ "case_id": 7, "steps": [2], "why": "x" }] })
}

#[test]
fn a_repair_save_brings_the_case_rewriters_rules_and_a_new_script_save_does_not() {
    let fake = Fake::new();
    let mut s = Session::new();
    s.tick(1_000);
    let new_script = texts(&send(&fake, &mut s, "save_autorun_script", json!({ "scripts": [{ "case_id": 7, "steps": [] }] })));
    assert_eq!(new_script, vec![ANSWER.to_string()]);
    let repair = texts(&send(&fake, &mut s, "save_autorun_script", repair_args()));
    assert_eq!(repair, vec![Role::CaseRewriter.text(), ANSWER.to_string()]);
}

#[test]
fn a_component_save_brings_the_case_rewriters_rules() {
    let fake = Fake::new();
    let mut s = Session::new();
    s.tick(1_000);
    let out = texts(&send(&fake, &mut s, "save_autorun_component", json!({ "name": "Pick a date", "actions": [] })));
    assert_eq!(out[0], Role::CaseRewriter.text());
    assert_eq!(out.last().unwrap(), ANSWER);
}

#[test]
fn a_refused_answer_still_carries_the_rules() {
    let fake = Fake::new();
    fake.status.set(400);
    let mut s = Session::new();
    s.tick(1_000);
    let result = send(&fake, &mut s, "get_autorun_failures", json!({}));
    assert_eq!(result["isError"], json!(true));
    let t = texts(&result);
    assert_eq!(t[0], Role::Healer.text());
    assert_eq!(t[1], format!("bridge returned 400: {ANSWER}"));
}

#[test]
fn reading_a_role_through_get_guide_counts_as_delivered() {
    let fake = Fake::new();
    let mut s = Session::new();
    s.tick(1_000);
    let guide = texts(&send(&fake, &mut s, "get_guide", json!({ "topic": "autorun", "role": "healer" })));
    assert_eq!(guide, vec![Role::Healer.text()]);
    assert!(s.delivered(Role::Healer));
    assert_eq!(failures(&fake, &mut s), vec![roles::pointer(Role::Healer), ANSWER.to_string()]);
}

#[test]
fn an_unreachable_app_delivers_nothing() {
    let fake = Fake::new();
    let mut s = Session::new();
    s.tick(1_000);
    fake.unreachable.set(true);
    let result = send(&fake, &mut s, "get_autorun_failures", json!({}));
    assert_eq!(result["isError"], json!(true));
    assert_eq!(texts(&result).len(), 1);
    assert!(!s.delivered(Role::Healer));
    fake.unreachable.set(false);
    assert_eq!(failures(&fake, &mut s), vec![Role::Healer.text(), ANSWER.to_string()]);
}

#[test]
fn twenty_idle_minutes_or_a_new_initialize_start_the_session_over() {
    let fake = Fake::new();
    let mut s = Session::new();
    let t0 = 1_000;
    s.tick(t0);
    failures(&fake, &mut s);
    s.tick(t0 + IDLE_RESET_MS - 60_000);
    assert_eq!(failures(&fake, &mut s)[0], roles::pointer(Role::Healer));
    // Twenty minutes after the LAST call starts the session over.
    let t1 = t0 + IDLE_RESET_MS - 60_000 + IDLE_RESET_MS;
    s.tick(t1);
    assert_eq!(failures(&fake, &mut s)[0], Role::Healer.text());
    s.tick(t1 + 1_000);
    assert_eq!(failures(&fake, &mut s)[0], roles::pointer(Role::Healer));
    let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }).to_string();
    let call = |m: &str, p: &str, b: &str| fake.answer(m, p, b);
    handle_message_in(&init, "1.0.0", &call, &mut s).unwrap();
    assert_eq!(failures(&fake, &mut s)[0], Role::Healer.text());
}

#[test]
fn handle_message_carries_no_session() {
    let fake = Fake::new();
    let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "get_autorun_failures", "arguments": {} } })
        .to_string();
    let call = |m: &str, p: &str, b: &str| fake.answer(m, p, b);
    for _ in 0..2 {
        let out = handle_message(&msg, "1.0.0", &call).unwrap();
        let result = serde_json::from_str::<Value>(&out).unwrap()["result"].clone();
        assert_eq!(texts(&result), vec![ANSWER.to_string()]);
    }
}

#[test]
fn a_session_pays_each_role_once() {
    let fake = Fake::new();
    let mut s = Session::new();
    s.tick(1_000);
    let mut notes: Vec<String> = vec![];
    for _ in 0..2 {
        for (tool, args) in [
            ("start_autorun_discovery", json!({ "account": "a" })),
            ("get_autorun_failures", json!({})),
            ("save_autorun_script", repair_args()),
            ("save_autorun_component", json!({ "name": "n", "actions": [] })),
        ] {
            let t = texts(&send(&fake, &mut s, tool, args));
            assert_eq!(t.len(), 2, "{tool}");
            notes.push(t[0].clone());
        }
    }
    for role in [Role::Writer, Role::Healer, Role::CaseRewriter] {
        assert_eq!(notes.iter().filter(|n| **n == role.text()).count(), 1, "{}", role.name());
    }
    for n in &notes {
        let full = Role::ALL.iter().any(|r| *n == r.text());
        assert!(full || (n.starts_with("(The ") && n.chars().count() < 220), "{n}");
    }
    let total: usize = notes.iter().map(|n| n.chars().count()).sum();
    // Three full role texts (each now holds the Playwright repo's files too) and the pointers.
    let full: usize = [Role::Writer, Role::Healer, Role::CaseRewriter].iter().map(|r| r.text().chars().count()).sum();
    assert!(total <= full + 5 * 220, "{total}");
}
