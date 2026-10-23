//! The agent registry: who is calling the bridge, by label.

use crate::serial;
use v2_lib::autorun::agents::{
    self, clean_client, clean_name, valid_id, AgentHeaders, AgentKey, Caller, FORGET_AFTER_MS,
    GONE_GRACE_MS,
};

const ID_A: &str = "0123456789abcdef0123456789abcdef";
const ID_B: &str = "1123456789abcdef0123456789abcdef";
const ID_C: &str = "2123456789abcdef0123456789abcdef";
const ID_D: &str = "3123456789abcdef0123456789abcdef";

fn headers(id: &str, name: Option<&str>) -> AgentHeaders {
    AgentHeaders {
        id: Some(id.to_string()),
        pid: Some(4242),
        client: Some("claude-code".to_string()),
        name: name.map(str::to_string),
    }
}

/// Register the caller these headers name, at `now`, and return its label.
fn call(id: &str, name: Option<&str>, now: u64) -> String {
    let h = headers(id, name);
    agents::note_call(&h.caller(), &h, now).expect("an agent is registered")
}

/// The check the sweep is given: this process runs, one that was waited for
/// does not, and pid 0 is never a process.
#[cfg(windows)]
#[test]
fn process_running_knows_this_process_and_an_ended_one() {
    use v2_lib::browser::tree::process_running;
    assert!(process_running(std::process::id()));
    assert!(!process_running(0));
    let mut child = std::process::Command::new("cmd").args(["/c", "exit", "0"]).spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    assert!(!process_running(pid));
}

fn key(id: &str, name: Option<&str>) -> AgentKey {
    AgentKey { id: id.to_string(), name: name.map(str::to_string) }
}

#[test]
fn headers_with_a_valid_id_name_an_agent() {
    let _g = serial::agents();
    agents::clear();
    assert!(valid_id(ID_A));
    assert_eq!(
        headers(ID_A, Some("writer-1")).caller(),
        Caller::Agent(AgentKey { id: ID_A.to_string(), name: Some("writer-1".to_string()) })
    );
    assert_eq!(headers(ID_A, None).caller(), Caller::Agent(key(ID_A, None)));
}

#[test]
fn a_missing_or_malformed_id_is_the_default_caller() {
    let _g = serial::agents();
    agents::clear();
    let with = |id: Option<&str>| AgentHeaders {
        id: id.map(str::to_string),
        name: Some("writer-1".to_string()),
        ..AgentHeaders::default()
    };
    assert_eq!(with(None).caller(), Caller::Default);
    assert_eq!(with(Some("xyz")).caller(), Caller::Default);
    assert_eq!(with(Some(&ID_A[..31])).caller(), Caller::Default);
    assert_eq!(with(Some(&ID_A.to_uppercase())).caller(), Caller::Default);
    assert_eq!(with(Some(&format!("{ID_A}0"))).caller(), Caller::Default);
    assert_eq!(AgentHeaders::default().caller(), Caller::Default);
    assert!(!valid_id(&"g".repeat(32)));
}

#[test]
fn a_malformed_name_is_dropped_and_a_client_is_cut() {
    let _g = serial::agents();
    agents::clear();
    assert_eq!(headers(ID_A, Some("a/b")).caller(), Caller::Agent(key(ID_A, None)));
    assert_eq!(clean_name("a/b"), None);
    assert_eq!(clean_name(""), None);
    assert_eq!(clean_name("   "), None);
    assert_eq!(clean_name(&"x".repeat(33)), None);
    assert_eq!(clean_name(&"x".repeat(32)), Some("x".repeat(32)));
    assert_eq!(clean_name("  my agent_1.2-b  "), Some("my agent_1.2-b".to_string()));
    assert_eq!(clean_name("é"), None);
    assert_eq!(clean_client("claude-code"), Some("claude-code".to_string()));
    assert_eq!(clean_client(&"x".repeat(40)), Some("x".repeat(32)));
    assert_eq!(clean_client("é/"), None);
    assert_eq!(clean_client("  "), None);
    assert_eq!(clean_client("a/b\\c"), Some("abc".to_string()));
}

