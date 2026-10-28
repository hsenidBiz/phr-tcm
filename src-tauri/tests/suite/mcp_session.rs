//! What one MCP session has been told: a role's rules once ahead of the first answer of the tool that
//! starts the role, a pointer line after, nothing for a call that never reached the app.

use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use v2_lib::autorun::pw_rules::Why;
use v2_lib::autorun::roles::{self, Role};
use v2_lib::mcp::{handle_message, handle_message_in};
use v2_lib::mcp_session::{Session, IDLE_RESET_MS};

const ANSWER: &str = "canned answer";

/// A bridge that answers every tool route with `ANSWER` (or `status`, or an error).
struct Fake {
    status: Cell<u16>,
    unreachable: Cell<bool>,
    calls: RefCell<Vec<String>>,
    /// What `GET /autorun-role` answers with: the status, and a body in place of the role's own text.
    role_status: Cell<u16>,
    role_body: RefCell<Option<String>>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            status: Cell::new(200),
            unreachable: Cell::new(false),
            calls: RefCell::new(vec![]),
            role_status: Cell::new(200),
            role_body: RefCell::new(None),
        }
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
        if let ("GET", Some(key)) = (method, path.strip_prefix("/autorun-role?role=")) {
            let body = self.role_body.borrow().clone().unwrap_or_else(|| Role::from_key(key).unwrap().text());
            return Ok((self.role_status.get(), body));
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

#[test]
fn a_role_text_comes_from_the_app() {
    let fake = Fake::new();
    *fake.role_body.borrow_mut() = Some("APP HEALER TEXT".into());
    let mut s = Session::new();
    s.tick(1_000);
    assert_eq!(failures(&fake, &mut s), vec!["APP HEALER TEXT".to_string(), ANSWER.to_string()]);
}

#[test]
fn when_the_app_does_not_give_the_rules_the_shipped_copy_says_so() {
    let fake = Fake::new();
    fake.role_status.set(404);
    let mut s = Session::new();
    s.tick(1_000);
    let first = failures(&fake, &mut s);
    assert_eq!(first[0], Role::Healer.shipped_text(Why::NotRead));
    assert!(first[0].lines().next().unwrap().contains("did not answer"), "{}", first[0]);
    assert_eq!(first[1], ANSWER);
    // The role still counts as delivered.
    s.tick(2_000);
    assert_eq!(failures(&fake, &mut s)[0], roles::pointer(Role::Healer));
}

#[test]
fn the_proxy_never_says_no_repo_is_set_when_it_cannot_know() {
    // The proxy has no settings, so it cannot know whether a Playwright repo is set. Its role text is
    // the app's answer, or the shipped copy whose own first line says the app did not answer.
    let gone: &dyn Fn(&str, &str, &str) -> Result<(u16, String), String> = &|_, _, _| Err("connection refused".into());
    let not_found: &dyn Fn(&str, &str, &str) -> Result<(u16, String), String> = &|_, _, _| Ok((404, "no route".into()));
    let broken: &dyn Fn(&str, &str, &str) -> Result<(u16, String), String> = &|_, _, _| Ok((500, "oops".into()));
    for role in [Role::Writer, Role::Healer, Role::CaseRewriter] {
        for call in [gone, not_found, broken] {
            let text = v2_lib::mcp::role_text_via(role, call);
            assert_eq!(text, role.shipped_text(Why::NotRead), "{}", role.name());
            let first = text.lines().next().unwrap();
            assert!(first.contains("did not answer"), "{first}");
            assert!(!text.contains("no Playwright repo is set"), "{}", role.name());
        }
    }
    let app: &dyn Fn(&str, &str, &str) -> Result<(u16, String), String> = &|_, _, _| Ok((200, "APP TEXT".into()));
    assert_eq!(v2_lib::mcp::role_text_via(Role::Healer, app), "APP TEXT");
    // And nothing in the proxy reads a role through the app's own settings.
    let src = include_str!("../../src/mcp.rs");
    assert!(!src.contains("role.text()") && !src.contains(".text_in("), "the proxy must take a role through role_text_via");
}

#[test]
fn each_agent_name_gets_the_role_rules_once() {
    use v2_lib::mcp_session::{Note, SessionBook};
    let book = SessionBook::new();
    let (tool, args) = ("get_autorun_failures", json!({}));
    let role = roles::role_for_call(tool, &args).expect("a call that starts a role");
    book.tick(None, 1_000);
    assert_eq!(book.note_kind(None, tool, &args), Some(Note::Rules(role)));
    assert_eq!(book.note_kind(None, tool, &args), Some(Note::Pointer(role)));
    // Another name has been told nothing yet, and then it has.
    book.tick(Some("writer-1"), 1_500);
    assert_eq!(book.note_kind(Some("writer-1"), tool, &args), Some(Note::Rules(role)));
    assert_eq!(book.note_kind(Some("writer-1"), tool, &args), Some(Note::Pointer(role)));
    // The first name is still told only the pointer.
    assert_eq!(book.note_kind(None, tool, &args), Some(Note::Pointer(role)));
    // A call that starts no role has no note.
    assert_eq!(book.note_kind(None, "get_tags", &json!({})), None);
}

#[test]
fn reset_all_clears_every_agent() {
    use v2_lib::mcp_session::{Note, SessionBook};
    let book = SessionBook::new();
    let (tool, args) = ("get_autorun_failures", json!({}));
    let role = roles::role_for_call(tool, &args).unwrap();
    for agent in [None, Some("a"), Some("b")] {
        book.tick(agent, 1_000);
        assert_eq!(book.note_kind(agent, tool, &args), Some(Note::Rules(role)));
    }
    book.reset_all();
    for agent in [None, Some("a"), Some("b")] {
        assert_eq!(book.note_kind(agent, tool, &args), Some(Note::Rules(role)), "{agent:?}");
    }
}

#[test]
fn a_reading_of_the_rules_and_the_idle_reset_are_per_agent_name() {
    use v2_lib::mcp_session::{Note, SessionBook};
    let book = SessionBook::new();
    let (tool, args) = ("get_autorun_failures", json!({}));
    let role = roles::role_for_call(tool, &args).unwrap();
    book.tick(Some("a"), 1_000);
    book.tick(Some("b"), 1_000);
    book.mark_read(Some("a"), role);
    assert_eq!(book.note_kind(Some("a"), tool, &args), Some(Note::Pointer(role)));
    assert_eq!(book.note_kind(Some("b"), tool, &args), Some(Note::Rules(role)));
    // Twenty idle minutes starts over only the name that was idle.
    book.tick(Some("a"), 1_000 + IDLE_RESET_MS);
    assert_eq!(book.note_kind(Some("a"), tool, &args), Some(Note::Rules(role)));
    assert_eq!(book.note_kind(Some("b"), tool, &args), Some(Note::Pointer(role)));
}

#[test]
fn the_shared_body_gives_each_agent_name_the_rules_once() {
    use v2_lib::mcp::handle_message_shared;
    use v2_lib::mcp_session::SessionBook;
    let fake = Fake::new();
    let book = SessionBook::new();
    let call = |m: &str, p: &str, b: &str| fake.answer(m, p, b);
    let ask = |agent: Option<&str>| -> Vec<String> {
        let mut arguments = json!({});
        if let Some(a) = agent {
            arguments["agent"] = json!(a);
        }
        let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "get_autorun_failures", "arguments": arguments } })
            .to_string();
        book.tick(agent, 1_000);
        let out = handle_message_shared(&msg, "1.0.0", &call, &book).expect("an answer");
        texts(&serde_json::from_str::<Value>(&out).unwrap()["result"])
    };
    assert_eq!(ask(None), vec![Role::Healer.text(), ANSWER.to_string()]);
    assert_eq!(ask(None), vec![roles::pointer(Role::Healer), ANSWER.to_string()]);
    assert_eq!(ask(Some("healer-1")), vec![Role::Healer.text(), ANSWER.to_string()]);
    assert_eq!(ask(Some("healer-1")), vec![roles::pointer(Role::Healer), ANSWER.to_string()]);
    // An initialize starts every name over.
    let init = json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {} }).to_string();
    handle_message_shared(&init, "1.0.0", &call, &book).unwrap();
    assert_eq!(ask(None)[0], Role::Healer.text());
    assert_eq!(ask(Some("healer-1"))[0], Role::Healer.text());
}

