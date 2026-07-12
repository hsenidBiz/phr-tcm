use v2_lib::ado::AdoClient;
use v2_lib::ado_testplan::{
    area_matches, build_iteration_details, default_plan_name, OutcomeUpdate,
};

#[test]
fn iteration_details_match_v1_shape() {
    let ids = vec!["7".to_string(), "12".to_string(), "abc".to_string()];
    let outcomes = vec![Some("Passed".to_string()), None, Some("Failed".to_string())];
    let details = build_iteration_details(&ids, &outcomes, "Failed").unwrap();
    let arr = details.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["outcome"], "Failed");
    let actions = arr[0]["actionResults"].as_array().unwrap();
    // Unmarked step 12 is skipped; numeric ids become 8-digit hex paths;
    // non-numeric ids pass through.
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0]["actionPath"], "00000007");
    assert_eq!(actions[0]["stepIdentifier"], "7");
    assert_eq!(actions[1]["actionPath"], "abc");
    assert_eq!(actions[1]["outcome"], "Failed");

    // Nothing marked -> None (v1: don't PATCH at all).
    assert!(build_iteration_details(&ids, &[None, None, None], "Passed").is_none());
    // Empty overall defaults to Failed.
    let d = build_iteration_details(&ids, &outcomes, "").unwrap();
    assert_eq!(d[0]["outcome"], "Failed");
}
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn area_matching_rules() {
    assert!(area_matches("Proj\\Team", "Proj\\Team"));
    assert!(area_matches("Proj\\Team", "Proj\\Team\\Sub"));
    assert!(area_matches("Proj/Team", "Proj\\Team\\Sub"));
    assert!(!area_matches("Proj\\Team", "Proj\\Other"));
    assert!(!area_matches("", "Proj"));
    assert!(!area_matches("Proj", ""));
    assert!(!area_matches("Proj\\Team", "Proj\\TeamX"));
}

#[test]
fn default_plan_names() {
    assert_eq!(default_plan_name("Proj\\Auth"), "Auth - Test Plan");
    assert_eq!(default_plan_name("Proj/Auth"), "Auth - Test Plan");
    assert_eq!(default_plan_name(""), "Test Plan");
}

#[tokio::test]
async fn plans_follow_continuation_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .and(query_param("continuationToken", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 2, "name": "P2", "areaPath": "A\\B"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ms-continuationtoken", "page2")
                .set_body_json(serde_json::json!({
                    "value": [{"id": 1, "name": "P1", "areaPath": "A"}]
                })),
        )
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let plans = client.get_test_plans("org", "proj").await.unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].id, 1);
    assert_eq!(plans[1].id, 2);
}

#[tokio::test]
async fn ensure_suite_reuses_existing_requirement_suite() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 9, "name": "Area Plan", "areaPath": "Proj\\Auth"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 90, "name": "root", "suiteType": "staticTestSuite"},
                {"id": 91, "name": "PBI suite", "suiteType": "requirementTestSuite", "requirementId": 42}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let ensured = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "")
        .await
        .unwrap();
    assert_eq!(ensured.plan_id, 9);
    assert_eq!(ensured.suite_id, 91);
    // Nothing was POSTed - reuse only.
    let posts = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "POST")
        .count();
    assert_eq!(posts, 0);
}

#[tokio::test]
async fn ensure_suite_creates_plan_and_suite_when_none_exist() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 50, "name": "Auth - Test Plan", "areaPath": "Proj\\Auth",
            "rootSuite": {"id": 500}
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/50/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 501})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let ensured = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "")
        .await
        .unwrap();
    assert_eq!(ensured.plan_id, 50);
    assert_eq!(ensured.suite_id, 501);
}

#[tokio::test]
async fn plans_without_suites_are_hidden() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 1, "name": "Empty Plan", "areaPath": "A", "rootSuite": {"id": 10}},
                {"id": 2, "name": "Full Plan", "areaPath": "A", "rootSuite": {"id": 20}},
                {"id": 3, "name": "Locked Plan", "areaPath": "A", "rootSuite": {"id": 30}}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/1/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 10, "name": "Empty Plan", "suiteType": "staticTestSuite"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/2/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 20, "name": "Full Plan", "suiteType": "staticTestSuite"},
                {"id": 21, "name": "PBI 42", "suiteType": "requirementTestSuite", "requirementId": 42}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/3/suites"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let plans = client.list_plans_with_suites("org", "proj").await.unwrap();
    // Empty plan hidden (root-only), locked plan skipped, full plan kept
    // with its root suite stripped.
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].plan.id, 2);
    assert_eq!(plans[0].suites.len(), 1);
    assert_eq!(plans[0].suites[0].id, 21);
    assert_eq!(plans[0].suites[0].requirement_id, Some(42));
}