#[test]
fn labels_take_the_smallest_free_number() {
    let _g = serial::agents();
    agents::clear();
    assert_eq!(call(ID_A, None, 1), "Assistant 1");
    assert_eq!(call(ID_B, None, 2), "Assistant 2");
    assert_eq!(call(ID_C, None, 3), "Assistant 3");
    agents::note_bye(ID_B, 10);
    assert_eq!(call(ID_D, None, 11), "Assistant 2");
    assert_eq!(call(ID_A, None, 12), "Assistant 1");
    let live: Vec<String> = agents::live().into_iter().map(|i| i.label).collect();
    assert_eq!(live, ["Assistant 1", "Assistant 2", "Assistant 3"]);
}

#[test]
fn subagent_names_of_one_proxy_are_separate_agents() {
    let _g = serial::agents();
    agents::clear();
    let first = call(ID_A, None, 1);
    let second = call(ID_A, Some("a"), 2);
    assert_ne!(first, second);
    let one = agents::info(&key(ID_A, None)).expect("the session");
    let two = agents::info(&key(ID_A, Some("a"))).expect("the subagent");
    assert_eq!(one.pid, Some(4242));
    assert_eq!(one.pid, two.pid);
    assert_eq!(one.client.as_deref(), Some("claude-code"));
    assert_eq!(agents::live().len(), 2);
    // The same id and name again is the same agent, not a third.
    assert_eq!(call(ID_A, Some("a"), 3), second);
    assert_eq!(agents::live().len(), 2);
    assert_eq!(agents::info(&key(ID_A, Some("a"))).unwrap().last_ms, 3);
    assert_eq!(agents::info(&key(ID_A, Some("a"))).unwrap().first_ms, 2);
    // One goodbye ends both.
    agents::note_bye(ID_A, 9);
    assert!(agents::live().is_empty());
    assert!(!agents::is_live(&key(ID_A, None)));
}

#[test]
fn the_default_caller_is_never_registered() {
    let _g = serial::agents();
    agents::clear();
    let h = headers(ID_A, None);
    assert_eq!(agents::note_call(&Caller::Default, &h, 1), None);
    assert!(agents::live().is_empty());
    assert_eq!(agents::info(&key(ID_A, None)), None);
}

#[test]
fn sweep_marks_dead_pids_gone_then_past_grace_once_then_forgets() {
    let _g = serial::agents();
    agents::clear();
    let t = 1_000_000;
    call(ID_A, None, t - 5);
    let k = key(ID_A, None);

    // A live pid changes nothing.
    assert_eq!(agents::sweep(t, &|_| true), agents::Sweep::default());
    assert!(agents::is_live(&k));

    let s = agents::sweep(t, &|_| false);
    assert_eq!(s.newly_gone, vec![k.clone()]);
    assert!(s.past_grace.is_empty() && s.forgotten.is_empty());
    assert!(!agents::is_live(&k));
    assert_eq!(agents::info(&k).unwrap().gone_since_ms, Some(t));

    assert!(agents::sweep(t + GONE_GRACE_MS - 1, &|_| false).past_grace.is_empty());
    assert_eq!(agents::sweep(t + GONE_GRACE_MS, &|_| false).past_grace, vec![k.clone()]);
    assert!(agents::sweep(t + GONE_GRACE_MS + 1_000, &|_| false).past_grace.is_empty());

    let s = agents::sweep(t + FORGET_AFTER_MS, &|_| false);
    assert_eq!(s.forgotten, vec![k.clone()]);
    assert_eq!(agents::info(&k), None);
}

