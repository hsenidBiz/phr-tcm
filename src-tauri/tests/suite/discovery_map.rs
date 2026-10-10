//! The per-project discovery map: what Auto Run has seen on the live app,
//! kept as paths and locators only.

use v2_lib::autorun::discovery_map::{
    forget_area, is_stale, load_map, map_path, mark_failed, path_only, record_matched, record_outcome, record_seen,
    record_write, seen_keys, seen_paths, WriteEntry, STALE_AFTER_MS,
};
use v2_lib::autorun::quirks::record_run_evidence;
use v2_lib::autorun::store::save_script;
use v2_lib::autorun::{CaseRecord, CaseScript, StepScript};
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
    record_seen(dir.path(), "Acme", "Web", Some("Leave"), "/leave", "Leave", &lines, Some("hr1"), Some(1000), 1000).unwrap();
    // Seeing the same lines again adds nothing.
    record_seen(dir.path(), "Acme", "Web", Some("Leave"), "/leave", "Leave", &lines, Some("hr1"), None, 2000).unwrap();
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
    assert!(err.starts_with("The discovery map projects/") && err.contains("is damaged and could not be read"), "{err}");
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
                    record_seen(&root, "Acme", "Web", Some("A"), "/p", "P", &l, None, None, 1).unwrap();
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
        Some(5),
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
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Go")], None, Some(1000), 1000).unwrap();
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
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Go")], Some("u"), None, 50).unwrap();
    let a = load_map(dir.path(), "Acme", "Web").unwrap().areas.remove(0);
    assert!(a.failed_since);
    assert_eq!(a.explored_at, None);
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Go")], Some("u"), Some(99), 99).unwrap();
    let a = load_map(dir.path(), "Acme", "Web").unwrap().areas.remove(0);
    assert!(!a.failed_since);
    assert_eq!(a.explored_at, Some(99));
    assert_eq!(a.account.as_deref(), Some("u"));
}

