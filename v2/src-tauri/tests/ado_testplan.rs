use v2_lib::ado::AdoClient;
use v2_lib::ado_testplan::{area_matches, default_plan_name, OutcomeUpdate};
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
            }],
        )
        .await
        .unwrap();
    client.complete_test_run("org", "proj", 300).await.unwrap();
}
