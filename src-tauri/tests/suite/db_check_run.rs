//! Running an `expect_database`: asking again until the answer is right,
//! binding the values it carries as quoted literals, the sentences it fails
//! with, and the step that carries it through the runner.

use crate::common::{FakePage, ScriptedDriver};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use v2_lib::autorun::db_check::{run_check, DbRows, DbSource, NO_DATABASE, READING_OFF};
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
use v2_lib::autorun::setup::resolve;
use v2_lib::autorun::{CaseScript, StepScript};
use v2_lib::browser::actions::{Action, ActionOutcome};
use v2_lib::browser::expect::NOTED_SECRET;
use v2_lib::db::query::DB_UNREACHABLE;

/// A database that answers from a queue (the last answer repeats) and
/// remembers every SQL it was asked.
struct Fake {
    answers: Mutex<VecDeque<Result<String, String>>>,
    asked: Mutex<Vec<String>>,
}

impl Fake {
    fn new(answers: Vec<Result<String, String>>) -> Self {
        Fake { answers: Mutex::new(answers.into()), asked: Mutex::new(Vec::new()) }
    }
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl DbRows for Fake {
    fn select<'a>(&'a self, sql: &'a str) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        self.asked.lock().unwrap().push(sql.to_string());
        let mut q = self.answers.lock().unwrap();
        let next = if q.len() > 1 { q.pop_front().unwrap() } else { q.front().cloned().unwrap() };
        Box::pin(async move { next })
    }
}

fn status(value: &str) -> Result<String, String> {
    Ok(format!("status\n------\n{value}\n\n(1 rows affected)\n"))
}

fn none() -> Result<String, String> {
    Ok("status\n------\n\n(0 rows affected)\n".to_string())
}

fn act(v: Value) -> Action {
    let mut v = v;
    v["kind"] = json!("expect_database");
    serde_json::from_value(v).unwrap()
}

fn approved() -> Action {
    act(json!({"sql": "SELECT status FROM r ORDER BY id", "column": "status", "equals": "Approved"}))
}

async fn check(db: &Fake, a: &Action, noted: &BTreeMap<String, String>, timeout_ms: u32) -> ActionOutcome {
    run_check(&DbSource::Ready(db), a, noted, timeout_ms, None).await
}

#[tokio::test]
async fn run_retries_until_row_appears() {
    let db = Fake::new(vec![none(), none(), status("Approved")]);
    let out = check(&db, &approved(), &BTreeMap::new(), 5_000).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(db.asked().len(), 3);
}

#[tokio::test]
async fn run_times_out_with_expected_found() {
    let db = Fake::new(vec![status("Pending")]);
    let out = check(&db, &approved(), &BTreeMap::new(), 1_000).await;
    assert!(!out.ok);
    assert_eq!(out.detail, "expected status 'Approved', found 'Pending'");
    assert!(db.asked().len() >= 2, "it asked again before giving up");
}

#[tokio::test]
async fn run_missing_db_says_sentence() {
    let out = run_check(&DbSource::default(), &approved(), &BTreeMap::new(), 1_000, None).await;
    assert!(!out.ok);
    assert_eq!(out.detail, NO_DATABASE);
    let out = run_check(&DbSource::Missing("no login saved".into()), &approved(), &BTreeMap::new(), 1_000, None).await;
    assert_eq!(out.detail, "no login saved");
}

#[tokio::test]
async fn run_reading_off_says_sentence() {
    let out = run_check(&DbSource::ReadingOff, &approved(), &BTreeMap::new(), 1_000, None).await;
    assert!(!out.ok);
    assert_eq!(out.detail, READING_OFF);
}

#[tokio::test]
async fn run_query_error_fails_at_once() {
    let db = Fake::new(vec![Err("Invalid column name 'stauts'.".into())]);
    let out = check(&db, &approved(), &BTreeMap::new(), 5_000).await;
    assert!(!out.ok);
    assert!(out.detail.contains("Invalid column name 'stauts'"), "{}", out.detail);
    assert_eq!(db.asked().len(), 1);
}

#[tokio::test]
async fn run_unreachable_stops_early() {
    let db = Fake::new(vec![Err("Sqlcmd: Error: Could not open a connection to SQL Server [db01.corp.example].".into())]);
    let out = check(&db, &approved(), &BTreeMap::new(), 5_000).await;
    assert!(!out.ok);
    assert_eq!(out.detail, DB_UNREACHABLE);
    assert!(!out.detail.contains("db01"), "{}", out.detail);
    assert_eq!(db.asked().len(), 1);
}

#[tokio::test]
async fn run_binds_noted_value_quoted() {
    let db = Fake::new(vec![status("Approved")]);
    let a = act(json!({"sql": "SELECT status FROM r WHERE no = {{noted.req}} ORDER BY id", "column": "status", "equals": "Approved"}));
    let noted = BTreeMap::from([("req".to_string(), "LR-0457".to_string())]);
    let out = check(&db, &a, &noted, 1_000).await;
    assert!(out.ok, "{out:?}");
    assert_eq!(db.asked(), vec!["SELECT status FROM r WHERE no = N'LR-0457' ORDER BY id".to_string()]);
}

