//! `activity_log`: detailed DB (and, later, API) records in their own
//! daily JSONL files, apart from `applog`.

use serde_json::json;
use std::time::{Duration, SystemTime};
use v2_lib::activity_log::{self, Kind};

#[test]
fn a_record_lands_in_todays_file_for_its_kind() {
    let _g = crate::serial::activity_log();
    let dir = tempfile::tempdir().unwrap();
    activity_log::init(dir.path().to_path_buf());
    activity_log::record(Kind::Db, json!({ "sql": "SELECT 1" }));
    let recs = crate::common::activity_records(dir.path(), "db");
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["sql"], "SELECT 1");
    assert!(recs[0]["at"].as_str().unwrap().len() >= 19);
    assert!(crate::common::activity_records(dir.path(), "api").is_empty());
}

#[test]
fn file_names_are_kind_and_date() {
    assert_eq!(activity_log::file_name(Kind::Api, "2026-09-28"), "api-2026-09-28.jsonl");
    assert_eq!(activity_log::file_name(Kind::Db, "2026-09-28"), "db-2026-09-28.jsonl");
}

/// Old files this module wrote go; a recent one and a file it never wrote
/// both stay - the same "never touch what it did not write" rule
/// `applog::prune` follows.
#[test]
fn prune_keeps_thirty_days_and_touches_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();

    let old_db = dir.path().join("db-2026-08-01.jsonl");
    let old_api = dir.path().join("api-2026-08-01.jsonl");
    let recent = dir.path().join("db-2026-08-30.jsonl");
    let notes = dir.path().join("notes.txt");
    for p in [&old_db, &old_api, &recent, &notes] {
        std::fs::write(p, "x").unwrap();
    }

    touch(&old_db, now - Duration::from_secs(31 * 86_400));
    touch(&old_api, now - Duration::from_secs(31 * 86_400));
    touch(&recent, now - Duration::from_secs(29 * 86_400));
    touch(&notes, now - Duration::from_secs(31 * 86_400));

    activity_log::prune(dir.path(), now, activity_log::KEEP_DAYS);

    assert!(!old_db.exists(), "a 31-day-old db activity file must be pruned");
    assert!(!old_api.exists(), "a 31-day-old api activity file must be pruned");
    assert!(recent.exists(), "a 29-day-old activity file must stay");
    assert!(notes.exists(), "prune must never touch a file this module did not write");
}

fn touch(path: &std::path::Path, when: SystemTime) {
    let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(when).unwrap();
}
