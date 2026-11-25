//! Auto Test over a test suite: the suite's own cases, a run record that
//! says which suite it was over, and orders kept per scope.

use crate::common;

use std::sync::atomic::AtomicBool;
use v2_lib::ado::AdoClient;
use v2_lib::autorun::replay::{run_selection, Browsers};
use v2_lib::autorun::{store, CaseScript, LocalRun, RunScope, SuiteRef};
use v2_lib::browser::timing::Timing;
use wiremock::matchers::{method as wm_method, path as wm_path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn suite(plan_id: i32, suite_id: i32) -> SuiteRef {
    SuiteRef { plan_id, suite_id, title: "Login".into() }
}

/// A suite with two cases on two configurations each, and a child suite
/// whose case only appears when the endpoint is asked for the children.
#[tokio::test]
async fn suite_cases_are_own_points_deduplicated_in_order() {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let points = || wm_path("/acme/Web/_apis/testplan/Plans/9/Suites/91/TestPoint");
    Mock::given(wm_method("GET"))
        .and(points())
        .and(query_param("isRecursive", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 9, "testCaseReference": { "id": 999, "name": "Child" }, "configuration": { "name": "Chrome" } }]
        })))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(points())
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                { "id": 1, "testCaseReference": { "id": 202, "name": "Second" }, "configuration": { "name": "Chrome" } },
                { "id": 2, "testCaseReference": { "id": 201, "name": "First" }, "configuration": { "name": "Chrome" } },
                { "id": 3, "testCaseReference": { "id": 202, "name": "Second" }, "configuration": { "name": "Edge" } },
                { "id": 4, "testCaseReference": { "id": 201, "name": "First" }, "configuration": { "name": "Edge" } }
            ]
        })))
        .with_priority(5)
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                { "id": 201, "fields": { "System.Title": "First" } },
                { "id": 202, "fields": { "System.Title": "Second" } }
            ]
        })))
        .mount(&server)
        .await;

    let cases = client.get_suite_cases_full("acme", "Web", 9, 91, None, None).await.unwrap();
    let ids: Vec<i32> = cases.iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![202, 201], "own cases only, each once, in the suite's order");
    let asked_for_children = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .any(|r| r.url.query().unwrap_or("").contains("isRecursive"));
    assert!(!asked_for_children, "the suite's own points are read, not its children's");
}

#[test]
fn old_run_record_reads_as_pbi_run() {
    let run: LocalRun = serde_json::from_str(r##"{"id":"r1","pbi_id":77,"started_at":"1","cases":[]}"##).unwrap();
    assert_eq!(run.suite, None);
    assert_eq!(run.scope(), RunScope::Pbi(77));
    let text = serde_json::to_string(&run).unwrap();
    assert!(!text.contains("suite"), "a PBI run is written as it always was: {text}");
}

#[test]
fn suite_run_record_round_trips() {
    let run = LocalRun {
        suite: Some(suite(9, 91)),
        id: "r2".into(),
        pbi_id: 0,
        started_at: "1".into(),
        cases: vec![],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
        video_unavailable: false,
    };
    let back: LocalRun = serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
    assert_eq!(back, run);
    assert_eq!(back.scope(), RunScope::Suite(suite(9, 91)));
}

#[test]
fn scope_wire_shape_and_key() {
    let pbi = serde_json::to_value(RunScope::Pbi(5)).unwrap();
    assert_eq!(pbi, serde_json::json!({ "kind": "pbi", "pbi_id": 5 }));
    let s = serde_json::to_value(RunScope::Suite(suite(9, 91))).unwrap();
    assert_eq!(s, serde_json::json!({ "kind": "suite", "plan_id": 9, "suite_id": 91, "title": "Login" }));
    let back: RunScope = serde_json::from_value(s).unwrap();
    assert_eq!(back, RunScope::Suite(suite(9, 91)));
    assert_eq!(RunScope::Pbi(5).key(), "pbi:5");
    assert_eq!(RunScope::Suite(suite(9, 91)).key(), "suite:9:91");
}

#[test]
fn order_files_are_keyed_by_scope() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // A PBI and a suite sharing a number never meet.
    store::save_order(root, RunScope::Pbi(91), &[1, 2]).unwrap();
    store::save_order(root, RunScope::Suite(suite(9, 91)), &[3, 4, 5]).unwrap();
    assert_eq!(store::load_order(root, RunScope::Pbi(91)), Some(vec![1, 2]));
    assert_eq!(store::load_order(root, RunScope::Suite(suite(9, 91))), Some(vec![3, 4, 5]));
    // The PBI path is the one orders have always had; a bare id still means a PBI.
    assert!(root.join("orders").join("91.json").is_file());
    assert!(root.join("orders").join("suite-9-91.json").is_file());
    assert_eq!(store::load_order(root, 91), Some(vec![1, 2]));
    store::clear_order(root, RunScope::Suite(suite(9, 91))).unwrap();
    assert_eq!(store::load_order(root, RunScope::Suite(suite(9, 91))), None);
    assert_eq!(store::load_order(root, RunScope::Pbi(91)), Some(vec![1, 2]));
}

struct OneBrowser(Option<common::ScriptedDriver>);

impl Browsers for OneBrowser {
    type D = common::ScriptedDriver;
    async fn open(&mut self) -> Result<Self::D, String> {
        self.0.take().ok_or_else(|| "no browser".to_string())
    }
    async fn close(&mut self, _d: Self::D) {}
}

#[tokio::test]
async fn replay_records_the_suite_scope() {
    let _leases = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let script: CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": 1, "title": "case 1", "account": null,
        "steps": [{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }]
    }))
    .unwrap();
    store::save_script(root, &script).unwrap();
    let timing = Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 20, highlight_ms: 0, lease_wait_ms: 300 };

    let scope = RunScope::Suite(suite(9, 91));
    let mut run = v2_lib::commands::autorun_replay::new_replay_run(&scope, "run-suite".into(), "1700000000000".into(), None);
    assert_eq!((run.pbi_id, run.suite.clone()), (0, Some(suite(9, 91))));
    let mut browsers = OneBrowser(Some(common::FakePage::default().driver()));
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(1, "case 1".to_string())], &timing, &cancel, &mut |_| {})
        .await
        .unwrap();

    let saved = store::load_run(root, "run-suite").unwrap().expect("the run was kept");
    assert_eq!(saved.scope(), RunScope::Suite(suite(9, 91)));
    assert_eq!(saved.pbi_id, 0);

    let pbi_run = v2_lib::commands::autorun_replay::new_replay_run(&RunScope::Pbi(42), "r".into(), "1".into(), None);
    assert_eq!((pbi_run.pbi_id, pbi_run.suite), (42, None));
}
