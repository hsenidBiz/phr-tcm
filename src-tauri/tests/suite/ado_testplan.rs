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
use wiremock::matchers::{method, path, path_regex, query_param};
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

/// ADO has two spellings of "no verdict" on a point: "unspecified" for one
/// that never ran, and "None" for one reset to Active (or whose result was
/// never marked). Both must read as never-run: a "None" that leaked through
/// showed as an outcome, and the Never run filter hid those cases.
#[tokio::test]
async fn points_without_a_verdict_read_as_never_run() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/9/Suites/91/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 1, "testCaseReference": {"id": 301, "name": "a"}, "results": {"outcome": "unspecified"}},
                {"id": 2, "testCaseReference": {"id": 302, "name": "b"}, "results": {"outcome": "None", "lastTestRunId": 4, "lastResultId": 40}},
                {"id": 3, "testCaseReference": {"id": 303, "name": "c"}, "results": {"outcome": "none"}},
                {"id": 4, "testCaseReference": {"id": 304, "name": "d"}, "results": {"outcome": "passed"}}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let pts = client.get_test_points("org", "proj", 9, 91, &[]).await.unwrap();
    let outcomes: Vec<&str> = pts.iter().map(|p| p.last_outcome.as_str()).collect();
    assert_eq!(outcomes, vec!["", "", "", "passed"]);
    // A reset keeps the run/result reference - only the verdict is gone.
    assert_eq!(pts[1].last_run_id, Some(4));
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
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    // Three runs, listed out of order - the sweep must sort newest first.
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/runs")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 1, "completedDate": "2026-07-10T10:00:00Z"},
                {"id": 3, "completedDate": "2026-07-12T10:00:00Z"},
                {"id": 2, "completedDate": "2026-07-11T10:00:00Z"}
            ]
        })))
        .mount(&server)
        .await;
    // Run 3 (newest): case 201 failed, plus an "Unspecified" and a "None"
    // result (no verdict yet) that must both be skipped.
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/Runs/3/results")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 9001, "outcome": "Failed", "completedDate": "2026-07-12T10:05:00Z", "testCase": {"id": "201"}},
                {"outcome": "Unspecified", "testCase": {"id": "201"}},
                {"outcome": "Passed", "completedDate": "2026-07-12T10:06:00Z", "testCase": {"id": "202"}},
                {"outcome": "None", "testCase": {"id": "202"}}
            ]
        })))
        .mount(&server)
        .await;
    // Runs 2 and 1: five more outcomes for case 201 (total 6 -> capped at 5).
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/Runs/2/results")))
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
        .and(path(format!("/{o}/{p}/_apis/test/Runs/1/results")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"outcome": "Passed", "completedDate": "2026-07-10T10:01:00Z", "testCase": {"id": "201"}},
                {"outcome": "Failed", "completedDate": "2026-07-10T10:02:00Z", "testCase": {"id": "201"}}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let mut history = client.run_history(&o, &p, 9, &[]).await.unwrap();
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

// ---- run history depth ----

/// An org/project pair no other test in this process uses. Completed runs are
/// kept in the process-wide session cache keyed by base URL + org + project +
/// run id; the OS reuses mock-server ports, so a shared name could be served
/// another test's cached runs.
fn unique_scope() -> (String, String) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (format!("org-{n}"), format!("proj-{n}"))
}


/// ISO date for run `id`, increasing with the id (sorts lexicographically).
fn run_date(id: i32) -> String {
    format!("2026-07-01T{:02}:{:02}:00Z", id / 60, id % 60)
}

fn run_row(id: i32, state: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "state": state, "completedDate": run_date(id)})
}

fn passed_row(case: &str) -> serde_json::Value {
    serde_json::json!({"id": 1, "outcome": "Passed", "completedDate": "2026-07-01T00:00:00Z", "testCase": {"id": case}})
}

async fn mount_runs(server: &MockServer, o: &str, p: &str, runs: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/runs")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": runs })))
        .mount(server)
        .await;
}