#[tokio::test]
async fn run_refuses_noted_secret() {
    let db = Fake::new(vec![status("Approved")]);
    let a = act(json!({"sql": "SELECT 1 FROM r WHERE p = {{noted.p}}", "exists": true}));
    let noted = BTreeMap::from([("p".to_string(), format!("{NOTED_SECRET}hunter2"))]);
    let out = check(&db, &a, &noted, 1_000).await;
    assert!(!out.ok);
    assert!(!out.detail.contains("hunter2"), "{}", out.detail);
    assert!(out.detail.contains("password"), "{}", out.detail);
    assert!(db.asked().is_empty());
}

#[tokio::test]
async fn run_names_a_value_nothing_gave() {
    let db = Fake::new(vec![status("Approved")]);
    let a = act(json!({"sql": "SELECT 1 FROM r WHERE p = {{noted.never}}", "exists": true}));
    let out = check(&db, &a, &BTreeMap::new(), 1_000).await;
    assert!(!out.ok);
    assert!(out.detail.contains("{{noted.never}}"), "{}", out.detail);
    assert!(db.asked().is_empty());
}

#[tokio::test]
async fn run_stops_when_cancelled() {
    let db = Fake::new(vec![status("Pending")]);
    let stop = AtomicBool::new(true);
    let out = run_check(&DbSource::Ready(&db), &approved(), &BTreeMap::new(), 5_000, Some(&stop)).await;
    assert!(!out.ok);
    assert!(db.asked().is_empty());
}

#[test]
fn substitute_steps_binds_setup_in_sql_as_literal() {
    let sc: CaseScript = serde_json::from_value(json!({
        "case_id": 1, "title": "t", "setup": { "fixture": "own" },
        "steps": [{ "step_number": 1, "actions": [
            { "kind": "expect_database", "sql": "SELECT 1 FROM e WHERE e = {{setup.emp}}", "exists": true },
            { "kind": "check_text", "value": "{{setup.emp}}" }
        ] }]
    }))
    .unwrap();
    let out = resolve(&sc, &BTreeMap::new(), &BTreeMap::from([("emp".to_string(), json!("O'Brien"))])).unwrap();
    let text = serde_json::to_string(&out.steps).unwrap();
    assert!(text.contains("N'O''Brien'"), "{text}");
    // Anything else is filled in as text, as it always was.
    assert!(text.contains("\"value\":\"O'Brien\""), "{text}");
}

#[test]
fn unreachable_db_check_is_transient() {
    use v2_lib::autorun::{CaseRecord, StepRecord};
    let script: CaseScript = serde_json::from_value(json!({
        "case_id": 1, "title": "t",
        "steps": [{ "step_number": 1, "actions": [
            { "kind": "expect_database", "sql": "SELECT 1", "exists": true }
        ] }]
    }))
    .unwrap();
    let case = |detail: &str| {
        let mut c: CaseRecord = serde_json::from_value(json!({
            "case_id": 1, "title": "t", "verdict": "", "note": "", "proposed": "Failed", "reason": format!("step 1: {detail}"),
            "steps": [], "duration_ms": 1
        }))
        .unwrap();
        c.steps.push(StepRecord {
            step_number: 1,
            outcomes: vec![ActionOutcome::failed(detail)],
            screenshot: None,
            downloads: Vec::new(),
            tab: None,
            dialog: None,
            components: Vec::new(),
            duration_ms: None,
        });
        c
    };
    assert!(v2_lib::autorun::transient::is_transient(&case(DB_UNREACHABLE), Some(&script)).is_some());
    // A wrong answer is the application's, never worth another go.
    assert!(v2_lib::autorun::transient::is_transient(&case("expected status 'A', found 'B'"), Some(&script)).is_none());
}

fn quick() -> v2_lib::browser::timing::Timing {
    v2_lib::browser::timing::Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

async fn run_one(d: &mut ScriptedDriver, run: &mut InRun<'_>) -> Vec<ActionOutcome> {
    let step: StepScript = serde_json::from_value(json!({ "step_number": 1, "actions": [
        { "kind": "expect_database", "sql": "SELECT status FROM r ORDER BY id", "column": "status", "equals": "Approved" }
    ] }))
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut account = None;
    let mut held = Held::supervised();
    run_step_in_run(
        d, dir.path(), "Acme", "Web", &step, &quick(), &mut account, &mut held, None,
        AreaRoute::Unknown(NEEDS_SCRIPT_AREA), run,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn runner_step_runs_expect_database() {
    let db = Fake::new(vec![status("Approved")]);
    let mut d: ScriptedDriver = FakePage::default().driver();
    let mut run = InRun { db: DbSource::Ready(&db), clock: Some(std::time::Instant::now()), ..Default::default() };
    let out = run_one(&mut d, &mut run).await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(db.asked().len(), 1);
    assert_eq!(run.marks.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(), ["Check the database"]);
}

#[tokio::test]
async fn runner_step_without_a_database_says_so() {
    let mut d: ScriptedDriver = FakePage::default().driver();
    let mut run = InRun::default();
    let out = run_one(&mut d, &mut run).await;
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, NO_DATABASE);
}
