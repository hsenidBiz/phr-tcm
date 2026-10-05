//! An unattended run pausing at a reset point (design §3): it waits after
//! the case before has ended and closed its browser, Continue runs the next
//! phase, and Stop - the panel's, the run's own, or the app closing - ends
//! the run with the rest recorded as not run. Each pause is kept in the
//! run's `resets`, and the report shows it between the cases.
//!
//! Browsers are fakes (`common::FakePage`); the wait is either a gate that
//! answers as told or the app's own gate over a local `ResetWaits`.

use crate::common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use v2_lib::autorun::plan::Reset;
use v2_lib::autorun::preconditions::{NoDb, PreconditionDb, NEED_DB};
use v2_lib::autorun::replay::{run_cases_planned, Browsers, CaseToRun, ResetGate, STOPPED_AT_RESET};
use v2_lib::autorun::reset_wait::{AppGate, ResetWaits, NOT_WAITING};
use v2_lib::autorun::{report, store, CaseScript, LocalRun, ResetRecord, RESET_CONTINUED, RESET_STOPPED};
use v2_lib::browser::timing::Timing;
use v2_lib::commands::autorun_replay::planned_cases;
use v2_lib::events::{AutorunResetNeeded, ReplayProgress};

/// Fake browsers that count, in a counter a gate can read too, how many
/// were closed.
struct CountingBrowsers {
    opened: usize,
    closed: Arc<AtomicUsize>,
}

impl Browsers for CountingBrowsers {
    type D = common::ScriptedDriver;
    async fn open(&mut self) -> Result<Self::D, String> {
        self.opened += 1;
        Ok(common::FakePage::default().driver())
    }
    async fn close(&mut self, _d: Self::D) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}

fn browsers() -> CountingBrowsers {
    CountingBrowsers { opened: 0, closed: Arc::new(AtomicUsize::new(0)) }
}

/// A gate that answers `answer` and remembers what it was asked, and how
/// many browsers had closed by then.
struct ToldGate {
    answer: bool,
    closed: Arc<AtomicUsize>,
    asked: Vec<(i32, Vec<i32>, usize)>,
}

impl ResetGate for ToldGate {
    async fn wait(&mut self, reset: &Reset, remaining: &[i32], _cancel: &AtomicBool) -> bool {
        self.asked.push((reset.before_case_id, remaining.to_vec(), self.closed.load(Ordering::SeqCst)));
        self.answer
    }
}

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 20, highlight_ms: 0, lease_wait_ms: 300 }
}

fn new_run(id: &str) -> LocalRun {
    LocalRun {
        id: id.into(),
        pbi_id: 42,
        started_at: "1700000000000".into(),
        cases: vec![],
        mode: "unattended".into(),
        published: None,
        environment: None,
        resets: vec![],
    }
}

/// A one-step script with one passing check, carrying `changes` and
/// `needs_unchanged` marks.
fn marked(case_id: i32, changes: &[&str], needs: &[&str]) -> CaseScript {
    serde_json::from_value(serde_json::json!({
        "case_id": case_id,
        "title": format!("case {case_id}"),
        "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }],
        "changes": changes,
        "needs_unchanged": needs,
    }))
    .unwrap()
}

fn to_run(ids: &[i32]) -> Vec<CaseToRun> {
    ids.iter().map(|&id| CaseToRun { case_id: id, title: format!("case {id}"), module: None }).collect()
}

/// Before case 2, revert "cycle published", which case 1 changed.
fn reset_before_2() -> Reset {
    Reset {
        before_case_id: 2,
        names: vec!["cycle published".into()],
        changed_by: vec![("cycle published".into(), vec![1])],
    }
}

fn no_db() -> PreconditionDb<NoDb> {
    PreconditionDb::Missing(NEED_DB.to_string())
}

/// Three scripted cases on disk; the reset sits before case 2.
fn three_cases(root: &std::path::Path) {
    store::save_script(root, &marked(1, &["cycle published"], &[])).unwrap();
    store::save_script(root, &marked(2, &[], &["cycle published"])).unwrap();
    store::save_script(root, &marked(3, &[], &[])).unwrap();
}

async fn run_with<G: ResetGate>(
    root: &std::path::Path,
    browsers: &mut CountingBrowsers,
    run: &mut LocalRun,
    cancel: &AtomicBool,
    gate: &mut G,
    events: &mut Vec<ReplayProgress>,
) -> Result<(), String> {
    run_cases_planned(
        browsers,
        root,
        "Acme",
        "Web",
        run,
        &to_run(&[1, 2, 3]),
        None,
        false,
        &quick(),
        cancel,
        &no_db(),
        &[reset_before_2()],
        gate,
        &mut |p: ReplayProgress| events.push(p),
    )
    .await
}