async fn mount_catch_all(server: &MockServer, o: &str, p: &str, rows: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path_regex(&format!(r"^/{o}/{p}/_apis/test/Runs/\d+/results$")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": rows })))
        .mount(server)
        .await;
}

async fn mount_run_results(server: &MockServer, o: &str, p: &str, run: i32, rows: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/Runs/{run}/results")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": rows })))
        .with_priority(1)
        .mount(server)
        .await;
}

async fn count_requests(server: &MockServer, path_: &str) -> usize {
    server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == path_).count()
}

async fn count_results_requests(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().contains("/test/Runs/") && r.url.path().ends_with("/results"))
        .count()
}

#[tokio::test]
async fn run_history_reaches_a_case_whose_only_results_are_in_the_twentieth_newest_run() {
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(
        &server,
        &o,
        &p,
        (1..=25)
            .map(|i| serde_json::json!({"id": i, "state": "Completed", "completedDate": format!("2026-07-{i:02}T10:00:00Z")}))
            .collect(),
    )
    .await;
    mount_catch_all(&server, &o, &p, vec![passed_row("900")]).await;
    mount_run_results(
        &server, &o, &p,
        6,
        vec![serde_json::json!({
            "id": 60, "outcome": "Passed", "completedDate": "2026-07-06T10:05:00Z", "testCase": {"id": "501"}
        })],
    )
    .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let history = client.run_history(&o, &p, 9, &[501]).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].test_case_id, 501);
    assert_eq!(history[0].outcomes.len(), 1);
    assert_eq!(history[0].outcomes[0].run_id, 6);
    assert_eq!(history[0].outcomes[0].result_id, 60);
}

#[tokio::test]
async fn run_history_reads_every_page_of_a_large_run() {
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(&server, &o, &p, vec![run_row(1, "Completed")]).await;
    let full: Vec<serde_json::Value> = (0..200).map(|_| passed_row("601")).collect();
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/Runs/1/results")))
        .and(query_param("$skip", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": full })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/Runs/1/results")))
        .and(query_param("$skip", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"outcome": "Failed", "testCase": {"id": "602"}}]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let mut history = client.run_history(&o, &p, 9, &[601, 602]).await.unwrap();
    history.sort_by_key(|h| h.test_case_id);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].outcomes.len(), 5);
    assert_eq!(history[1].outcomes.len(), 1);
    assert_eq!(history[1].outcomes[0].outcome, "Failed");
}

#[tokio::test]
async fn run_history_reads_every_page_of_the_run_list() {
    let _pacer = crate::serial::pacer();
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    let first: Vec<serde_json::Value> = (1..=200)
        .map(|i| serde_json::json!({"id": i, "state": "Completed", "completedDate": "2026-01-01T10:00:00Z"}))
        .collect();
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/runs")))
        .and(query_param("$skip", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": first })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/runs")))
        .and(query_param("$skip", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": 201, "state": "Completed", "completedDate": "2026-09-01T10:00:00Z"}]
        })))
        .mount(&server)
        .await;
    mount_catch_all(&server, &o, &p, vec![]).await;
    mount_run_results(&server, &o, &p, 201, vec![passed_row("801")]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let history = client.run_history(&o, &p, 9, &[801]).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].outcomes[0].run_id, 201);
}

