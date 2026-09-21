//! Scripts and results live on THIS machine and nowhere else. These
//! tests pin the round trip and the two rules that matter: a run always
//! records the verdict the human gave (never one the machine inferred),
//! and nothing here has an Azure DevOps shape.

use v2_lib::autorun::store::{
    list_runs, load_script, load_shot, new_run_id, safe_shot_name, save_run, save_script,
    save_scripts_atomically, save_shot, save_shot_keeping, SaveScriptsError,
};
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, StepRecord, StepScript};
use v2_lib::browser::actions::{Action, ActionOutcome};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-autorun-test-{nanos}-{n}"));
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

fn script() -> CaseScript {
    CaseScript {
        case_id: 201,
        title: "Valid login".to_string(),
        steps: vec![StepScript {
            step_number: 1,
            actions: vec![
                Action::Navigate { url: "https://app.invalid/login".to_string() },
                Action::Fill { selector: "#user".into(), value: "kim".to_string() },
                Action::Click { selector: "text=Sign in".into() },
                Action::CheckText { value: "Dashboard".to_string() },
            ],
        }],
    }
}

#[test]
fn a_script_round_trips_by_case_id() {
    let dir = TempDir::new();
    save_script(dir.path(), &script()).unwrap();
    let back = load_script(dir.path(), 201).unwrap().unwrap();
    assert_eq!(back, script());
}

#[test]
fn a_case_with_no_script_reads_as_none_not_an_error() {
    let dir = TempDir::new();
    assert!(load_script(dir.path(), 999).unwrap().is_none());
}

#[test]
fn run_ids_are_unique() {
    assert_ne!(new_run_id(), {
        std::thread::sleep(std::time::Duration::from_millis(2));
        new_run_id()
    });
}

/// The verdict is whatever the human typed in. Nothing in this module
/// may infer one from the outcomes - that is the whole point of a
/// supervised runner.
#[test]
fn a_run_round_trips_with_the_humans_verdict() {
    let dir = TempDir::new();
    let run = LocalRun {
        id: "run-1".to_string(),
        pbi_id: 42,
        started_at: "1786000000000".to_string(),
        cases: vec![CaseRecord {
            case_id: 201,
            title: "Valid login".to_string(),
            verdict: "Failed".to_string(),
            note: "the dashboard came up but the name was wrong".to_string(),
            steps: vec![StepRecord {
                step_number: 1,
                // Every action succeeded and the human still said Failed.
                outcomes: vec![ActionOutcome {
                    ok: true,
                    detail: "clicked Sign in".to_string(),
                    screenshot: None,
                    harness: false,
                }],
            }],
        }],
    };
    save_run(dir.path(), &run).unwrap();

    let all = list_runs(dir.path());
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].cases[0].verdict, "Failed");
    assert_eq!(all[0].cases[0].steps[0].outcomes[0].ok, true);
}

#[test]
fn runs_come_back_newest_first() {
    let dir = TempDir::new();
    for (id, at) in [("run-1", "1000"), ("run-2", "3000"), ("run-3", "2000")] {
        save_run(
            dir.path(),
            &LocalRun {
                id: id.to_string(),
                pbi_id: 42,
                started_at: at.to_string(),
                cases: vec![],
            },
        )
        .unwrap();
    }
    let ids: Vec<String> = list_runs(dir.path()).into_iter().map(|r| r.id).collect();
    assert_eq!(ids, vec!["run-2", "run-3", "run-1"]);
}

/// An unreadable file is skipped, not a panic and not a lost list - one
/// corrupt run must never hide every other run from the results view.
#[test]
fn a_corrupt_run_file_is_skipped_rather_than_fatal() {
    let dir = TempDir::new();
    save_run(
        dir.path(),
        &LocalRun {
            id: "good".to_string(),
            pbi_id: 1,
            started_at: "1".to_string(),
            cases: vec![],
        },
    )
    .unwrap();
    std::fs::write(dir.path().join("runs").join("broken.json"), "{ not json").unwrap();
    let all = list_runs(dir.path());
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, "good");
}

fn one_step_script(case_id: i32, title: &str) -> CaseScript {
    CaseScript {
        case_id,
        title: title.to_string(),
        steps: vec![StepScript {
            step_number: 1,
            actions: vec![Action::CheckText { value: "ok".to_string() }],
        }],
    }
}

/// The whole point of the shared helper: every entry lands, and it lands
/// with the write-then-rename shape, not a direct write.
#[test]
fn a_bundle_saves_every_entry() {
    let dir = TempDir::new();
    let bundle = vec![one_step_script(101, "First"), one_step_script(102, "Second")];
    save_scripts_atomically(dir.path(), &bundle).unwrap();
    assert_eq!(load_script(dir.path(), 101).unwrap().unwrap().title, "First");
    assert_eq!(load_script(dir.path(), 102).unwrap().unwrap().title, "Second");
}

/// `case-0.json` or `case--3.json` can never correspond to a real work
/// item - saving one would report success for a script that can never be
/// matched back up.
#[test]
fn a_case_id_that_cannot_be_a_real_work_item_is_rejected() {
    let dir = TempDir::new();
    for bad_id in [0, -1, -100] {
        let err = save_scripts_atomically(dir.path(), &[one_step_script(bad_id, "Bad")])
            .expect_err(&format!("case id {bad_id} was accepted"));
        assert!(matches!(err, SaveScriptsError::Invalid(_)));
    }
}

