//! `upload_journal`: the note an upload writes before its first batch.

use v2_lib::model::TestCase;
use v2_lib::upload_journal::{self, JournalRead, UploadJournal};

fn case(title: &str, update_id: Option<i32>) -> TestCase {
    TestCase { title: title.into(), update_id, ..Default::default() }
}

fn journal(org: &str, pbi: i32) -> UploadJournal {
    UploadJournal {
        organization: org.into(),
        project: "Web".into(),
        pbi_id: pbi,
        since: "2026-11-11T10:00:00Z".into(),
        titles: vec!["A".into()],
        sigs: vec!["sa".into()],
        ids: vec![7],
    }
}

#[test]
fn journal_for_submit_lists_only_creates() {
    let q = vec![case("A", None), case("B", Some(5)), case("C", None)];
    let sigs = vec!["sa".to_string(), "sb".to_string(), "sc".to_string()];
    let j = upload_journal::journal_for_submit("acme", "Web", 42, "2026-11-11T10:00:00Z", &q, &[7, 7, 8], &sigs).unwrap();
    assert_eq!(j.titles, vec!["A", "C"]);
    assert_eq!(j.sigs, vec!["sa", "sc"]);
    assert_eq!(j.ids, vec![7, 8]);
    assert_eq!(j.since, "2026-11-11T10:00:00Z");
    assert_eq!((j.organization.as_str(), j.project.as_str(), j.pbi_id), ("acme", "Web", 42));
}

#[test]
fn misaligned_sigs_are_dropped_not_trusted() {
    let q = vec![case("A", None), case("B", Some(5)), case("C", None)];
    let sigs = vec!["sa".to_string(), "sb".to_string()];
    let j = upload_journal::journal_for_submit("acme", "Web", 42, "t", &q, &[], &sigs).unwrap();
    assert_eq!(j.titles, vec!["A", "C"]);
    assert!(j.sigs.is_empty());
}

#[test]
fn an_upload_of_only_updates_writes_no_journal() {
    let q = vec![case("B", Some(5))];
    assert!(upload_journal::journal_for_submit("acme", "Web", 42, "t", &q, &[1], &["s".to_string()]).is_none());
}

#[test]
fn write_read_clear_round_trip() {
    let _g = crate::serial::upload_journal();
    let dir = tempfile::tempdir().unwrap();
    let j = journal("acme", 42);
    upload_journal::write_in(dir.path(), "o1", &j).unwrap();
    assert_eq!(upload_journal::read_in(dir.path(), "o1", "acme", 42), JournalRead::Found { journal: j });
    upload_journal::clear_in(dir.path(), "o1", "acme", 42).unwrap();
    assert_eq!(upload_journal::read_in(dir.path(), "o1", "acme", 42), JournalRead::None);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn another_account_never_sees_or_clears_a_journal() {
    let _g = crate::serial::upload_journal();
    let dir = tempfile::tempdir().unwrap();
    let j = journal("acme", 42);
    upload_journal::write_in(dir.path(), "o1", &j).unwrap();
    assert_eq!(upload_journal::read_in(dir.path(), "o2", "acme", 42), JournalRead::None);
    assert!(upload_journal::list_in(dir.path(), "o2", "acme").is_empty());
    assert!(upload_journal::clear_in(dir.path(), "o2", "acme", 42).is_ok());
    assert!(dir.path().join(upload_journal::file_name("acme", 42)).exists());
    assert_eq!(upload_journal::read_in(dir.path(), "o1", "acme", 42), JournalRead::Found { journal: j });
}

#[test]
fn a_file_naming_another_org_or_pbi_is_not_found_and_not_cleared() {
    let _g = crate::serial::upload_journal();
    let dir = tempfile::tempdir().unwrap();
    // A journal for ("other", 7) sitting under the file name of ("acme", 42).
    let stray = journal("other", 7);
    upload_journal::write_in(dir.path(), "o1", &stray).unwrap();
    std::fs::rename(
        dir.path().join(upload_journal::file_name("other", 7)),
        dir.path().join(upload_journal::file_name("acme", 42)),
    )
    .unwrap();
    assert_eq!(upload_journal::read_in(dir.path(), "o1", "acme", 42), JournalRead::None);
    upload_journal::clear_in(dir.path(), "o1", "acme", 42).unwrap();
    assert!(dir.path().join(upload_journal::file_name("acme", 42)).exists());
}

#[test]
fn clear_does_nothing_while_that_upload_runs() {
    let _g = crate::serial::upload_journal();
    let dir = tempfile::tempdir().unwrap();
    upload_journal::write_in(dir.path(), "o1", &journal("acme", 42)).unwrap();
    let m = upload_journal::mark_running("acme", 42);
    upload_journal::clear_in(dir.path(), "o1", "acme", 42).unwrap();
    assert!(dir.path().join(upload_journal::file_name("acme", 42)).exists());
    drop(m);
    upload_journal::clear_in(dir.path(), "o1", "acme", 42).unwrap();
    assert!(!dir.path().join(upload_journal::file_name("acme", 42)).exists());
}

#[test]
fn list_is_per_org() {
    let _g = crate::serial::upload_journal();
    let dir = tempfile::tempdir().unwrap();
    upload_journal::write_in(dir.path(), "o1", &journal("acme", 42)).unwrap();
    upload_journal::write_in(dir.path(), "o1", &journal("other", 42)).unwrap();
    upload_journal::write_in(dir.path(), "o1", &journal("acme", 43)).unwrap();
    let got = upload_journal::list_in(dir.path(), "o1", "acme");
    assert_eq!(got.iter().map(|j| (j.organization.as_str(), j.pbi_id)).collect::<Vec<_>>(), vec![("acme", 42), ("acme", 43)]);
}

#[test]
fn file_names_carry_no_org_text() {
    let n = upload_journal::file_name("acme corp", 42);
    assert!(!n.contains("acme"));
    assert!(n.ends_with("-42.json"));
    assert_eq!(n.len(), 8 + "-42.json".len());
}

#[test]
fn writing_leaves_no_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    upload_journal::write_in(dir.path(), "o1", &journal("acme", 42)).unwrap();
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![upload_journal::file_name("acme", 42)]);
}

