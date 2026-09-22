//! Sending a reviewed run to Azure DevOps: the pure helpers, and the whole
//! `publish_run` lifecycle against a mock server. Follows the matcher
//! shapes pinned in `ado_testplan.rs`'s `run_lifecycle_create_update_complete`
//! and its neighbours rather than guessing endpoint paths.

use v2_lib::ado::AdoClient;
use v2_lib::ado_testplan::EnsuredSuite;
use v2_lib::autorun::publish::{
    comment_for, pictures_for, publish_run, step_marks, PublishCase, PublishResult, SkippedCase,
};
use v2_lib::autorun::{store, LocalRun};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn suite() -> EnsuredSuite {
    EnsuredSuite { plan_id: 10, plan_name: "Plan".into(), suite_id: 20, created_plan: false }
}

/// Case 7 confirmed Failed (step 2 failed, with a failure picture and a
/// step picture), case 8 confirmed Passed, case 9 proposed Passed but NOT
/// confirmed.
fn reviewed_run(root: &std::path::Path) -> LocalRun {
    let shot = store::save_shot(root, b"\xFF\xD8\xFF\xD9").unwrap();
    let run: LocalRun = serde_json::from_value(serde_json::json!({
        "id": "run-5", "pbi_id": 42, "started_at": "1700000000000", "mode": "unattended",
        "cases": [
          { "case_id": 7, "title": "Leave request", "verdict": "Failed", "note": "Save is missing on the new form",
            "proposed": "Failed", "reason": "step 2: button \"Save\" not found", "duration_ms": 8123, "account": "hr.admin",
            "steps": [
              { "step_number": 0, "outcomes": [{ "ok": true, "detail": "signed in as HR Admin" }] },
              { "step_number": 1, "outcomes": [{ "ok": true, "detail": "loaded" }], "screenshot": shot },
              { "step_number": 2, "outcomes": [{ "ok": false, "detail": "button \"Save\" not found", "screenshot": shot }], "screenshot": shot },
              { "step_number": 3, "outcomes": [{ "ok": false, "detail": "not run: an earlier step of this case failed" }] }
            ] },
          { "case_id": 8, "title": "Cancel request", "verdict": "Passed", "note": "", "proposed": "Passed",
            "reason": "every action of 1 steps passed",
            "steps": [{ "step_number": 1, "outcomes": [{ "ok": true, "detail": "ok" }] }] },
          { "case_id": 9, "title": "Unconfirmed", "verdict": "", "note": "", "proposed": "Passed", "reason": "every action of 1 steps passed",
            "steps": [{ "step_number": 1, "outcomes": [{ "ok": true, "detail": "ok" }] }] }
        ]
    })).unwrap();
    store::save_run(root, &run).unwrap();
    run
}

/// The case ids and real Azure DevOps step ids the review screen would
/// send alongside the run: case 7 has three real steps (the sign-in step
/// never gets one), case 8 has one.
fn publish_cases() -> Vec<PublishCase> {
    vec![
        PublishCase { case_id: 7, step_ids: vec!["101".into(), "102".into(), "103".into()] },
        PublishCase { case_id: 8, step_ids: vec!["201".into()] },
    ]
}

// ---------------------------------------------------------------------
// Pure-function tests (no server).
// ---------------------------------------------------------------------

#[test]
fn the_persons_note_leads_the_comment_and_is_never_the_part_that_is_cut() {
    let dir = tempfile::tempdir().unwrap();
    let run = reviewed_run(dir.path());
    let c = comment_for(&run.cases[0]);
    assert!(c.starts_with("Save is missing on the new form"), "{c}");
    assert!(c.contains("Auto Run as hr.admin: step 2: button \"Save\" not found"), "{c}");
    let mut long = run.cases[0].clone();
    long.reason = "x".repeat(5000);
    let cut = comment_for(&long);
    assert!(cut.chars().count() <= 1000);
    assert!(cut.starts_with("Save is missing on the new form"));
    assert_eq!(comment_for(&run.cases[1]), "Auto Run: every action of 1 steps passed");
}

