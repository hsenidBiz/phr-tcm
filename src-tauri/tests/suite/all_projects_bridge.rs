//! The AI bridge while "Treat all projects as one" is on: what it reads in
//! the PBI's own project rather than the home project.

use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::{route, BridgeContext};
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ctx(all_projects: bool) -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        module_ref: Some("Custom.Module".into()),
        preconditions_ref: Some("Custom.Preconditions".into()),
        disabled_tools: vec![],
        working_dir: None,
        db_id: None,
        db_secrets: None,
        db_writes: false,
        api_writes: false,
        network_tools: false,
        all_projects,
        db_no_ask: false,
        gate: Default::default(),
    }
}

async fn ado_stub() -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

/// PBI `pbi` lives in `project`, which holds one plan with its requirement
/// suite and two points (one failed). Nothing is mounted for any other
/// project, so a request there answers 404.
async fn mount_pbi_in(server: &MockServer, project: &str, pbi: i64) {
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/_apis/wit/workitems/{pbi}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": pbi,
            "fields": { "System.TeamProject": project }
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/testplan/plans")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 9, "name": "Payroll Plan", "areaPath": project, "rootSuite": { "id": 90 } }]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/testplan/Plans/9/suites")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 91, "name": format!("{pbi} : Pay run"), "suiteType": "requirementTestSuite", "requirementId": pbi }]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/testplan/Plans/9/Suites/91/TestPoint")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {
                    "id": 7,
                    "testCaseReference": { "id": 201, "name": "Pay run closes" },
                    "configuration": { "name": "Windows 10" },
                    "results": { "outcome": "failed", "lastTestRunId": 3, "lastResultId": 30 }
                },
                {
                    "id": 8,
                    "testCaseReference": { "id": 202, "name": "Pay run opens" },
                    "configuration": { "name": "Windows 10" },
                    "results": { "outcome": "passed", "lastTestRunId": 3, "lastResultId": 31 }
                }
            ]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/acme/{project}/_apis/test/Runs/3/Results/30")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comment": "Totals do not match.",
            "associatedBugs": [{ "id": "555" }]
        })))
        .mount(server)
        .await;
}

fn paths(reqs: &[wiremock::Request]) -> Vec<String> {
    reqs.iter().map(|r| r.url.path().to_string()).collect()
}

#[tokio::test]
async fn combined_run_results_read_the_pbis_own_project() {
    let (server, client) = ado_stub().await;
    // PBI 4201 is in Payroll while the home project (ctx.project) is Web.
    mount_pbi_in(&server, "Payroll", 4201).await;

    let (status, body) =
        route(&ctx(true), Some(&client), "GET", "/run-results?pbi=4201", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases_in_suite"], 2, "{body}");
    assert_eq!(v["matched"], 1);
    assert_eq!(v["results"][0]["case_id"], 201);
    assert_eq!(v["results"][0]["comment"], "Totals do not match.");

    let seen = paths(&server.received_requests().await.unwrap());
    assert!(
        seen.iter().any(|p| p.starts_with("/acme/Payroll/_apis/testplan/")),
        "the suite scan runs in the PBI's project: {seen:?}"
    );
    assert!(
        seen.iter().any(|p| p == "/acme/Payroll/_apis/test/Runs/3/Results/30"),
        "the result detail is read in the PBI's project: {seen:?}"
    );
    assert!(
        !seen.iter().any(|p| p.starts_with("/acme/Web/")),
        "nothing is asked of the home project: {seen:?}"
    );
}

#[tokio::test]
async fn combined_run_results_ask_for_a_pbis_project_once() {
    let (server, client) = ado_stub().await;
    mount_pbi_in(&server, "Payroll", 4202).await;

    for _ in 0..2 {
        let (status, body) =
            route(&ctx(true), Some(&client), "GET", "/run-results?pbi=4202", "", "2.3.1").await;
        assert_eq!(status, 200, "{body}");
    }
    let seen = paths(&server.received_requests().await.unwrap());
    let reads = seen.iter().filter(|p| *p == "/acme/_apis/wit/workitems/4202").count();
    assert_eq!(reads, 1, "the PBI's project does not change, so it is read once: {seen:?}");
}

#[tokio::test]
async fn switched_off_run_results_stay_in_the_chosen_project() {
    let (server, client) = ado_stub().await;
    // Off, the bridge reads the project it was given and never looks the
    // PBI up: the same requests as beta.7.
    mount_pbi_in(&server, "Web", 4203).await;

    let (status, body) =
        route(&ctx(false), Some(&client), "GET", "/run-results?pbi=4203", "", "2.3.1").await;
    assert_eq!(status, 200, "{body}");
    let seen = paths(&server.received_requests().await.unwrap());
    assert!(
        !seen.iter().any(|p| p.starts_with("/acme/_apis/wit/workitems")),
        "no work item lookup when switched off: {seen:?}"
    );
    assert!(seen.iter().any(|p| p.starts_with("/acme/Web/_apis/testplan/")), "{seen:?}");
}

#[tokio::test]
async fn combined_run_results_name_a_pbi_that_is_not_in_the_organization() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems/4204"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(true), Some(&client), "GET", "/run-results?pbi=4204", "", "2.3.1").await;
    assert_eq!(status, 404, "{body}");
    assert!(body.contains("4204"), "{body}");
}