/// Two entries for the same case in one bundle is ambiguous - "last wins"
/// silently is worse than refusing and asking which one was meant.
#[test]
fn a_duplicate_case_id_within_one_bundle_is_rejected() {
    let dir = TempDir::new();
    let bundle = vec![one_step_script(55, "First"), one_step_script(55, "Second")];
    let err = save_scripts_atomically(dir.path(), &bundle).expect_err("duplicate was accepted");
    assert!(matches!(err, SaveScriptsError::Invalid(_)));
    assert!(
        load_script(dir.path(), 55).unwrap().is_none(),
        "a duplicate bundle wrote something anyway"
    );
}

/// A script with no steps runs nothing - saving it would earn a "Script
/// ready" badge for a case that has nothing behind it.
#[test]
fn a_script_with_no_steps_is_rejected() {
    let dir = TempDir::new();
    let bundle = vec![CaseScript { case_id: 9, title: "Empty".to_string(), steps: vec![] }];
    let err = save_scripts_atomically(dir.path(), &bundle).expect_err("empty steps were accepted");
    assert!(matches!(err, SaveScriptsError::Invalid(_)));
}

/// A step with no ACTIONS is a different thing entirely - the guide tells
/// an assistant to leave a step like this for a manual check rather than
/// invent a check that proves nothing. That must keep saving cleanly.
#[test]
fn a_step_with_no_actions_is_still_accepted() {
    let dir = TempDir::new();
    let bundle = vec![CaseScript {
        case_id: 60,
        title: "Manual step included".to_string(),
        steps: vec![StepScript { step_number: 1, actions: vec![] }],
    }];
    save_scripts_atomically(dir.path(), &bundle).unwrap();
    assert!(load_script(dir.path(), 60).unwrap().is_some());
}

/// A locator that names nothing, or a javascript: address, is refused
/// where the script is saved - not discovered halfway through a run.
#[test]
fn a_script_with_an_invalid_action_is_refused_whole() {
    let dir = tempfile::tempdir().unwrap();
    let script: v2_lib::autorun::CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": 501,
        "title": "t",
        "steps": [
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] },
            { "step_number": 2, "actions": [
                { "kind": "check_text", "value": "ok" },
                { "kind": "click", "selector": { "name": "Save" } }
            ] }
        ]
    }))
    .unwrap();
    let err = v2_lib::autorun::store::save_scripts_atomically(dir.path(), &[script]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("case 501 step 2 action 2"), "{msg}");
    assert!(v2_lib::autorun::store::load_script(dir.path(), 501).unwrap().is_none(), "nothing may be written");
}

/// The real atomicity claim, exercised directly against the store rather
/// than through the bridge route: case 101's target is pre-occupied by a
/// directory, so its write can never land. Case 100 - which validates and
/// would otherwise write cleanly - must not end up on disk either.
#[test]
fn a_write_failure_for_one_entry_leaves_none_of_the_bundle_behind() {
    let dir = TempDir::new();
    let scripts_dir = dir.path().join("scripts");
    std::fs::create_dir_all(&scripts_dir).unwrap();
    std::fs::create_dir_all(scripts_dir.join("case-101.json")).unwrap();

    let bundle = vec![one_step_script(100, "Would succeed"), one_step_script(101, "Blocked")];
    let err = save_scripts_atomically(dir.path(), &bundle).expect_err("the write should fail");
    assert!(matches!(err, SaveScriptsError::Io(_)));
    assert!(
        load_script(dir.path(), 100).unwrap().is_none(),
        "case 100 was written even though case 101 in the same bundle could not be"
    );
}

#[test]
fn a_shot_is_saved_under_a_safe_name_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let name = save_shot(dir.path(), &[0xFF, 0xD8, 0xFF, 0x00]).unwrap();
    assert!(safe_shot_name(&name), "{name}");
    assert!(dir.path().join("shots").join(&name).is_file());
    assert_eq!(load_shot(dir.path(), &name).unwrap(), vec![0xFF, 0xD8, 0xFF, 0x00]);
}

/// The name arrives from the webview. It must never be able to read a file
/// outside the shots folder.
#[test]
fn a_shot_name_cannot_leave_the_shots_folder() {
    for bad in ["../runs/run-1.json", "shot-1.jpg/../../x", "C:\\x.jpg", "shot-1.png", "", "shot-..jpg", "x.jpg"] {
        assert!(!safe_shot_name(bad), "{bad:?} was accepted");
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(load_shot(dir.path(), "../runs/run-1.json").is_err());
}

/// Screenshots are evidence for the run in front of the person, not an
/// archive. Only the newest are kept, so the folder (and the app's backup
/// of it) cannot grow without limit.
#[test]
fn only_the_newest_shots_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut names = vec![];
    for i in 0..5u8 {
        names.push(save_shot_keeping(dir.path(), &[i], 3).unwrap());
    }
    let mut left: Vec<String> = std::fs::read_dir(dir.path().join("shots"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    left.sort();
    assert_eq!(left, names[2..].to_vec(), "the three newest stay");
}

/// The write already landed on disk by the time pruning runs. Whatever
/// pruning keeps or drops, the caller must still learn the name of the
/// file that exists - losing it would mean an outcome points at a
/// screenshot nobody can find.
#[test]
fn saving_still_returns_the_name_even_keeping_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let name = save_shot_keeping(dir.path(), &[1, 2, 3], 0).unwrap();
    assert!(safe_shot_name(&name), "{name}");
}

/// Pruning only ever touches files that look like screenshots - anything
/// else in the shots folder is left alone.
#[test]
fn a_non_screenshot_file_in_the_shots_folder_survives_pruning() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("shots")).unwrap();
    std::fs::write(dir.path().join("shots").join("notes.txt"), b"not a screenshot").unwrap();
    for i in 0..3u8 {
        save_shot_keeping(dir.path(), &[i], 1).unwrap();
    }
    assert!(dir.path().join("shots").join("notes.txt").is_file());
}