#[test]
fn an_entry_past_grace_and_forget_age_is_reported_before_it_is_forgotten() {
    let _g = serial::agents();
    agents::clear();
    call(ID_A, None, 1);
    let k = key(ID_A, None);
    agents::note_bye(ID_A, 1_000);
    // One long gap: past both ages in the first sweep that looks.
    let s = agents::sweep(1_000 + FORGET_AFTER_MS + 5, &|_| true);
    assert_eq!(s.past_grace, vec![k.clone()]);
    assert!(s.forgotten.is_empty());
    assert_eq!(agents::info(&k).map(|i| i.label), Some("Assistant 1".to_string()));
    // The next sweep forgets it.
    let s = agents::sweep(1_000 + FORGET_AFTER_MS + 6, &|_| true);
    assert!(s.past_grace.is_empty());
    assert_eq!(s.forgotten, vec![k.clone()]);
    assert_eq!(agents::info(&k), None);
}

#[test]
fn a_call_without_pid_or_client_keeps_the_stored_ones() {
    let _g = serial::agents();
    agents::clear();
    call(ID_A, None, 1);
    let bare = AgentHeaders { id: Some(ID_A.to_string()), ..AgentHeaders::default() };
    agents::note_call(&bare.caller(), &bare, 2);
    let i = agents::info(&key(ID_A, None)).unwrap();
    assert_eq!(i.pid, Some(4242));
    assert_eq!(i.client.as_deref(), Some("claude-code"));
    assert_eq!(i.last_ms, 2);
    // An unreadable client does not wipe it either.
    let odd = AgentHeaders { client: Some("é/".to_string()), ..bare };
    agents::note_call(&odd.caller(), &odd, 3);
    assert_eq!(agents::info(&key(ID_A, None)).unwrap().client.as_deref(), Some("claude-code"));
}

#[test]
fn sweep_does_not_call_alive_with_the_registry_locked() {
    let _g = serial::agents();
    agents::clear();
    call(ID_A, None, 1);
    let k = key(ID_A, None);
    let seen = std::cell::Cell::new(false);
    // `alive` reads and writes the registry: it would deadlock if the sweep
    // held the lock around it.
    let s = agents::sweep(100, &|_| {
        seen.set(agents::info(&key(ID_A, None)).is_some());
        let h = headers(ID_B, None);
        agents::note_call(&h.caller(), &h, 50);
        false
    });
    assert!(seen.get());
    assert_eq!(s.newly_gone, vec![k.clone()]);
    assert!(agents::is_live(&key(ID_B, None)));
    assert!(!agents::is_live(&k));
}

#[test]
fn an_entry_without_a_pid_is_never_swept_gone() {
    let _g = serial::agents();
    agents::clear();
    let h = AgentHeaders { id: Some(ID_A.to_string()), ..AgentHeaders::default() };
    assert!(agents::note_call(&h.caller(), &h, 1).is_some());
    assert_eq!(agents::sweep(5_000_000, &|_| false), agents::Sweep::default());
    assert!(agents::is_live(&key(ID_A, None)));
}

#[test]
fn a_call_after_bye_brings_the_agent_back() {
    let _g = serial::agents();
    agents::clear();
    let label = call(ID_A, None, 1);
    agents::note_bye(ID_A, 10);
    assert_eq!(agents::info(&key(ID_A, None)).unwrap().gone_since_ms, Some(10));
    assert_eq!(call(ID_A, None, 20), label);
    let back = agents::info(&key(ID_A, None)).unwrap();
    assert_eq!(back.gone_since_ms, None);
    assert_eq!(back.last_ms, 20);
    assert!(agents::is_live(&key(ID_A, None)));
    // Gone again, it reaches its grace once more.
    agents::note_bye(ID_A, 100);
    assert_eq!(
        agents::sweep(100 + GONE_GRACE_MS, &|_| true).past_grace,
        vec![key(ID_A, None)]
    );
}

#[test]
fn a_returning_agent_whose_number_was_taken_gets_a_free_one() {
    let _g = serial::agents();
    agents::clear();
    assert_eq!(call(ID_A, None, 1), "Assistant 1");
    agents::note_bye(ID_A, 5);
    assert_eq!(call(ID_B, None, 6), "Assistant 1");
    assert_eq!(call(ID_A, None, 7), "Assistant 2");
    let labels: Vec<String> = agents::live().into_iter().map(|i| i.label).collect();
    assert_eq!(labels, ["Assistant 1", "Assistant 2"]);
}