#[tokio::test]
async fn run_history_stops_once_every_asked_case_has_five() {
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(&server, &o, &p, (1..=40).map(|i| run_row(i, "Completed")).collect()).await;
    mount_catch_all(&server, &o, &p, vec![passed_row("301")]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let history = client.run_history(&o, &p, 9, &[301]).await.unwrap();
    assert_eq!(count_results_requests(&server).await, 8);
    let ids: Vec<i32> = history[0].outcomes.iter().map(|o| o.run_id).collect();
    assert_eq!(ids, vec![40, 39, 38, 37, 36]);
}

#[tokio::test]
async fn run_history_stops_at_the_run_cap() {
    let _pacer = crate::serial::pacer();
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(&server, &o, &p, (1..=160).map(|i| run_row(i, "Completed")).collect()).await;
    mount_catch_all(&server, &o, &p, vec![]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let history = client.run_history(&o, &p, 9, &[302]).await.unwrap();
    assert_eq!(count_results_requests(&server).await, 150);
    assert!(history.is_empty());
}

#[tokio::test]
async fn run_history_skips_runs_it_may_not_read() {
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(&server, &o, &p, (1..=3).map(|i| run_row(i, "Completed")).collect()).await;
    mount_run_results(&server, &o, &p, 1, vec![passed_row("303")]).await;
    for (run, status) in [(2, 403), (3, 404)] {
        Mock::given(method("GET"))
            .and(path(format!("/{o}/{p}/_apis/test/Runs/{run}/results")))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
    }
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let history = client.run_history(&o, &p, 9, &[303]).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].outcomes.len(), 1);
    assert_eq!(history[0].outcomes[0].run_id, 1);
    client.run_history(&o, &p, 9, &[303]).await.unwrap();
    assert_eq!(count_requests(&server, &format!("/{o}/{p}/_apis/test/Runs/2/results")).await, 2);
    assert_eq!(count_requests(&server, &format!("/{o}/{p}/_apis/test/Runs/3/results")).await, 2);
}

