//! Scripts and results live on THIS machine and nowhere else. These
//! tests pin the round trip and the two rules that matter: a run always
//! records the verdict the human gave (never one the machine inferred),
//! and nothing here has an Azure DevOps shape.

use v2_lib::autorun::store::{list_runs, load_script, new_run_id, save_run, save_script};
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
                Action::Fill { selector: "#user".to_string(), value: "kim".to_string() },
                Action::Click { selector: "text=Sign in".to_string() },
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
                outcomes: vec![ActionOutcome { ok: true, detail: "clicked Sign in".to_string() }],
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
