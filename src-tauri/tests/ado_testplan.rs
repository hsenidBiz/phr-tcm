use v2_lib::ado::{AdoClient, AdoError};
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
    assert!(!ensured.created_plan, "reusing a found suite must not claim plan creation");
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
    assert!(
        ensured.created_plan,
        "no plan existed - the caller must be able to tell the user one was created"
    );
    assert_eq!(ensured.plan_name, "Auth - Test Plan");
}

#[tokio::test]
async fn existing_plan_missing_suite_creates_suite_but_not_plan() {
    // An area-matched plan exists; only the PBI's requirement suite is
    // missing. created_plan must stay false - the user is only told about
    // PLAN creation, suite creation is routine.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 9, "name": "Area Plan", "areaPath": "Proj\\Auth", "rootSuite": {"id": 90}}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 90, "name": "root", "suiteType": "staticTestSuite"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 95})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let ensured = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "")
        .await
        .unwrap();
    assert_eq!(ensured.plan_id, 9);
    assert_eq!(ensured.suite_id, 95);
    assert!(!ensured.created_plan, "the plan already existed - only the suite was created");
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
                {"id": 9001, "outcome": "Failed", "completedDate": "2026-07-12T10:05:00Z", "testCase": {"id": "201"}},
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
    // The result row id rides along, so a history view can fetch a PRIOR
    // result's comment - the point itself only carries the latest.
    assert_eq!(c201.outcomes[0].result_id, 9001);
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

/// The run-results endpoint is the Test Management API: it pages with
/// $top/$skip and does NOT send the continuation-token header the testplan
/// helper reads. A single unpaged GET returned only the first page, so
/// every outcome beyond it looked like "no matching result" and was
/// dropped - the tester's results simply never existed.
#[tokio::test]
async fn run_results_are_read_across_every_page() {
    let server = wiremock::MockServer::start().await;
    let page = |from: i32, n: i32| {
        let value: Vec<serde_json::Value> = (0..n)
            .map(|i| {
                serde_json::json!({
                    "id": from + i,
                    "testCase": { "id": (from + i).to_string() },
                    "testPoint": { "id": (1000 + from + i).to_string() }
                })
            })
            .collect();
        serde_json::json!({ "value": value })
    };
    // Full page, then a short one - which is how the loop knows to stop.
    wiremock::Mock::given(wiremock::matchers::query_param("$skip", "0"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(page(1, 200)))
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::query_param("$skip", "200"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(page(201, 30)))
        .mount(&server)
        .await;

    let client = v2_lib::ado::AdoClient::with_base_url("t".into(), server.uri());
    let results = client.get_run_results("o", "p", 7).await.unwrap();
    assert_eq!(results.len(), 230, "the second page was dropped");
    // A point from the second page must be findable, which is the whole point.
    assert!(results.iter().any(|r| r.point_id == Some(1230)));
}

#[tokio::test]
async fn reset_points_patches_reset_to_active() {
    // The deselect path: resetToActive is ADO's own "reset test" - assert
    // the exact verb, path and body so this can never drift into something
    // destructive or silently wrong.
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/test/Plans/5/Suites/9/points/101,102"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "resetToActive": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    client
        .reset_points_to_active("org", "proj", 5, 9, &[101, 102])
        .await
        .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn reset_points_with_no_ids_makes_no_request() {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    client
        .reset_points_to_active("org", "proj", 5, 9, &[])
        .await
        .unwrap();
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// Audit finding R-5: ADO's continuation token is opaque and was
/// concatenated into the query raw, so a token containing `&`, `+` or `%`
/// would corrupt paging - and the loops had no page cap or cycle guard, so
/// a server that repeated a token would spin forever rather than error.
#[tokio::test]
async fn continuation_tokens_are_encoded_and_a_repeat_stops_the_loop() {
    let server = MockServer::start().await;
    // Page 1 hands back a hostile token; every later page repeats it, which
    // without a cycle guard is an infinite loop.
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/testplan/plans"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ms-continuationtoken", "a&b+c%d")
                .set_body_json(serde_json::json!({
                    "value": [{ "id": 1, "name": "Plan", "areaPath": "A", "rootSuite": { "id": 9 } }]
                })),
        )
        .mount(&server)
        .await;

    let client = AdoClient::with_base_url("t".into(), server.uri());
    // Terminates at all - the whole point of the cycle guard.
    let plans = client.get_test_plans("o", "p").await.unwrap();
    assert!(!plans.is_empty());

    let asked = server.received_requests().await.unwrap();
    assert!(
        asked.len() <= 3,
        "a repeated token must stop the loop almost immediately, not spin: {} requests",
        asked.len()
    );
    // The second request carries the token ENCODED, not raw.
    let second = asked[1].url.as_str();
    assert!(
        second.contains("continuationToken=a%26b%2Bc%25d"),
        "the token must be percent-encoded: {second}"
    );
    // Decoded back to the original by the query parser - proof the encoding
    // is correct rather than merely different.
    let decoded = asked[1]
        .url
        .query_pairs()
        .find(|(k, _)| k == "continuationToken")
        .unwrap()
        .1
        .to_string();
    assert_eq!(decoded, "a&b+c%d");
}

/// Two plans cover the same area. The old rule took the first one listed
/// - the oldest - which in the field was a plan the user could not create
/// suites in. The newest plan whose iteration is the PBI's wins now.
#[tokio::test]
async fn ensure_picks_the_newest_iteration_matching_plan_among_equal_areas() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 100, "name": "Old plan", "areaPath": "Proj\\Auth", "iteration": "Proj\\2024",
             "state": "Inactive", "rootSuite": {"id": 1000}},
            {"id": 200, "name": "New plan", "areaPath": "Proj\\Auth", "iteration": "Proj\\2026\\S3",
             "state": "Active", "rootSuite": {"id": 2000}}
        ]})))
        .mount(&server)
        .await;
    for id in [100, 200] {
        Mock::given(method("GET"))
            .and(path(format!("/org/proj/_apis/testplan/Plans/{id}/suites")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
            .mount(&server)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/200/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 2001})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/100/suites"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let ensured = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "Proj\\2026\\S3")
        .await
        .unwrap();
    assert_eq!(ensured.plan_id, 200, "the newest, iteration-matching, active plan");
    assert_eq!(ensured.suite_id, 2001);
    assert!(!ensured.created_plan);
}