#[test]
fn step_marks_follow_the_cases_own_steps_and_leave_the_unrun_unmarked() {
    let dir = tempfile::tempdir().unwrap();
    let run = reviewed_run(dir.path());
    let ids = vec!["2".to_string(), "3".to_string(), "4".to_string(), "5".to_string()];
    assert_eq!(
        step_marks(&run.cases[0], &ids),
        vec![Some("Passed".to_string()), Some("Failed".to_string()), None, None],
        "step 3 was not run and the case has no step 4; the sign-in is never a step"
    );
}

#[test]
fn pictures_are_the_failures_then_the_last_step_without_repeats_and_capped() {
    let dir = tempfile::tempdir().unwrap();
    let run = reviewed_run(dir.path());
    let p = pictures_for(&run.cases[0]);
    assert_eq!(p.len(), 1, "one file stands for the failure and the step picture: {p:?}");
    assert_eq!(p[0].0, 2);
    assert!(pictures_for(&run.cases[1]).is_empty());
}

// ---------------------------------------------------------------------
// Server tests.
// ---------------------------------------------------------------------

/// Mounts the whole happy-path lifecycle: two points for case 7 (two
/// configurations), one for case 8; run 900 is created, its three results
/// are 1, 2 and 3; every PATCH and the two attachment POSTs answer ok.
async fn mount_full_run_mocks(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/10/Suites/20/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 701, "testCaseReference": {"id": 7, "name": "Leave request"}, "configuration": {"name": "Windows"}, "results": {}},
                {"id": 702, "testCaseReference": {"id": 7, "name": "Leave request"}, "configuration": {"name": "Mac"}, "results": {}},
                {"id": 801, "testCaseReference": {"id": 8, "name": "Cancel request"}, "configuration": {"name": "Windows"}, "results": {}}
            ]
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/test/runs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 900, "webAccessUrl": "https://x/run/900"
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/test/Runs/900/results"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 1, "testCase": {"id": "7"}, "testPoint": {"id": "701"}},
                {"id": 2, "testCase": {"id": "7"}, "testPoint": {"id": "702"}},
                {"id": 3, "testCase": {"id": "8"}, "testPoint": {"id": "801"}}
            ]
        })))
        .mount(server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/test/Runs/900/results"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"count": 1})))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/test/Runs/900/Results/1/attachments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 1})))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/test/Runs/900/Results/2/attachments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 2})))
        .mount(server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/test/runs/900"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 900})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_reviewed_run_becomes_one_azure_devops_run() {
    let dir = tempfile::tempdir().unwrap();
    let run = reviewed_run(dir.path());
    let server = MockServer::start().await;
    mount_full_run_mocks(&server).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let cases = publish_cases();

    let result = publish_run(&client, dir.path(), "org", "proj", &suite(), "Auto Run", &run.id, &cases)
        .await
        .unwrap();
    let PublishResult::Sent(report) = result else { panic!("expected Sent") };
    assert_eq!(report.run_id, 900);
    assert_eq!(report.web_url, "https://x/run/900");
    assert_eq!(report.sent, vec![7, 8]);
    assert_eq!(report.skipped, vec![SkippedCase { case_id: 9, why: "no verdict was confirmed".into() }]);
    assert!(report.problems.is_empty(), "{:?}", report.problems);

    let requests = server.received_requests().await.unwrap();
    for r in &requests {
        assert!(
            matches!(r.method.as_str(), "GET" | "POST" | "PATCH"),
            "unexpected method {} at {}",
            r.method,
            r.url
        );
        let body = String::from_utf8_lossy(&r.body);
        assert!(!body.contains("Unconfirmed"), "case 9 leaked into a request: {body}");
    }

    // The results PATCHes: outcome for results 1 and 2 is Failed, for 3 is
    // Passed; the Failed comment leads with the person's own note.
    let results_patches: Vec<serde_json::Value> = requests
        .iter()
        .filter(|r| r.method.as_str() == "PATCH" && r.url.path().ends_with("/results"))
        .filter_map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).ok())
        .collect();
    let outcome_of = |result_id: i64| -> Option<String> {
        results_patches.iter().find_map(|v| {
            let item = v.as_array()?.iter().find(|i| i["id"] == result_id)?;
            item.get("outcome").and_then(|o| o.as_str()).map(str::to_string)
        })
    };
    assert_eq!(outcome_of(1).as_deref(), Some("Failed"));
    assert_eq!(outcome_of(2).as_deref(), Some("Failed"));
    assert_eq!(outcome_of(3).as_deref(), Some("Passed"));
    let failed_comment = results_patches
        .iter()
        .find_map(|v| {
            let item = v.as_array()?.iter().find(|i| i["id"] == 1 && i.get("outcome").is_some())?;
            item.get("comment").and_then(|c| c.as_str()).map(str::to_string)
        })
        .unwrap();
    assert!(failed_comment.starts_with("Save is missing on the new form"), "{failed_comment}");

    // An iterationDetails PATCH went out for both of case 7's results.
    let iteration_targets: std::collections::HashSet<i64> = results_patches
        .iter()
        .filter_map(|v| {
            let item = v.as_array()?.first()?;
            if item.get("iterationDetails").is_some() { item["id"].as_i64() } else { None }
        })
        .collect();
    assert!(iteration_targets.contains(&1) && iteration_targets.contains(&2), "{iteration_targets:?}");

    // One attachment POST per Failed result, both the same picture.
    let attach_posts: Vec<serde_json::Value> = requests
        .iter()
        .filter(|r| r.method.as_str() == "POST" && r.url.path().contains("/attachments"))
        .filter_map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).ok())
        .collect();
    assert_eq!(attach_posts.len(), 2, "{attach_posts:?}");
    for a in &attach_posts {
        assert_eq!(a["fileName"], "case-7-step-2.jpg");
    }

    assert!(
        requests.iter().any(|r| r.method.as_str() == "PATCH" && r.url.path().ends_with("/runs/900")),
        "the run must be completed"
    );

    let saved = store::load_run(dir.path(), &run.id).unwrap().unwrap();
    assert_eq!(saved.published.unwrap().run_id, 900);
}