#[test]
fn forget_area_removes_only_that_area() {
    let dir = tempfile::tempdir().unwrap();
    for area in ["A", "B"] {
        record_seen(dir.path(), "Acme", "Web", Some(area), "/p", "P", &[line("button", "Go")], None, Some(1), 1).unwrap();
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
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/a", "A", &[line("button", "InA")], None, Some(1), 1).unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("B"), "/b", "B", &[line("button", "InB")], None, Some(1), 1).unwrap();
    record_seen(dir.path(), "Acme", "Web", None, "/c", "C", &[line("button", "Loose")], None, None, 1).unwrap();
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
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Save")], None, Some(1), 1).unwrap();
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

#[test]
fn an_iframe_chain_puts_every_link_in_seen_keys() {
    let dir = tempfile::tempdir().unwrap();
    let chain = Target::Chain(vec![
        LocatorStep { css: Some("iframe#pay".into()), ..LocatorStep::default() },
        LocatorStep { role: Some("button".into()), name: Some("Save".into()), ..LocatorStep::default() },
    ]);
    let framed = SnapLine { role: "button".into(), name: "Save".into(), locator: chain, required: false };
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[framed, line("button", "Save")], None, Some(1), 1).unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    // The same name in the page and in the frame stays two elements.
    assert_eq!(map.areas[0].pages[0].elements.len(), 2);
    let keys = seen_keys(&map, &["A"]);
    assert!(keys.contains(&SeenKey::Css("iframe#pay".into())));
    assert!(keys.contains(&role_key("button", "save")));
}

#[test]
fn record_write_stores_the_path_only() {
    let dir = tempfile::tempdir().unwrap();
    record_write(
        dir.path(),
        "Acme",
        "Web",
        "A",
        WriteEntry { method: "POST".into(), path: "https://h/api/x?token=abc".into(), at: 1, step: "Save".into() },
    )
    .unwrap();
    let text = std::fs::read_to_string(map_path(dir.path(), "Acme", "Web")).unwrap();
    assert!(text.contains("/api/x"));
    assert!(!text.contains("token") && !text.contains("https://h"), "{text}");
}

fn run_case(case_id: i32, proposed: &str) -> CaseRecord {
    CaseRecord {
        case_id,
        title: "a case".into(),
        verdict: String::new(),
        note: String::new(),
        steps: vec![],
        proposed: proposed.into(),
        reason: String::new(),
        duration_ms: None,
        account: None,
        retried: None,
        notice: None,
        page_errors_seen: 0, phases: None,
    }
}

fn saved_script(root: &std::path::Path, case_id: i32, area: Option<&str>) {
    let script = CaseScript {
        case_id,
        title: "a case".into(),
        account: None,
        area: area.map(str::to_string),
        steps: vec![StepScript { step_number: 2, actions: vec![], unchecked: None }],
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None,
        ignore_page_errors: vec![], organization: None, project: None, checked: false,
    };
    save_script(root, &script).unwrap();
}

#[test]
fn a_failed_case_marks_its_area_stale() {
    let dir = tempfile::tempdir().unwrap();
    saved_script(dir.path(), 7, Some("Orders"));
    record_run_evidence(dir.path(), "Acme", "Web", &[run_case(7, "Failed")], 10);
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    assert!(map.areas.iter().find(|a| a.area == "Orders").unwrap().failed_since);
}

#[test]
fn a_passed_case_does_not() {
    let dir = tempfile::tempdir().unwrap();
    saved_script(dir.path(), 7, Some("Orders"));
    record_run_evidence(dir.path(), "Acme", "Web", &[run_case(7, "Passed")], 10);
    assert!(load_map(dir.path(), "Acme", "Web").unwrap().areas.is_empty());
}

#[test]
fn a_case_without_an_area_marks_nothing() {
    let dir = tempfile::tempdir().unwrap();
    saved_script(dir.path(), 7, None);
    saved_script(dir.path(), 8, Some("  "));
    record_run_evidence(dir.path(), "Acme", "Web", &[run_case(7, "Failed"), run_case(8, "Failed"), run_case(9, "Failed")], 10);
    assert!(load_map(dir.path(), "Acme", "Web").unwrap().areas.is_empty());
}

// ---------------------------------------- final review: area names, damage

/// Records an area named `name` under module `module` in the project's
/// areas file, as the Areas dialog does.
fn recorded_area(root: &std::path::Path, name: &str, module: &str) {
    v2_lib::autorun::nav::put_path(
        root,
        "Acme",
        "Web",
        v2_lib::autorun::nav::ModulePath {
            area: name.to_string(),
            module: module.to_string(),
            clicks: vec![Target::One(LocatorStep {
                role: Some("link".into()),
                name: Some(name.to_string()),
                ..LocatorStep::default()
            })],
            arrived: "/hr/leave".to_string(),
            recorded: "2026-10-08T10:00:00Z".to_string(),
            start: String::new(),
            made_by: v2_lib::autorun::nav::MadeBy::Person,
        },
    )
    .unwrap();
}

/// Finding 2: what a discovery in "leave" saw counts for a script that
/// names "Leave", and is filed under the recorded area's own name. With
/// no area recorded, the names still compare ignoring case and spaces.
#[test]
#[allow(non_snake_case)]
fn discovering_leave_counts_for_a_script_naming_Leave() {
    let dir = tempfile::tempdir().unwrap();
    recorded_area(dir.path(), "Leave", "HR");
    record_seen(dir.path(), "Acme", "Web", Some(" leave "), "/hr/leave", "", &[line("button", "Apply")], Some("hr1"), Some(1000), 1000)
        .unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(map.areas.iter().map(|a| a.area.as_str()).collect::<Vec<_>>(), ["Leave"]);
    assert!(seen_keys(&map, &["Leave"]).contains(&role_key("button", "apply")));

    let bare = tempfile::tempdir().unwrap();
    record_seen(bare.path(), "Acme", "Web", Some("leave"), "/hr/leave", "", &[line("button", "Apply")], None, Some(1000), 1000).unwrap();
    record_matched(bare.path(), "Acme", "Web", Some("LEAVE"), "/hr/leave", &Target::One(LocatorStep {
        role: Some("button".into()),
        name: Some("Cancel".into()),
        ..LocatorStep::default()
    }), 1000)
    .unwrap();
    let map = load_map(bare.path(), "Acme", "Web").unwrap();
    assert_eq!(map.areas.len(), 1, "one area, however it was spelled: {:?}", map.areas);
    let keys = seen_keys(&map, &["Leave"]);
    assert!(keys.contains(&role_key("button", "apply")) && keys.contains(&role_key("button", "cancel")));
    // The script names "Leave"; the map says "leave": still explored.
    let section = v2_lib::autorun::discovery_map::explore_section(&["Leave"], &map, 1000);
    assert_eq!(section, "", "a fresh map under another case was listed to explore: {section}");
    forget_area(bare.path(), "Acme", "Web", "LEAVE").unwrap();
    assert!(load_map(bare.path(), "Acme", "Web").unwrap().areas.is_empty(), "Forget map missed the area");
}

/// Finding 2: a failed run marks the recorded area stale even when the
/// script spells it in another case, and makes no second area.
#[test]
fn a_failed_run_marks_the_area_stale_whatever_its_case() {
    let dir = tempfile::tempdir().unwrap();
    recorded_area(dir.path(), "Leave", "HR");
    record_seen(dir.path(), "Acme", "Web", Some("Leave"), "/hr/leave", "", &[line("button", "Apply")], Some("hr1"), Some(5), 5)
        .unwrap();
    saved_script(dir.path(), 7, Some("leave"));
    record_run_evidence(dir.path(), "Acme", "Web", &[run_case(7, "Failed")], 10);
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(map.areas.len(), 1, "{:?}", map.areas);
    assert!(map.areas[0].failed_since, "Leave stayed fresh after its script failed");
    assert!(is_stale(&map.areas[0], 10));

    // With no area recorded, an existing entry is still found by its key.
    let bare = tempfile::tempdir().unwrap();
    record_seen(bare.path(), "Acme", "Web", Some("Leave"), "/hr/leave", "", &[line("button", "Apply")], None, Some(5), 5).unwrap();
    mark_failed(bare.path(), "Acme", "Web", "LEAVE ").unwrap();
    let map = load_map(bare.path(), "Acme", "Web").unwrap();
    assert_eq!(map.areas.len(), 1, "{:?}", map.areas);
    assert!(map.areas[0].failed_since);
}

/// Finding 6: only what a discovery sees in a named area marks it
/// explored; the bucket for no area never is.
#[test]
fn a_discovery_with_no_area_marks_nothing_explored() {
    let dir = tempfile::tempdir().unwrap();
    record_seen(dir.path(), "Acme", "Web", None, "/", "", &[line("link", "Home")], Some("hr1"), Some(5), 5).unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(map.areas[0].area, "");
    assert_eq!(map.areas[0].explored_at, None);
}

/// Finding 9: a damaged map names its file by its project-relative name,
/// never a full path. Reset map moves it aside - never deleting it - and
/// discovery starts an empty one; a map that reads is not reset.
#[test]
fn a_damaged_map_names_its_file_and_reset_map_moves_it_aside() {
    use v2_lib::autorun::discovery_map::{map_file_name, reset_map};
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(reset_map(dir.path(), "Acme", "Web", 1).unwrap(), None, "no file, nothing to move");

    record_seen(dir.path(), "Acme", "Web", Some("Leave"), "/hr/leave", "", &[line("button", "Apply")], None, Some(5), 5).unwrap();
    let refused = reset_map(dir.path(), "Acme", "Web", 1).unwrap_err();
    assert!(refused.contains("Forget map"), "{refused}");
    assert_eq!(load_map(dir.path(), "Acme", "Web").unwrap().areas.len(), 1, "a readable map was reset");

    let path = map_path(dir.path(), "Acme", "Web");
    std::fs::write(&path, "{ not a map").unwrap();
    let file = map_file_name("Acme", "Web");
    assert!(file.starts_with("projects/") && file.ends_with("-map.json"), "{file}");
    let why = load_map(dir.path(), "Acme", "Web").unwrap_err();
    assert!(why.contains(&file), "the refusal does not name the file: {why}");
    assert!(why.contains("Reset map"), "the refusal gives no way out: {why}");
    let full = dir.path().to_string_lossy().to_string();
    assert!(!why.contains(&full), "a full path reached the sentence: {why}");
    assert!(forget_area(dir.path(), "Acme", "Web", "Leave").is_err(), "a damaged map is never overwritten silently");

    let aside = reset_map(dir.path(), "Acme", "Web", 1234).unwrap().expect("the file was moved");
    assert!(aside.starts_with("projects/") && aside.ends_with("-map.corrupt-1234.json"), "{aside}");
    assert_eq!(std::fs::read_to_string(dir.path().join(&aside)).unwrap(), "{ not a map", "the damaged file was not kept");
    assert!(!path.exists());
    assert!(load_map(dir.path(), "Acme", "Web").unwrap().areas.is_empty());
    record_seen(dir.path(), "Acme", "Web", Some("Leave"), "/hr/leave", "", &[line("button", "Apply")], None, Some(5), 5).unwrap();
    assert_eq!(load_map(dir.path(), "Acme", "Web").unwrap().areas.len(), 1, "saves work again after the reset");
}

// ------------------------------------------ exploring again replaces a page

fn names_on(dir: &std::path::Path, path: &str) -> Vec<String> {
    let map = load_map(dir, "Acme", "Web").unwrap();
    let page = map.areas.iter().flat_map(|a| a.pages.iter()).find(|p| p.path == path).expect("no such page");
    page.elements.iter().map(|e| e.name.clone()).collect()
}

/// A discovery's read of a page is what the page holds now: an element a
/// later discovery no longer sees is dropped, on that page only.
#[test]
fn re_exploring_a_page_drops_what_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let first = [line("button", "Save"), line("button", "Old")];
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &first, None, Some(1000), 1000).unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/q", "Q", &[line("button", "Other")], None, Some(1000), 1000).unwrap();
    let again = [line("button", "Save"), line("link", "New")];
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &again, None, Some(2000), 2000).unwrap();
    assert_eq!(names_on(dir.path(), "/p"), ["Save", "New"]);
    assert_eq!(names_on(dir.path(), "/q"), ["Other"], "another page was touched");
    // A second read in the same discovery replaces what only a read showed.
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("link", "New")], None, Some(2000), 2100).unwrap();
    assert_eq!(names_on(dir.path(), "/p"), ["New"]);
}