#[tokio::test]
async fn run_history_does_not_keep_an_in_progress_run() {
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(
        &server,
        &o,
        &p,
        vec![serde_json::json!({"id": 1, "state": "InProgress", "startedDate": "2026-07-01T10:00:00Z"})],
    )
    .await;
    mount_catch_all(&server, &o, &p, vec![passed_row("304")]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let a = client.run_history(&o, &p, 9, &[304]).await.unwrap();
    let b = client.run_history(&o, &p, 9, &[304]).await.unwrap();
    assert_eq!(count_requests(&server, &format!("/{o}/{p}/_apis/test/Runs/1/results")).await, 2);
    assert_eq!(a[0].outcomes.len(), 1);
    assert_eq!(b[0].outcomes.len(), 1);
}

#[tokio::test]
async fn run_history_reads_a_completed_run_once() {
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(&server, &o, &p, (1..=2).map(|i| run_row(i, "Completed")).collect()).await;
    mount_catch_all(&server, &o, &p, vec![passed_row("305")]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let a = client.run_history(&o, &p, 9, &[305]).await.unwrap();
    let b = client.run_history(&o, &p, 9, &[305]).await.unwrap();
    assert_eq!(count_requests(&server, &format!("/{o}/{p}/_apis/test/Runs/1/results")).await, 1);
    assert_eq!(count_requests(&server, &format!("/{o}/{p}/_apis/test/Runs/2/results")).await, 1);
    assert_eq!(count_requests(&server, &format!("/{o}/{p}/_apis/test/runs")).await, 2);
    let ids = |h: &Vec<v2_lib::ado_testplan::CaseHistory>| -> Vec<i32> {
        h[0].outcomes.iter().map(|o| o.run_id).collect()
    };
    assert_eq!(ids(&a), vec![2, 1]);
    assert_eq!(ids(&b), ids(&a));
}

#[tokio::test]
async fn run_history_logs_one_summary_line() {
    let _log = crate::serial::log_tail();
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(&server, &o, &p, (1..=2).map(|i| run_row(i, "Completed")).collect()).await;
    mount_catch_all(&server, &o, &p, vec![passed_row("306")]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    client.run_history(&o, &p, 9306, &[306]).await.unwrap();
    let lines: Vec<String> = v2_lib::applog::recent(500)
        .into_iter()
        .map(|l| l.message)
        .filter(|m| m.starts_with("run history: plan 9306:"))
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("runs listed"), "{}", lines[0]);
    assert!(lines[0].contains("1/1 asked cases with history"), "{}", lines[0]);
    assert!(lines[0].contains("stopped: runs exhausted"), "{}", lines[0]);
    assert!(!lines[0].contains("http"), "{}", lines[0]);
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

/// A 403 on a plan says nothing about a plan for ANOTHER area: "Manage
/// test suites" is granted per area path. The suite lands in the next
/// area's plan instead of nowhere.
#[tokio::test]
async fn ensure_falls_back_to_the_next_plan_when_the_first_forbids_suites() {
    let server = MockServer::builder().start().await; // unshared: a 403 is remembered per server URL
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 100, "name": "Old plan", "areaPath": "Proj\\Auth", "iteration": "Proj\\2024",
             "state": "Active", "rootSuite": {"id": 1000}},
            {"id": 200, "name": "New plan", "areaPath": "Proj\\Auth\\Web", "iteration": "Proj\\2026\\S3",
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
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth\\Web", "Proj\\2026\\S3")
        .await
        .unwrap();
    assert_eq!(ensured.plan_id, 100, "the plan that allowed it");
    assert_eq!(ensured.suite_id, 1001);
}

/// When the candidate forbids it, the error names the plan and its area -
/// what to ask for - instead of a bare "no permission". The second plan
/// shares the area, so it is reported as skipped rather than asked.
#[tokio::test]
async fn ensure_names_the_plans_when_every_candidate_forbids_suites() {
    let server = MockServer::builder().start().await; // unshared: a 403 is remembered per server URL
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
            assert!(body.contains("New plan"), "the newest plan is the one asked: {body}");
            assert!(!body.contains("Old plan"), "same area - skipped, not listed: {body}");
            assert!(body.contains("1 other plan"), "{body}");
            assert!(body.contains("permission"), "{body}");
            assert!(body.contains("#42"), "{body}");
        }
        other => panic!("expected a named 403, got {other:?}"),
    }
}

/// The resolved-suite cache is one map shared by the upload, Run Tests and
/// the AI bridge: whoever resolves a PBI's suite first spares the others
/// the plan scan. Keyed by base_url too, so mock servers never collide.
#[test]
fn the_suite_cache_is_shared_keyed_and_forgettable() {
    use v2_lib::ado_testplan::{cached_suite, forget_suite, remember_suite, EnsuredSuite};
    let base = format!("http://cache-test-{}", std::process::id());
    assert!(cached_suite(&base, "acme", "Web", 4242).is_none());
    let s = EnsuredSuite { plan_id: 9, plan_name: "Web - Auth Plan".into(), suite_id: 91, created_plan: false };
    remember_suite(&base, "acme", "Web", 4242, &s);
    assert_eq!(cached_suite(&base, "acme", "Web", 4242), Some(s.clone()));
    assert!(cached_suite(&base, "acme", "Mobile", 4242).is_none(), "another project is another key");
    assert!(cached_suite("http://elsewhere", "acme", "Web", 4242).is_none(), "another base_url is another key");
    forget_suite(&base, "acme", "Web", 4242);
    assert!(cached_suite(&base, "acme", "Web", 4242).is_none());
}

/// Creating a suite only ever happens under an area-matched plan, so the
/// ensure path stops scanning once those are checked - a suite parked
/// under some other area's plan is not one it would use. The read-only
/// lookup still walks every plan, because Run Tests must find the suite
/// wherever someone put it.
#[tokio::test]
async fn ensure_scans_only_area_matched_plans_but_find_scans_all() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 9, "name": "Auth plan", "areaPath": "Proj\\Auth", "rootSuite": {"id": 90}},
            {"id": 8, "name": "Other plan", "areaPath": "Proj\\Billing", "rootSuite": {"id": 80}}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 90, "name": "root", "suiteType": "staticTestSuite"}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/8/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 80, "name": "root", "suiteType": "staticTestSuite"},
            {"id": 81, "name": "42 : elsewhere", "suiteType": "requirementTestSuite", "requirementId": 42}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 91})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let ensured = client.ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "").await.unwrap();
    assert_eq!(ensured.plan_id, 9, "created under the area-matched plan");
    assert_eq!(ensured.suite_id, 91);
    let other_plan_reads = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/Plans/8/suites"))
        .count();
    assert_eq!(other_plan_reads, 0, "the other area's plan was never scanned");

    let found = client.find_pbi_requirement_suite("org", "proj", 42, "Proj\\Auth").await.unwrap();
    assert_eq!(found.map(|s| s.suite_id), Some(81), "the read-only lookup still finds it anywhere");
}