#[tokio::test]
async fn points_parse_reference_and_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/9/Suites/91/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 7,
                "testCaseReference": {"id": 201, "name": "TC one", "state": "Design"},
                "configuration": {"id": 1, "name": "Windows 10"},
                "tester": {"displayName": "Avin"},
                "results": {"outcome": "passed", "lastTestRunId": 3, "lastResultId": 30}
            }]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let pts = client
        .get_test_points("org", "proj", 9, 91, &[])
        .await
        .unwrap();
    assert_eq!(pts.len(), 1);
    assert_eq!(pts[0].point_id, 7);
    assert_eq!(pts[0].test_case_id, Some(201));
    assert_eq!(pts[0].last_outcome, "passed");
    assert_eq!(pts[0].last_run_id, Some(3));
}

#[tokio::test]
async fn run_lifecycle_create_update_complete() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/test/runs"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "pointIds": [7], "automated": false
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 300, "webAccessUrl": "https://x/run/300"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/test/Runs/300/results"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 100000, "testCase": {"id": "201"}, "testPoint": {"id": "7"}}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/test/Runs/300/results"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!([
            {"id": 100000, "outcome": "Passed", "state": "Completed"}
        ])))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"count": 1})))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/test/runs/300"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({"state": "Completed"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 300})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let run = client
        .create_test_run("org", "proj", 9, "My run", &[7])
        .await
        .unwrap();
    assert_eq!(run.run_id, 300);
    let results = client.get_run_results("org", "proj", 300).await.unwrap();
    assert_eq!(results[0].result_id, 100000);
    assert_eq!(results[0].test_case_id, Some(201));
    client
        .update_run_results(
            "org",
            "proj",
            300,
            &[OutcomeUpdate {
                id: 100000,
                outcome: "Passed".into(),
                comment: Some("ok".into()),
                duration_ms: Some(1200),
                bug_ids: None,
            }],
        )
        .await
        .unwrap();
    client.complete_test_run("org", "proj", 300).await.unwrap();
}

#[tokio::test]
async fn run_history_aggregates_newest_first_and_caps_at_five() {
    let server = MockServer::start().await;
    // Three runs, listed out of order - the sweep must sort newest first.
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/test/runs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 1, "completedDate": "2026-07-10T10:00:00Z"},
                {"id": 3, "completedDate": "2026-07-12T10:00:00Z"},
                {"id": 2, "completedDate": "2026-07-11T10:00:00Z"}
            ]
        })))
        .mount(&server)
        .await;
    // Run 3 (newest): case 201 failed + an unspecified result to skip.
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/test/Runs/3/results"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"outcome": "Failed", "completedDate": "2026-07-12T10:05:00Z", "testCase": {"id": "201"}},
                {"outcome": "Unspecified", "testCase": {"id": "201"}},
                {"outcome": "Passed", "completedDate": "2026-07-12T10:06:00Z", "testCase": {"id": "202"}}
            ]
        })))
        .mount(&server)
        .await;
    // Runs 2 and 1: five more outcomes for case 201 (total 6 -> capped at 5).
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/test/Runs/2/results"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"outcome": "Passed", "completedDate": "2026-07-11T10:01:00Z", "testCase": {"id": "201"}},
                {"outcome": "Passed", "completedDate": "2026-07-11T10:02:00Z", "testCase": {"id": "201"}},
                {"outcome": "Blocked", "completedDate": "2026-07-11T10:03:00Z", "testCase": {"id": "201"}}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/test/Runs/1/results"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"outcome": "Passed", "completedDate": "2026-07-10T10:01:00Z", "testCase": {"id": "201"}},
                {"outcome": "Failed", "completedDate": "2026-07-10T10:02:00Z", "testCase": {"id": "201"}}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let mut history = client.run_history("o", "p", 9).await.unwrap();
    history.sort_by_key(|h| h.test_case_id);

    assert_eq!(history.len(), 2);
    let c201 = &history[0];
    assert_eq!(c201.test_case_id, 201);
    // Newest run's outcome first, capped at 5 (6 valid outcomes existed).
    assert_eq!(c201.outcomes.len(), 5);
    assert_eq!(c201.outcomes[0].outcome, "Failed");
    assert_eq!(c201.outcomes[0].run_id, 3);
    assert_eq!(c201.outcomes[1].outcome, "Passed");
    assert_eq!(c201.outcomes[1].run_id, 2);
    let c202 = &history[1];
    assert_eq!(c202.outcomes.len(), 1);
    assert_eq!(c202.outcomes[0].outcome, "Passed");
}

#[tokio::test]
async fn result_report_info_parses_comment_and_bugs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/test/Runs/5/Results/50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comment": "It exploded",
            "associatedBugs": [{"id": "901"}, {"id": 902}]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let (comment, bugs) = client.get_result_report_info("o", "p", 5, 50).await.unwrap();
    assert_eq!(comment, "It exploded");
    assert_eq!(bugs, vec![901, 902]);
}