/// A 403 on one plan says nothing about the next: plans have owners. The
/// suite lands in the next candidate instead of nowhere.
#[tokio::test]
async fn ensure_falls_back_to_the_next_plan_when_the_first_forbids_suites() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 100, "name": "Old plan", "areaPath": "Proj\\Auth", "iteration": "Proj\\2024",
             "state": "Active", "rootSuite": {"id": 1000}},
            {"id": 200, "name": "New plan", "areaPath": "Proj\\Auth", "iteration": "Proj\\2026\\S3",
             "state": "Active", "rootSuite": {"id": 2000}}
        ]})))
        .mount(&server)
        .await;
    for id in [100, 200] {
        Mock::given(method("GET"))
            .and(path(format!("/org/proj/_apis/testplan/Plans/{id}/suites")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
            .mount(&server)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/200/suites"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/100/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 1001})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let ensured = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "Proj\\2026\\S3")
        .await
        .unwrap();
    assert_eq!(ensured.plan_id, 100, "the plan that allowed it");
    assert_eq!(ensured.suite_id, 1001);
}

/// When every candidate forbids it, the error names the plans - whose
/// door to knock on - instead of a bare "no permission".
#[tokio::test]
async fn ensure_names_the_plans_when_every_candidate_forbids_suites() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 100, "name": "Old plan", "areaPath": "Proj\\Auth", "iteration": "",
             "state": "Active", "rootSuite": {"id": 1000}},
            {"id": 200, "name": "New plan", "areaPath": "Proj\\Auth", "iteration": "",
             "state": "Active", "rootSuite": {"id": 2000}}
        ]})))
        .mount(&server)
        .await;
    for id in [100, 200] {
        Mock::given(method("GET"))
            .and(path(format!("/org/proj/_apis/testplan/Plans/{id}/suites")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/org/proj/_apis/testplan/Plans/{id}/suites")))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
    }

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let err = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "")
        .await
        .unwrap_err();
    match err {
        AdoError::Http { status, body } => {
            assert_eq!(status, 403);
            assert!(body.contains("New plan"), "{body}");
            assert!(body.contains("Old plan"), "{body}");
            assert!(body.contains("permission"), "{body}");
            assert!(body.contains("#42"), "{body}");
        }
        other => panic!("expected a named 403, got {other:?}"),
    }
}