/// Plans that share an area path share the answer, so a 403 in one is a
/// 403 in all of them: the rest are skipped, not tried. On 2026-09-22 the
/// walk tried 48 plans under the project root one after another - every
/// sprint plan back to 2025, Marketplace, payroll plans - for 70 s of
/// 403s that all meant the same thing. One attempt per area, best plan
/// first, and the error says which AREA needs the permission.
#[tokio::test]
async fn ensure_tries_one_plan_per_area_and_names_the_area() {
    let server = MockServer::builder().start().await; // unshared: a 403 is remembered per server URL
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 100, "name": "Marketplace", "areaPath": "Proj", "iteration": "",
             "state": "Active", "rootSuite": {"id": 1000}},
            {"id": 200, "name": "Sprint 1", "areaPath": "Proj", "iteration": "Proj\\2025\\S1",
             "state": "Inactive", "rootSuite": {"id": 2000}},
            {"id": 300, "name": "Sprint 9", "areaPath": "Proj", "iteration": "Proj\\2026\\S9",
             "state": "Active", "rootSuite": {"id": 3000}}
        ]})))
        .mount(&server)
        .await;
    for id in [100, 200, 300] {
        Mock::given(method("GET"))
            .and(path(format!("/org/proj/_apis/testplan/Plans/{id}/suites")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
            .mount(&server)
            .await;
    }
    // The best-ranked plan (iteration match, active) is the ONE asked.
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/300/suites"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    for id in [100, 200] {
        Mock::given(method("POST"))
            .and(path(format!("/org/proj/_apis/testplan/Plans/{id}/suites")))
            .respond_with(ResponseTemplate::new(403))
            .expect(0)
            .mount(&server)
            .await;
    }

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let err = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\PMS", "Proj\\2026\\S9")
        .await
        .unwrap_err();
    match err {
        AdoError::Http { status, body } => {
            assert_eq!(status, 403);
            assert!(body.contains("Sprint 9"), "{body}");
            assert!(!body.contains("Marketplace"), "skipped plans are not listed: {body}");
            assert!(body.contains("area 'Proj'"), "the area the permission lives on: {body}");
            assert!(body.contains("2 other plan"), "says what was skipped and why: {body}");
        }
        other => panic!("expected a named 403, got {other:?}"),
    }
    server.verify().await;
}

/// A shared-step row has no step id of its own ("" from parse_step_ids).
/// A mark on it must be dropped, never sent under a made-up action path.
#[test]
fn iteration_details_skip_a_step_without_an_id() {
    let ids = vec!["2".to_string(), String::new(), "4".to_string()];
    let outcomes = vec![Some("Passed".to_string()), Some("Failed".to_string()), Some("Passed".to_string())];
    let details = build_iteration_details(&ids, &outcomes, "Failed").unwrap();
    let actions = details[0]["actionResults"].as_array().unwrap();
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0]["stepIdentifier"], "2");
    assert_eq!(actions[1]["stepIdentifier"], "4");
}

// ------------------------------------------- a refused suite route, remembered

/// One plan for `Proj\Auth` with no suite for the PBI yet; the suite
/// create answers `create`. A server of its own: the refusal is remembered
/// per server URL, and a pooled server would carry it into another test.
async fn one_plan_server(create: ResponseTemplate) -> MockServer {
    let server = MockServer::builder().start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 100, "name": "Auth plan", "areaPath": "Proj\\Auth", "iteration": "",
             "state": "Active", "rootSuite": {"id": 1000}}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/100/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/100/suites"))
        .respond_with(create)
        .mount(&server)
        .await;
    server
}

fn suite_creates(asked: &[wiremock::Request]) -> usize {
    asked
        .iter()
        .filter(|r| r.method.as_str() == "POST" && r.url.path() == "/org/proj/_apis/testplan/Plans/100/suites")
        .count()
}