#[tokio::test]
async fn nothing_is_sent_without_a_confirmed_verdict() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = reviewed_run(dir.path());
    for c in &mut run.cases {
        c.verdict = String::new();
    }
    store::save_run(dir.path(), &run).unwrap();
    let server = MockServer::start().await;
    let client = AdoClient::with_base_url("t".into(), server.uri());

    let result = publish_run(&client, dir.path(), "org", "proj", &suite(), "Auto Run", &run.id, &publish_cases())
        .await
        .unwrap();
    match result {
        PublishResult::Refused { why } => assert_eq!(why, "confirm at least one verdict before sending"),
        other => panic!("expected Refused, got {other:?}"),
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_run_is_never_sent_twice() {
    let dir = tempfile::tempdir().unwrap();
    let run = reviewed_run(dir.path());
    let server = MockServer::start().await;
    mount_full_run_mocks(&server).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let cases = publish_cases();

    let first = publish_run(&client, dir.path(), "org", "proj", &suite(), "Auto Run", &run.id, &cases)
        .await
        .unwrap();
    assert!(matches!(first, PublishResult::Sent(_)));
    let count_after_first = server.received_requests().await.unwrap().len();

    let second = publish_run(&client, dir.path(), "org", "proj", &suite(), "Auto Run", &run.id, &cases)
        .await
        .unwrap();
    match second {
        PublishResult::Refused { why } => assert!(why.contains("https://x/run/900"), "{why}"),
        other => panic!("expected Refused, got {other:?}"),
    }
    let count_after_second = server.received_requests().await.unwrap().len();
    assert_eq!(count_after_first, count_after_second, "the second attempt must make no request at all");
}

#[tokio::test]
async fn a_missing_run_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    let client = AdoClient::with_base_url("t".into(), server.uri());

    let result = publish_run(&client, dir.path(), "org", "proj", &suite(), "Auto Run", "run-nope", &publish_cases())
        .await
        .unwrap();
    assert!(matches!(result, PublishResult::Refused { .. }));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_case_outside_the_suite_is_skipped_and_none_inside_is_a_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let run = reviewed_run(dir.path());
    let cases = publish_cases();

    // Only case 8 has a point in the suite.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/10/Suites/20/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 801, "testCaseReference": {"id": 8, "name": "Cancel request"}, "configuration": {"name": "Windows"}, "results": {}}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/test/runs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 900, "webAccessUrl": "https://x/run/900"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/test/Runs/900/results"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 3, "testCase": {"id": "8"}, "testPoint": {"id": "801"}}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/test/Runs/900/results"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"count": 1})))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/test/runs/900"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 900})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let result = publish_run(&client, dir.path(), "org", "proj", &suite(), "Auto Run", &run.id, &cases)
        .await
        .unwrap();
    let PublishResult::Sent(report) = result else { panic!("expected Sent") };
    assert!(
        report.skipped.iter().any(|s| s.case_id == 7 && s.why == "it is not in this PBI's test suite"),
        "{:?}",
        report.skipped
    );
    assert_eq!(report.sent, vec![8]);

    // With no points at all: Refused, and no POST is made.
    let dir2 = tempfile::tempdir().unwrap();
    let run2 = reviewed_run(dir2.path());
    let server2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/10/Suites/20/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
        .mount(&server2)
        .await;
    let client2 = AdoClient::with_base_url("t".into(), server2.uri());
    let result2 = publish_run(&client2, dir2.path(), "org", "proj", &suite(), "Auto Run", &run2.id, &cases)
        .await
        .unwrap();
    match result2 {
        PublishResult::Refused { why } => {
            assert_eq!(why, "none of these cases is in the PBI's test suite in Azure DevOps")
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    let posts = server2
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "POST")
        .count();
    assert_eq!(posts, 0);
}

#[tokio::test]
async fn what_goes_wrong_after_the_run_exists_is_reported_not_thrown() {
    let dir = tempfile::tempdir().unwrap();
    let run = reviewed_run(dir.path());
    let cases = publish_cases();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/10/Suites/20/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 701, "testCaseReference": {"id": 7, "name": "Leave request"}, "configuration": {"name": "Windows"}, "results": {}},
                {"id": 801, "testCaseReference": {"id": 8, "name": "Cancel request"}, "configuration": {"name": "Windows"}, "results": {}}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/test/runs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 900, "webAccessUrl": "https://x/run/900"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/test/Runs/900/results"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 1, "testCase": {"id": "7"}, "testPoint": {"id": "701"}},
                {"id": 3, "testCase": {"id": "8"}, "testPoint": {"id": "801"}}
            ]
        })))
        .mount(&server)
        .await;
    // Every results PATCH (outcomes and iteration details alike) fails,
    // and so does the completion PATCH.
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/test/Runs/900/results"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/test/runs/900"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    let result = publish_run(&client, dir.path(), "org", "proj", &suite(), "Auto Run", &run.id, &cases)
        .await
        .unwrap();
    let PublishResult::Sent(report) = result else { panic!("expected Sent") };
    assert_eq!(report.run_id, 900);
    assert!(report.sent.is_empty(), "{:?}", report.sent);
    assert!(report.problems.iter().any(|p| p.contains("case 7")), "{:?}", report.problems);
    assert!(report.problems.iter().any(|p| p.contains("case 8")), "{:?}", report.problems);
    assert!(
        report.problems.iter().any(|p| p.contains("In Progress")),
        "expected a sentence about the run staying In Progress: {:?}",
        report.problems
    );

    // The run still exists in Azure DevOps, so it is still marked sent -
    // sending it again would create a second one.
    let saved = store::load_run(dir.path(), &run.id).unwrap().unwrap();
    assert_eq!(saved.published.unwrap().run_id, 900);
}

#[tokio::test]
async fn a_picture_that_is_gone_is_a_problem_not_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let run = reviewed_run(dir.path());
    let shot_name = run.cases[0].steps[2].outcomes[0].screenshot.clone().unwrap();
    std::fs::remove_file(dir.path().join("shots").join(&shot_name)).unwrap();

    let server = MockServer::start().await;
    mount_full_run_mocks(&server).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let cases = publish_cases();

    let result = publish_run(&client, dir.path(), "org", "proj", &suite(), "Auto Run", &run.id, &cases)
        .await
        .unwrap();
    let PublishResult::Sent(report) = result else { panic!("expected Sent") };
    assert_eq!(report.sent, vec![7, 8]);
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("case 7"), "{}", report.problems[0]);
    assert!(report.problems[0].to_lowercase().contains("picture"), "{}", report.problems[0]);
}