#[tokio::test]
async fn a_run_pauses_at_a_reset_point_once_the_case_before_has_closed_its_browser() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    three_cases(root);
    let mut b = browsers();
    let mut gate = ToldGate { answer: true, closed: Arc::clone(&b.closed), asked: vec![] };
    let mut run = new_run("run-pause");
    let cancel = AtomicBool::new(false);
    let mut events = vec![];
    run_with(root, &mut b, &mut run, &cancel, &mut gate, &mut events).await.unwrap();

    // Asked once, before case 2, with cases 2 and 3 still to run, and only
    // after case 1's browser had closed.
    assert_eq!(gate.asked, vec![(2, vec![2, 3], 1)]);
}

#[tokio::test]
async fn continue_runs_the_next_phase_in_the_same_run() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    three_cases(root);
    let mut b = browsers();
    let mut gate = ToldGate { answer: true, closed: Arc::clone(&b.closed), asked: vec![] };
    let mut run = new_run("run-continue");
    let cancel = AtomicBool::new(false);
    let mut events = vec![];
    run_with(root, &mut b, &mut run, &cancel, &mut gate, &mut events).await.unwrap();

    assert_eq!(b.opened, 3);
    assert_eq!(run.cases.iter().map(|c| c.case_id).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", run.cases);
    assert_eq!(run.resets.len(), 1);
    assert_eq!(run.resets[0].outcome, RESET_CONTINUED);
    assert_eq!(run.resets[0].before_case_id, 2);
    let saved = store::load_run(root, "run-continue").unwrap().unwrap();
    assert_eq!(saved.resets, run.resets);
}

#[tokio::test]
async fn stop_ends_the_run_and_records_every_case_left_as_not_run() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    three_cases(root);
    let mut b = browsers();
    let mut gate = ToldGate { answer: false, closed: Arc::clone(&b.closed), asked: vec![] };
    let mut run = new_run("run-stop");
    let cancel = AtomicBool::new(false);
    let mut events = vec![];
    run_with(root, &mut b, &mut run, &cancel, &mut gate, &mut events).await.unwrap();

    assert_eq!(b.opened, 1, "no browser opens after Stop");
    assert_eq!(run.cases.len(), 3);
    assert_eq!(run.cases[0].proposed, "Passed");
    for c in &run.cases[1..] {
        assert_eq!(c.reason, STOPPED_AT_RESET);
        assert_eq!(c.proposed, "");
        assert_eq!(c.verdict, "");
        assert!(!c.steps.is_empty());
        assert!(c.steps.iter().flat_map(|s| &s.outcomes).all(|o| !o.ok && o.detail == STOPPED_AT_RESET));
    }
    assert_eq!(run.resets.len(), 1);
    assert_eq!(run.resets[0].outcome, RESET_STOPPED);
    // On disk as it ended, and the screen was told each case is done.
    let saved = store::load_run(root, "run-stop").unwrap().unwrap();
    assert_eq!(saved, run);
    let done: Vec<i32> = events.iter().filter(|e| e.phase == "done").map(|e| e.case_id).collect();
    assert_eq!(done, vec![1, 2, 3]);
}

/// The app's own gate over `waits`, telling `told` what the panel would show.
fn app_gate<'a>(
    waits: &'a ResetWaits,
    run_id: &str,
    notify: &'a (dyn Fn(&AutorunResetNeeded) + Send + Sync),
) -> AppGate<'a> {
    AppGate { waits, run_id: run_id.to_string(), notify }
}