/// The first upload is refused; the second does not ask again - it comes
/// back refused at once (the same sentence, so the caller takes the Boards
/// route) without a request to the documented create.
#[tokio::test]
async fn a_refused_area_skips_the_regular_route_next_time() {
    use v2_lib::ado_testplan::area_refused;
    let server = one_plan_server(ResponseTemplate::new(403)).await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert!(!area_refused(&server.uri(), "org", "proj", "Proj\\Auth"));

    for _ in 0..2 {
        match client.ensure_requirement_suite_for_upload("org", "proj", 42, "Proj\\Auth", "").await {
            Err(AdoError::Http { status: 403, body }) => {
                assert!(body.contains("'Auth plan' (id 100)"), "{body}");
            }
            other => panic!("expected the named 403, got {other:?}"),
        }
    }
    let asked = server.received_requests().await.unwrap();
    assert_eq!(suite_creates(&asked), 1, "the second upload must not ask again");
    // Either spelling of the area is the same area.
    assert!(area_refused(&server.uri(), "org", "proj", "proj/auth"));
    assert!(!area_refused(&server.uri(), "org", "other", "Proj\\Auth"), "another project is another key");
}

/// A week later the documented route is asked again - and a create that
/// works there clears the memory.
#[tokio::test]
async fn the_refusal_expires_after_seven_days() {
    use v2_lib::ado_testplan::{area_key, area_refused};
    use v2_lib::cache::{keys::suite_refused, keys::SUITE_REFUSED_TTL_MS, now_ms, put_at};
    let server = one_plan_server(
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 1001})),
    )
    .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    // The cache's own stamp is what ages; `put_at` back-dates it.
    let key = suite_refused(&server.uri(), "org", "proj", &area_key("Proj\\Auth"));

    // Refused six days ago: still remembered, nothing asked.
    let day = 24 * 60 * 60 * 1000;
    put_at(&key, &true, now_ms() - 6 * day);
    assert!(area_refused(&server.uri(), "org", "proj", "Proj\\Auth"));
    assert!(client.ensure_requirement_suite_for_upload("org", "proj", 42, "Proj\\Auth", "").await.is_err());
    assert_eq!(suite_creates(&server.received_requests().await.unwrap()), 0);

    // Refused just over seven days ago: expired, so the route is asked.
    put_at(&key, &true, now_ms() - SUITE_REFUSED_TTL_MS - 1);
    assert!(!area_refused(&server.uri(), "org", "proj", "Proj\\Auth"));
    let ensured = client
        .ensure_requirement_suite_for_upload("org", "proj", 42, "Proj\\Auth", "")
        .await
        .unwrap();
    assert_eq!(ensured.suite_id, 1001);
    assert_eq!(suite_creates(&server.received_requests().await.unwrap()), 1);
    // The create worked, so the old refusal is gone, not merely stale.
    assert!(v2_lib::cache::get::<bool>(&key).is_none());
}

/// Run Tests has no Boards route, so it never takes the remembered
/// refusal as its answer: someone granted the permission since gets their
/// suite at once, not "no permission" for the rest of the week - and the
/// create that works clears the memory for the upload too.
#[tokio::test]
async fn run_tests_ensure_asks_again_even_when_the_area_is_remembered_as_refused() {
    use v2_lib::ado_testplan::{area_refused, remember_area_refused};
    let server = one_plan_server(
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 1001})),
    )
    .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    remember_area_refused(&server.uri(), "org", "proj", "Proj\\Auth");

    // The call Run Tests makes (`ensure_pbi_suite`).
    let ensured = client
        .ensure_requirement_suite_cb("org", "proj", 42, "Proj\\Auth", "", |_| {})
        .await
        .unwrap();
    assert_eq!(ensured.suite_id, 1001);
    assert_eq!(suite_creates(&server.received_requests().await.unwrap()), 1, "asked despite the memory");
    assert!(!area_refused(&server.uri(), "org", "proj", "Proj\\Auth"), "a create that works clears it");
}

