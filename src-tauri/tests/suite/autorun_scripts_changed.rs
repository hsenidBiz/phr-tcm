//! Every write or removal of an Auto Run script tells the store's listeners
//! which cases changed, whoever made it: the app registers one at startup
//! that says so to the window (`AutorunScriptsChanged`), so the Auto Run
//! screen reads those scripts again without a reload.
//!
//! The listeners are process-wide, so each test keeps only what was said
//! about its own store root.

use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::store;
use v2_lib::autorun::{CaseScript, SuspectedDefect};

/// What the listener heard about this root, one entry per change.
fn listen(root: &Path) -> Arc<Mutex<Vec<Vec<i32>>>> {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let mine: PathBuf = root.to_path_buf();
    let into = Arc::clone(&heard);
    store::on_scripts_changed(move |at, ids| {
        if at == mine {
            into.lock().unwrap().push(ids.to_vec());
        }
    });
    heard
}

fn script(case_id: i32) -> CaseScript {
    serde_json::from_value(json!({ "case_id": case_id, "title": format!("case {case_id}"), "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ] }))
    .unwrap()
}

#[test]
fn a_bundle_save_names_every_case_it_wrote_once() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    store::save_scripts_atomically(dir.path(), &[script(1), script(2)]).unwrap();
    assert_eq!(*heard.lock().unwrap(), vec![vec![1, 2]]);
}

#[test]
fn a_refused_save_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    let empty: CaseScript = serde_json::from_value(json!({ "case_id": 3, "title": "t", "steps": [] })).unwrap();
    assert!(store::save_scripts_atomically(dir.path(), &[empty]).is_err());
    assert!(heard.lock().unwrap().is_empty());
}

#[test]
fn a_mark_set_and_cleared_each_say_so() {
    let dir = tempfile::tempdir().unwrap();
    store::save_script(dir.path(), &script(4)).unwrap();
    let heard = listen(dir.path());
    let mark = SuspectedDefect { step_number: 1, note: "the app".into(), marked_at: "1".into() };
    store::set_suspected_defect(dir.path(), 4, Some(mark)).unwrap();
    assert!(store::clear_suspected_defect_any(dir.path(), 4).unwrap());
    // Nothing to clear: nothing written, nothing said.
    assert!(!store::clear_suspected_defect_any(dir.path(), 4).unwrap());
    assert_eq!(*heard.lock().unwrap(), vec![vec![4], vec![4]]);
}

#[test]
fn clearing_scripts_names_only_the_cases_that_had_one() {
    let dir = tempfile::tempdir().unwrap();
    store::save_script(dir.path(), &script(5)).unwrap();
    let heard = listen(dir.path());
    assert_eq!(store::clear_scripts(dir.path(), &[5, 6]).unwrap(), 1);
    // Nothing left to remove: nothing said.
    assert_eq!(store::clear_scripts(dir.path(), &[5]).unwrap(), 0);
    assert_eq!(*heard.lock().unwrap(), vec![vec![5]]);
}

#[test]
fn a_raw_write_says_so_too() {
    let dir = tempfile::tempdir().unwrap();
    let heard = listen(dir.path());
    store::save_script(dir.path(), &script(7)).unwrap();
    assert_eq!(*heard.lock().unwrap(), vec![vec![7]]);
}

/// The app's listener emits the window event; that event is registered,
/// so emitting it can never panic.
#[test]
fn the_event_is_registered_and_the_app_listens() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap().replace("\r\n", "\n");
    assert!(lib.contains("events::AutorunScriptsChanged,"), "listed in collect_events");
    assert!(lib.contains("autorun::store::on_scripts_changed("), "the app registers its listener");
}