/// Wait until a reset is waiting in `waits`; its run's id.
async fn until_waiting(waits: &ResetWaits) -> String {
    for _ in 0..400 {
        if let Some(needed) = waits.waiting() {
            return needed.run_id;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the run never paused");
}

#[tokio::test]
async fn the_panel_is_told_ids_and_names_and_its_continue_reaches_only_the_waiting_run() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    three_cases(root);
    let waits = ResetWaits::new();
    let told: Mutex<Vec<AutorunResetNeeded>> = Mutex::new(vec![]);
    let notify = |n: &AutorunResetNeeded| told.lock().unwrap().push(n.clone());
    let mut gate = app_gate(&waits, "run-app", &notify);
    let mut b = browsers();
    let mut run = new_run("run-app");
    let cancel = AtomicBool::new(false);
    let mut events = vec![];
    let answer = async {
        assert_eq!(until_waiting(&waits).await, "run-app");
        assert_eq!(waits.answer("run-other", true), Err(NOT_WAITING.to_string()));
        waits.answer("run-app", true).unwrap();
        // Answered once only.
        assert_eq!(waits.answer("run-app", true), Err(NOT_WAITING.to_string()));
    };
    let (res, ()) = tokio::join!(run_with(root, &mut b, &mut run, &cancel, &mut gate, &mut events), answer);
    res.unwrap();

    assert_eq!(run.resets[0].outcome, RESET_CONTINUED);
    assert_eq!(run.cases.len(), 3);
    let told = told.lock().unwrap();
    assert_eq!(
        *told,
        vec![AutorunResetNeeded {
            run_id: "run-app".into(),
            before_case_id: 2,
            names: vec!["cycle published".into()],
            changed_by: vec![("cycle published".into(), vec![1])],
            remaining: vec![2, 3],
        }]
    );
    assert_eq!(waits.waiting(), None);
}

#[tokio::test]
async fn the_runs_own_stop_while_paused_counts_as_stop() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    three_cases(root);
    let waits = ResetWaits::new();
    let notify = |_: &AutorunResetNeeded| {};
    let mut gate = app_gate(&waits, "run-cancel", &notify);
    let mut b = browsers();
    let mut run = new_run("run-cancel");
    let cancel = AtomicBool::new(false);
    let mut events = vec![];
    let press_stop = async {
        until_waiting(&waits).await;
        cancel.store(true, Ordering::SeqCst);
    };
    let (res, ()) = tokio::join!(run_with(root, &mut b, &mut run, &cancel, &mut gate, &mut events), press_stop);
    res.unwrap();

    assert_eq!(b.opened, 1);
    assert_eq!(run.resets[0].outcome, RESET_STOPPED);
    assert!(run.cases[1..].iter().all(|c| c.reason == STOPPED_AT_RESET), "{:?}", run.cases);
    assert_eq!(waits.waiting(), None, "a stopped wait leaves nothing to answer");
}

#[tokio::test]
async fn closing_the_app_while_paused_stops_the_run_and_saves_it_as_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    three_cases(root);
    let waits = ResetWaits::new();
    let notify = |_: &AutorunResetNeeded| {};
    let mut gate = app_gate(&waits, "run-exit", &notify);
    let mut b = browsers();
    let mut run = new_run("run-exit");
    let cancel = AtomicBool::new(false);
    let mut events = vec![];
    assert!(!waits.stop_for_exit(), "nothing waits before the run pauses");
    let exit = async {
        until_waiting(&waits).await;
        assert!(waits.stop_for_exit());
    };
    let (res, ()) = tokio::join!(run_with(root, &mut b, &mut run, &cancel, &mut gate, &mut events), exit);
    res.unwrap();

    let saved = store::load_run(root, "run-exit").unwrap().unwrap();
    assert_eq!(saved.resets.len(), 1);
    assert_eq!(saved.resets[0].outcome, RESET_STOPPED);
    assert_eq!(saved.cases.len(), 3, "every case is on disk: none is left waiting");
    assert!(saved.cases[1..].iter().all(|c| c.reason == STOPPED_AT_RESET));
}

#[test]
fn the_resets_record_is_written_only_when_there_is_one() {
    let plain = serde_json::to_value(new_run("run-a")).unwrap();
    assert!(plain.get("resets").is_none(), "{plain}");

    let mut run = new_run("run-b");
    run.resets.push(ResetRecord {
        before_case_id: 2,
        names: vec!["cycle published".into()],
        changed_by: vec![("cycle published".into(), vec![1])],
        waited_ms: 1234,
        outcome: RESET_CONTINUED.into(),
    });
    let v = serde_json::to_value(&run).unwrap();
    assert_eq!(
        v["resets"],
        serde_json::json!([{
            "before_case_id": 2,
            "names": ["cycle published"],
            "changed_by": [["cycle published", [1]]],
            "waited_ms": 1234,
            "outcome": "continued"
        }])
    );
    let back: LocalRun = serde_json::from_value(v).unwrap();
    assert_eq!(back, run);
}

#[test]
fn an_old_run_file_without_resets_loads_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let old = serde_json::json!({
        "id": "run-old",
        "pbi_id": 7,
        "started_at": "1700000000000",
        "cases": [{ "case_id": 1, "title": "t", "verdict": "Passed", "note": "", "steps": [] }],
        "mode": "unattended"
    });
    let back: LocalRun = serde_json::from_value(old.clone()).unwrap();
    assert!(back.resets.is_empty());
    // Saved again, it reads exactly as it was written.
    store::save_run(root, &back).unwrap();
    let again = store::load_run(root, "run-old").unwrap().unwrap();
    assert_eq!(again, back);
    assert_eq!(serde_json::to_value(&again).unwrap(), old);

    // A reset written with only what it must have still loads.
    let bare: ResetRecord = serde_json::from_value(serde_json::json!({ "before_case_id": 3 })).unwrap();
    assert_eq!(bare, ResetRecord { before_case_id: 3, ..Default::default() });
}

