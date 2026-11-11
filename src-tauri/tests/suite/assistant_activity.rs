//! `assistant_activity`: the curated route table, what a record may hold,
//! and the timeline's filter. Every test that writes reads back only its own
//! unique org's lines: other modules' bridge calls record into the same
//! directory without this lock.

use serde_json::Value;
use std::time::{Duration, SystemTime};
use v2_lib::activity_log;
use v2_lib::assistant_activity::*;
use v2_lib::autorun::agents::Caller;

/// Every assistant record in `dir` whose org is `org`.
fn lines_for(dir: &std::path::Path, org: &str) -> Vec<Value> {
    crate::common::activity_records(dir, "assistant").into_iter().filter(|v| v["org"] == org).collect()
}

fn fresh() -> (std::sync::MutexGuard<'static, ()>, tempfile::TempDir) {
    let g = crate::serial::activity_log();
    let dir = tempfile::tempdir().unwrap();
    activity_log::init(dir.path().to_path_buf());
    (g, dir)
}

#[test]
fn the_table_names_writes_only() {
    assert_eq!(route_action("POST", "/autorun-quirk", "{}"), Some((ActivityGroup::Saved, "Saved a quirk")));
    assert_eq!(
        route_action("POST", "/autorun-setup-save", r#"{"name":"x","remove":true}"#),
        Some((ActivityGroup::Saved, "Removed a setup"))
    );
    assert_eq!(
        route_action("POST", "/autorun-setup-save", r#"{"name":"x","remove":"true"}"#),
        Some((ActivityGroup::Saved, "Saved a setup"))
    );
    assert_eq!(route_action("POST", "/autorun-try", "{}"), Some((ActivityGroup::Tried, "Tried an action")));
    assert_eq!(route_action("POST", "/autorun-script-delete", "{}"), Some((ActivityGroup::Saved, "Removed a script")));
    assert_eq!(route_action("POST", "/api-template-fixture-run", "{}"), Some((ActivityGroup::Tried, "Ran an API fixture")));
    assert_eq!(route_action("GET", "/autorun-page", ""), None);
    assert_eq!(route_action("GET", "/autorun-quirk", ""), None);
    assert_eq!(route_action("POST", "/autorun-requests", "{}"), None);
    assert_eq!(route_action("POST", "/crystal-open", "{}"), None);
    assert_eq!(route_action("POST", "/db-query", "{}"), None);
}

#[test]
fn a_default_caller_is_called_assistant() {
    assert_eq!(caller_label(&Caller::Default), "Assistant");
}

#[test]
fn a_record_carries_no_body_or_query() {
    let (_g, dir) = fresh();
    record_call(
        &Caller::Default,
        "org-noleak",
        "env-1",
        "POST",
        "/autorun-try?token=QSECRET",
        r#"{"case_id":12,"value":"hunter2","password":"p4ss"}"#,
        200,
    );
    let text = lines_for(dir.path(), "org-noleak").iter().map(|v| v.to_string()).collect::<String>();
    assert!(!text.is_empty());
    for bad in ["QSECRET", "token", "hunter2", "p4ss", "password", "value", "?"] {
        assert!(!text.contains(bad), "{bad}");
    }
}

#[test]
fn a_record_has_exactly_the_allowed_keys() {
    let (_g, dir) = fresh();
    record_call(&Caller::Default, "org-keys", "env-1", "POST", "/autorun-try", r#"{"case_id":12,"x":1}"#, 200);
    let recs = lines_for(dir.path(), "org-keys");
    assert_eq!(recs.len(), 1);
    let mut keys: Vec<&str> = recs[0].as_object().unwrap().keys().map(|k| k.as_str()).collect();
    keys.sort();
    assert_eq!(keys, ["action", "at", "case_id", "env", "group", "label", "ok", "org"]);
    assert_eq!(recs[0]["ok"], true);
    assert_eq!(recs[0]["case_id"], 12);
    assert_eq!(recs[0]["group"], "tried");
    assert_eq!(recs[0]["label"], "Assistant");
}

#[test]
fn a_refused_call_is_recorded_as_not_ok() {
    let (_g, dir) = fresh();
    record_call(&Caller::Default, "org-refused", "env-1", "POST", "/autorun-try", "{}", 409);
    let recs = lines_for(dir.path(), "org-refused");
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["ok"], false);
    assert!(recs[0].get("case_id").is_none());
}

#[test]
fn a_read_is_not_recorded() {
    let (_g, dir) = fresh();
    record_call(&Caller::Default, "org-read", "env-1", "GET", "/autorun-page", "", 200);
    record_call(&Caller::Default, "org-read", "env-1", "POST", "/crystal-open", "{}", 200);
    assert!(lines_for(dir.path(), "org-read").is_empty());
}

#[test]
fn the_timeline_shows_only_this_org_and_environment() {
    let (_g, dir) = fresh();
    record_call(&Caller::Default, "org-a", "env-1", "POST", "/autorun-try", "{}", 200);
    record_call(&Caller::Default, "org-b", "env-1", "POST", "/autorun-try", "{}", 200);
    record_call(&Caller::Default, "org-a", "env-2", "POST", "/autorun-try", "{}", 200);
    let got = timeline_in(dir.path(), "org-a", "env-1", 7, SystemTime::now());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].label, "Assistant");
    assert_eq!(got[0].action, "Tried an action");
    assert_eq!((got[0].org.as_str(), got[0].env.as_str()), ("org-a", "env-1"));
}

