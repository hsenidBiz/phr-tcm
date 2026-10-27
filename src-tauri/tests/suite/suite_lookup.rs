//! A PBI's requirement suite is asked of its own test cases before any plan
//! is walked.
//!
//! The response shape these mocks pin is Get Suites By Test Case Id:
//! `GET {org}/_apis/testplan/suites?testCaseId=N`, answering
//! `{"count": n, "value": [TestSuite]}` where each TestSuite carries `id`,
//! `name`, `suiteType`, `requirementId` (requirement suites), `plan`
//! (`{id, name}`) and `project` (`{id, name}`).

use v2_lib::ado::AdoClient;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn pbi_with_cases(ids: &[i32]) -> serde_json::Value {
    serde_json::json!({
        "id": 42,
        "relations": ids.iter().map(|i| serde_json::json!({
            "rel": "Microsoft.VSTS.Common.TestedBy-Forward",
            "url": format!("https://dev.azure.com/org/_apis/wit/workItems/{i}"),
        })).collect::<Vec<_>>(),
    })
}

async fn mount_pbi(server: &MockServer, ids: &[i32]) {
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pbi_with_cases(ids)))
        .mount(server)
        .await;
}

async fn mount_case_suites(server: &MockServer, case: i32, suites: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/org/_apis/testplan/suites"))
        .and(query_param("testCaseId", case.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": suites.as_array().map(|a| a.len()).unwrap_or(0),
            "value": suites,
        })))
        .mount(server)
        .await;
}

fn requirement_suite(id: i32, pbi: i32, plan: i32, project: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "name": format!("Suite {id}"), "suiteType": "requirementTestSuite",
        "requirementId": pbi,
        "plan": {"id": plan, "name": format!("Plan {plan}")},
        "project": {"id": "p-guid", "name": project},
    })
}

async fn plan_list_calls(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/org/proj/_apis/testplan/plans")
        .count()
}

#[tokio::test]
async fn hit_on_the_first_case_walks_no_plan() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100, 101]).await;
    mount_case_suites(
        &server,
        100,
        serde_json::json!([
            {"id": 5, "name": "Static", "suiteType": "staticTestSuite",
             "plan": {"id": 9, "name": "Plan 9"}, "project": {"name": "proj"}},
            requirement_suite(91, 42, 9, "proj"),
        ]),
    )
    .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42).await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (9, 91));
    assert_eq!(found.plan_name, "Plan 9");
    assert!(!found.created_plan);

    let ensured = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "")
        .await
        .unwrap();
    assert_eq!((ensured.plan_id, ensured.suite_id), (9, 91));
    let finder = client.find_pbi_requirement_suite("org", "proj", 42, "").await.unwrap().unwrap();
    assert_eq!(finder.suite_id, 91);
    assert_eq!(plan_list_calls(&server).await, 0, "no plan list may be requested");
}

#[tokio::test]
async fn other_pbis_and_other_projects_do_not_count() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100, 101]).await;
    // Case 100: a static suite, another PBI's requirement suite, and this
    // PBI's requirement suite in another project.
    mount_case_suites(
        &server,
        100,
        serde_json::json!([
            {"id": 5, "name": "Static", "suiteType": "staticTestSuite",
             "plan": {"id": 9, "name": "Plan 9"}, "project": {"name": "proj"}},
            requirement_suite(60, 77, 9, "proj"),
            requirement_suite(61, 42, 8, "other"),
        ]),
    )
    .await;
    mount_case_suites(&server, 101, serde_json::json!([requirement_suite(92, 42, 10, "PROJ")])).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42).await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (10, 92), "the second case holds the right one");
}

#[tokio::test]
async fn a_suite_that_names_no_project_is_confirmed_by_its_plan() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100]).await;
    mount_case_suites(
        &server,
        100,
        serde_json::json!([{
            "id": 91, "name": "S", "suiteType": "requirementTestSuite", "requirementId": 42,
            "plan": {"id": 9, "name": "Plan 9"}
        }]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/plans/9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 9, "name": "Confirmed Plan", "areaPath": "proj"
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42).await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (9, 91));
    assert_eq!(found.plan_name, "Confirmed Plan");
}

#[tokio::test]
async fn no_tested_by_links_falls_back_to_the_walk() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[]).await;
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
            "value": [{"id": 91, "name": "S", "suiteType": "requirementTestSuite", "requirementId": 42}]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert!(client.suite_from_cases("org", "proj", 42).await.unwrap().is_none());
    let ensured = client
        .ensure_requirement_suite("org", "proj", 42, "Proj\\Auth", "")
        .await
        .unwrap();
    assert_eq!((ensured.plan_id, ensured.suite_id), (9, 91));
    assert_eq!(plan_list_calls(&server).await, 1, "the walk happened as before");
}

#[tokio::test]
async fn a_403_on_one_case_skips_to_the_next() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100, 101]).await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/testplan/suites"))
        .and(query_param("testCaseId", "100"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    mount_case_suites(&server, 101, serde_json::json!([requirement_suite(92, 42, 10, "proj")])).await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let found = client.suite_from_cases("org", "proj", 42).await.unwrap().unwrap();
    assert_eq!((found.plan_id, found.suite_id), (10, 92));
}

#[tokio::test]
async fn only_the_first_three_cases_are_asked() {
    let server = MockServer::start().await;
    mount_pbi(&server, &[100, 101, 102, 103]).await;
    for c in [100, 101, 102, 103] {
        mount_case_suites(&server, c, serde_json::json!([])).await;
    }

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    assert!(client.suite_from_cases("org", "proj", 42).await.unwrap().is_none());
    let asked = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/org/_apis/testplan/suites")
        .count();
    assert_eq!(asked, 3);
}
