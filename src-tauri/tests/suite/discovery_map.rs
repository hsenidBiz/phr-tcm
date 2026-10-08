//! The per-project discovery map: what Auto Run has seen on the live app,
//! kept as paths and locators only.

use v2_lib::autorun::discovery_map::{
    forget_area, is_stale, load_map, map_path, mark_failed, path_only, record_matched, record_outcome, record_seen,
    record_write, seen_keys, seen_paths, WriteEntry, STALE_AFTER_MS,
};
use v2_lib::browser::locator::{LocatorStep, SeenKey, Target};
use v2_lib::browser::snapshot::SnapLine;

fn line(role: &str, name: &str) -> SnapLine {
    SnapLine {
        role: role.to_string(),
        name: name.to_string(),
        locator: Target::One(LocatorStep {
            role: Some(role.to_string()),
            name: Some(name.to_string()),
            ..LocatorStep::default()
        }),
        required: false,
    }
}

fn role_key(role: &str, name: &str) -> SeenKey {
    SeenKey::Role { role: role.to_string(), name: name.to_lowercase() }
}

#[test]
fn record_then_load_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let mut save = line("button", "Save");
    save.required = true;
    let lines = vec![
        save,
        line("textbox", "Name"),
        line("link", "Home"),
        line("table", "Rates"),
        line("dialog", "Confirm"),
        line("heading", "Rates"),
    ];
    record_seen(dir.path(), "Acme", "Web", Some("Leave"), "/leave", "Leave", &lines, Some("hr1"), true, 1000).unwrap();
    // Seeing the same lines again adds nothing.
    record_seen(dir.path(), "Acme", "Web", Some("Leave"), "/leave", "Leave", &lines, Some("hr1"), false, 2000).unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(map.areas.len(), 1);
    let a = &map.areas[0];
    assert_eq!(a.area, "Leave");
    assert_eq!(a.explored_at, Some(1000));
    assert_eq!(a.account.as_deref(), Some("hr1"));
    assert_eq!(a.pages.len(), 1);
    let p = &a.pages[0];
    assert_eq!((p.path.as_str(), p.title.as_str()), ("/leave", "Leave"));
    assert_eq!(p.elements.len(), 6);
    let kinds: Vec<&str> = p.elements.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["button", "field", "link", "table", "dialog", "other"]);
    assert!(p.elements[0].required);
    assert_eq!(p.elements[0].key, role_key("button", "save"));
}

#[test]
fn missing_map_is_empty_and_corrupt_map_says_so() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_map(dir.path(), "Acme", "Web").unwrap().areas.is_empty());
    let path = map_path(dir.path(), "Acme", "Web");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ not json").unwrap();
    let err = load_map(dir.path(), "Acme", "Web").unwrap_err();
    assert!(err.starts_with("the discovery map could not be read: "), "{err}");
}