#[test]
fn the_timeline_is_newest_first_and_capped() {
    let (_g, dir) = fresh();
    let now = SystemTime::now();
    let stamp = |secs_ago: u64| v2_lib::applog::stamp_of(now - Duration::from_secs(secs_ago));
    let line = |at: String, action: &str| {
        serde_json::json!({ "at": at, "label": "Assistant", "group": "saved", "action": action,
            "ok": true, "env": "e", "org": "org-order" })
        .to_string()
    };
    let old = stamp(8 * 86_400);
    let (t1, t2, t3) = (stamp(300), stamp(200), stamp(100));
    let file = |at: &str| dir.path().join(format!("assistant-{}.jsonl", &at[..10]));
    let mut by_file: std::collections::BTreeMap<std::path::PathBuf, String> = Default::default();
    for (at, a) in [(&old, "too old"), (&t2, "second"), (&t1, "first"), (&t3, "third")] {
        let e = by_file.entry(file(at)).or_default();
        e.push_str(&line(at.clone(), a));
        e.push('\n');
    }
    for (p, text) in by_file {
        std::fs::write(p, text).unwrap();
    }
    let got = timeline_in(dir.path(), "org-order", "e", 7, now);
    let actions: Vec<&str> = got.iter().map(|r| r.action.as_str()).collect();
    assert_eq!(actions, ["third", "second", "first"]);

    // The cap: more than TIMELINE_CAP lines answer TIMELINE_CAP.
    let mut many = String::new();
    for _ in 0..(TIMELINE_CAP + 20) {
        many.push_str(&line(stamp(10), "many"));
        many.push('\n');
    }
    std::fs::write(file(&stamp(10)), many).unwrap();
    assert_eq!(timeline_in(dir.path(), "org-order", "e", 7, now).len(), TIMELINE_CAP);
}

#[test]
fn recording_marks_the_timeline_changed() {
    let (_g, _dir) = fresh();
    // Bridge tests record without this lock, so "false afterwards" cannot be
    // asserted here; the flag being set by a record is.
    let _ = activity_log::take_assistant_changed();
    record_call(&Caller::Default, "org-flag", "env-1", "POST", "/autorun-try", "{}", 200);
    assert!(activity_log::take_assistant_changed());
}

#[test]
fn a_garbled_line_is_skipped() {
    let (_g, dir) = fresh();
    let now = SystemTime::now();
    let at = v2_lib::applog::stamp_of(now);
    let good = serde_json::json!({ "at": at, "label": "Assistant", "group": "tried", "action": "Tried an action",
        "ok": true, "env": "e", "org": "org-garble" });
    std::fs::write(dir.path().join(format!("assistant-{}.jsonl", &at[..10])), format!("not json\n{good}\n")).unwrap();
    assert_eq!(timeline_in(dir.path(), "org-garble", "e", 1, now).len(), 1);
}
