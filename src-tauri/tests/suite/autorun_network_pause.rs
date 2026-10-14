//! An unattended run and the company network: it asks its gate before each
//! case and after a Failed or Blocked one, waits when the network is gone,
//! re-runs the case the drop interrupted (at most three times), and ends
//! cleanly - everything left recorded as not run - when the wait is stopped
//! or gives up. Each wait is kept in the run's `network_pauses`.
//!
//! Browsers are fakes (`common::FakePage`); the gate answers from a script.

use crate::common;

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use v2_lib::autorun::network::{Network, WaitEnd};
use v2_lib::autorun::plan::Pause;
use v2_lib::autorun::preconditions::{NoDb, PreconditionDb, NEED_DB};
use v2_lib::autorun::replay::{
    run_cases_planned, Browsers, CaseToRun, NeverPauses, ResetGate, NETWORK_EVERY_TIME, NETWORK_GAVE_UP, NETWORK_STOPPED,
};
use v2_lib::autorun::{store, CaseScript, LocalRun};
use v2_lib::browser::timing::Timing;
use v2_lib::events::ReplayProgress;

/// Fake browsers: the `n`th open (1-based, across the whole run) shows a page
/// without the text a case checks for when `n` is in `fail_opens`.
struct FakeBrowsers {
    opened: usize,
    fail_opens: Vec<usize>,
}

impl Browsers for FakeBrowsers {
    type D = common::ScriptedDriver;
    async fn open(&mut self) -> Result<Self::D, String> {
        self.opened += 1;
        let body_has_text = !self.fail_opens.contains(&self.opened);
        Ok(common::FakePage { body_has_text, ..common::FakePage::default() }.driver())
    }
    async fn close(&mut self, _d: Self::D) {}
}

fn browsers(fail_opens: &[usize]) -> FakeBrowsers {
    FakeBrowsers { opened: 0, fail_opens: fail_opens.to_vec() }
}

/// A gate that answers the network from a queue (`fallback` once it runs
/// out), ends each wait from another queue (`Resumed` once it runs out) and
/// remembers what it was asked.
struct ScriptedGate {
    networks: VecDeque<Network>,
    fallback: Network,
    waits: VecDeque<WaitEnd>,
    waited_before: Vec<i32>,
}

fn gate(networks: &[Network], fallback: Network, waits: &[WaitEnd]) -> ScriptedGate {
    ScriptedGate {
        networks: networks.iter().copied().collect(),
        fallback,
        waits: waits.iter().copied().collect(),
        waited_before: vec![],
    }
}

impl ResetGate for ScriptedGate {
    async fn wait(&mut self, _reset: &Pause, _remaining: &[i32], _cancel: &AtomicBool) -> bool {
        true
    }
    async fn network(&mut self) -> Network {
        self.networks.pop_front().unwrap_or(self.fallback)
    }
    async fn wait_for_network(&mut self, before_case_id: i32, _cancel: &AtomicBool) -> WaitEnd {
        self.waited_before.push(before_case_id);
        self.waits.pop_front().unwrap_or(WaitEnd::Resumed)
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
        network_pauses: vec![],
    }
}

/// A one-step script with one check for the text "ok".
fn script(case_id: i32) -> CaseScript {
    serde_json::from_value(serde_json::json!({
        "case_id": case_id,
        "title": format!("case {case_id}"),
        "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }],
    }))
    .unwrap()
}

fn to_run(ids: &[i32]) -> Vec<CaseToRun> {
    ids.iter().map(|&id| CaseToRun { case_id: id, title: format!("case {id}"), module: None }).collect()
}

fn no_db() -> PreconditionDb<NoDb> {
    PreconditionDb::Missing(NEED_DB.to_string())
}

fn saved_cases(root: &std::path::Path, ids: &[i32]) {
    for &id in ids {
        store::save_script(root, &script(id)).unwrap();
    }
}

