//! Scripts and results live on THIS machine and nowhere else. These
//! tests pin the round trip and the two rules that matter: a run always
//! records the verdict the human gave (never one the machine inferred),
//! and nothing here carries a plan, suite or test-point id. The one
//! exception is `published` (a run id and web url, set only after a
//! person presses Send in `autorun::publish`) - proof of where a run
//! landed, never a foothold for driving Azure DevOps from here.

use v2_lib::autorun::store::{
    clear_runs, clear_scripts, list_runs, load_run, load_script, load_shot, new_run_id,
    safe_shot_name, save_run, save_script, save_scripts_atomically, save_shot, save_shot_keeping,
    SaveScriptsError,
};
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, PublishedRun, StepRecord, StepScript};
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
        account: None,
        steps: vec![StepScript {
            step_number: 1,
            actions: vec![
                Action::Navigate { url: "https://app.invalid/login".to_string() },
                Action::Fill { selector: "#user".into(), value: "kim".to_string() },
                Action::Click { selector: "text=Sign in".into() },
                Action::CheckText { value: "Dashboard".to_string() },
            ],
            unchecked: None,
        }],
        repairs: 0,
        last_repair: None,
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
                screenshot: None,
            }],
            proposed: String::new(),
            reason: String::new(),
            duration_ms: None,
            account: None,
        }],
        mode: String::new(),
        published: None,
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
                mode: String::new(),
                published: None,
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
            mode: String::new(),
            published: None,
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
        account: None,
        steps: vec![StepScript {
            step_number: 1,
            actions: vec![Action::CheckText { value: "ok".to_string() }],
            unchecked: None,
        }],
        repairs: 0,
        last_repair: None,
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
    let bundle = vec![CaseScript { case_id: 9, title: "Empty".to_string(), account: None, steps: vec![], repairs: 0, last_repair: None }];
    let err = save_scripts_atomically(dir.path(), &bundle).expect_err("empty steps were accepted");
    assert!(matches!(err, SaveScriptsError::Invalid(_)));
}

/// Two entries for the same step number within one script is the same
/// ambiguity as a duplicate case id, one level down - a reader (or the
/// `autorun::edits` gate, which builds a map keyed by step_number) cannot
/// tell which of the two is the real step, so no path may save either.
#[test]
fn a_duplicate_step_number_within_one_script_is_rejected() {
    let dir = TempDir::new();
    let bundle = vec![CaseScript {
        case_id: 70,
        title: "Duplicate step".to_string(),
        account: None,
        steps: vec![
            StepScript { step_number: 1, actions: vec![Action::CheckText { value: "a".to_string() }], unchecked: None },
            StepScript { step_number: 1, actions: vec![Action::CheckText { value: "b".to_string() }], unchecked: None },
        ],
        repairs: 0,
        last_repair: None,
    }];
    let err = save_scripts_atomically(dir.path(), &bundle).expect_err("duplicate step number was accepted");
    assert!(matches!(err, SaveScriptsError::Invalid(_)));
    assert_eq!(err.to_string(), "case 70: step 1 appears more than once");
    assert!(
        load_script(dir.path(), 70).unwrap().is_none(),
        "a script with a duplicate step number wrote something anyway"
    );
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
        account: None,
        steps: vec![StepScript { step_number: 1, actions: vec![], unchecked: None }],
        repairs: 0,
        last_repair: None,
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

/// A shot referenced by an unpublished run's own steps must survive
/// pruning even when it is the oldest thing in the folder - it is
/// evidence for a run nobody has sent yet, not disposable. Once that run
/// IS published, its pictures are no longer protected and the next prune
/// is free to drop them like any other old shot.
#[test]
fn an_unpublished_runs_own_shots_survive_pruning_and_are_freed_once_sent() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let shot_a = save_shot_keeping(root, b"A", 2).unwrap();
    let mut run = LocalRun {
        id: "run-1".into(),
        pbi_id: 1,
        started_at: "1".into(),
        mode: "unattended".into(),
        published: None,
        cases: vec![CaseRecord {
            case_id: 1,
            title: "t".into(),
            verdict: "".into(),
            note: "".into(),
            steps: vec![StepRecord { step_number: 1, outcomes: vec![], screenshot: Some(shot_a.clone()) }],
            proposed: "".into(),
            reason: "".into(),
            duration_ms: None,
            account: None,
        }],
    };
    save_run(root, &run).unwrap();

    // More new shots than `keep` - without the guard, shot A (the oldest
    // file in the folder) would be pruned away first.
    for i in 0..5u8 {
        save_shot_keeping(root, &[i], 2).unwrap();
    }
    assert!(
        root.join("shots").join(&shot_a).is_file(),
        "an unpublished run's own picture must never be pruned"
    );

    // Once the run is sent, its picture is no longer protected.
    run.published = Some(PublishedRun { run_id: 1, web_url: "https://x/run/1".into(), at: "1".into() });
    save_run(root, &run).unwrap();
    for i in 5..8u8 {
        save_shot_keeping(root, &[i], 2).unwrap();
    }
    assert!(
        !root.join("shots").join(&shot_a).is_file(),
        "a sent run's picture is no longer protected and should have been pruned"
    );
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

/// A script names its account by key, and one that names none still writes
/// and reads back exactly as it always did.
#[test]
fn a_script_may_name_an_account_and_one_without_is_written_as_before() {
    let with: v2_lib::autorun::CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": 7, "title": "t", "account": "hr.supervisor",
        "steps": [{ "step_number": 1, "actions": [{ "kind": "sign_in", "account": "emp" }] }]
    })).unwrap();
    assert_eq!(with.account.as_deref(), Some("hr.supervisor"));
    let without: v2_lib::autorun::CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": 8, "title": "t", "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }]
    })).unwrap();
    assert_eq!(without.account, None);
    assert!(serde_json::to_value(&without).unwrap().get("account").is_none());
}

