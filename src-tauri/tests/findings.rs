//! The findings store: what an assistant records is on disk, scoped to an
//! org and project, newest first, validated on the way in, and bounded.

use v2_lib::findings::{list, record, remove, set_status, Finding, NewFinding, CAP};

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir()
            .join(format!("tcm-findings-{nanos}-{}", N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn new_finding(kind: &str, title: &str) -> NewFinding {
    NewFinding {
        org: "acme".into(),
        project: "Web".into(),
        kind: kind.into(),
        subject: "155170".into(),
        title: title.into(),
        detail: "Step 3 expects a toast the spec never mentions.".into(),
    }
}

#[test]
fn a_recorded_finding_is_listed_newest_first_and_open() {
    let dir = TempDir::new();
    let a = record(dir.path(), new_finding("test_case", "First")).unwrap();
    let b = record(dir.path(), new_finding("spec", "Second")).unwrap();
    assert_ne!(a.id, b.id, "ids are unique within a store");
    assert_eq!(a.status, "open");
    assert!(!a.created_at.is_empty());
    let all = list(dir.path(), "acme", "Web");
    assert_eq!(all.iter().map(|f| f.title.as_str()).collect::<Vec<_>>(), vec!["Second", "First"]);
    let raw = std::fs::read_to_string(dir.path().join("findings.json")).unwrap();
    let parsed: Vec<Finding> = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed.len(), 2);
}

#[test]
fn listing_is_scoped_to_the_org_and_project() {
    let dir = TempDir::new();
    record(dir.path(), new_finding("code", "Ours")).unwrap();
    record(dir.path(), NewFinding { project: "Mobile".into(), ..new_finding("code", "Theirs") }).unwrap();
    let ours = list(dir.path(), "acme", "Web");
    assert_eq!(ours.len(), 1);
    assert_eq!(ours[0].title, "Ours");
    assert!(list(dir.path(), "acme", "Nope").is_empty());
}

#[test]
fn kind_and_title_are_validated() {
    let dir = TempDir::new();
    let err = record(dir.path(), new_finding("vibes", "x")).unwrap_err();
    assert!(err.contains("test_case, spec or code"), "{err}");
    let err = record(dir.path(), new_finding("spec", "   ")).unwrap_err();
    assert!(err.contains("title"), "{err}");
    assert!(list(dir.path(), "acme", "Web").is_empty(), "nothing invalid is written");
}

#[test]
fn status_changes_and_removal_are_by_id() {
    let dir = TempDir::new();
    let f = record(dir.path(), new_finding("spec", "Ambiguous cut-off")).unwrap();
    let done = set_status(dir.path(), &f.id, "resolved").unwrap();
    assert_eq!(done.status, "resolved");
    assert_eq!(list(dir.path(), "acme", "Web")[0].status, "resolved");
    let err = set_status(dir.path(), &f.id, "sideways").unwrap_err();
    assert!(err.contains("open or resolved"), "{err}");
    assert!(set_status(dir.path(), "no-such-id", "open").is_err());
    remove(dir.path(), &f.id).unwrap();
    assert!(list(dir.path(), "acme", "Web").is_empty());
    assert!(remove(dir.path(), &f.id).is_err(), "removing twice is an error, not silence");
}

#[test]
fn the_store_is_bounded_oldest_dropped_first() {
    let dir = TempDir::new();
    for i in 0..(CAP + 5) {
        record(dir.path(), new_finding("code", &format!("F{i}"))).unwrap();
    }
    let all = list(dir.path(), "acme", "Web");
    assert_eq!(all.len(), CAP);
    assert_eq!(all[0].title, format!("F{}", CAP + 4), "newest kept");
    assert_eq!(all[CAP - 1].title, "F5", "the five oldest are gone");
}

#[test]
fn a_corrupt_file_reads_as_empty_and_is_replaced_on_the_next_write() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("findings.json"), "{ not json").unwrap();
    assert!(list(dir.path(), "acme", "Web").is_empty());
    record(dir.path(), new_finding("spec", "After corruption")).unwrap();
    assert_eq!(list(dir.path(), "acme", "Web").len(), 1);
}

/// The bridge serves connections concurrently and the Tauri commands run
/// on other threads, so `record` has to be safe against interleaved
/// writers: without a lock held across load-and-save, two threads can
/// both read the same snapshot, both write, and the loser's write wipes
/// out everything the winner just added.
#[test]
fn concurrent_recorders_never_lose_a_write() {
    let dir = TempDir::new();
    let root = dir.path().to_path_buf();
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let root = root.clone();
            std::thread::spawn(move || {
                for i in 0..25 {
                    record(&root, new_finding("code", &format!("T{t}-{i}"))).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let all = list(dir.path(), "acme", "Web");
    assert_eq!(all.len(), 200, "no write was lost to an interleaved save");
    let raw = std::fs::read_to_string(dir.path().join("findings.json")).unwrap();
    let parsed: Vec<Finding> = serde_json::from_str(&raw).expect("the file always parses");
    assert_eq!(parsed.len(), 200);
}

/// The commands are thin over the store, but the root they use is the one
/// setup published - the same one the bridge writes through.
#[test]
fn the_commands_use_the_configured_root() {
    use v2_lib::findings::{configured_root, set_root};
    let dir = TempDir::new();
    set_root(dir.path().to_path_buf());
    assert_eq!(configured_root().as_deref(), Some(dir.path()));
    record(dir.path(), new_finding("code", "Via the store")).unwrap();
    let listed = v2_lib::commands::findings::list_findings_at(dir.path(), "acme", "Web");
    assert_eq!(listed.len(), 1);
}