fn deep() -> Target {
    Target::One(LocatorStep { role: Some("button".into()), name: Some("Deep".into()), ..LocatorStep::default() })
}

/// A locator a probe or a try matched in this discovery stays, though the
/// page read was cut off before it.
#[test]
fn a_locator_matched_earlier_in_the_same_discovery_survives_a_re_read() {
    let dir = tempfile::tempdir().unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Save")], None, Some(1000), 1000).unwrap();
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &deep(), 1500).unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Save")], None, Some(1000), 1800).unwrap();
    assert_eq!(names_on(dir.path(), "/p"), ["Save", "Deep"]);
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let kept = map.areas[0].pages[0].elements.iter().find(|e| e.name == "Deep").unwrap();
    assert_eq!(kept.seen_at, 1500);
    // Matching an element a read already holds stamps it, so it survives too.
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &Target::One(LocatorStep {
        role: Some("button".into()),
        name: Some("Save".into()),
        ..LocatorStep::default()
    }), 1900)
    .unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[], None, Some(1000), 1950).unwrap();
    assert_eq!(names_on(dir.path(), "/p"), ["Save", "Deep"]);
}

#[test]
fn a_locator_matched_in_an_older_discovery_is_dropped_on_re_read() {
    let dir = tempfile::tempdir().unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Save")], None, Some(1000), 1000).unwrap();
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &deep(), 1500).unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Save")], None, Some(5000), 5000).unwrap();
    assert_eq!(names_on(dir.path(), "/p"), ["Save"]);
}

