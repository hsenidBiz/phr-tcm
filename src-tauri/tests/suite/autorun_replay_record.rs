//! A replay that stopped on a failure, kept as its case's newest word
//! (`autorun::replay_record`): `get_autorun_failures` and
//! `mark_autorun_suspected_defect` see it when no run has the case, or when
//! it is newer than the case's newest run.

use serde_json::json;
use std::path::Path;
use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::autorun::replay_record::{as_run, latest_for_case, load, note, REPLAY_MODE};
use v2_lib::autorun::replay_to::{ReplayEnd, ReplayPhase};
use v2_lib::autorun::store::{self, save_run};
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, StepRecord};
use v2_lib::browser::actions::ActionOutcome;

const ID: i32 = 77;

fn ctx() -> BridgeContext {
    BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() }
}

fn script() -> CaseScript {
    serde_json::from_value(json!({
        "case_id": ID,
        "title": "Leave request",
        "steps": [
            { "step_number": 1, "actions": [ { "kind": "click", "selector": { "css": "#s1" } } ] },
            { "step_number": 2, "actions": [ { "kind": "click", "selector": { "css": "#s2" } } ] },
            { "step_number": 3, "actions": [ { "kind": "click", "selector": { "css": "#s3" } } ] }
        ]
    }))
    .unwrap()
}

/// A replay to step 3 that stopped at step 2.
fn stopped_at_2() -> ReplayEnd {
    ReplayEnd::StoppedAt {
        phase: ReplayPhase::Step,
        step: 2,
        why: "nothing on the page answers to #s2".into(),
        outcomes: vec![ActionOutcome::failed("nothing on the page answers to #s2")],
    }
}

/// A run at `started_at` in which case 77 failed at step 1.
fn run_failing_step_1(started_at: u64) -> LocalRun {
    LocalRun {
        id: format!("run-{started_at}"),
        pbi_id: 1,
        started_at: started_at.to_string(),
        cases: vec![CaseRecord {
            case_id: ID,
            title: "Leave request".into(),
            verdict: "Failed".into(),
            note: String::new(),
            steps: vec![StepRecord {
                step_number: 1,
                outcomes: vec![ActionOutcome::failed("nothing on the page answers to #s1")],
                screenshot: None,
                downloads: vec![],
                tab: None,
                dialog: None,
                components: Vec::new(),
                duration_ms: None,
            }],
            proposed: String::new(),
            reason: String::new(),
            duration_ms: None,
            account: None,
            retried: None,
            notice: None,
            page_errors_seen: 0,
            phases: None,
        }],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
    }
}

fn setup(root: &Path) {
    store::set_root(root.to_path_buf());
    store::save_script(root, &script()).unwrap();
}

/// A replay that stopped is kept, read back as a run that says it came from
/// a replay to its step; one of the same case that reached its step lets it
/// go; a stop the person asked for keeps what was there.
#[test]
fn a_failing_replay_is_kept_and_a_replay_that_reaches_its_step_lets_it_go() {
    let dir = tempfile::tempdir().unwrap();
    note(dir.path(), ID, "Leave request", 3, &stopped_at_2(), Some("admin".into()), 5);
    let kept = load(dir.path(), ID).expect("the failing replay was not kept");
    assert_eq!((kept.step, kept.to_step, kept.at, kept.account.as_deref()), (2, 3, 5, Some("admin")));
    let run = as_run(&kept);
    assert_eq!((run.id.as_str(), run.mode.as_str()), ("a replay to step 3", REPLAY_MODE));
    assert_eq!(run.cases[0].steps[0].step_number, 2);

    note(dir.path(), ID, "Leave request", 3, &ReplayEnd::Stopped { step: 2 }, None, 6);
    assert_eq!(load(dir.path(), ID).map(|r| r.at), Some(5), "a stop replaced the failure");
    note(dir.path(), ID, "Leave request", 3, &ReplayEnd::Ready { case_id: ID, step: 3, notice: None }, None, 7);
    assert!(load(dir.path(), ID).is_none(), "a replay that reached its step kept the old failure");
}

/// No run has the case: its failures come from the replay, and say so.
#[tokio::test]
async fn failures_come_from_a_replay_when_no_run_has_the_case() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let (status, out) = route(&ctx(), None, "GET", &format!("/autorun-failures?case_id={ID}"), "", "1.0.0").await;
    assert_eq!((status, out.as_str()), (404, "no run on this machine has case 77"));

    note(dir.path(), ID, "Leave request", 3, &stopped_at_2(), None, 5);
    let (status, out) = route(&ctx(), None, "GET", &format!("/autorun-failures?case_id={ID}"), "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("## Case 77 \"Leave request\" (from a replay to step 3, proposed Failed)"), "{out}");
    assert!(out.contains("step 2, action 1:") && out.contains("nothing on the page answers to #s2"), "{out}");
}

/// A run newer than the replay is the newest word; a replay newer than the
/// run is.
#[tokio::test]
async fn the_newer_of_the_run_and_the_replay_is_read() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    note(dir.path(), ID, "Leave request", 3, &stopped_at_2(), None, 2_000);
    save_run(dir.path(), &run_failing_step_1(1_000)).unwrap();
    assert_eq!(latest_for_case(dir.path(), ID).map(|r| r.mode), Some(REPLAY_MODE.to_string()));
    let (_, out) = route(&ctx(), None, "GET", &format!("/autorun-failures?case_id={ID}"), "", "1.0.0").await;
    assert!(out.contains("from a replay to step 3"), "{out}");

    save_run(dir.path(), &run_failing_step_1(3_000)).unwrap();
    assert_eq!(latest_for_case(dir.path(), ID).map(|r| r.id), Some("run-3000".to_string()));
    let (_, out) = route(&ctx(), None, "GET", &format!("/autorun-failures?case_id={ID}"), "", "1.0.0").await;
    assert!(out.contains("run run-3000") && !out.contains("replay"), "{out}");
}

/// A suspected defect can be marked straight after a replay that stopped
/// at the step, with no run of the case on this machine.
#[tokio::test]
async fn a_defect_can_be_marked_after_a_replay() {
    let _a = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let body = json!({ "case_id": ID, "step_number": 2, "note": "the Save button never appears" }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-defect", &body, "1.0.0").await;
    assert_eq!((status, out.as_str()), (400, "no run on this machine has case 77"));

    note(dir.path(), ID, "Leave request", 3, &stopped_at_2(), None, 5);
    let (status, out) = route(&ctx(), None, "POST", "/autorun-defect", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let marked = store::load_script(dir.path(), ID).unwrap().unwrap().suspected_defect.expect("not marked");
    assert_eq!(marked.step_number, 2);

    // A step that did not fail in the replay is still refused.
    let other = json!({ "case_id": ID, "step_number": 1, "note": "x" }).to_string();
    let (status, _) = route(&ctx(), None, "POST", "/autorun-defect", &other, "1.0.0").await;
    assert_eq!(status, 400);
}

/// The app's replay keeps its end, the person's and the assistant's alike.
#[test]
fn every_replay_notes_its_end() {
    let source = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    let replay = &source[source.find("pub(crate) async fn replay_supervised").unwrap()..];
    let replay = &replay[..replay.find("\n}\n").unwrap()];
    assert!(replay.contains("crate::autorun::replay_record::note(&root, case_id, &title, step, &end,"), "{replay}");
}