/// Saving checks the FORM of an account key (and refuses a login typed
/// straight into a script), but never whether the account exists - that is
/// checked only when the script actually runs.
#[test]
fn a_bad_account_key_or_a_login_placeholder_in_a_script_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let save = |v: serde_json::Value| {
        let sc: v2_lib::autorun::CaseScript = serde_json::from_value(v).unwrap();
        v2_lib::autorun::store::save_scripts_atomically(dir.path(), &[sc]).map_err(|e| e.to_string())
    };
    let step = serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }]);
    let bad_key = save(serde_json::json!({ "case_id": 1, "title": "t", "account": "HR Admin", "steps": step })).unwrap_err();
    assert!(bad_key.contains("case 1") && bad_key.contains("HR Admin"), "{bad_key}");
    let bad_action = save(serde_json::json!({ "case_id": 1, "title": "t", "steps": [{ "step_number": 2, "actions": [
        { "kind": "sign_in", "account": "Nope Nope" }] }] })).unwrap_err();
    assert!(bad_action.contains("case 1 step 2 action 1"), "{bad_action}");
    let placeholder = save(serde_json::json!({ "case_id": 1, "title": "t", "steps": [{ "step_number": 1, "actions": [
        { "kind": "fill", "selector": "#p", "value": "{{password}}" }] }] })).unwrap_err();
    assert!(placeholder.contains("sign-in recipe"), "{placeholder}");
    // An account that does not exist on THIS machine is fine to save.
    assert!(save(serde_json::json!({ "case_id": 1, "title": "t", "account": "someone.elses", "steps": step })).is_ok());
}

/// A run file saved before unattended runs existed has none of the new
/// fields. It must still load, and a supervised run must still be written
/// with exactly the shape it always had - no new keys appear just because
/// the type now knows how to carry them.
#[test]
fn a_run_file_written_before_unattended_runs_still_loads_and_is_rewritten_unchanged() {
    let old = serde_json::json!({
        "id": "run-1", "pbi_id": 42, "started_at": "1700000000000",
        "cases": [{ "case_id": 7, "title": "t", "verdict": "Passed", "note": "",
                    "steps": [{ "step_number": 1, "outcomes": [{ "ok": true, "detail": "ok" }] }] }]
    });
    let run: v2_lib::autorun::LocalRun = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(run.mode, "");
    assert!(run.published.is_none());
    assert_eq!(run.cases[0].proposed, "");
    assert_eq!(serde_json::to_value(&run).unwrap(), old, "a supervised run must be written exactly as before");
}

/// A script saved before this plan has neither `unchecked` nor `repairs`
/// in its JSON, and one saved before `last_repair` existed has none of
/// that either. All three must still load with their defaults, and be
/// written back byte-for-byte identical - the new fields never appear
/// just because the type now knows how to carry them.
#[test]
fn a_script_saved_before_this_plan_is_written_exactly_as_before() {
    let old = serde_json::json!({ "case_id": 7, "title": "t",
        "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }] });
    let sc: v2_lib::autorun::CaseScript = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(sc.repairs, 0);
    assert_eq!(sc.last_repair, None);
    assert_eq!(sc.steps[0].unchecked, None);
    assert_eq!(serde_json::to_value(&sc).unwrap(), old);
}

/// `unchecked` round-trips with `repairs` and `last_repair`, and a blank
/// reason (an assistant that filled the field but said nothing) is
/// refused at save time the same way a missing one would be.
#[test]
fn an_unchecked_step_needs_a_reason_and_repairs_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut sc: v2_lib::autorun::CaseScript = serde_json::from_value(serde_json::json!({ "case_id": 7, "title": "t", "repairs": 2, "last_repair": "the toast lost its id",
        "steps": [{ "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://a.example/" }], "unchecked": "the PDF preview cannot be read" }] })).unwrap();
    v2_lib::autorun::store::save_scripts_atomically(dir.path(), std::slice::from_ref(&sc)).unwrap();
    let back = v2_lib::autorun::store::load_script(dir.path(), 7).unwrap().unwrap();
    assert_eq!(back, sc);
    assert_eq!(back.last_repair.as_deref(), Some("the toast lost its id"));
    sc.steps[0].unchecked = Some("   ".into());
    let err = v2_lib::autorun::store::save_scripts_atomically(dir.path(), std::slice::from_ref(&sc)).unwrap_err().to_string();
    assert!(err.contains("case 7 step 1") && err.contains("reason"), "{err}");
}