/// Reading suites is not creating one: a suite the PBI already has is
/// found and used whatever the area's refusal says.
#[tokio::test]
async fn an_existing_suite_is_still_used_when_the_area_is_remembered_as_refused() {
    use v2_lib::ado_testplan::remember_area_refused;
    let server = MockServer::builder().start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 100, "name": "Auth plan", "areaPath": "Proj\\Auth", "iteration": "",
             "state": "Active", "rootSuite": {"id": 1000}}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/Plans/100/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"id": 1000, "name": "root", "suiteType": "staticTestSuite"},
            {"id": 1005, "name": "42 : Login", "suiteType": "requirementTestSuite",
             "requirementId": 42, "parentSuite": {"id": 1000}}
        ]})))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    remember_area_refused(&server.uri(), "org", "proj", "Proj\\Auth");

    let ensured = client
        .ensure_requirement_suite_for_upload("org", "proj", 42, "Proj\\Auth", "")
        .await
        .unwrap();
    assert_eq!((ensured.plan_id, ensured.suite_id), (100, 1005));
    for req in server.received_requests().await.unwrap() {
        assert_eq!(req.method.as_str(), "GET", "nothing is created: {}", req.url);
    }
}

#[tokio::test]
async fn run_history_keeps_what_it_gathered_when_a_later_run_fails() {
    let _log = crate::serial::log_tail();
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(&server, &o, &p, (1..=3).map(|i| run_row(i, "Completed")).collect()).await;
    mount_run_results(&server, &o, &p, 3, vec![passed_row("307")]).await;
    mount_run_results(&server, &o, &p, 2, vec![passed_row("307")]).await;
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/Runs/1/results")))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let history = client.run_history(&o, &p, 9307, &[307]).await.unwrap();
    assert_eq!(history.len(), 1);
    let ids: Vec<i32> = history[0].outcomes.iter().map(|o| o.run_id).collect();
    assert_eq!(ids, vec![3, 2]);
    let lines: Vec<String> = v2_lib::applog::recent(500)
        .into_iter()
        .map(|l| l.message)
        .filter(|m| m.contains("run history") && m.contains("9307"))
        .collect();
    assert!(lines.iter().any(|l| l.contains("partial")), "{lines:?}");
    assert!(lines.iter().all(|l| !l.contains("http")), "{lines:?}");
    // The warning is one tidy sentence, not one padded with source indentation.
    let warning = lines.iter().find(|l| l.contains("could not be read")).expect("a warning");
    assert!(!warning.contains("   "), "{warning}");
}

#[tokio::test]
async fn run_history_still_fails_when_the_first_run_fails() {
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(&server, &o, &p, (1..=3).map(|i| run_row(i, "Completed")).collect()).await;
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/Runs/3/results")))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&server)
        .await;
    mount_catch_all(&server, &o, &p, vec![passed_row("308")]).await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    assert!(client.run_history(&o, &p, 9, &[308]).await.is_err());
}

/// Two good runs that hold none of the asked cases, then a failure: returning
/// an empty history would overwrite a good saved one, so it is an error.
#[tokio::test]
async fn run_history_fails_when_the_runs_read_held_none_of_the_asked_cases() {
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(&server, &o, &p, (1..=3).map(|i| run_row(i, "Completed")).collect()).await;
    mount_run_results(&server, &o, &p, 3, vec![passed_row("999")]).await;
    mount_run_results(&server, &o, &p, 2, vec![passed_row("999")]).await;
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/Runs/1/results")))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    assert!(client.run_history(&o, &p, 9, &[309]).await.is_err());
}

#[tokio::test]
async fn run_history_reports_unauthorized_even_after_some_runs_were_read() {
    let (o, p) = unique_scope();
    let server = MockServer::start().await;
    mount_runs(&server, &o, &p, (1..=3).map(|i| run_row(i, "Completed")).collect()).await;
    mount_run_results(&server, &o, &p, 3, vec![passed_row("310")]).await;
    mount_run_results(&server, &o, &p, 2, vec![passed_row("310")]).await;
    Mock::given(method("GET"))
        .and(path(format!("/{o}/{p}/_apis/test/Runs/1/results")))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(1)
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    let err = client.run_history(&o, &p, 9, &[310]).await.unwrap_err();
    assert!(matches!(err, AdoError::Unauthorized), "{err:?}");
}