/// A page read while healing or replaying adds what it shows and drops
/// nothing: it is not a discovery.
#[test]
fn healing_reads_only_add() {
    let dir = tempfile::tempdir().unwrap();
    let first = [line("button", "Save"), line("button", "Old")];
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &first, None, Some(1000), 1000).unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Save")], None, None, 2000).unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("link", "New")], None, None, 3000).unwrap();
    assert_eq!(names_on(dir.path(), "/p"), ["Save", "Old", "New"]);
}

// ------------------------------------------------------------- map size

/// Two records of one kind are one page: a path segment that is an id
/// (all digits, a GUID, or 16 or more hex characters) is kept as `:id`.
/// The save-request log keeps the path as it was sent.
#[test]
fn addresses_that_differ_only_by_ids_are_one_page() {
    use v2_lib::autorun::discovery_map::page_path;
    let dir = tempfile::tempdir().unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "https://h/leave/12345/edit?x=1", "", &[line("button", "Save")], None, None, 1)
        .unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/leave/98765/edit", "", &[line("button", "Cancel")], None, None, 2).unwrap();
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/leave/555/edit", &Target::from("#id"), 3).unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let pages: Vec<&str> = map.areas[0].pages.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(pages, ["/leave/:id/edit"]);
    assert_eq!(map.areas[0].pages[0].elements.len(), 3);
    assert!(seen_paths(&map).contains("/leave/:id/edit"));

    assert_eq!(page_path("/x/3F2504E0-4F89-11D3-9A0C-0305E82C3301/view"), "/x/:id/view");
    assert_eq!(page_path("/h/0123456789abcdef"), "/h/:id");
    assert_eq!(page_path("/h/0123456789abcde"), "/h/0123456789abcde", "15 hex characters are a name");
    assert_eq!(page_path("/api/v2/items/"), "/api/v2/items/");
    assert_eq!(page_path("https://h/a/7?q=1#f"), "/a/:id");
    assert_eq!(page_path("/"), "/");

    record_write(
        dir.path(),
        "Acme",
        "Web",
        "A",
        WriteEntry { method: "POST".into(), path: "/api/leave/12345".into(), at: 4, step: "Save".into() },
    )
    .unwrap();
    let a = load_map(dir.path(), "Acme", "Web").unwrap().areas.remove(0);
    assert_eq!(a.writes[0].path, "/api/leave/12345");
}