async fn run_with<G: ResetGate>(
    root: &std::path::Path,
    b: &mut FakeBrowsers,
    run: &mut LocalRun,
    ids: &[i32],
    g: &mut G,
    events: &mut Vec<ReplayProgress>,
) {
    let cancel = AtomicBool::new(false);
    run_cases_planned(
        b,
        root,
        "Acme",
        "Web",
        run,
        &to_run(ids),
        None,
        false,
        &quick(),
        &cancel,
        &no_db(),
        &[],
        g,
        &mut |p: ReplayProgress| events.push(p),
    )
    .await
    .unwrap();
}

/// What a record says, without its timings.
fn shape(run: &LocalRun) -> Vec<(i32, String, String)> {
    run.cases.iter().map(|c| (c.case_id, c.proposed.clone(), c.reason.clone())).collect()
}

#[tokio::test]
async fn unchecked_runs_exactly_as_before() {
    let dir = tempfile::tempdir().unwrap();
    saved_cases(dir.path(), &[1, 2]);
    // The second case fails, so the failure path is covered too.
    let mut plain = new_run("plain");
    run_with(dir.path(), &mut browsers(&[2]), &mut plain, &[1, 2], &mut NeverPauses, &mut vec![]).await;

    let mut g = gate(&[], Network::Unchecked, &[]);
    let mut checked = new_run("checked");
    run_with(dir.path(), &mut browsers(&[2]), &mut checked, &[1, 2], &mut g, &mut vec![]).await;

    assert_eq!(shape(&checked), shape(&plain));
    assert_eq!(shape(&checked)[1].1, "Failed");
    assert!(checked.network_pauses.is_empty());
    assert!(g.waited_before.is_empty());
}

#[tokio::test]
async fn unreachable_before_a_case_waits_then_runs_it() {
    let dir = tempfile::tempdir().unwrap();
    saved_cases(dir.path(), &[1]);
    let mut g = gate(&[Network::Unreachable], Network::Reachable, &[WaitEnd::Resumed]);
    let mut b = browsers(&[]);
    let mut run = new_run("before");
    run_with(dir.path(), &mut b, &mut run, &[1], &mut g, &mut vec![]).await;

    assert_eq!(run.network_pauses.len(), 1);
    assert_eq!(run.network_pauses[0].before_case_id, 1);
    assert_eq!(run.network_pauses[0].outcome, "resumed");
    assert_eq!(g.waited_before, vec![1]);
    assert_eq!(run.cases.len(), 1);
    assert_eq!(run.cases[0].proposed, "Passed");
    assert_eq!(b.opened, 1);
}

#[tokio::test]
async fn a_failure_while_unreachable_is_rerun_not_recorded() {
    let dir = tempfile::tempdir().unwrap();
    saved_cases(dir.path(), &[1]);
    // Before: answering. The first go fails and the network is gone; the
    // second go passes.
    let mut g = gate(&[Network::Reachable, Network::Unreachable], Network::Reachable, &[WaitEnd::Resumed]);
    let mut b = browsers(&[1]);
    let mut run = new_run("rerun");
    run_with(dir.path(), &mut b, &mut run, &[1], &mut g, &mut vec![]).await;

    assert_eq!(b.opened, 2);
    assert_eq!(run.cases.len(), 1, "the interrupted go is not recorded");
    assert_eq!(run.cases[0].proposed, "Passed");
    assert_eq!(run.network_pauses.len(), 1);
    assert_eq!(run.network_pauses[0].before_case_id, 1);
    assert_eq!(run.network_pauses[0].outcome, "resumed");
    let saved = store::load_run(dir.path(), "rerun").unwrap().unwrap();
    assert_eq!(saved, run);
}

