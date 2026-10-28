//! Every successful write to the data an assistant adds through the AI bridge
//! (areas, components, the discovery map and its summary, quirks, test files,
//! proposed accounts) tells the data listeners which kind changed, whoever
//! made it: the app registers one at startup that says so to the window
//! (`AutorunDataChanged`), so the Auto Run screen reads that data again
//! without a reload. A refused or no-op write says nothing.
//!
//! The listeners are process-wide, so each test keeps only what was said
//! about its own tempdir.

use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::data_changed;
use v2_lib::autorun::{components, discovery_map, mapping_summary, nav, quirks};
use v2_lib::events::AutorunDataKind as Kind;

/// What the listener heard about this root, one entry per change.
fn listen(root: &Path) -> Arc<Mutex<Vec<Vec<Kind>>>> {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let mine: PathBuf = root.to_path_buf();
    let into = Arc::clone(&heard);
    data_changed::on_data_changed(move |at, kinds| {
        if at == mine {
            into.lock().unwrap().push(kinds.to_vec());
        }
    });
    heard
}

fn component(name: &str) -> components::Component {
    serde_json::from_value(json!({
        "name": name, "description": "d", "inputs": [],
        "actions": [{ "kind": "check_text", "value": "ok" }]
    }))
    .unwrap()
}

#[test]
fn saving_areas_says_areas() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    let file: nav::NavFile = serde_json::from_value(json!({})).unwrap();
    nav::save_nav(dir.path(), "acme", "Web", &file).unwrap();
    assert_eq!(*heard.lock().unwrap(), vec![vec![Kind::Areas]]);
}

#[test]
fn a_refused_areas_save_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    let file: nav::NavFile = serde_json::from_value(json!({})).unwrap();
    assert!(nav::save_nav(dir.path(), "", "Web", &file).is_err());
    assert!(heard.lock().unwrap().is_empty());
}

#[test]
fn components_saved_removed_and_reset_say_components() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    components::put(dir.path(), "acme", "Web", component("sign in")).unwrap();
    // Saving the same thing again changes nothing: nothing said.
    components::put(dir.path(), "acme", "Web", component("sign in")).unwrap();
    components::remove(dir.path(), "acme", "Web", "sign in").unwrap();
    assert_eq!(*heard.lock().unwrap(), vec![vec![Kind::Components], vec![Kind::Components]]);

    let path = components::components_path(dir.path(), "acme", "Web");
    std::fs::write(&path, "{ not json").unwrap();
    components::reset_components(dir.path(), "acme", "Web").unwrap();
    assert_eq!(heard.lock().unwrap().len(), 3);
    assert_eq!(heard.lock().unwrap()[2], vec![Kind::Components]);
}

#[test]
fn a_reset_with_nothing_to_reset_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    assert!(components::reset_components(dir.path(), "acme", "Web").is_err());
    assert!(heard.lock().unwrap().is_empty());
}

#[test]
fn a_discovery_map_write_says_map_and_a_no_op_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    discovery_map::mark_failed(dir.path(), "acme", "Web", "Leave").unwrap();
    assert_eq!(*heard.lock().unwrap(), vec![vec![Kind::Map]]);
    // Already marked: the map is as it was, so nothing is written or said.
    discovery_map::mark_failed(dir.path(), "acme", "Web", "Leave").unwrap();
    assert_eq!(heard.lock().unwrap().len(), 1);
    // Forgetting an area the map never had changes nothing either.
    discovery_map::forget_area(dir.path(), "acme", "Web", "Nowhere").unwrap();
    assert_eq!(heard.lock().unwrap().len(), 1);
}

#[test]
fn resetting_a_damaged_map_says_map() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    let path = discovery_map::map_path(dir.path(), "acme", "Web");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ not json").unwrap();
    assert!(discovery_map::reset_map(dir.path(), "acme", "Web", 1).unwrap().is_some());
    assert_eq!(*heard.lock().unwrap(), vec![vec![Kind::Map]]);
    // Nothing left to move aside: nothing said.
    assert!(discovery_map::reset_map(dir.path(), "acme", "Web", 2).unwrap().is_none());
    assert_eq!(heard.lock().unwrap().len(), 1);
}