/// A map written before ids were collapsed still has its pages found: a
/// new sighting files under the collapsed path and takes the old page over.
#[test]
fn an_older_map_page_with_an_id_is_taken_over_by_its_collapsed_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = map_path(dir.path(), "Acme", "Web");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let old = serde_json::json!({ "areas": [{ "area": "A", "explored_at": null, "account": null, "failed_since": false,
        "pages": [{ "path": "/leave/42/edit", "title": "", "elements": [] }], "outcomes": [], "writes": [] }] });
    std::fs::write(&path, old.to_string()).unwrap();
    assert!(seen_paths(&load_map(dir.path(), "Acme", "Web").unwrap()).contains("/leave/:id/edit"));
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/leave/43/edit", "", &[line("button", "Save")], None, None, 1).unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let pages: Vec<&str> = map.areas[0].pages.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(pages, ["/leave/:id/edit"]);
}

#[test]
fn the_write_log_is_capped_at_500() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..505u64 {
        let w = WriteEntry { method: "POST".into(), path: "/api/x".into(), at: i, step: format!("step {i}") };
        record_write(dir.path(), "Acme", "Web", "A", w).unwrap();
    }
    let a = load_map(dir.path(), "Acme", "Web").unwrap().areas.remove(0);
    assert_eq!(a.writes.len(), 500);
    assert_eq!(a.writes[0].at, 5);
    assert_eq!(a.writes[499].at, 504);
}