/// A guide section answer opens with the Quick rules; a session that has had
/// them leaves them out of the next section and says they still hold.
#[test]
fn a_second_guide_section_leaves_the_rules_out_and_says_they_hold() {
    use v2_lib::autorun::guide::{autorun_guide, guide_section, quick_rules};
    let section = guide_section(&autorun_guide(), "Selectors").unwrap();
    assert!(section.starts_with(quick_rules().trim_end()), "the answer opens with the rules");
    let call = |m: &str, p: &str, _b: &str| -> Result<(u16, String), String> {
        if m == "GET" && p == "/tools" {
            return Ok((200, json!({ "disabled": [], "modules": v2_lib::modules::Module::ALL.iter().map(|m| m.key()).collect::<Vec<_>>() }).to_string()));
        }
        Ok((200, section.clone()))
    };
    let ask = |s: &mut Session, args: Value| {
        let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "get_guide", "arguments": args } }).to_string();
        let out = handle_message_in(&msg, "1.0.0", &call, s).expect("an answer");
        texts(&serde_json::from_str::<Value>(&out).unwrap()["result"]).concat()
    };
    let args = json!({ "topic": "autorun", "section": "Selectors", "agent": "writer-1" });
    let mut s = Session::new();
    s.tick(1_000);
    let first = ask(&mut s, args.clone());
    assert!(first.contains(quick_rules().trim_end()), "the first answer keeps the rules: {}", &first[..first.len().min(300)]);
    let second = ask(&mut s, args.clone());
    assert!(second.contains(v2_lib::mcp::RULES_HOLD), "{second}");
    assert!(!second.contains(quick_rules().trim_end()), "the rules are not repeated");
    assert!(second.contains("## Selectors"), "the section itself is still there");
    // A call that names no agent shares nobody's memory: the rules are never left out of it.
    let unnamed = json!({ "topic": "autorun", "section": "Selectors" });
    assert!(ask(&mut s, unnamed.clone()).contains(quick_rules().trim_end()));
    assert!(ask(&mut s, unnamed).contains(quick_rules().trim_end()));
    // Nor from an answer to a call without a section.
    let whole = ask(&mut s, json!({ "topic": "autorun", "agent": "writer-1" }));
    assert!(whole.contains(quick_rules().trim_end()), "no section: the rules stay");
    // An untracked session (every caller but the proxy) never leaves them out.
    let mut plain = Session::untracked();
    ask(&mut plain, args.clone());
    assert!(ask(&mut plain, args.clone()).contains(quick_rules().trim_end()));
    // An initialize starts the session over: the rules come again.
    s.reset();
    assert!(ask(&mut s, args).contains(quick_rules().trim_end()));
}
