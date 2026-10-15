//! The assistant reads a saved script back whole (`get_autorun_script`)
//! and deletes one (`delete_autorun_script`). A delete never removes the
//! file: it is moved aside into `deleted-scripts` beside the scripts with
//! its deletion time, and purged after 30 days. Refused while the case is
//! being replayed. Local files only: no request of any kind.

use serde_json::json;
use std::path::Path;
use std::sync::{Arc, Mutex};
use v2_lib::ai_bridge::{autorun_script_delete_at, autorun_script_read_at};
use v2_lib::autorun::store::{self, DELETED_KEPT_DAYS};
use v2_lib::autorun::CaseScript;

const DAY: u64 = 86_400_000;
const NOW: u64 = 1_800_000_000_000;

fn script(case_id: i32) -> CaseScript {
    serde_json::from_value(json!({ "case_id": case_id, "title": format!("case {case_id}"), "account": "hr.admin", "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ] }))
    .unwrap()
}

fn aside(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("deleted-scripts"))
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn read_answers_the_script_as_saved_and_it_saves_back_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    store::save_scripts_atomically(dir.path(), &[script(11)]).unwrap();
    let saved = store::load_script(dir.path(), 11).unwrap().unwrap();
    let (status, body) = autorun_script_read_at(dir.path(), "/autorun-script?case_id=11");
    assert_eq!(status, 200, "{body}");
    let read: CaseScript = serde_json::from_str(&body).unwrap();
    assert_eq!(serde_json::to_value(&read).unwrap(), serde_json::to_value(&saved).unwrap());
    assert_eq!(read.account.as_deref(), Some("hr.admin"));
}

#[test]
fn read_of_a_case_with_no_script_or_a_bad_id_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (status, body) = autorun_script_read_at(dir.path(), "/autorun-script?case_id=12");
    assert_eq!((status, body.as_str()), (404, "case 12 has no script on this machine"));
    for target in ["/autorun-script", "/autorun-script?case_id=x", "/autorun-script?case_id=-3"] {
        let (status, body) = autorun_script_read_at(dir.path(), target);
        assert_eq!(status, 400, "{target}: {body}");
        assert!(body.contains("case_id"), "{body}");
    }
}

#[test]
fn delete_moves_the_script_aside_with_its_deletion_time() {
    let dir = tempfile::tempdir().unwrap();
    store::save_script(dir.path(), &script(13)).unwrap();
    let before = std::fs::read(dir.path().join("scripts").join("case-13.json")).unwrap();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let (mine, into) = (dir.path().to_path_buf(), Arc::clone(&heard));
    store::on_scripts_changed(move |at, ids| {
        if at == mine {
            into.lock().unwrap().push(ids.to_vec());
        }
    });

    let (status, body) = autorun_script_delete_at(dir.path(), r#"{"case_id": 13}"#, NOW);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("kept aside on this machine for 30 days"), "{body}");
    assert!(!dir.path().join("scripts").join("case-13.json").exists(), "gone from the scripts");
    assert!(store::load_script(dir.path(), 13).unwrap().is_none());
    assert!(store::list_scripts(dir.path()).is_empty(), "and from every list");
    let kept = aside(dir.path());
    assert_eq!(kept, vec![format!("case-13-deleted-{NOW}.json")]);
    assert_eq!(std::fs::read(dir.path().join("deleted-scripts").join(&kept[0])).unwrap(), before, "kept as it was");
    assert_eq!(store::deleted_at(&kept[0]), Some(NOW));
    assert_eq!(*heard.lock().unwrap(), vec![vec![13]], "the Auto Run screen is told");

    // Nothing left to delete.
    let (status, body) = autorun_script_delete_at(dir.path(), r#"{"case_id": 13}"#, NOW);
    assert_eq!((status, body.as_str()), (404, "case 13 has no script on this machine"));
}

#[test]
fn a_case_deleted_twice_in_one_moment_keeps_both() {
    let dir = tempfile::tempdir().unwrap();
    store::save_script(dir.path(), &script(14)).unwrap();
    store::delete_script(dir.path(), 14, NOW).unwrap();
    store::save_script(dir.path(), &script(14)).unwrap();
    store::delete_script(dir.path(), 14, NOW).unwrap();
    assert_eq!(aside(dir.path()).len(), 2);
}

#[test]
fn purge_removes_only_what_was_deleted_more_than_30_days_ago() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("deleted-scripts");
    std::fs::create_dir_all(&bin).unwrap();
    let kept_for = DELETED_KEPT_DAYS * DAY;
    let old = NOW - kept_for - 1;
    let edge = NOW - kept_for;
    let young = NOW - DAY;
    for name in [
        format!("case-1-deleted-{old}.json"),
        format!("case-2-deleted-{edge}.json"),
        format!("case-3-deleted-{young}.json"),
        "notes.txt".to_string(),
        "case-4-deleted-soon.json".to_string(),
    ] {
        std::fs::write(bin.join(name), "{}").unwrap();
    }
    assert_eq!(store::purge_deleted_scripts(dir.path(), NOW), 1);
    assert_eq!(
        aside(dir.path()),
        vec![
            format!("case-2-deleted-{edge}.json"),
            format!("case-3-deleted-{young}.json"),
            "case-4-deleted-soon.json".to_string(),
            "notes.txt".to_string(),
        ]
    );
    // No folder yet: nothing to purge.
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(store::purge_deleted_scripts(empty.path(), NOW), 0);
}

#[test]
fn the_delete_route_purges_old_ones_first() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("deleted-scripts");
    std::fs::create_dir_all(&bin).unwrap();
    let old = NOW - (DELETED_KEPT_DAYS + 1) * DAY;
    std::fs::write(bin.join(format!("case-9-deleted-{old}.json")), "{}").unwrap();
    store::save_script(dir.path(), &script(15)).unwrap();
    assert_eq!(autorun_script_delete_at(dir.path(), r#"{"case_id": 15}"#, NOW).0, 200);
    assert_eq!(aside(dir.path()), vec![format!("case-15-deleted-{NOW}.json")]);
}

#[test]
fn a_delete_is_refused_while_the_case_is_replaying() {
    let dir = tempfile::tempdir().unwrap();
    store::save_script(dir.path(), &script(16)).unwrap();
    let going = store::replaying(16);
    assert!(store::is_replaying(16));
    assert!(!store::is_replaying(17));
    let (status, body) = autorun_script_delete_at(dir.path(), r#"{"case_id": 16}"#, NOW);
    assert_eq!(status, 409, "{body}");
    assert_eq!(body, store::replaying_refusal(16));
    assert!(dir.path().join("scripts").join("case-16.json").exists(), "left where it was");
    drop(going);
    assert!(!store::is_replaying(16));
    assert_eq!(autorun_script_delete_at(dir.path(), r#"{"case_id": 16}"#, NOW).0, 200);
}

#[test]
fn a_delete_needs_a_case_id() {
    let dir = tempfile::tempdir().unwrap();
    for body in ["{}", r#"{"case_id": "x"}"#, r#"{"case_id": 0}"#, "not json"] {
        let (status, said) = autorun_script_delete_at(dir.path(), body, NOW);
        assert_eq!(status, 400, "{body}: {said}");
    }
}

/// Both tools are listed, offered with Auto Run, and reach their routes;
/// neither is an HTTP DELETE, and nothing in the store sends a request.
#[test]
fn both_tools_are_listed_and_neither_sends_a_delete() {
    let mcp = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/mcp.rs")).unwrap().replace("\r\n", "\n");
    for tool in ["get_autorun_script", "delete_autorun_script"] {
        assert_eq!(v2_lib::ai_tools::module_of_tool(tool), Some(v2_lib::modules::Module::AutoRun), "{tool}");
        assert!(mcp.contains(&format!("\"name\": \"{tool}\"")), "{tool} listed");
    }
    assert!(mcp.contains(r#""delete_autorun_script" => call("POST", "/autorun-script-delete""#));
    assert!(mcp.contains(r#"call("GET", &format!("/autorun-script?case_id="#));
    let store_src =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/autorun/store.rs")).unwrap();
    assert!(!store_src.contains("reqwest") && !store_src.contains("AdoClient"), "the store sends nothing");
}

/// A replay notes its case before it reads the script, and the note and a
/// delete are one after the other: once noted, the script stays where it
/// is until the note goes. Run against a delete looping beside it.
#[test]
fn a_noted_replay_and_a_delete_never_interleave() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    store::save_script(&root, &script(18)).unwrap();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let deleter = {
        let (root, stop) = (root.clone(), Arc::clone(&stop));
        std::thread::spawn(move || {
            let mut deleted = 0;
            let mut now = NOW;
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                if store::delete_script(&root, 18, now).is_ok() {
                    deleted += 1;
                    now += 1;
                    store::save_script(&root, &script(18)).unwrap();
                }
            }
            deleted
        })
    };
    let path = root.join("scripts").join("case-18.json");
    for _ in 0..300 {
        let note = store::replaying(18);
        let there = path.exists() && store::load_script(&root, 18).ok().flatten().is_some();
        std::thread::yield_now();
        if there {
            assert!(path.exists(), "the script was moved aside under a noted replay");
            assert!(store::load_script(&root, 18).unwrap().is_some());
        }
        drop(note);
        std::thread::yield_now();
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let deleted = deleter.join().unwrap();
    assert!(deleted > 0, "the delete ran between the replays");
}

/// A delete that came first wins: the replay noted after it finds no script.
#[test]
fn a_replay_noted_after_a_delete_finds_no_script() {
    let dir = tempfile::tempdir().unwrap();
    store::save_script(dir.path(), &script(19)).unwrap();
    store::delete_script(dir.path(), 19, NOW).unwrap();
    let _note = store::replaying(19);
    assert!(store::load_script(dir.path(), 19).unwrap().is_none());
}

/// A watched step-by-step run keeps its case noted for as long as it runs:
/// a delete is refused until its result is saved, another case is stepped
/// or the browser closes (the note is dropped with it).
#[test]
fn a_watched_stepped_run_keeps_its_case_from_being_deleted() {
    use v2_lib::commands::autorun::{hold_stepped_run, release_saved};
    let dir = tempfile::tempdir().unwrap();
    for id in [20, 21] {
        store::save_script(dir.path(), &script(id)).unwrap();
    }
    let mut held = None;
    hold_stepped_run(&mut held, store::replaying(20));
    // A second step of the same case keeps the one note.
    hold_stepped_run(&mut held, store::replaying(20));
    assert_eq!(autorun_script_delete_at(dir.path(), r#"{"case_id": 20}"#, NOW).0, 409);
    // A result saved for another case leaves it.
    release_saved(&mut held, &[21]);
    assert_eq!(autorun_script_delete_at(dir.path(), r#"{"case_id": 20}"#, NOW).0, 409);
    // The run moves on to case 21: case 20 is free, 21 is held.
    hold_stepped_run(&mut held, store::replaying(21));
    assert!(!store::is_replaying(20));
    assert_eq!(autorun_script_delete_at(dir.path(), r#"{"case_id": 21}"#, NOW).0, 409);
    // Its result is saved: the run has ended.
    release_saved(&mut held, &[20, 21]);
    assert!(held.is_none());
    assert_eq!(autorun_script_delete_at(dir.path(), r#"{"case_id": 21}"#, NOW).0, 200);
    assert_eq!(autorun_script_delete_at(dir.path(), r#"{"case_id": 20}"#, NOW).0, 200);
    // And the browser closing drops a note it still holds.
    store::save_script(dir.path(), &script(20)).unwrap();
    hold_stepped_run(&mut held, store::replaying(20));
    drop(held);
    assert!(!store::is_replaying(20));
}

/// The step command notes its case before anything reads the script, and
/// the replay to a step notes it before its check reads it.
#[test]
fn the_notes_are_taken_before_the_script_is_read() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands/autorun.rs"))
        .unwrap()
        .replace("\r\n", "\n");
    let end = "\n}\n";
    let step = &src[src.find("pub async fn auto_run_step(").unwrap()..];
    let step = &step[..step.find(end).unwrap()];
    let noted = step.find("store::replaying(case_id)").expect("the step notes its case");
    assert!(noted < step.find("resolve_step(").unwrap());
    assert!(noted < step.find("load_script(").unwrap());
    assert!(step.contains("hold_stepped_run(&mut session.stepping, replaying)"));
    let replay = &src[src.find("pub(crate) async fn replay_supervised(").unwrap()..];
    let noted = replay.find("store::replaying(req.case_id)").expect("the replay notes its case");
    assert!(noted < replay.find("replay_to::check(").unwrap());
    let save = &src[src.find("pub fn auto_run_save_run(").unwrap()..];
    assert!(save[..save.find(end).unwrap()].contains("release_stepped_run("));
}

/// A listener runs with no lock of the store held: one that registers
/// another listener, or reads the store, does not hang.
#[test]
fn a_listener_may_use_the_store_and_register_another() {
    let dir = tempfile::tempdir().unwrap();
    let mine = dir.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = Mutex::new(tx);
    store::on_scripts_changed(move |at, _| {
        if at == mine {
            store::on_scripts_changed(|_, _| {});
            let _ = store::list_scripts(at);
            let _ = tx.lock().unwrap().send(());
        }
    });
    let root = dir.path().to_path_buf();
    std::thread::spawn(move || store::save_script(&root, &script(22)).unwrap());
    rx.recv_timeout(std::time::Duration::from_secs(5)).expect("the listener ran and returned");
}