#[tokio::test]
async fn a_failure_while_reachable_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    saved_cases(dir.path(), &[1]);
    let mut g = gate(&[], Network::Reachable, &[]);
    let mut b = browsers(&[1]);
    let mut run = new_run("kept");
    run_with(dir.path(), &mut b, &mut run, &[1], &mut g, &mut vec![]).await;

    assert_eq!(b.opened, 1);
    assert_eq!(run.cases.len(), 1);
    assert_eq!(run.cases[0].proposed, "Failed");
    assert!(!run.cases[0].reason.ends_with(NETWORK_EVERY_TIME));
    assert!(run.network_pauses.is_empty());
}

#[tokio::test]
async fn reruns_stop_after_three() {
    let dir = tempfile::tempdir().unwrap();
    saved_cases(dir.path(), &[1]);
    // Answering before the case, then gone after each of its four goes.
    let mut g = gate(&[Network::Reachable], Network::Unreachable, &[]);
    let mut b = browsers(&[1, 2, 3, 4]);
    let mut run = new_run("cap");
    run_with(dir.path(), &mut b, &mut run, &[1], &mut g, &mut vec![]).await;

    assert_eq!(b.opened, 4, "the first go and three re-runs");
    assert_eq!(run.network_pauses.len(), 3);
    assert!(run.network_pauses.iter().all(|p| p.outcome == "resumed"));
    assert_eq!(run.cases.len(), 1);
    assert_eq!(run.cases[0].proposed, "Failed");
    assert!(run.cases[0].reason.ends_with(NETWORK_EVERY_TIME), "{}", run.cases[0].reason);
}

#[tokio::test]
async fn stop_and_give_up_end_the_run() {
    for (end, reason, outcome) in [(WaitEnd::Stopped, NETWORK_STOPPED, "stopped"), (WaitEnd::GaveUp, NETWORK_GAVE_UP, "gave_up")] {
        let dir = tempfile::tempdir().unwrap();
        saved_cases(dir.path(), &[1, 2, 3]);
        // Case 1 runs; the network is gone before case 2.
        let mut g = gate(&[Network::Reachable, Network::Unreachable], Network::Reachable, &[end]);
        let mut b = browsers(&[]);
        let mut run = new_run("ended");
        let mut events = vec![];
        run_with(dir.path(), &mut b, &mut run, &[1, 2, 3], &mut g, &mut events).await;

        assert_eq!(b.opened, 1, "no browser opens after the wait ended the run");
        assert_eq!(run.cases.len(), 3);
        assert_eq!(run.cases[0].proposed, "Passed");
        for c in &run.cases[1..] {
            assert_eq!(c.reason, reason);
            assert_eq!(c.proposed, "");
            assert!(!c.steps.is_empty());
            assert!(c.steps.iter().flat_map(|s| &s.outcomes).all(|o| !o.ok && o.detail.starts_with("not run:")));
        }
        assert_eq!(run.network_pauses.len(), 1);
        assert_eq!(run.network_pauses[0].before_case_id, 2);
        assert_eq!(run.network_pauses[0].outcome, outcome);
        let saved = store::load_run(dir.path(), "ended").unwrap().unwrap();
        assert_eq!(saved, run);
        let done: Vec<i32> = events.iter().filter(|e| e.phase == "done").map(|e| e.case_id).collect();
        assert_eq!(done, vec![1, 2, 3]);
    }
}

#[tokio::test]
async fn stopped_during_a_rerun_wait_records_the_case_too() {
    let dir = tempfile::tempdir().unwrap();
    saved_cases(dir.path(), &[1, 2]);
    let mut g = gate(&[Network::Reachable, Network::Unreachable], Network::Reachable, &[WaitEnd::Stopped]);
    let mut b = browsers(&[1]);
    let mut run = new_run("mid");
    run_with(dir.path(), &mut b, &mut run, &[1, 2], &mut g, &mut vec![]).await;

    // The interrupted go is dropped and the case is recorded as not run.
    assert_eq!(b.opened, 1);
    assert_eq!(run.cases.len(), 2);
    assert!(run.cases.iter().all(|c| c.reason == NETWORK_STOPPED && c.proposed.is_empty()));
}