#[test]
fn a_running_upload_reads_as_running_and_is_not_listed() {
    let _g = crate::serial::upload_journal();
    let dir = tempfile::tempdir().unwrap();
    upload_journal::write_in(dir.path(), "o1", &journal("acme", 42)).unwrap();
    let m = upload_journal::mark_running("acme", 42);
    assert!(upload_journal::is_running("acme", 42));
    assert_eq!(upload_journal::read_in(dir.path(), "o1", "acme", 42), JournalRead::Running);
    assert!(upload_journal::list_in(dir.path(), "o1", "acme").is_empty());
    drop(m);
    assert!(!upload_journal::is_running("acme", 42));
    assert!(matches!(upload_journal::read_in(dir.path(), "o1", "acme", 42), JournalRead::Found { .. }));
    assert_eq!(upload_journal::list_in(dir.path(), "o1", "acme").len(), 1);
}

#[test]
fn submit_queue_writes_the_journal_before_the_first_batch_and_never_clears_it() {
    let src = include_str!("../../src/commands/queue.rs").replace("\r\n", "\n");
    let body = &src[src.find("pub async fn submit_queue(").unwrap()..];
    let body = &body[..body.find("\n}\n").unwrap()];
    let write = body.find("upload_journal::write_in").expect("journal written in submit_queue");
    assert!(write < body.find(".wit_batch(").unwrap());
    assert!(body.find("mark_running").unwrap() < write);
    assert!(!src.contains("upload_journal::clear_in"));
}

fn second(titles: &[&str], sigs: &[&str], ids: &[i32]) -> UploadJournal {
    UploadJournal {
        organization: "acme".into(),
        project: "Mobile".into(),
        pbi_id: 42,
        since: "2026-11-12T09:00:00Z".into(),
        titles: titles.iter().map(|s| s.to_string()).collect(),
        sigs: sigs.iter().map(|s| s.to_string()).collect(),
        ids: ids.to_vec(),
    }
}

#[test]
fn a_new_journal_merges_into_an_unrecovered_one() {
    let _g = crate::serial::upload_journal();
    let dir = tempfile::tempdir().unwrap();
    let mut old = journal("acme", 42);
    old.ids = vec![7, 8];
    upload_journal::write_in(dir.path(), "o1", &old).unwrap();
    upload_journal::write_in(dir.path(), "o1", &second(&["B", "C"], &["sb", "sc"], &[8, 9])).unwrap();
    let JournalRead::Found { journal: j } = upload_journal::read_in(dir.path(), "o1", "acme", 42) else {
        panic!("expected a journal");
    };
    assert_eq!(j.since, "2026-11-11T10:00:00Z");
    assert_eq!(j.project, "Mobile");
    assert_eq!(j.titles, vec!["A", "B", "C"]);
    assert_eq!(j.sigs, vec!["sa", "sb", "sc"]);
    // The older upload's baseline is kept: the new journal's ids were read
    // after the older upload ran, so they include cases it created, and
    // excluding those from Check would hide them.
    assert_eq!(j.ids, vec![7, 8]);
}

#[test]
fn merging_with_misaligned_sigs_leaves_them_empty() {
    let _g = crate::serial::upload_journal();
    let dir = tempfile::tempdir().unwrap();
    upload_journal::write_in(dir.path(), "o1", &journal("acme", 42)).unwrap();
    upload_journal::write_in(dir.path(), "o1", &second(&["B", "C"], &[], &[])).unwrap();
    let JournalRead::Found { journal: j } = upload_journal::read_in(dir.path(), "o1", "acme", 42) else {
        panic!("expected a journal");
    };
    assert_eq!(j.titles, vec!["A", "B", "C"]);
    assert!(j.sigs.is_empty());
}

#[test]
fn a_journal_of_another_account_is_overwritten_not_merged() {
    let _g = crate::serial::upload_journal();
    let dir = tempfile::tempdir().unwrap();
    upload_journal::write_in(dir.path(), "o1", &journal("acme", 42)).unwrap();
    let new = second(&["B"], &["sb"], &[1]);
    upload_journal::write_in(dir.path(), "o2", &new).unwrap();
    assert_eq!(upload_journal::read_in(dir.path(), "o2", "acme", 42), JournalRead::Found { journal: new });
}