#[test]
fn concurrent_recordings_both_survive() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let handles: Vec<_> = ["Alpha", "Beta"]
        .into_iter()
        .map(|name| {
            let root = root.clone();
            std::thread::spawn(move || {
                for i in 0..10 {
                    let l = vec![line("button", &format!("{name}{i}"))];
                    record_seen(&root, "Acme", "Web", Some("A"), "/p", "P", &l, None, false, 1).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let map = load_map(&root, "Acme", "Web").unwrap();
    assert_eq!(map.areas[0].pages[0].elements.len(), 20);
}

#[test]
fn the_map_stores_paths_and_locators_only() {
    let dir = tempfile::tempdir().unwrap();
    record_seen(
        dir.path(),
        "Acme",
        "Web",
        Some("Leave"),
        "https://h/x/leave?token=abc#f",
        "Leave",
        &[line("button", "Save")],
        None,
        true,
        5,
    )
    .unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(map.areas[0].pages[0].path, "/x/leave");
    let text = std::fs::read_to_string(map_path(dir.path(), "Acme", "Web")).unwrap();
    for banned in ["token", "abc", "https://h"] {
        assert!(!text.contains(banned), "{banned} in {text}");
    }
    assert_eq!(path_only("https://h:8080/a/b?q=1"), "/a/b");
    assert_eq!(path_only("/a/b#frag"), "/a/b");
    assert_eq!(path_only("https://h"), "/");
}

#[test]
fn stale_after_thirty_days_or_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Go")], None, true, 1000).unwrap();
    let a = load_map(dir.path(), "Acme", "Web").unwrap().areas.remove(0);
    assert!(!is_stale(&a, 1000 + STALE_AFTER_MS));
    assert!(is_stale(&a, 1000 + STALE_AFTER_MS + 1));
    mark_failed(dir.path(), "Acme", "Web", "A").unwrap();
    let a = load_map(dir.path(), "Acme", "Web").unwrap().areas.remove(0);
    assert!(a.failed_since);
    assert!(is_stale(&a, 1001));
    // An area never explored is stale too.
    mark_failed(dir.path(), "Acme", "Web", "B").unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    assert!(is_stale(map.areas.iter().find(|a| a.area == "B").unwrap(), 0));
}

#[test]
fn discovering_clears_failed_since_and_sets_explored_at() {
    let dir = tempfile::tempdir().unwrap();
    mark_failed(dir.path(), "Acme", "Web", "A").unwrap();
    // A plain sighting leaves the failure and the date alone.
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Go")], Some("u"), false, 50).unwrap();
    let a = load_map(dir.path(), "Acme", "Web").unwrap().areas.remove(0);
    assert!(a.failed_since);
    assert_eq!(a.explored_at, None);
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Go")], Some("u"), true, 99).unwrap();
    let a = load_map(dir.path(), "Acme", "Web").unwrap().areas.remove(0);
    assert!(!a.failed_since);
    assert_eq!(a.explored_at, Some(99));
    assert_eq!(a.account.as_deref(), Some("u"));
}

#[test]
fn forget_area_removes_only_that_area() {
    let dir = tempfile::tempdir().unwrap();
    for area in ["A", "B"] {
        record_seen(dir.path(), "Acme", "Web", Some(area), "/p", "P", &[line("button", "Go")], None, true, 1).unwrap();
    }
    forget_area(dir.path(), "Acme", "Web", "A").unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let names: Vec<&str> = map.areas.iter().map(|a| a.area.as_str()).collect();
    assert_eq!(names, ["B"]);
}

#[test]
fn outcomes_are_capped_at_two_hundred() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..205 {
        record_outcome(dir.path(), "Acme", "Web", "A", &format!("line {i}")).unwrap();
    }
    let a = load_map(dir.path(), "Acme", "Web").unwrap().areas.remove(0);
    assert_eq!(a.outcomes.len(), 200);
    assert_eq!(a.outcomes[0], "line 5");
    assert_eq!(a.outcomes[199], "line 204");
}

#[test]
fn seen_keys_include_the_unattributed_bucket() {
    let dir = tempfile::tempdir().unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/a", "A", &[line("button", "InA")], None, true, 1).unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("B"), "/b", "B", &[line("button", "InB")], None, true, 1).unwrap();
    record_seen(dir.path(), "Acme", "Web", None, "/c", "C", &[line("button", "Loose")], None, false, 1).unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let keys = seen_keys(&map, &["A"]);
    assert!(keys.contains(&role_key("button", "ina")));
    assert!(keys.contains(&role_key("button", "loose")));
    assert!(!keys.contains(&role_key("button", "inb")));
    let paths = seen_paths(&map);
    assert_eq!(paths.len(), 3);
    assert!(paths.contains("/b"));
}

#[test]
fn matched_links_are_added_once_and_writes_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Save")], None, true, 1).unwrap();
    let chain = Target::Chain(vec![
        LocatorStep { role: Some("dialog".into()), name: Some("Confirm".into()), ..LocatorStep::default() },
        LocatorStep { role: Some("button".into()), name: Some("Save".into()), ..LocatorStep::default() },
    ]);
    record_matched(dir.path(), "Acme", "Web", Some("A"), "https://h/p?x=1", &chain, 2).unwrap();
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &chain, 3).unwrap();
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &Target::from("#id"), 4).unwrap();
    record_write(
        dir.path(),
        "Acme",
        "Web",
        "A",
        WriteEntry { method: "POST".into(), path: "/api/x".into(), at: 7, step: "Save".into() },
    )
    .unwrap();
    let a = load_map(dir.path(), "Acme", "Web").unwrap().areas.remove(0);
    assert_eq!(a.pages.len(), 1);
    let els = &a.pages[0].elements;
    assert_eq!(els.len(), 3, "{els:?}");
    assert_eq!(els[1].kind, "dialog");
    assert_eq!(els[2].key, SeenKey::Css("#id".into()));
    assert_eq!(els[2].kind, "other");
    assert_eq!(a.writes.len(), 1);
}