#[test]
fn the_report_shows_the_reset_line_between_the_cases() {
    let mut run = new_run("run-report");
    for id in [1, 2] {
        run.cases.push(serde_json::from_value(serde_json::json!({
            "case_id": id, "title": format!("case {id}"), "verdict": "", "note": "", "steps": [], "proposed": "Passed"
        })).unwrap());
    }
    run.resets.push(ResetRecord {
        before_case_id: 2,
        names: vec!["cycle <published>".into(), "rota".into()],
        changed_by: vec![],
        waited_ms: 10,
        outcome: RESET_STOPPED.into(),
    });
    assert_eq!(
        report::reset_lines(&run.resets[0]),
        vec!["Reset: revert \"cycle <published>\" - stopped".to_string(), "Reset: revert \"rota\" - stopped".to_string()]
    );
    let html = report::build_with_downloads(&run, &[], "", &|_| false, &|_| None);
    let line = "Reset: revert &quot;cycle &lt;published&gt;&quot; - stopped";
    let at = html.find(line).unwrap_or_else(|| panic!("no reset line in {html}"));
    // Between case 1 and case 2 in the cases table.
    let one = html.find("<td class=\"id\">#1</td>").unwrap();
    let two = html.find("<td class=\"id\">#2</td>").unwrap();
    assert!(one < at && at < two, "{html}");
    assert!(html.contains("Reset: revert &quot;rota&quot; - stopped"));
    // And between the two in the detail.
    let detail = html.find("<h2>Cases in detail</h2>").unwrap();
    let later = html[detail..].find(line).unwrap() + detail;
    let one_d = html[detail..].find("#1</span>").unwrap() + detail;
    let two_d = html[detail..].find("#2</span>").unwrap() + detail;
    assert!(one_d < later && later < two_d);
    // A run with no resets shows none.
    let none = report::build_with_downloads(&new_run("run-plain"), &[], "", &|_| false, &|_| None);
    assert!(!none.contains("Reset: revert"));
}

#[test]
fn the_run_plans_again_from_the_scripts_and_its_order_wins_over_the_order_sent() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // A cycle: one reset whatever the order.
    store::save_script(root, &marked(10, &["x"], &["y"])).unwrap();
    store::save_script(root, &marked(20, &["y"], &["x"])).unwrap();
    store::save_script(root, &marked(30, &[], &[])).unwrap();
    let want = v2_lib::commands::autorun::plan_at(root, 5, &[10, 20, 30], None);
    assert_eq!(want.resets.len(), 1);

    let (list, resets) = planned_cases(root, 5, &to_run(&[10, 20, 30]));
    assert_eq!(list.iter().map(|c| c.case_id).collect::<Vec<_>>(), want.order);
    assert_eq!(resets, want.resets);
    assert_eq!(list[0].title, format!("case {}", want.order[0]), "each case keeps what was sent with it");

    // A saved order wins over whatever order arrives.
    store::save_order(root, 5, &[30, 20, 10]).unwrap();
    let (list, resets) = planned_cases(root, 5, &to_run(&[10, 20, 30]));
    assert_eq!(list.iter().map(|c| c.case_id).collect::<Vec<_>>(), vec![30, 20, 10]);
    assert_eq!(resets.len(), 1);
    assert_eq!(resets[0].before_case_id, 10);
}

#[tokio::test]
async fn waiting_gives_the_panels_payload_until_the_pause_is_answered() {
    let waits = ResetWaits::new();
    assert_eq!(waits.waiting(), None);
    let needed = AutorunResetNeeded {
        run_id: "run-w".into(),
        before_case_id: 2,
        names: vec!["cycle published".into()],
        changed_by: vec![("cycle published".into(), vec![1])],
        remaining: vec![2, 3],
    };
    let cancel = AtomicBool::new(false);
    let answer = async {
        // A screen that comes back finds the pause, with all it showed.
        until_waiting(&waits).await;
        assert_eq!(waits.waiting(), Some(needed.clone()));
        waits.answer("run-w", false).unwrap();
        assert_eq!(waits.waiting(), None, "nothing waits once it is answered");
    };
    let (go_on, ()) = tokio::join!(waits.wait(needed.clone(), &cancel), answer);
    assert!(!go_on);
    assert_eq!(waits.waiting(), None);
}