#[test]
fn saving_the_mapping_summary_says_mapping_summary() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    let summary: mapping_summary::MappingSummary = serde_json::from_value(json!({
        "ran_at": 1, "modules": [], "added": [], "updated": [], "unchanged": [],
        "unreached": [], "blocked_writes": 0
    }))
    .unwrap();
    mapping_summary::save_summary(dir.path(), "acme", "Web", &summary).unwrap();
    assert_eq!(*heard.lock().unwrap(), vec![vec![Kind::MappingSummary]]);
}

#[test]
fn recording_a_quirk_says_quirks_and_a_refused_one_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    let quirk: quirks::Quirk =
        serde_json::from_value(json!({ "text": "the grid loads late", "by": "assistant", "at": "1" })).unwrap();
    quirks::save_quirks(dir.path(), "acme", "Web", &[quirk.clone()]).unwrap();
    assert_eq!(*heard.lock().unwrap(), vec![vec![Kind::Quirks]]);
    assert!(quirks::save_quirks(dir.path(), "", "Web", &[quirk]).is_err());
    assert_eq!(heard.lock().unwrap().len(), 1);
}

#[test]
fn saving_proposals_says_proposals() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    let one: v2_lib::environments::StoredProposal =
        serde_json::from_value(json!({ "key": "k", "label": "Admin", "username": "admin" })).unwrap();
    v2_lib::environments::save_proposals(dir.path(), "env-0a1b2c3d", &[one]).unwrap();
    assert_eq!(*heard.lock().unwrap(), vec![vec![Kind::Proposals]]);
    // Clearing the list removes the file, which is a change too.
    v2_lib::environments::save_proposals(dir.path(), "env-0a1b2c3d", &[]).unwrap();
    assert_eq!(heard.lock().unwrap().len(), 2);
    // Nothing left to remove: nothing said.
    v2_lib::environments::save_proposals(dir.path(), "env-0a1b2c3d", &[]).unwrap();
    assert_eq!(heard.lock().unwrap().len(), 2);
}

#[test]
fn a_test_file_saved_and_removed_say_test_files_and_a_refused_one_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    v2_lib::test_files::save_bytes(dir.path(), "notes.txt", b"hello", false, false).unwrap();
    assert_eq!(*heard.lock().unwrap(), vec![vec![Kind::TestFiles]]);
    // Already there and not replacing: refused, nothing said.
    assert!(v2_lib::test_files::save_bytes(dir.path(), "notes.txt", b"again", false, false).is_err());
    assert_eq!(heard.lock().unwrap().len(), 1);
    v2_lib::test_files::remove(dir.path(), "notes.txt").unwrap();
    assert_eq!(heard.lock().unwrap().len(), 2);
    assert!(v2_lib::test_files::remove(dir.path(), "notes.txt").is_err());
    assert_eq!(heard.lock().unwrap().len(), 2);
}

#[test]
fn a_listener_that_panics_cannot_break_the_write() {
    // The "a bad listener" panic message on stderr is expected: `notify` catches it. The listener
    // stays registered for the rest of the process, harmlessly - it only panics for this test's own
    // temp folder (`at == mine`), which no other test writes under, and `notify` catches that too.
    let dir = tempfile::tempdir().unwrap();
    let mine = dir.path().to_path_buf();
    data_changed::on_data_changed(move |at, _| {
        if at == mine {
            panic!("a bad listener");
        }
    });
    let file: nav::NavFile = serde_json::from_value(json!({})).unwrap();
    nav::save_nav(dir.path(), "acme", "Web", &file).unwrap();
}

/// The app's listener emits the window event; that event is registered,
/// so emitting it can never panic.
#[test]
fn the_event_is_registered_and_the_app_listens() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap().replace("\r\n", "\n");
    assert!(lib.contains("events::AutorunDataChanged,"), "listed in collect_events");
    assert!(lib.contains("autorun::data_changed::on_data_changed("), "the app registers its listener");
}