#[test]
fn recording_nothing_new_does_not_rewrite_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = map_path(dir.path(), "Acme", "Web");
    let lines = [line("button", "Save"), line("link", "Home")];
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &lines, None, None, 1).unwrap();
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &Target::from("#id"), 5).unwrap();
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(60));

    // The same read, a part of it, the same match: nothing changes.
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &lines, None, None, 2).unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "https://h/p?q=1", "", &lines[..1], None, None, 3).unwrap();
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &Target::from("#id"), 5).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before, "the file was rewritten");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

    // Something new is written.
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "New")], None, None, 4).unwrap();
    assert_ne!(std::fs::read_to_string(&path).unwrap(), text);
}

// ------------------------------------- every distinct sighting is kept

/// The area's sightings: (key, last seen).
fn sightings_of(dir: &std::path::Path, area: &str) -> Vec<(SeenKey, u64)> {
    let map = load_map(dir, "Acme", "Web").unwrap();
    let a = map.areas.iter().find(|a| a.area == area).expect("no such area");
    a.sightings.iter().map(|s| (s.key.clone(), s.last_seen)).collect()
}

/// A wizard keeps one address for every step: its later steps replace the
/// page's elements, and 201 newer sightings and outcome lines come after.
/// The step seen first is still seen; a later discovery of the page that
/// no longer shows it replaces it.
#[test]
fn a_sighting_survives_201_newer_ones() {
    let dir = tempfile::tempdir().unwrap();
    let eval = "Step 2 of 9 \u{2014} Eval Rules";
    let evaluators = "Step 4 of 9 \u{2014} Evaluators";
    record_seen(dir.path(), "Acme", "Web", Some("Cycles"), "/cycles/new", "New", &[line("progressbar", eval)], None, Some(1000), 1000)
        .unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("Cycles"), "/cycles/new", "New", &[line("progressbar", evaluators)], None, Some(1000), 1100)
        .unwrap();
    assert_eq!(names_on(dir.path(), "/cycles/new"), [evaluators], "the page holds what it shows now");
    let many: Vec<SnapLine> = (0..201).map(|i| line("button", &format!("Action {i}"))).collect();
    record_seen(dir.path(), "Acme", "Web", Some("Cycles"), "/cycles/list", "List", &many, None, None, 1200).unwrap();
    for i in 0..201 {
        record_outcome(dir.path(), "Acme", "Web", "Cycles", &format!("line {i}")).unwrap();
    }
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let keys = seen_keys(&map, &["Cycles"]);
    assert!(keys.contains(&role_key("progressbar", eval)), "the first sighting was let go");
    assert!(keys.contains(&role_key("button", "action 200")));
    assert_eq!(sightings_of(dir.path(), "Cycles").len(), 203);

    // A later discovery reads the page and no longer sees it: replaced.
    record_seen(dir.path(), "Acme", "Web", Some("Cycles"), "/cycles/new", "New", &[line("progressbar", evaluators)], None, Some(5000), 5000)
        .unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let keys = seen_keys(&map, &["Cycles"]);
    assert!(!keys.contains(&role_key("progressbar", eval)));
    assert!(keys.contains(&role_key("button", "action 0")), "another page was touched");
}

/// The same link seen again is one sighting, stamped with the latest time
/// a discovery or a match saw it; an older stamp never takes it back.
#[test]
fn repeated_sightings_are_kept_once() {
    let dir = tempfile::tempdir().unwrap();
    let save = [line("button", "Save")];
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &save, None, Some(1000), 1000).unwrap();
    let as_matched = Target::One(LocatorStep { role: Some("button".into()), name: Some("SAVE".into()), ..LocatorStep::default() });
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &as_matched, 1500).unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &save, None, None, 2000).unwrap();
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &as_matched, 1200).unwrap();
    assert_eq!(sightings_of(dir.path(), "A"), [(role_key("button", "save"), 1500)]);
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/q", "Q", &save, None, Some(3000), 3000).unwrap();
    assert_eq!(sightings_of(dir.path(), "A"), [(role_key("button", "save"), 3000)]);
}