/// An unattended run's proposal travels apart from `verdict`, which stays
/// the human's word alone - the machine never fills it in, even in a
/// replay.
#[test]
fn an_unattended_run_keeps_the_proposal_apart_from_the_verdict() {
    let dir = tempfile::tempdir().unwrap();
    let run: v2_lib::autorun::LocalRun = serde_json::from_value(serde_json::json!({
        "id": "run-2", "pbi_id": 42, "started_at": "1700000000000", "mode": "unattended",
        "cases": [{ "case_id": 7, "title": "t", "verdict": "", "note": "", "proposed": "Failed",
                    "reason": "step 2: button \"Save\" not found", "duration_ms": 8123, "account": "hr.admin",
                    "steps": [{ "step_number": 1, "outcomes": [], "screenshot": "shot-1-000001.jpg" }] }]
    })).unwrap();
    save_run(dir.path(), &run).unwrap();
    let back = load_run(dir.path(), "run-2").unwrap().unwrap();
    assert_eq!(back, run);
    assert_eq!(back.cases[0].verdict, "", "the machine never fills in the verdict");
    assert!(load_run(dir.path(), "run-nope").unwrap().is_none());
    assert!(load_run(dir.path(), "../escape").is_err());
}

// ---- Clearing scripts and results (dev-only Auto Run toolbar) ----------

/// Only the named cases' scripts are removed; everything else on disk is
/// left alone, and asking to clear an id nobody scripted is not an error -
/// it simply does not add to the count.
#[test]
fn clear_scripts_removes_only_the_named_ids_and_a_missing_one_is_fine() {
    let dir = TempDir::new();
    save_script(dir.path(), &one_step_script(1, "One")).unwrap();
    save_script(dir.path(), &one_step_script(2, "Two")).unwrap();
    save_script(dir.path(), &one_step_script(3, "Three")).unwrap();

    let removed = clear_scripts(dir.path(), &[1, 3, 999]).unwrap();
    assert_eq!(removed, 2, "999 has no script on disk - it does not count, and is not an error");
    assert!(load_script(dir.path(), 1).unwrap().is_none());
    assert!(load_script(dir.path(), 2).unwrap().is_some(), "case 2 was not named - it must survive");
    assert!(load_script(dir.path(), 3).unwrap().is_none());
}

/// An empty root - nothing has ever been scripted or run - clears nothing
/// and is not an error either way.
#[test]
fn clear_scripts_and_clear_runs_on_an_empty_root_return_zero() {
    let dir = TempDir::new();
    assert_eq!(clear_scripts(dir.path(), &[1, 2, 3]).unwrap(), 0);
    assert_eq!(clear_runs(dir.path()).unwrap(), 0);
}

/// Every run is removed, including one already sent to Azure DevOps - the
/// record there is the durable one, which is exactly why the confirm the
/// screen shows before calling this says so. Every screenshot goes with
/// them, and the count returned is runs, never shots (one run's evidence
/// can be many pictures).
#[test]
fn clear_runs_removes_every_run_and_shot_published_or_not() {
    let dir = TempDir::new();
    let root = dir.path();
    let shot = save_shot_keeping(root, b"A", 10).unwrap();
    let unpublished = LocalRun {
        id: "run-1".into(),
        pbi_id: 1,
        started_at: "1".into(),
        mode: "unattended".into(),
        published: None,
        cases: vec![CaseRecord {
            case_id: 1,
            title: "t".into(),
            verdict: "".into(),
            note: "".into(),
            steps: vec![StepRecord { step_number: 1, outcomes: vec![], screenshot: Some(shot.clone()) }],
            proposed: "".into(),
            reason: "".into(),
            duration_ms: None,
            account: None,
        }],
    };
    save_run(root, &unpublished).unwrap();
    let published = LocalRun {
        id: "run-2".into(),
        pbi_id: 1,
        started_at: "2".into(),
        mode: "unattended".into(),
        published: Some(PublishedRun {
            run_id: 42,
            web_url: "https://dev.azure.com/org/proj/_workitems/edit/42".into(),
            at: "2".into(),
        }),
        cases: vec![],
    };
    save_run(root, &published).unwrap();

    let removed = clear_runs(root).unwrap();
    assert_eq!(removed, 2, "both runs count toward the total, published or not");
    assert!(list_runs(root).is_empty());
    assert!(
        !root.join("shots").join(&shot).is_file(),
        "every screenshot is removed along with the runs, even one an unpublished run still referenced"
    );
}

/// A `runs/` directory that cannot be listed is not the same thing as one
/// that was never created: the first is a real failure (permissions, or -
/// as reproduced here - something else sitting where the directory should
/// be) and must reach the caller as an error naming the path, never get
/// folded into the same "0 removed" a genuinely empty root returns.
#[test]
fn clear_runs_reports_an_unreadable_runs_directory_instead_of_zero() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("runs"), b"not a directory").unwrap();

    let err = clear_runs(dir.path()).unwrap_err();
    assert!(err.contains("runs"), "the message should name the path that failed: {err}");
}