/// A map written before sightings were kept loads, its elements still
/// count as seen, and the next sighting is added to it.
#[test]
fn an_old_map_file_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "Save")], None, Some(1000), 1000).unwrap();
    let path = map_path(dir.path(), "Acme", "Web");
    let mut old: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for area in old["areas"].as_array_mut().unwrap() {
        area.as_object_mut().unwrap().remove("sightings").expect("this version writes sightings");
        for page in area["pages"].as_array_mut().unwrap() {
            for e in page["elements"].as_array_mut().unwrap() {
                e.as_object_mut().unwrap().remove("seen_at");
            }
        }
    }
    std::fs::write(&path, format!("\u{feff}{old}")).unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    assert!(map.areas[0].sightings.is_empty());
    assert!(seen_keys(&map, &["A"]).contains(&role_key("button", "save")));
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("link", "Home")], None, None, 2000).unwrap();
    assert_eq!(sightings_of(dir.path(), "A"), [(role_key("link", "home"), 2000)]);
}

/// Forget map clears an area's sightings with the rest of it, and only
/// that area's.
#[test]
fn forget_map_still_clears_an_area() {
    let dir = tempfile::tempdir().unwrap();
    for area in ["A", "B"] {
        record_seen(dir.path(), "Acme", "Web", Some(area), "/p", "P", &[line("button", area)], None, Some(1), 1).unwrap();
    }
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &Target::from("#only-a"), 3).unwrap();
    assert!(seen_keys(&load_map(dir.path(), "Acme", "Web").unwrap(), &["A"]).contains(&role_key("button", "a")));
    forget_area(dir.path(), "Acme", "Web", "a").unwrap();
    let map = load_map(dir.path(), "Acme", "Web").unwrap();
    let names: Vec<&str> = map.areas.iter().map(|a| a.area.as_str()).collect();
    assert_eq!(names, ["B"]);
    assert!(seen_keys(&map, &["A"]).is_empty());
    assert!(seen_keys(&map, &["B"]).contains(&role_key("button", "b")));
}

/// Past the safety ceiling, the least recently seen sighting goes: not the
/// first one kept, which a match has seen since.
#[test]
fn the_ceiling_evicts_the_least_recently_seen() {
    use v2_lib::autorun::discovery_map::MAX_SIGHTINGS;
    let dir = tempfile::tempdir().unwrap();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/p", "P", &[line("button", "First")], None, None, 1).unwrap();
    let many: Vec<SnapLine> = (0..MAX_SIGHTINGS - 2).map(|i| line("button", &format!("N{i}"))).collect();
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/q", "Q", &many, None, None, 2).unwrap();
    let first = Target::One(LocatorStep { role: Some("button".into()), name: Some("First".into()), ..LocatorStep::default() });
    record_matched(dir.path(), "Acme", "Web", Some("A"), "/p", &first, 3).unwrap();
    assert_eq!(sightings_of(dir.path(), "A").len(), MAX_SIGHTINGS - 1);
    record_seen(dir.path(), "Acme", "Web", Some("A"), "/r", "R", &[line("button", "New1"), line("button", "New2")], None, None, 4)
        .unwrap();
    let kept = sightings_of(dir.path(), "A");
    assert_eq!(kept.len(), MAX_SIGHTINGS);
    let has = |name: &str| kept.iter().any(|(k, _)| *k == role_key("button", name));
    assert!(has("first"), "the first one kept was seen since");
    assert!(!has("n0"), "the least recently seen goes");
    assert!(has("n1") && has("new1") && has("new2"));
}
